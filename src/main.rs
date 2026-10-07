mod app;
mod bounded;
mod bulk_actions;
mod bulk_plan;
mod bulk_upload;
mod cache;
mod cache_refresh;
mod config;
mod jira_client;
mod jira_issue;
mod jira_reads;
mod jira_rest;
mod jira_search;
mod jql;
mod lists;
mod local_cache;
mod mouse;
mod move_picker;
mod moves;
mod settings;
mod setup;
mod subtasks;
mod theme;
mod transitions;
mod views;
mod widgets;

use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::collections::HashSet;
use std::io;
use std::process::Command;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

use crate::bulk_actions::{BulkAction, BulkCall, BulkSummary, BulkTarget};
use crate::bulk_plan::{BulkJob, BulkPlan, FetchedTransitions};
use crate::config::AppConfig;
use crate::jira_rest::describe;
use crate::move_picker::JiraCall;
use app::{
    App, BulkUploadPreview, BulkUploadState, BulkUploadSummary, DetailMode, FilterFocus, Tab,
    TicketSyncStage,
};
use cache_refresh::CacheRefreshPhase;

/// Results of background work. `requested_at` is `app.moves.now()` when a Jira read was
/// requested, so a read that predates a confirmed move can't undo it.
enum BackgroundMessage {
    EpicsRefreshed {
        request: u64,
        requested_at: u64,
        result: std::result::Result<Vec<crate::cache::Epic>, String>,
    },
    CacheRefreshed {
        request: u64,
        phase: CacheRefreshPhase,
        requested_at: u64,
        result: std::result::Result<crate::cache::Cache, String>,
    },
    TicketDetailFetched {
        key: String,
        requested_at: u64,
        result: std::result::Result<crate::cache::Ticket, String>,
    },
    /// Jira's transitions for the move picker. `request` is the picker's `MoveLoading` request.
    TransitionsFetched {
        key: String,
        request: u64,
        result: std::result::Result<Vec<crate::transitions::Transition>, String>,
    },
    /// Every selected ticket's transitions for a bulk move, for the bulk `MoveLoading` request.
    BulkTransitionsFetched {
        request: u64,
        fetched: FetchedTransitions,
    },
    TicketMoved {
        key: String,
        result: std::result::Result<(), String>,
    },
    TicketCreated(std::result::Result<String, String>),
    CommentAdded(std::result::Result<String, String>),
    TicketAssigned {
        key: String,
        result: std::result::Result<(), String>,
    },
    TicketEdited {
        key: String,
        result: std::result::Result<(), String>,
    },
    BulkCompleted {
        request: u64,
        summary: BulkSummary,
    },
    BulkUploadPreviewReady(std::result::Result<BulkUploadPreview, String>),
    BulkUploadCompleted(BulkUploadSummary),
    FilterResults {
        requested_at: u64,
        result: std::result::Result<Vec<crate::cache::Ticket>, String>,
    },
}

fn spawn_epics_refresh(app: &mut App, tx: &UnboundedSender<BackgroundMessage>, config: &AppConfig) {
    let requested_at = app.moves.now();
    let request = app.next_request_id();
    app.epic_refresh_request = request;
    app.epics_refreshing = true;
    let tx = tx.clone();
    let config = config.clone();
    tokio::spawn(async move {
        let result = jira_reads::refresh_epics_cache(&config)
            .await
            .map_err(|e| describe(&e));
        let _ = tx.send(BackgroundMessage::EpicsRefreshed {
            request,
            requested_at,
            result,
        });
    });
}

fn spawn_cache_refresh(
    app: &mut App,
    tx: &UnboundedSender<BackgroundMessage>,
    phase: CacheRefreshPhase,
    config: &AppConfig,
) {
    let requested_at = app.moves.now();
    let request = app.begin_cache_refresh();
    let tx = tx.clone();
    let config = config.clone();
    let details = app.details.clone();
    tokio::spawn(async move {
        let result = match phase {
            CacheRefreshPhase::ActiveOnly => jira_reads::fetch_active_only(&config, &details).await,
            CacheRefreshPhase::Full => jira_reads::fetch_all(&config, &details).await,
            CacheRefreshPhase::Manual => jira_reads::fetch_all(&config, &details).await,
        }
        .map_err(|e| describe(&e));
        let _ = tx.send(BackgroundMessage::CacheRefreshed {
            request,
            phase,
            requested_at,
            result,
        });
    });
}

fn spawn_ticket_detail_fetch(
    tx: &UnboundedSender<BackgroundMessage>,
    key: String,
    requested_at: u64,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = jira_search::fetch_ticket_detail(&key)
            .await
            .map_err(|e| describe(&e));
        let _ = tx.send(BackgroundMessage::TicketDetailFetched {
            key,
            requested_at,
            result,
        });
    });
}

fn spawn_ticket_detail_prefetch(
    tx: &UnboundedSender<BackgroundMessage>,
    keys: Vec<String>,
    requested_at: u64,
) {
    if keys.is_empty() {
        return;
    }

    let tx = tx.clone();
    tokio::spawn(async move {
        jira_search::fetch_ticket_details(&keys, |key, result| {
            let _ = tx.send(BackgroundMessage::TicketDetailFetched {
                key,
                requested_at,
                result,
            });
        })
        .await;
    });
}

fn queue_detail_prefetch(app: &mut App, bg_tx: &UnboundedSender<BackgroundMessage>) {
    let prefetch_keys = app
        .missing_detail_ticket_keys()
        .into_iter()
        .filter(|k| app.begin_detail_fetch(k))
        .collect::<Vec<_>>();
    spawn_ticket_detail_prefetch(bg_tx, prefetch_keys, app.moves.now());
}

fn spawn_transitions_fetch(tx: &UnboundedSender<BackgroundMessage>, key: String, request: u64) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = jira_rest::get_transitions(&key)
            .await
            .map_err(|e| describe(&e));
        let _ = tx.send(BackgroundMessage::TransitionsFetched {
            key,
            request,
            result,
        });
    });
}

fn spawn_ticket_move(
    tx: &UnboundedSender<BackgroundMessage>,
    key: String,
    transition_id: String,
    resolution_id: Option<String>,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = jira_rest::transition(&key, &transition_id, resolution_id.as_deref())
            .await
            .map_err(|e| describe(&e));
        let _ = tx.send(BackgroundMessage::TicketMoved { key, result });
    });
}

/// Starts the Jira call the move picker asked for, if any.
fn start_picker_call(tx: &UnboundedSender<BackgroundMessage>, call: Option<JiraCall>) {
    match call {
        Some(JiraCall::FetchTransitions { key, request }) => {
            spawn_transitions_fetch(tx, key, request)
        }
        Some(JiraCall::Transition {
            key,
            id,
            resolution_id,
        }) => spawn_ticket_move(tx, key, id, resolution_id),
        None => {}
    }
}

/// How many Jira calls a bulk action makes at once.
const MAX_BULK_CONCURRENCY: usize = 6;

/// Runs `task` on each item, at most `MAX_BULK_CONCURRENCY` at a time, and collects the
/// `(ticket key, result)` pairs the tasks return, in the order they finish.
async fn run_bounded<I, T, F, Fut>(
    items: Vec<I>,
    task: F,
) -> Vec<(String, std::result::Result<T, String>)>
where
    F: Fn(I) -> Fut,
    Fut: std::future::Future<Output = (String, std::result::Result<T, String>)> + Send + 'static,
    T: Send + 'static,
{
    let mut results = Vec::new();
    bounded::for_each_bounded(MAX_BULK_CONCURRENCY, items, task, |_, joined| {
        results.push(joined.unwrap_or_else(|err| ("unknown".to_string(), Err(err.to_string()))));
        std::ops::ControlFlow::Continue(())
    })
    .await;
    results
}

fn spawn_bulk_transitions_fetch(
    tx: &UnboundedSender<BackgroundMessage>,
    targets: Vec<String>,
    request: u64,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let mut fetched = run_bounded(targets.clone(), |key| async move {
            let result = jira_rest::get_transitions(&key)
                .await
                .map_err(|e| describe(&e));
            (key, result)
        })
        .await;
        // Back into selection order, which the skip reasons in the summary follow.
        fetched.sort_by_key(|(key, _)| targets.iter().position(|target| target == key));
        let _ = tx.send(BackgroundMessage::BulkTransitionsFetched { request, fetched });
    });
}

fn spawn_bulk_execution(
    tx: &UnboundedSender<BackgroundMessage>,
    request: u64,
    target: BulkTarget,
    plan: BulkPlan,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let action = match target {
            BulkTarget::Move { .. } => BulkAction::Move,
            BulkTarget::Assign { .. } => BulkAction::Assign,
        };
        let results = run_bounded(plan.jobs, |(key, job)| async move {
            let result = match job {
                BulkJob::Move {
                    transition,
                    resolution_id,
                } => jira_rest::transition(&key, &transition.id, resolution_id.as_deref()).await,
                BulkJob::Assign { email } => jira_client::assign_ticket(&key, &email).await,
            };
            (key, result.map_err(|e| describe(&e)))
        })
        .await;
        let summary = bulk_actions::summarize(action, target, results, plan.skipped);
        let _ = tx.send(BackgroundMessage::BulkCompleted { request, summary });
    });
}

fn build_bulk_upload_context(app: &App) -> bulk_upload::BulkUploadContext {
    let known_epic_keys: HashSet<String> = app
        .cache
        .epics
        .iter()
        .map(|e| e.key.to_ascii_uppercase())
        .collect();

    let mut existing_summaries = HashSet::new();
    for ticket in app
        .cache
        .my_tickets
        .iter()
        .chain(app.cache.team_tickets.iter())
        .chain(app.filter_results.iter())
    {
        let normalized = bulk_upload::normalize_summary(&ticket.summary);
        if !normalized.is_empty() {
            existing_summaries.insert(normalized);
        }
    }

    bulk_upload::BulkUploadContext::new(known_epic_keys, existing_summaries)
}

fn spawn_bulk_upload_preview(
    tx: &UnboundedSender<BackgroundMessage>,
    path: String,
    context: bulk_upload::BulkUploadContext,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = bulk_upload::parse_csv_preview(&path, &context).map_err(|e| e.to_string());
        let _ = tx.send(BackgroundMessage::BulkUploadPreviewReady(result));
    });
}

fn spawn_bulk_upload_execution(
    tx: &UnboundedSender<BackgroundMessage>,
    preview: BulkUploadPreview,
    project: String,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let mut created_keys = Vec::new();
        let mut failed_details = Vec::new();
        let attempt_rows = preview
            .rows
            .iter()
            .filter(|row| row.errors.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        let attempted = attempt_rows.len();

        for row in attempt_rows {
            let labels = if row.labels.is_empty() {
                None
            } else {
                Some(row.labels.as_slice())
            };

            let result = jira_client::create_ticket_with_fields(
                &project,
                row.issue_type.as_str(),
                row.summary.as_str(),
                row.assignee_email.as_deref(),
                row.epic_key.as_deref(),
                row.description.as_deref(),
                labels,
            )
            .await
            .map_err(|e| e.to_string());

            match result {
                Ok(key) => created_keys.push(key),
                Err(err) => failed_details.push((row.row_number, row.summary, err)),
            }
        }

        let summary = BulkUploadSummary {
            source_path: preview.source_path,
            total_rows: preview.total_rows,
            attempted,
            succeeded: created_keys.len(),
            failed: failed_details.len(),
            created_keys,
            failed_details,
        };
        let _ = tx.send(BackgroundMessage::BulkUploadCompleted(summary));
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    maybe_run_dev_mode()?;
    // Before the terminal is set up, so a config error prints to a normal terminal.
    let loaded_config = config::load_config()?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut config = match loaded_config {
        Some(config) => config,
        None => setup::run_setup(&mut terminal).await?,
    };

    let mut app = App::new();
    app.set_epics_i_care_about(config.epics_i_care_about_ordered());
    app.set_status_rules(&config.statuses);
    app.show_done = config.preferences.show_done;
    app.theme = theme::resolve(&config.preferences.theme, &config.themes);
    app.active_tab = Tab::all()
        .iter()
        .copied()
        .find(|tab| tab.title() == config.preferences.start_tab)
        .unwrap_or(Tab::MyWork);
    let (bg_tx, mut bg_rx) = tokio::sync::mpsc::unbounded_channel();
    local_cache::remove_old_temp_dir_caches(&config.jira.project);
    app.details = local_cache::DetailCache::spawn(&config.jira.project);
    jira_reads::keep_my_email_current(&config.jira.project);

    // Fast startup: load persisted snapshot immediately, then revalidate in stages.
    if let Some(snapshot) = local_cache::load_startup_cache_snapshot(&config.jira.project) {
        app.replace_cache(snapshot.cache, app.moves.now());
        app.loading = false;
        app.cache_stale_age_secs = Some(snapshot.age_secs);
        app.ticket_sync_stage = Some(TicketSyncStage::ActiveOnly);
        app.flash = Some("Loaded cached data. Refreshing active tickets...".to_string());
        spawn_cache_refresh(&mut app, &bg_tx, CacheRefreshPhase::ActiveOnly, &config);
    } else {
        let cache = match jira_reads::fetch_active_only(&config, &app.details).await {
            Ok(cache) => cache,
            Err(e) => {
                // No snapshot to fall back on. Leave the screen first, or the error is lost
                // in it (a missing JIRA_API_TOKEN lands here on a first run).
                restore_terminal(&mut terminal)?;
                return Err(e);
            }
        };
        app.replace_cache(cache, app.moves.now());
        app.loading = false;
        app.ticket_sync_stage = Some(TicketSyncStage::Full);
        app.flash = Some("Loaded active tickets. Syncing recently done...".to_string());
        spawn_cache_refresh(&mut app, &bg_tx, CacheRefreshPhase::Full, &config);
    }

    spawn_epics_refresh(&mut app, &bg_tx, &config);
    queue_detail_prefetch(&mut app, &bg_tx);

    let mut draw_needed = true;

    // Main loop
    loop {
        let mut state_changed = false;
        while let Ok(message) = bg_rx.try_recv() {
            state_changed = true;
            match message {
                BackgroundMessage::EpicsRefreshed {
                    request,
                    requested_at,
                    result,
                } => {
                    if request != app.epic_refresh_request {
                        continue;
                    }
                    app.epics_refreshing = false;
                    match result {
                        Ok(epics) => {
                            app.replace_epics(epics, requested_at);
                            app.flash = Some("Epic relationships refreshed".to_string());
                        }
                        Err(e) => {
                            app.flash = Some(format!("Epic refresh failed: {}", e));
                        }
                    }
                }
                BackgroundMessage::CacheRefreshed {
                    request,
                    phase,
                    requested_at,
                    result,
                } => {
                    let follow_up = app.apply_cache_refresh(phase, request, requested_at, result);
                    if follow_up.prefetch_details {
                        queue_detail_prefetch(&mut app, &bg_tx);
                    }
                    if follow_up.save_snapshot {
                        let saved =
                            local_cache::save_full_cache_snapshot(&config.jira.project, &app.cache)
                                .map_err(|e| e.to_string());
                        app.snapshot_saved(phase, saved);
                    }
                    if let Some(next) = follow_up.next_phase {
                        spawn_cache_refresh(&mut app, &bg_tx, next, &config);
                    }
                    if follow_up.refresh_epics {
                        spawn_epics_refresh(&mut app, &bg_tx, &config);
                    }
                }
                BackgroundMessage::TicketDetailFetched {
                    key,
                    requested_at,
                    result,
                } => {
                    app.end_detail_fetch(&key);
                    match result {
                        Ok(detail) => {
                            if app.enrich_ticket(&key, requested_at, &detail) {
                                app.details.record(detail);
                            }
                        }
                        Err(e) => app.fail_detail_fetch(&key, e),
                    }
                }
                BackgroundMessage::TransitionsFetched {
                    key,
                    request,
                    result,
                } => move_picker::receive(&mut app, &key, request, result),
                BackgroundMessage::BulkTransitionsFetched { request, fetched } => {
                    bulk_actions::receive_transitions(&mut app, request, fetched)
                }
                BackgroundMessage::TicketMoved { key, result } => {
                    let succeeded = result.is_ok();
                    app.finish_move(&key, result);
                    if succeeded {
                        app.begin_detail_fetch(&key);
                        spawn_ticket_detail_fetch(&bg_tx, key, app.moves.now());
                    }
                }
                BackgroundMessage::TicketCreated(result) => {
                    match result {
                        Ok(key) => {
                            app.flash = Some(format!("Created {}", key));
                            // Trigger a manual refresh to pick up the new ticket
                            if !app.loading {
                                app.loading = true;
                                app.ticket_sync_stage = None;
                                spawn_cache_refresh(
                                    &mut app,
                                    &bg_tx,
                                    CacheRefreshPhase::Manual,
                                    &config,
                                );
                            }
                        }
                        Err(e) => {
                            app.flash = Some(format!("Create failed: {}", e));
                        }
                    }
                }
                BackgroundMessage::CommentAdded(result) => match result {
                    Ok(key) => {
                        app.flash = Some(format!("Comment added to {}", key));
                    }
                    Err(e) => {
                        app.flash = Some(format!("Comment failed: {}", e));
                    }
                },
                BackgroundMessage::TicketAssigned { key, result } => match result {
                    Ok(()) => {
                        app.flash = Some(format!("Assigned {}", key));
                    }
                    Err(e) => {
                        app.flash = Some(format!("Assign failed for {}: {}", key, e));
                    }
                },
                BackgroundMessage::TicketEdited { key, result } => match result {
                    Ok(()) => {
                        app.flash = Some(format!("Updated {}", key));
                    }
                    Err(e) => {
                        app.flash = Some(format!("Edit failed for {}: {}", key, e));
                    }
                },
                BackgroundMessage::BulkCompleted { request, summary } => {
                    bulk_actions::complete(&mut app, request, summary);
                }
                BackgroundMessage::BulkUploadPreviewReady(result) => match result {
                    Ok(preview) => {
                        let total_rows = preview.total_rows;
                        let invalid_rows = preview.invalid_rows;
                        app.bulk_upload_state = Some(BulkUploadState::Preview {
                            preview,
                            selected: 0,
                        });
                        app.flash = Some(format!(
                            "Preview ready: {} rows ({} invalid)",
                            total_rows, invalid_rows
                        ));
                    }
                    Err(e) => {
                        if let Some(BulkUploadState::PathInput { loading, .. }) =
                            app.bulk_upload_state.as_mut()
                        {
                            *loading = false;
                        }
                        app.flash = Some(format!("CSV preview failed: {}", e));
                    }
                },
                BackgroundMessage::BulkUploadCompleted(summary) => {
                    let succeeded = summary.succeeded;
                    let failed = summary.failed;
                    if matches!(app.bulk_upload_state, Some(BulkUploadState::Running { .. })) {
                        app.bulk_upload_state = Some(BulkUploadState::Result { summary });
                    }
                    app.flash = Some(format!(
                        "Bulk upload complete: {} succeeded, {} failed",
                        succeeded, failed
                    ));
                    if !app.loading {
                        app.loading = true;
                        app.ticket_sync_stage = None;
                        spawn_cache_refresh(&mut app, &bg_tx, CacheRefreshPhase::Manual, &config);
                    }
                }
                BackgroundMessage::FilterResults {
                    requested_at,
                    result,
                } => {
                    app.filter_loading = false;
                    match result {
                        Ok(tickets) => {
                            let count = tickets.len();
                            app.show_filter_results(tickets, requested_at);
                            app.prune_selection_to_visible();
                            app.filter_focus = FilterFocus::Results;
                            app.selected_index = 0;
                            app.flash = Some(format!("Filter returned {} tickets", count));
                        }
                        Err(e) => {
                            app.filter_results.clear();
                            app.collapsed_filters.clear();
                            app.mark_cache_changed();
                            app.prune_selection_to_visible();
                            app.flash = Some(format!("Filter query failed: {}", e));
                        }
                    }
                }
            }
        }

        if state_changed {
            draw_needed = true;
        }
        if draw_needed {
            terminal.draw(|f| ui(f, &app, &config))?;
            draw_needed = false;
        }

        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    handle_key(&mut app, key, &bg_tx, &mut config).await;
                    draw_needed = true;
                }
                Event::Paste(text) => {
                    handle_paste(&mut app, &text);
                    draw_needed = true;
                }
                Event::Mouse(mouse) => {
                    mouse::handle(&mut app, mouse, &bg_tx, &mut config).await;
                    draw_needed = true;
                }
                Event::Resize(_, _) => draw_needed = true,
                _ => {}
            }
        }

        if app.external_editor_requested {
            app.external_editor_requested = false;
            if let Err(error) = edit_externally(&mut terminal, &mut app) {
                app.flash = Some(format!("Editor failed: {error:#}"));
            }
            draw_needed = true;
        }

        if app.should_quit {
            break;
        }
    }

    // Details recorded in the last moments are saved before the process ends.
    app.details.close(&app.listed_keys());
    restore_terminal(&mut terminal)
}

/// Leaves raw mode and the alternate screen, so what prints next stays readable.
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;
    Ok(())
}

async fn handle_key(
    app: &mut App,
    key: KeyEvent,
    bg_tx: &UnboundedSender<BackgroundMessage>,
    config: &mut AppConfig,
) {
    // Flash messages clear on any keypress. Move failures stay until dismissed.
    app.flash = None;
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        mouse::copy_selected(app);
        return;
    }
    if key.code == KeyCode::Esc && mouse::selected_text(app).is_some() {
        mouse::clear(app);
        return;
    }
    mouse::clear_screen(app);
    if (key.code == KeyCode::F(4)
        || (key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL)))
        && app.focused_field().is_some()
    {
        if app.current_editor().is_some() {
            app.external_editor_requested = true;
        } else {
            app.flash = Some("Select a text field to use the editor".into());
        }
        return;
    }
    let show_done_before = app.show_done;

    if !app.moves.failures().is_empty() {
        handle_move_failure_keys(app, key.code);
    } else if app.settings.is_some() {
        if settings::handle_key(app, key.code, key.modifiers, config) {
            app.loading = true;
            app.ticket_sync_stage = None;
            spawn_cache_refresh(app, bg_tx, CacheRefreshPhase::Manual, config);
            spawn_epics_refresh(app, bg_tx, config);
        }
    } else if app.is_filter_edit_open() {
        handle_filter_edit_keys(app, key.code, key.modifiers, config);
    } else if app.is_bulk_upload_open() {
        handle_bulk_upload_keys(app, key.code, bg_tx, config);
    } else if app.is_create_ticket_open() {
        handle_create_ticket_keys(app, key.code, key.modifiers, bg_tx, config).await;
    } else if app.is_comment_open() {
        handle_comment_keys(app, key.code, key.modifiers, bg_tx);
    } else if app.is_assign_open() {
        handle_assign_keys(app, key.code, bg_tx);
    } else if app.is_edit_open() {
        handle_edit_keys(app, key.code, key.modifiers, bg_tx);
    } else if app.is_bulk_open() {
        match bulk_actions::handle_key(app, key.code) {
            Some(BulkCall::FetchTransitions { targets, request }) => {
                spawn_bulk_transitions_fetch(bg_tx, targets, request);
            }
            Some(BulkCall::Execute {
                request,
                target,
                plan,
            }) => {
                spawn_bulk_execution(bg_tx, request, target, plan);
            }
            None => {}
        }
    } else if app.show_keybindings {
        handle_keybindings_keys(app, key.code);
    } else if app.is_detail_open() {
        handle_detail_keys(app, key.code, bg_tx);
    } else if app.search.is_some() {
        handle_search_keys(app, key.code, key.modifiers, bg_tx).await;
    } else if app.active_tab == Tab::Filters {
        handle_filter_keys(app, key.code, bg_tx, config);
    } else {
        handle_main_keys(app, key.code, key.modifiers, bg_tx, config).await;
    }
    if show_done_before != app.show_done
        && config.preferences.show_done != app.show_done
        && app.settings.is_none()
    {
        config.preferences.show_done = app.show_done;
        if let Err(error) = config::save_config(config) {
            app.flash = Some(format!("Couldn't save Done preference: {error}"));
        }
    }
}

fn handle_paste(app: &mut App, text: &str) {
    mouse::clear_screen(app);
    if let Some((editor, multiline)) = app.current_editor() {
        widgets::form::paste(editor, text, multiline);
        return;
    }
    if !app.moves.failures().is_empty() || app.show_keybindings {
        return;
    }
    let text = text.replace(['\r', '\n'], " ");
    if let Some(BulkUploadState::PathInput {
        path,
        loading: false,
    }) = &mut app.bulk_upload_state
    {
        widgets::form::paste(path, text.trim(), false);
    } else if let Some(state) = &mut app.assign_state {
        state.search.push_str(&text);
        let options = app
            .cache
            .team_members
            .iter()
            .map(|m| format!("{} ({})", m.name, m.email))
            .collect::<Vec<_>>();
        widgets::form::choose(
            &options,
            &mut state.selected,
            &mut state.search,
            KeyCode::Null,
        );
    } else if let Some(state) = &mut app.create_ticket {
        let field = state.focused_field;
        if matches!(field, 2 | 3) {
            let options = if field == 2 {
                widgets::create_ticket::build_assignee_options(app)
            } else {
                widgets::create_ticket::build_epic_options(app)
            };
            let state = app.create_ticket.as_mut().unwrap();
            let (index, query) = if field == 2 {
                (&mut state.assignee_idx, &mut state.assignee_search)
            } else {
                (&mut state.epic_idx, &mut state.epic_search)
            };
            query.push_str(&text);
            widgets::form::choose(&options, index, query, KeyCode::Null);
        }
    } else if !app.is_detail_open()
        && !app.is_bulk_open()
        && !app.is_bulk_upload_open()
        && app.settings.is_none()
    {
        if let Some(search) = &mut app.search {
            search.push_str(&text);
            app.clamp_selection();
        }
    }
}

fn edit_externally(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};
    let Some((editor, multiline)) = app.current_editor() else {
        return Ok(());
    };
    let original = widgets::form::text(editor);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!(
        "lazyjira-editor-{}-{stamp}.txt",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(original.as_bytes())?;
    drop(file);
    let editor_command = ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "vi".into());
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    let result = Command::new("sh")
        .arg("-c")
        .arg(format!("exec {editor_command} \"$1\""))
        .arg("lazyjira-editor")
        .arg(&path)
        .status();
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    terminal.clear()?;
    let edited = std::fs::read_to_string(&path);
    if let Ok(text) = &edited {
        if let Some((editor, _)) = app.current_editor() {
            *editor = widgets::form::editor("");
            widgets::form::paste(editor, text.trim_end_matches('\n'), multiline);
        }
        let _ = std::fs::remove_file(&path);
    }
    edited
        .with_context(|| format!("Couldn't read edited text; file kept at {}", path.display()))?;
    if !result?.success() {
        anyhow::bail!("external editor exited unsuccessfully; its text was kept in the form");
    }
    Ok(())
}

fn dev_manifest_path(
    working_dir: &std::path::Path,
    compiled_dir: &std::path::Path,
) -> std::path::PathBuf {
    working_dir
        .ancestors()
        .map(|dir| dir.join("Cargo.toml"))
        .find(|path| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
                .is_some_and(|manifest| {
                    manifest
                        .get("package")
                        .and_then(|package| package.get("name"))
                        .and_then(toml::Value::as_str)
                        == Some("lazyjira")
                })
        })
        .unwrap_or_else(|| compiled_dir.join("Cargo.toml"))
}

fn maybe_run_dev_mode() -> Result<()> {
    let mut force_rebuild = false;
    let mut release = false;
    let mut show_help = false;
    let mut passthrough_args = Vec::new();

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--dev" | "--rebuild" => force_rebuild = true,
            "--dev-release" => {
                force_rebuild = true;
                release = true;
            }
            "--help" | "-h" => show_help = true,
            _ => passthrough_args.push(arg),
        }
    }

    if show_help {
        println!("lazyjira");
        println!("  --dev, --rebuild   Build from source and run (debug)");
        println!("  --dev-release      Build from source and run (release)");
        println!("  -h, --help         Show this help");
        std::process::exit(0);
    }

    if !force_rebuild {
        return Ok(());
    }

    let manifest = dev_manifest_path(
        &std::env::current_dir()?,
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
    );
    anyhow::ensure!(
        manifest.is_file(),
        "No lazyjira source checkout found. Run --dev from a lazyjira checkout."
    );
    println!("Building lazyjira from {}", manifest.display());
    let mut cmd = Command::new("cargo");
    cmd.current_dir(manifest.parent().unwrap());
    cmd.arg("run");

    if release {
        cmd.arg("--release");
    }

    cmd.arg("--manifest-path").arg(manifest);

    if !passthrough_args.is_empty() {
        cmd.arg("--");
        cmd.args(passthrough_args);
    }

    let status = cmd.status()?;
    std::process::exit(status.code().unwrap_or(1));
}

fn format_age_minutes(age_secs: u64) -> String {
    let mins = age_secs / 60;
    if mins == 0 {
        "<1m".to_string()
    } else {
        format!("{}m", mins)
    }
}

fn shortcut_hints(text: &str) -> ratatui::text::Line<'static> {
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};

    let muted = Style::default().fg(Color::DarkGray);
    let mut spans = Vec::new();
    // Hints use "key: label"; status indicators use "name:value".
    for hint in text.split_inclusive("  ") {
        if let Some((key, label)) = hint.split_once(": ") {
            spans.push(Span::styled(
                key.to_string(),
                Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::styled(format!(": {}", label), muted));
        } else {
            spans.push(Span::styled(hint.to_string(), muted));
        }
    }
    Line::from(spans)
}

fn ui(f: &mut ratatui::Frame, app: &App, config: &AppConfig) {
    use crate::views::common::panel;
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Tabs;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Content
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    mouse::begin_layer(app);
    app.text_selection.borrow_mut().area = panel().inner(chunks[1]);
    // Tab bar
    let mut tab_x = chunks[0].x + 2;
    for tab in Tab::all() {
        let width = tab.title().len() as u16;
        let rect = ratatui::layout::Rect::new(
            tab_x,
            chunks[0].y + 1,
            (width + 2).min(chunks[0].right().saturating_sub(tab_x)),
            1,
        );
        app.mouse_targets
            .borrow_mut()
            .push((rect, mouse::Target::Tab(*tab)));
        tab_x += width + 5;
    }
    let tab_titles: Vec<Line> = Tab::all().iter().map(|t| Line::from(t.title())).collect();
    let tabs = Tabs::new(tab_titles)
        .block(panel().title(" lazyjira "))
        .divider(Span::styled(" · ", Style::default().fg(Color::DarkGray)))
        .select(match app.active_tab {
            Tab::MyWork => 0,
            Tab::Team => 1,
            Tab::Epics => 2,
            Tab::Unassigned => 3,
            Tab::Filters => 4,
        })
        .style(Style::default().fg(Color::Reset))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, chunks[0]);
    if chunks[0].right().saturating_sub(tab_x) >= 12 {
        widgets::form::buttons(
            f,
            app,
            ratatui::layout::Rect::new(chunks[0].right() - 12, chunks[0].y + 1, 10, 1),
            &[("Settings", KeyCode::Char('S'))],
        );
    }

    // Content area
    if app.loading
        && app.cache.my_tickets.is_empty()
        && app.cache.team_tickets.is_empty()
        && app.cache.epics.is_empty()
        && app.filter_results.is_empty()
    {
        let loading = ratatui::widgets::Paragraph::new("Loading...").block(panel());
        f.render_widget(loading, chunks[1]);
    } else {
        match app.active_tab {
            Tab::MyWork => views::my_work::render(f, chunks[1], app),
            Tab::Team => views::team::render(f, chunks[1], app),
            Tab::Epics => views::epics::render(f, chunks[1], app),
            Tab::Unassigned => views::unassigned::render(f, chunks[1], app),
            Tab::Filters => views::filters::render(f, chunks[1], app, config),
        }
    }

    // Status bar
    let status_line = if let Some(ref flash) = app.flash {
        // One line: the terminal drops a line break, which would run Jira's messages together.
        Line::from(Span::styled(
            flash.lines().collect::<Vec<_>>().join("; "),
            Style::default().fg(Color::Red),
        ))
    } else if let Some(ref search) = app.search {
        Line::from(Span::styled(
            format!("/{}", search),
            Style::default().fg(Color::Yellow),
        ))
    } else if let Some(pending) = app.moves.pending_message() {
        Line::from(Span::styled(pending, Style::default().fg(Color::Yellow)))
    } else {
        let selected_count = app.selected_ticket_count();
        if app.active_tab == Tab::Filters {
            let pane = match app.filter_focus {
                FilterFocus::Sidebar => "sidebar",
                FilterFocus::Results => "results",
            };
            shortcut_hints(&format!(
                    " j/k: navigate  Space: mark  A: all  u: clear  B: bulk  U: upload  z/Z: fold  sel:{}  Tab/S-Tab: switch pane({})  Enter: run/open  n: new  e: edit  x: delete  ?: keys  q: quit ",
                    selected_count, pane
                ))
        } else {
            let done_state = if app.show_done { "on" } else { "off" };
            let epic_state = if app.epics_refreshing {
                "syncing"
            } else {
                "ready"
            };
            let ticket_state = match app.ticket_sync_stage {
                Some(TicketSyncStage::ActiveOnly) => "sync-active",
                Some(TicketSyncStage::Full) => "sync-full",
                None if app.loading => "refreshing",
                None => "ready",
            };
            let freshness_state = app
                .cache_stale_age_secs
                .map(|age| format!("stale {}", format_age_minutes(age)))
                .unwrap_or_else(|| "fresh".to_string());
            let focus_state = app.status_focus.as_deref().unwrap_or("all");
            shortcut_hints(&format!(
                    " Tab: switch  j/k: navigate  Space: mark  A: all  u: clear  B: bulk  U: upload  sel:{}  Enter: detail  z: fold  d: done({})  f/F: focus({})  ?: keys  t:{}  c:{}  e:{}  r: refresh  /: search  q: quit ",
                    selected_count, done_state, focus_state, ticket_state, freshness_state, epic_state
                ))
        }
    };
    f.render_widget(ratatui::widgets::Paragraph::new(status_line), chunks[2]);

    // Detail overlay
    if app.is_detail_open() {
        mouse::begin_layer(app);
        widgets::ticket_detail::render(f, app);
    }
    if app.is_create_ticket_open() {
        mouse::begin_layer(app);
        widgets::create_ticket::render(f, app);
    }
    if app.is_comment_open() {
        mouse::begin_layer(app);
        widgets::comment::render(f, app);
    }
    if app.is_assign_open() {
        mouse::begin_layer(app);
        widgets::assign::render(f, app);
    }
    if app.is_edit_open() {
        mouse::begin_layer(app);
        widgets::edit_fields::render(f, app);
    }
    if app.is_filter_edit_open() {
        mouse::begin_layer(app);
        render_filter_edit_modal(f, app);
    }
    if app.is_bulk_open() {
        mouse::begin_layer(app);
        widgets::bulk_actions::render(f, app);
    }
    if app.is_bulk_upload_open() {
        mouse::begin_layer(app);
        widgets::bulk_upload::render(f, app);
    }
    if app.show_keybindings {
        mouse::begin_layer(app);
        widgets::keybindings_help::render(f, app);
    }
    if app.settings.is_some() {
        mouse::begin_layer(app);
        settings::render(f, app);
    }
    if !app.moves.failures().is_empty() {
        mouse::begin_layer(app);
        let area = f.area();
        widgets::form::buttons(
            f,
            app,
            ratatui::layout::Rect::new(
                area.x + 2,
                area.bottom().saturating_sub(2),
                area.width.saturating_sub(4),
                1,
            ),
            &[("Dismiss", KeyCode::Enter), ("Browser", KeyCode::Char('o'))],
        );
    }
    widgets::move_failure::render(f, app, app.moves.failures());
    mouse::render_selection(f, app);
    app.settings
        .as_ref()
        .map_or(app.theme, |state| {
            theme::resolve(&state.themes[state.theme], &config.themes)
        })
        .apply(f.buffer_mut());
}

fn render_filter_edit_modal(f: &mut ratatui::Frame, app: &App) {
    use ratatui::layout::{Constraint, Layout};
    use widgets::form;
    let Some(state) = &app.filter_edit else {
        return;
    };
    let title = if state.editing_idx.is_some() {
        "Edit Filter"
    } else {
        "New Filter"
    };
    let inner = form::render_modal_frame(f, app, title, 80, 50);
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(inner);
    form::render_editor(
        f,
        app,
        areas[0],
        "Name",
        &state.name,
        state.focused_field == 0,
        0,
    );
    form::render_editor(
        f,
        app,
        areas[1],
        "JQL",
        &state.jql,
        state.focused_field == 1,
        1,
    );
    form::buttons(
        f,
        app,
        areas[2],
        &[
            ("Save", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
    );
}

fn handle_keybindings_keys(app: &mut App, key: KeyCode) {
    let scroll = app.keybindings_scroll.min(app.keybindings_scroll_max.get());
    let page = app.keybindings_page_height.get();
    match key {
        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => app.close_keybindings(),
        KeyCode::Down | KeyCode::Char('j') => {
            app.keybindings_scroll = scroll
                .saturating_add(1)
                .min(app.keybindings_scroll_max.get());
        }
        KeyCode::Up | KeyCode::Char('k') => app.keybindings_scroll = scroll.saturating_sub(1),
        KeyCode::PageDown | KeyCode::Char(' ') => {
            app.keybindings_scroll = scroll
                .saturating_add(page)
                .min(app.keybindings_scroll_max.get());
        }
        KeyCode::PageUp => app.keybindings_scroll = scroll.saturating_sub(page),
        KeyCode::Home | KeyCode::Char('g') => app.keybindings_scroll = 0,
        KeyCode::End | KeyCode::Char('G') => {
            app.keybindings_scroll = app.keybindings_scroll_max.get();
        }
        _ => {}
    }
}

fn handle_move_failure_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => app.moves.dismiss_failure(),
        KeyCode::Char('o') => {
            if let Some(failure) = app.moves.failures().first() {
                let key = failure.key.clone();
                open_ticket_in_browser(app, &key);
            }
        }
        _ => {}
    }
}

fn open_ticket_in_browser(app: &mut App, key: &str) {
    let result = jira_rest::browse_url(key)
        .and_then(|url| Command::new("open").arg(url).spawn().map_err(Into::into));
    if let Err(error) = result {
        app.flash = Some(format!("Couldn't open browser: {error:#}"));
    }
}

fn handle_detail_keys(app: &mut App, key: KeyCode, bg_tx: &UnboundedSender<BackgroundMessage>) {
    match app.detail_mode.clone() {
        DetailMode::View => {
            let ticket_detail_key = app
                .detail_ticket_key
                .as_ref()
                .and_then(|k| app.find_ticket(k).map(|_| k.clone()));

            match key {
                KeyCode::Esc => app.close_detail(),
                KeyCode::Up | KeyCode::Char('k') => app.scroll_detail_up(),
                KeyCode::Down | KeyCode::Char('j') => app.scroll_detail_down(),
                KeyCode::PageUp => app.scroll_detail_page(false),
                KeyCode::PageDown | KeyCode::Char(' ') => app.scroll_detail_page(true),
                KeyCode::Home | KeyCode::Char('g') => app.scroll_detail_to(false),
                KeyCode::End | KeyCode::Char('G') => app.scroll_detail_to(true),
                KeyCode::Char('z') => app.detail_fullscreen = !app.detail_fullscreen,
                KeyCode::Left | KeyCode::Right => {
                    if let Some(key) = app.step_detail(key == KeyCode::Right) {
                        spawn_ticket_detail_fetch(bg_tx, key, app.moves.now());
                    }
                }
                KeyCode::Char('o') => {
                    if let Some(key) = ticket_detail_key.as_ref() {
                        open_ticket_in_browser(app, key);
                    } else if let Some(epic_key) = app.detail_epic_key.as_ref() {
                        let key = epic_key.clone();
                        open_ticket_in_browser(app, &key);
                    }
                }
                KeyCode::Char('m') => start_picker_call(bg_tx, move_picker::open(app)),
                KeyCode::Char('C') => {
                    if let Some(key) = ticket_detail_key {
                        app.comment_state = Some(app::CommentState {
                            ticket_key: key,
                            body: widgets::form::editor(""),
                        });
                    }
                }
                KeyCode::Char('a') => {
                    if let Some(key) = app
                        .detail_ticket_key
                        .as_ref()
                        .and_then(|k| app.find_ticket(k).map(|_| k.clone()))
                    {
                        app.assign_state = Some(app::AssignState {
                            ticket_key: key,
                            selected: 0,
                            search: String::new(),
                        });
                    }
                }
                KeyCode::Char('e') => {
                    if let Some(key) = app
                        .detail_ticket_key
                        .as_ref()
                        .and_then(|k| app.find_ticket(k).map(|_| k.clone()))
                    {
                        let ticket = app.find_ticket(&key);
                        let summary = ticket.map(|t| t.summary.clone()).unwrap_or_default();
                        let labels = ticket.map(|t| t.labels.join(", ")).unwrap_or_default();
                        app.edit_state = Some(app::EditFieldsState {
                            ticket_key: key,
                            focused_field: 0,
                            summary: widgets::form::editor(&summary),
                            labels: widgets::form::editor(&labels),
                            description: widgets::form::editor(
                                ticket.and_then(|t| t.description.as_deref()).unwrap_or(""),
                            ),
                        });
                    }
                }
                KeyCode::Char('h') => {
                    if app
                        .detail_ticket_key
                        .as_ref()
                        .and_then(|k| app.find_ticket(k))
                        .is_some()
                    {
                        app.detail_mode = DetailMode::History { scroll: 0 };
                    }
                }
                _ => {}
            }
        }
        DetailMode::MoveLoading { .. }
        | DetailMode::MovePicker(_)
        | DetailMode::ResolutionPicker { .. } => {
            start_picker_call(bg_tx, move_picker::handle_key(app, key))
        }
        DetailMode::History { scroll } => match key {
            KeyCode::Esc => app.detail_mode = DetailMode::View,
            KeyCode::Down | KeyCode::Char('j') => {
                app.detail_mode = DetailMode::History { scroll: scroll + 1 };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.detail_mode = DetailMode::History {
                    scroll: scroll.saturating_sub(1),
                };
            }
            _ => {}
        },
    }
}

async fn handle_search_keys(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    bg_tx: &UnboundedSender<BackgroundMessage>,
) {
    match key {
        KeyCode::Esc => {
            app.search = None;
            app.clamp_selection();
        }
        KeyCode::Enter => {
            if let Some(group_id) = app.selected_header_group_id() {
                if app.active_tab == Tab::Epics {
                    app.open_epic_detail(group_id);
                } else if app.is_collapsed(app.active_tab, &group_id) {
                    app.toggle_group_collapse(&group_id);
                }
            } else if let Some(key) = app.selected_ticket_key() {
                if let Some(key) = app.open_fresh_detail(key) {
                    spawn_ticket_detail_fetch(bg_tx, key, app.moves.now());
                }
            }
        }
        KeyCode::Char('U') => {
            app.search = None;
            app.bulk_upload_state = Some(BulkUploadState::PathInput {
                path: widgets::form::editor("").into(),
                loading: false,
            });
        }
        KeyCode::Backspace => {
            if let Some(ref mut s) = app.search {
                s.pop();
                if s.is_empty() {
                    app.search = None;
                }
            }
            app.clamp_selection();
        }
        KeyCode::Down => app.move_selection_down(),
        KeyCode::Up => app.move_selection_up(),
        KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_selection_down()
        }
        KeyCode::Char('k') if modifiers.contains(KeyModifiers::CONTROL) => app.move_selection_up(),
        KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_selection_down()
        }
        KeyCode::Char('p') if modifiers.contains(KeyModifiers::CONTROL) => app.move_selection_up(),
        KeyCode::Char(c) => {
            if let Some(ref mut s) = app.search {
                s.push(c);
            }
            app.clamp_selection();
        }
        _ => {}
    }
}

fn handle_bulk_upload_keys(
    app: &mut App,
    key: KeyCode,
    bg_tx: &UnboundedSender<BackgroundMessage>,
    config: &AppConfig,
) {
    let state = match app.bulk_upload_state.clone() {
        Some(s) => s,
        None => return,
    };

    match state {
        BulkUploadState::PathInput { mut path, loading } => match key {
            KeyCode::Esc => app.bulk_upload_state = None,
            KeyCode::Enter => {
                if loading {
                    return;
                }
                let trimmed = widgets::form::text(&path).trim().to_string();
                if trimmed.is_empty() {
                    app.flash = Some("CSV path is required".to_string());
                    return;
                }
                app.bulk_upload_state = Some(BulkUploadState::PathInput {
                    path: widgets::form::editor(&trimmed).into(),
                    loading: true,
                });
                let context = build_bulk_upload_context(app);
                spawn_bulk_upload_preview(bg_tx, trimmed, context);
            }
            _ if !loading => {
                widgets::form::input(&mut path, key, KeyModifiers::NONE, false);
                app.bulk_upload_state = Some(BulkUploadState::PathInput { path, loading });
            }
            _ => {}
        },
        BulkUploadState::Preview {
            preview,
            mut selected,
        } => match key {
            KeyCode::Esc => app.bulk_upload_state = None,
            KeyCode::Char('j') | KeyCode::Down => {
                if selected + 1 < preview.rows.len() {
                    selected += 1;
                }
                app.bulk_upload_state = Some(BulkUploadState::Preview { preview, selected });
            }
            KeyCode::Char('k') | KeyCode::Up => {
                selected = selected.saturating_sub(1);
                app.bulk_upload_state = Some(BulkUploadState::Preview { preview, selected });
            }
            KeyCode::Char('r') => {
                app.bulk_upload_state = Some(BulkUploadState::PathInput {
                    path: widgets::form::editor(&preview.source_path).into(),
                    loading: true,
                });
                let context = build_bulk_upload_context(app);
                spawn_bulk_upload_preview(bg_tx, preview.source_path, context);
            }
            KeyCode::Enter | KeyCode::Char('y') => {
                if !preview.can_submit() {
                    app.flash = Some(
                        "Upload blocked: fix invalid rows in the CSV and reload preview"
                            .to_string(),
                    );
                    return;
                }
                app.bulk_upload_state = Some(BulkUploadState::Running {
                    preview: preview.clone(),
                });
                spawn_bulk_upload_execution(bg_tx, preview, config.jira.project.clone());
            }
            _ => {}
        },
        BulkUploadState::Running { .. } => {
            if key == KeyCode::Esc {
                app.bulk_upload_state = None;
            }
        }
        BulkUploadState::Result { summary } => match key {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => app.bulk_upload_state = None,
            KeyCode::Char('r') => {
                app.bulk_upload_state = Some(BulkUploadState::PathInput {
                    path: widgets::form::editor(&summary.source_path).into(),
                    loading: true,
                });
                let context = build_bulk_upload_context(app);
                spawn_bulk_upload_preview(bg_tx, summary.source_path, context);
            }
            _ => {}
        },
    }
}

async fn handle_create_ticket_keys(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    bg_tx: &UnboundedSender<BackgroundMessage>,
    config: &AppConfig,
) {
    use widgets::form;
    let assignees = widgets::create_ticket::build_assignee_options(app);
    let epics = widgets::create_ticket::build_epic_options(app);
    let Some(state) = app.create_ticket.as_mut() else {
        return;
    };
    match key {
        KeyCode::Esc => app.create_ticket = None,
        KeyCode::Tab => state.focused_field = (state.focused_field + 1) % 6,
        KeyCode::BackTab => state.focused_field = (state.focused_field + 5) % 6,
        KeyCode::Enter if modifiers.contains(KeyModifiers::SHIFT) => {
            if state.focused_field == 5 {
                state.description.insert_newline();
            }
        }
        KeyCode::Enter => {
            let summary = form::text(&state.summary);
            if summary.trim().is_empty() {
                app.flash = Some("Summary is required".into());
                return;
            }
            if !form::matching(&assignees, &state.assignee_search).contains(&state.assignee_idx)
                || !form::matching(&epics, &state.epic_search).contains(&state.epic_idx)
            {
                app.flash =
                    Some("Choose a matching assignee and epic, or clear their search".into());
                return;
            }
            let issue_type = app::ISSUE_TYPES[state.issue_type_idx].to_string();
            let assignee = state
                .assignee_idx
                .checked_sub(1)
                .and_then(|i| app.cache.team_members.get(i))
                .map(|member| member.email.clone());
            let epic = state
                .epic_idx
                .checked_sub(1)
                .and_then(|i| app.cache.epics.get(i))
                .map(|epic| epic.key.clone());
            let description = form::text(&state.description);
            let labels: Vec<String> = form::text(&state.labels)
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            app.create_ticket = None;
            app.flash = Some("Creating ticket...".into());
            let project = config.jira.project.clone();
            let tx = bg_tx.clone();
            tokio::spawn(async move {
                let result = jira_client::create_ticket_with_fields(
                    &project,
                    &issue_type,
                    &summary,
                    assignee.as_deref(),
                    epic.as_deref(),
                    Some(&description),
                    Some(&labels),
                )
                .await
                .map_err(|e| e.to_string());
                let _ = tx.send(BackgroundMessage::TicketCreated(result));
            });
        }
        _ => match state.focused_field {
            0 if matches!(key, KeyCode::Char('j') | KeyCode::Down) => {
                state.issue_type_idx = (state.issue_type_idx + 1).min(app::ISSUE_TYPES.len() - 1)
            }
            0 if matches!(key, KeyCode::Char('k') | KeyCode::Up) => {
                state.issue_type_idx = state.issue_type_idx.saturating_sub(1)
            }
            1 => form::input(&mut state.summary, key, modifiers, false),
            2 => form::choose(
                &assignees,
                &mut state.assignee_idx,
                &mut state.assignee_search,
                key,
            ),
            3 => form::choose(&epics, &mut state.epic_idx, &mut state.epic_search, key),
            4 => form::input(&mut state.labels, key, modifiers, false),
            5 => form::input(&mut state.description, key, modifiers, true),
            _ => {}
        },
    }
}

fn handle_comment_keys(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    bg_tx: &UnboundedSender<BackgroundMessage>,
) {
    let Some(state) = app.comment_state.as_mut() else {
        return;
    };
    match key {
        KeyCode::Esc => app.comment_state = None,
        KeyCode::Enter if !modifiers.contains(KeyModifiers::SHIFT) => {
            let body = widgets::form::text(&state.body);
            if body.trim().is_empty() {
                app.flash = Some("Comment body is required".into());
                return;
            }
            let ticket_key = state.ticket_key.clone();
            app.comment_state = None;
            app.flash = Some(format!("Adding comment to {}...", ticket_key));
            let tx = bg_tx.clone();
            tokio::spawn(async move {
                let result = jira_client::add_comment(&ticket_key, &body)
                    .await
                    .map(|_| ticket_key)
                    .map_err(|e| e.to_string());
                let _ = tx.send(BackgroundMessage::CommentAdded(result));
            });
        }
        _ => widgets::form::input(&mut state.body, key, modifiers, true),
    }
}

fn handle_assign_keys(app: &mut App, key: KeyCode, bg_tx: &UnboundedSender<BackgroundMessage>) {
    let options = app
        .cache
        .team_members
        .iter()
        .map(|m| format!("{} ({})", m.name, m.email))
        .collect::<Vec<_>>();
    let Some(state) = app.assign_state.as_mut() else {
        return;
    };
    match key {
        KeyCode::Esc => app.assign_state = None,
        KeyCode::Enter => {
            if !widgets::form::matching(&options, &state.search).contains(&state.selected) {
                app.flash = Some("No matching assignee".into());
                return;
            }
            let Some(member) = app.cache.team_members.get(state.selected) else {
                return;
            };
            let (email, name) = (member.email.clone(), member.name.clone());
            let ticket_key = state.ticket_key.clone();
            app.update_ticket_assignee(&ticket_key, &name, &email);
            app.assign_state = None;
            app.flash = Some(format!("Assigning {} to {}...", ticket_key, name));
            let tx = bg_tx.clone();
            tokio::spawn(async move {
                let result = jira_client::assign_ticket(&ticket_key, &email)
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx.send(BackgroundMessage::TicketAssigned {
                    key: ticket_key,
                    result,
                });
            });
        }
        _ => widgets::form::choose(&options, &mut state.selected, &mut state.search, key),
    }
}

fn handle_edit_keys(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    bg_tx: &UnboundedSender<BackgroundMessage>,
) {
    let Some(state) = app.edit_state.as_mut() else {
        return;
    };
    match key {
        KeyCode::Esc => app.edit_state = None,
        KeyCode::Tab => state.focused_field = (state.focused_field + 1) % 3,
        KeyCode::BackTab => state.focused_field = (state.focused_field + 2) % 3,
        KeyCode::Enter if modifiers.contains(KeyModifiers::SHIFT) => {
            if state.focused_field == 2 {
                state.description.insert_newline();
            }
        }
        KeyCode::Enter => {
            let key = state.ticket_key.clone();
            let summary = widgets::form::text(&state.summary);
            if summary.trim().is_empty() {
                app.flash = Some("Summary is required".into());
                return;
            }
            let labels: Vec<String> = widgets::form::text(&state.labels)
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            let description = widgets::form::text(&state.description);
            // ponytail: jira-cli omits empty bodies; enable clearing when its edit command supports them.
            if description.is_empty()
                && app
                    .find_ticket(&key)
                    .and_then(|ticket| ticket.description.as_deref())
                    .is_some_and(|body| !body.is_empty())
            {
                app.flash = Some("Use the ticket's browser action to clear its description".into());
                return;
            }
            app.update_ticket_fields(&key, &summary, &labels);
            app.update_ticket_description(&key, &description);
            app.edit_state = None;
            app.flash = Some(format!("Updating {}...", key));
            let tx = bg_tx.clone();
            tokio::spawn(async move {
                let result = jira_client::edit_ticket(
                    &key,
                    Some(&summary),
                    if labels.is_empty() {
                        None
                    } else {
                        Some(&labels)
                    },
                    Some(&description),
                )
                .await
                .map_err(|e| e.to_string());
                let _ = tx.send(BackgroundMessage::TicketEdited { key, result });
            });
        }
        _ => match state.focused_field {
            0 => widgets::form::input(&mut state.summary, key, modifiers, false),
            1 => widgets::form::input(&mut state.labels, key, modifiers, false),
            _ => widgets::form::input(&mut state.description, key, modifiers, true),
        },
    }
}

fn handle_filter_edit_keys(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    config: &mut AppConfig,
) {
    let Some(state) = app.filter_edit.as_mut() else {
        return;
    };
    match key {
        KeyCode::Esc => app.filter_edit = None,
        KeyCode::Tab | KeyCode::BackTab => state.focused_field = 1 - state.focused_field,
        KeyCode::Enter => {
            let name = widgets::form::text(&state.name).trim().to_string();
            let jql = widgets::form::text(&state.jql).trim().to_string();
            if name.is_empty() || jql.is_empty() {
                app.flash = Some("Both name and JQL are required".into());
                return;
            }
            let filter = crate::config::SavedFilter { name, jql };
            if let Some(idx) = state.editing_idx {
                if idx < config.filters.len() {
                    config.filters[idx] = filter;
                }
            } else {
                config.filters.push(filter);
                app.filter_sidebar_idx = config.filters.len() - 1;
            }
            app.flash = Some(match crate::config::save_config(config) {
                Ok(()) => "Filter saved".into(),
                Err(e) => format!("Failed to save filter: {}", e),
            });
            app.filter_edit = None;
        }
        _ => widgets::form::input(
            if state.focused_field == 0 {
                &mut state.name
            } else {
                &mut state.jql
            },
            key,
            modifiers,
            false,
        ),
    }
}

fn handle_filter_keys(
    app: &mut App,
    key: KeyCode,
    bg_tx: &UnboundedSender<BackgroundMessage>,
    config: &mut AppConfig,
) {
    match key {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('S') => app.settings = Some(settings::Settings::new(config)),
        KeyCode::Char('?') => app.toggle_keybindings(),
        KeyCode::Tab => {
            if app.filter_focus == FilterFocus::Sidebar {
                if !app.filter_results.is_empty() {
                    app.filter_focus = FilterFocus::Results;
                    app.selected_index = 0;
                } else {
                    app.next_tab();
                }
            } else {
                app.next_tab();
            }
        }
        KeyCode::BackTab => {
            if app.filter_focus == FilterFocus::Results {
                app.filter_focus = FilterFocus::Sidebar;
            }
        }
        KeyCode::Char('n') => {
            app.filter_edit = Some(app::FilterEditState {
                focused_field: 0,
                name: widgets::form::editor(""),
                jql: widgets::form::editor(""),
                editing_idx: None,
            });
        }
        KeyCode::Char('e') => {
            if app.filter_focus == FilterFocus::Sidebar {
                if let Some(filter) = config.filters.get(app.filter_sidebar_idx) {
                    app.filter_edit = Some(app::FilterEditState {
                        focused_field: 0,
                        name: widgets::form::editor(&filter.name),
                        jql: widgets::form::editor(&filter.jql),
                        editing_idx: Some(app.filter_sidebar_idx),
                    });
                }
            }
        }
        KeyCode::Char('x') => {
            if app.filter_focus == FilterFocus::Sidebar
                && app.filter_sidebar_idx < config.filters.len()
            {
                let removed_name = config.filters.remove(app.filter_sidebar_idx).name;
                match crate::config::save_config(config) {
                    Ok(()) => {
                        app.flash = Some(format!("Deleted filter '{}'", removed_name));
                        if app.filter_sidebar_idx > 0
                            && app.filter_sidebar_idx >= config.filters.len()
                        {
                            app.filter_sidebar_idx = config.filters.len().saturating_sub(1);
                        }
                    }
                    Err(e) => {
                        app.flash = Some(format!("Failed to delete filter: {}", e));
                    }
                }
            }
        }
        KeyCode::Char('U') => {
            app.bulk_upload_state = Some(BulkUploadState::PathInput {
                path: widgets::form::editor("").into(),
                loading: false,
            });
        }
        KeyCode::Char(' ') => {
            if app.filter_focus == FilterFocus::Results {
                app.toggle_selection_at_cursor();
            }
        }
        KeyCode::Char('A') => {
            if app.filter_focus == FilterFocus::Results {
                app.select_all_visible_tickets();
                app.flash = Some(format!(
                    "Selected {} tickets",
                    app.selected_visible_ticket_keys_in_order().len()
                ));
            }
        }
        KeyCode::Char('u') => {
            if app.filter_focus == FilterFocus::Results {
                app.clear_selected_tickets();
                app.flash = Some("Selection cleared".to_string());
            }
        }
        KeyCode::Char('B') => {
            if app.filter_focus == FilterFocus::Results {
                bulk_actions::open(app);
            }
        }
        KeyCode::Char('z') => {
            if app.filter_focus == FilterFocus::Results {
                app.toggle_fold_at_cursor();
            }
        }
        KeyCode::Char('Z') => {
            if app.filter_focus == FilterFocus::Results {
                app.toggle_all_groups_collapse();
            }
        }
        KeyCode::Char('j') | KeyCode::Down => match app.filter_focus {
            FilterFocus::Sidebar => {
                if !config.filters.is_empty() && app.filter_sidebar_idx < config.filters.len() - 1 {
                    app.filter_sidebar_idx += 1;
                }
            }
            FilterFocus::Results => app.move_selection_down(),
        },
        KeyCode::Char('k') | KeyCode::Up => match app.filter_focus {
            FilterFocus::Sidebar => {
                app.filter_sidebar_idx = app.filter_sidebar_idx.saturating_sub(1);
            }
            FilterFocus::Results => app.move_selection_up(),
        },
        KeyCode::Enter => match app.filter_focus {
            FilterFocus::Sidebar => {
                // Run the selected filter
                if let Some(filter) = config.filters.get(app.filter_sidebar_idx) {
                    app.filter_loading = true;
                    app.filter_results.clear();
                    app.collapsed_filters.clear();
                    app.mark_cache_changed();
                    app.flash = Some(format!("Running filter '{}'...", filter.name));

                    let tx = bg_tx.clone();
                    let cfg = config.clone();
                    let jql = filter.jql.clone();
                    let requested_at = app.moves.now();
                    tokio::spawn(async move {
                        let result = jira_reads::fetch_jql_query(&cfg, &jql)
                            .await
                            .map_err(|e| describe(&e));
                        let _ = tx.send(BackgroundMessage::FilterResults {
                            requested_at,
                            result,
                        });
                    });
                }
            }
            FilterFocus::Results => {
                if let Some(group_id) = app.selected_header_group_id() {
                    if app.is_collapsed(Tab::Filters, &group_id) {
                        app.toggle_group_collapse(&group_id);
                    }
                } else if let Some(key) = app.selected_ticket_key() {
                    if let Some(key) = app.open_fresh_detail(key) {
                        spawn_ticket_detail_fetch(bg_tx, key, app.moves.now());
                    }
                }
            }
        },
        KeyCode::Char('/') => app.search = Some(String::new()),
        KeyCode::Char('r') => {
            if app.loading {
                app.flash = Some("Refresh already in progress".to_string());
            } else {
                app.loading = true;
                app.ticket_sync_stage = None;
                app.flash = Some("Refreshing tickets...".to_string());
                spawn_cache_refresh(app, bg_tx, CacheRefreshPhase::Manual, config);
            }
        }
        _ => {}
    }
}

async fn handle_main_keys(
    app: &mut App,
    key: KeyCode,
    _modifiers: KeyModifiers,
    bg_tx: &UnboundedSender<BackgroundMessage>,
    config: &AppConfig,
) {
    match key {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('S') => app.settings = Some(settings::Settings::new(config)),
        KeyCode::Tab => app.next_tab(),
        KeyCode::BackTab => {
            let index = (app.active_tab.index() + Tab::all().len() - 1) % Tab::all().len();
            app.switch_tab(Tab::all()[index]);
        }
        KeyCode::Char('j') | KeyCode::Down => app.move_selection_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection_up(),
        KeyCode::Char(' ') => app.toggle_selection_at_cursor(),
        KeyCode::Char('A') => {
            app.select_all_visible_tickets();
            app.flash = Some(format!(
                "Selected {} tickets",
                app.selected_visible_ticket_keys_in_order().len()
            ));
        }
        KeyCode::Char('u') => {
            app.clear_selected_tickets();
            app.flash = Some("Selection cleared".to_string());
        }
        KeyCode::Char('B') => bulk_actions::open(app),
        KeyCode::Char('/') => app.search = Some(String::new()),
        KeyCode::Char('?') => app.toggle_keybindings(),
        KeyCode::Char('d') => {
            app.toggle_show_done();
            app.flash = Some(if app.show_done {
                "Showing Done tickets".to_string()
            } else {
                "Hiding Done tickets".to_string()
            });
        }
        KeyCode::Char(c @ ('f' | 'F')) => {
            if matches!(app.active_tab, Tab::MyWork | Tab::Team) {
                app.cycle_status_focus(c == 'f');
                app.flash = Some(app.status_focus_message());
            } else {
                app.flash = Some("Status focus works in My Work and Team".to_string());
            }
        }
        KeyCode::Char('r') => {
            if app.loading {
                app.flash = Some("Refresh already in progress".to_string());
            } else {
                app.loading = true;
                app.ticket_sync_stage = None;
                app.flash = Some("Refreshing tickets...".to_string());
                spawn_cache_refresh(app, bg_tx, CacheRefreshPhase::Manual, config);
            }
        }
        KeyCode::Char('z') => app.toggle_fold_at_cursor(),
        KeyCode::Char('Z') => {
            app.toggle_all_groups_collapse();
        }
        KeyCode::Char('c') => {
            app.create_ticket = Some(app::CreateTicketState {
                focused_field: 0,
                issue_type_idx: 0,
                summary: widgets::form::editor(""),
                labels: widgets::form::editor(""),
                description: widgets::form::editor(""),
                assignee_search: String::new(),
                epic_search: String::new(),
                assignee_idx: 0,
                epic_idx: 0,
            });
        }
        KeyCode::Char('U') => {
            app.bulk_upload_state = Some(BulkUploadState::PathInput {
                path: widgets::form::editor("").into(),
                loading: false,
            });
        }
        KeyCode::Enter => {
            if let Some(group_id) = app.selected_header_group_id() {
                if app.active_tab == Tab::Epics {
                    app.open_epic_detail(group_id);
                } else if app.is_collapsed(app.active_tab, &group_id) {
                    app.toggle_group_collapse(&group_id);
                }
            } else if let Some(key) = app.selected_ticket_key() {
                if let Some(key) = app.open_fresh_detail(key) {
                    spawn_ticket_detail_fetch(bg_tx, key, app.moves.now());
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bulk_actions::BulkState;
    use std::collections::BTreeMap;

    #[test]
    fn dev_mode_prefers_the_active_checkout_over_the_installed_git_source() {
        let root = std::env::temp_dir().join(format!("lazyjira-dev-path-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let checkout = root.join("checkout");
        let nested = checkout.join("src/widgets");
        let compiled = root.join("cargo/git/old-release");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&compiled).unwrap();
        let local_manifest = checkout.join("Cargo.toml");
        let compiled_manifest = compiled.join("Cargo.toml");
        std::fs::write(&compiled_manifest, "[package]\nname = 'lazyjira'\n").unwrap();
        std::fs::write(&local_manifest, "[package]\nname = 'different-project'\n").unwrap();
        assert_eq!(dev_manifest_path(&nested, &compiled), compiled_manifest);
        std::fs::write(&local_manifest, "[package]\nname = 'lazyjira'\n").unwrap();
        assert_eq!(dev_manifest_path(&checkout, &compiled), local_manifest);
        assert_eq!(dev_manifest_path(&nested, &compiled), local_manifest);
        std::fs::write(&local_manifest, "invalid TOML").unwrap();
        assert_eq!(dev_manifest_path(&nested, &compiled), compiled_manifest);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn sample_config() -> AppConfig {
        AppConfig {
            jira: crate::config::JiraConfig {
                project: "DEMO".to_string(),
                team_name: "Payments Platform".to_string(),
                done_window_days: 14,
                epics_i_care_about: vec![],
            },
            team: BTreeMap::new(),
            statuses: crate::config::StatusConfig::default(),
            filters: vec![],
            preferences: Default::default(),
            themes: Default::default(),
        }
    }

    #[tokio::test]
    async fn forms_render_after_resizing_and_keep_actions_visible_at_normal_sizes() {
        use ratatui::{backend::TestBackend, Terminal};
        let config = sample_config();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        for (width, height) in [(160, 55), (80, 24), (40, 18), (8, 4)] {
            let mut app = App::new();
            app.loading = false;
            handle_main_keys(
                &mut app,
                KeyCode::Char('c'),
                KeyModifiers::NONE,
                &tx,
                &config,
            )
            .await;
            for modal in ["Create", "Comment", "Preferences"] {
                if modal == "Comment" {
                    app.create_ticket = None;
                    app.comment_state = Some(app::CommentState {
                        ticket_key: "DEMO-1".into(),
                        body: widgets::form::editor("First line\nSecond line"),
                    });
                } else if modal == "Preferences" {
                    app.comment_state = None;
                    app.settings = Some(settings::Settings::new(&config));
                }
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|f| ui(f, &app, &config)).unwrap();
                if width >= 80 {
                    let text: String = terminal
                        .backend()
                        .buffer()
                        .content
                        .iter()
                        .map(|cell| cell.symbol())
                        .collect();
                    assert!(
                        text.contains("[Cancel]"),
                        "{modal} at {width}x{height}: {text}"
                    );
                    assert!(
                        text.contains("[Editor]"),
                        "{modal} at {width}x{height}: {text}"
                    );
                }
            }
        }
    }

    /// The most calls `run_bounded` has going at once over `count` items that each take a moment.
    async fn most_at_once(count: usize) -> usize {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        use std::sync::Arc;

        let running = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let (r, m) = (running.clone(), most.clone());
        let results = run_bounded((0..count).collect(), move |n| {
            let (running, most) = (r.clone(), m.clone());
            async move {
                most.fetch_max(running.fetch_add(1, SeqCst) + 1, SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                running.fetch_sub(1, SeqCst);
                (format!("DEMO-{n}"), Ok::<(), String>(()))
            }
        })
        .await;
        assert_eq!(results.len(), count);
        most.load(SeqCst)
    }

    #[tokio::test(start_paused = true)]
    async fn a_bulk_action_makes_six_jira_calls_at_a_time() {
        assert_eq!(most_at_once(5).await, 5);
        assert_eq!(most_at_once(6).await, 6);
        // The seventh waits for one of the six to finish.
        assert_eq!(most_at_once(7).await, 6);
    }

    #[test]
    fn a_multi_line_error_reads_as_one_line_in_the_status_bar() {
        use ratatui::backend::TestBackend;

        let mut app = App::new();
        app.flash = Some(
            "Refresh failed: Jira answered 400 Bad Request.\nError in the JQL Query".to_string(),
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();

        terminal.draw(|f| ui(f, &app, &sample_config())).unwrap();

        let buffer = terminal.backend().buffer();
        let text: String = (0..100).map(|x| buffer[(x, 11)].symbol()).collect();
        assert!(
            text.contains("400 Bad Request.; Error in the JQL Query"),
            "{text}"
        );
    }

    #[test]
    fn main_footer_highlights_shortcuts_and_keeps_status_indicators_muted() {
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;

        let mut app = App::new();
        app.ticket_sync_stage = Some(TicketSyncStage::Full);
        app.cache_stale_age_secs = Some(180);
        app.epics_refreshing = true;
        app.status_focus = Some("In Progress".to_string());
        let mut terminal = Terminal::new(TestBackend::new(300, 12)).unwrap();

        for tab in Tab::all() {
            app.active_tab = *tab;
            terminal.draw(|f| ui(f, &app, &sample_config())).unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = (0..300).map(|x| buffer[(x, 11)].symbol()).collect();
            let mut expected = vec![
                ("j/k", Color::Cyan),
                ("Space", Color::Cyan),
                ("Enter", Color::Cyan),
                ("q: quit", Color::Cyan),
                ("navigate", Color::DarkGray),
                ("sel:0", Color::DarkGray),
            ];
            if *tab == Tab::Filters {
                expected.extend([("Tab/S-Tab", Color::Cyan), ("z/Z", Color::Cyan)]);
            } else {
                expected.extend([
                    ("Tab", Color::Cyan),
                    ("f/F", Color::Cyan),
                    ("focus(In Progress)", Color::DarkGray),
                    ("t:sync-full", Color::DarkGray),
                    ("c:stale 3m", Color::DarkGray),
                    ("e:syncing", Color::DarkGray),
                ]);
            }
            for (token, color) in expected {
                let x = text
                    .find(token)
                    .unwrap_or_else(|| panic!("missing {token}: {text}"));
                assert_eq!(buffer[(x as u16, 11)].fg, color, "{token}");
            }
        }
    }

    fn ticket(key: &str, summary: &str, status: &str) -> crate::cache::Ticket {
        crate::cache::Ticket {
            summary: summary.to_string(),
            ..crate::cache::Ticket::for_test(key, status)
        }
    }

    #[tokio::test]
    async fn keybindings_regression_main_navigation_still_works() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::MyWork;
        app.cache.my_tickets = vec![ticket("DEMO-1", "A", "In Progress")];
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_main_keys(
            &mut app,
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            &tx,
            &sample_config(),
        )
        .await;
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn presets_theme_every_cell_on_every_tab_and_overlay() {
        use ratatui::{backend::TestBackend, style::Color};
        let mut config = sample_config();
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = [
            "In Progress",
            "Ready for Work",
            "Blocked",
            "In Review",
            "Done",
            "Odd",
        ]
        .iter()
        .enumerate()
        .map(|(i, status)| {
            let mut ticket = ticket(&format!("DEMO-{i}"), "Summary", status);
            ticket.description =
                Some("*bold* [link|https://example.com] {color:red}red{color} @alex".into());
            ticket.labels = vec!["label".into()];
            ticket
        })
        .collect();
        for (name, theme) in theme::PRESETS {
            config.preferences.theme = name.to_string();
            app.theme = *theme;
            for overlay in 0..4 {
                for tab in Tab::all() {
                    app.active_tab = *tab;
                    // A ticket row, so the selected row's colors are drawn too.
                    app.selected_index = 1;
                    app.close_detail();
                    app.show_keybindings = overlay == 1;
                    app.settings = (overlay == 2).then(|| settings::Settings::new(&config));
                    if overlay == 3 {
                        app.open_detail("DEMO-0".into());
                    }
                    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
                    terminal.draw(|f| ui(f, &app, &config)).unwrap();
                    for cell in &terminal.backend().buffer().content {
                        assert!(
                            matches!((cell.fg, cell.bg), (Color::Rgb(..), Color::Rgb(..))),
                            "{name}, {tab:?}, overlay {overlay}: {cell:?} kept a terminal color"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn keybindings_help_adapts_scrolls_and_keeps_close_visible() {
        use ratatui::backend::TestBackend;
        use ratatui::buffer::Buffer;
        use ratatui::style::Color;

        fn draw(app: &App, width: u16, height: u16) -> (Buffer, Vec<String>) {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| ui(f, app, &sample_config())).unwrap();
            let buffer = terminal.backend().buffer().clone();
            let lines = (0..height)
                .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
                .collect();
            (buffer, lines)
        }

        let mut app = App::new();
        app.toggle_keybindings();
        let (buffer, wide) = draw(&app, 160, 55);
        let heading = wide
            .iter()
            .position(|line| line.contains("Navigation"))
            .unwrap();
        assert!(
            wide[heading].contains("Detail navigation"),
            "two columns on wide terminals"
        );
        assert!(wide
            .iter()
            .any(|line| line.contains("New / edit / delete filter")));
        assert!(wide.iter().any(|line| line.contains("Esc/? close")));
        assert_eq!(app.keybindings_scroll_max.get(), 0);
        let key_row = wide
            .iter()
            .position(|line| line.contains("Next tab"))
            .unwrap();
        let key_x = wide[key_row].find("Tab").unwrap();
        assert_eq!(buffer[(key_x as u16, key_row as u16)].fg, Color::Cyan);

        for width in [80, 40] {
            let (_, lines) = draw(&app, width, 24);
            assert!(app.keybindings_scroll_max.get() > 0);
            assert!(lines.iter().any(|line| line.contains("Esc/? close")));
            handle_keybindings_keys(&mut app, KeyCode::PageDown);
            assert_eq!(app.keybindings_scroll, app.keybindings_page_height.get());
            handle_keybindings_keys(&mut app, KeyCode::End);
            let (_, bottom) = draw(&app, width, 24);
            assert!(bottom
                .iter()
                .any(|line| line.contains("New / edit / delete filter")));
            assert!(bottom.iter().any(|line| line.contains("Esc/? close")));
            handle_keybindings_keys(&mut app, KeyCode::Home);
        }
        handle_keybindings_keys(&mut app, KeyCode::Down);
        assert_eq!(app.keybindings_scroll, 1);
        handle_keybindings_keys(&mut app, KeyCode::Up);
        assert_eq!(app.keybindings_scroll, 0);
        handle_keybindings_keys(&mut app, KeyCode::End);
        draw(&app, 160, 55); // Resizing clamps a stale scroll offset.
        handle_keybindings_keys(&mut app, KeyCode::Down);
        assert_eq!(app.keybindings_scroll, 0);
        handle_keybindings_keys(&mut app, KeyCode::Esc);
        assert!(!app.show_keybindings);
        app.toggle_keybindings();
        assert_eq!(app.keybindings_scroll, 0);
        draw(&app, 8, 4); // Tiny terminals must not panic.
    }

    #[test]
    fn comment_shift_enter_inserts_newline() {
        let mut app = App::new();
        app.comment_state = Some(crate::app::CommentState {
            ticket_key: "DEMO-1".to_string(),
            body: widgets::form::editor("hello"),
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        handle_comment_keys(&mut app, KeyCode::Enter, KeyModifiers::SHIFT, &tx);

        let state = app.comment_state.expect("comment modal should remain open");
        assert_eq!(state.body.lines().join("\n"), "hello\n");
    }

    #[test]
    fn comment_enter_requires_non_empty_body() {
        let mut app = App::new();
        app.comment_state = Some(crate::app::CommentState {
            ticket_key: "DEMO-1".to_string(),
            body: widgets::form::editor("   "),
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        handle_comment_keys(&mut app, KeyCode::Enter, KeyModifiers::NONE, &tx);

        assert!(app.comment_state.is_some());
        assert_eq!(app.flash.as_deref(), Some("Comment body is required"));
    }

    #[test]
    fn comment_ctrl_j_inserts_newline() {
        let mut app = App::new();
        app.comment_state = Some(crate::app::CommentState {
            ticket_key: "DEMO-1".to_string(),
            body: widgets::form::editor("hello"),
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        handle_comment_keys(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL, &tx);

        let state = app.comment_state.expect("comment modal should remain open");
        assert_eq!(state.body.lines().join("\n"), "hello\n");
    }

    #[tokio::test]
    async fn bulk_menu_falls_back_to_current_ticket_when_nothing_selected() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::MyWork;
        app.cache.my_tickets = vec![ticket("DEMO-1", "A", "In Progress")];
        app.selected_index = 1; // current ticket row

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_main_keys(
            &mut app,
            KeyCode::Char('B'),
            KeyModifiers::NONE,
            &tx,
            &sample_config(),
        )
        .await;

        match app.bulk_state {
            Some(BulkState::ActionPicker { targets, .. }) => {
                assert_eq!(targets, vec!["DEMO-1".to_string()]);
            }
            _ => panic!("expected bulk action picker"),
        }
    }

    #[tokio::test]
    async fn uppercase_u_opens_bulk_upload_from_main_tabs() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::MyWork;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_main_keys(
            &mut app,
            KeyCode::Char('U'),
            KeyModifiers::NONE,
            &tx,
            &sample_config(),
        )
        .await;

        assert!(matches!(
            app.bulk_upload_state,
            Some(BulkUploadState::PathInput { .. })
        ));
    }

    #[tokio::test]
    async fn enter_on_epic_header_opens_epic_detail_in_main_mode() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        app.cache.epics = vec![crate::cache::Epic {
            key: "DEMO-500".to_string(),
            summary: "Epic Header".to_string(),
            children: vec![],
        }];
        app.selected_index = 0;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_main_keys(
            &mut app,
            KeyCode::Enter,
            KeyModifiers::NONE,
            &tx,
            &sample_config(),
        )
        .await;

        assert_eq!(app.detail_epic_key.as_deref(), Some("DEMO-500"));
        assert!(app.detail_ticket_key.is_none());
    }

    #[tokio::test]
    async fn enter_on_epic_header_opens_epic_detail_in_search_mode() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        app.search = Some(String::new());
        app.cache.epics = vec![crate::cache::Epic {
            key: "DEMO-501".to_string(),
            summary: "Epic Header Search".to_string(),
            children: vec![],
        }];
        app.selected_index = 0;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_search_keys(&mut app, KeyCode::Enter, KeyModifiers::NONE, &tx).await;

        assert_eq!(app.detail_epic_key.as_deref(), Some("DEMO-501"));
        assert!(app.detail_ticket_key.is_none());
    }

    #[test]
    fn uppercase_u_opens_bulk_upload_from_filters() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();
        handle_filter_keys(&mut app, KeyCode::Char('U'), &tx, &mut config);
        assert!(matches!(
            app.bulk_upload_state,
            Some(BulkUploadState::PathInput { .. })
        ));
    }

    #[test]
    fn z_toggles_current_filter_group_from_results() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        app.filter_focus = FilterFocus::Results;
        app.filter_results = vec![
            ticket("DEMO-70", "Grouped", "In Progress"),
            ticket("DEMO-71", "Grouped too", "In Progress"),
        ];
        app.mark_cache_changed();
        app.selected_index = 1;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();
        handle_filter_keys(&mut app, KeyCode::Char('z'), &tx, &mut config);

        assert!(app.collapsed_filters.contains("In Progress"));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn uppercase_z_toggles_all_filter_groups() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        app.filter_focus = FilterFocus::Results;
        app.filter_results = vec![
            ticket("DEMO-72", "In progress", "In Progress"),
            ticket("DEMO-73", "Ready", "Ready for Work"),
        ];
        app.mark_cache_changed();

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();
        handle_filter_keys(&mut app, KeyCode::Char('Z'), &tx, &mut config);

        assert!(app.collapsed_filters.contains("Ready for Work"));
        assert!(!app.collapsed_filters.contains("In Progress"));

        handle_filter_keys(&mut app, KeyCode::Char('Z'), &tx, &mut config);
        assert!(app.collapsed_filters.is_empty());
    }

    #[test]
    fn enter_on_collapsed_filter_header_expands_group() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        app.filter_focus = FilterFocus::Results;
        app.filter_results = vec![ticket("DEMO-74", "Grouped", "In Progress")];
        app.collapsed_filters.insert("In Progress".to_string());
        app.mark_cache_changed();
        app.selected_index = 0;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();
        handle_filter_keys(&mut app, KeyCode::Enter, &tx, &mut config);

        assert!(!app.collapsed_filters.contains("In Progress"));
        assert!(app.detail_ticket_key.is_none());
    }

    #[tokio::test]
    async fn enter_on_filter_ticket_opens_detail() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        app.filter_focus = FilterFocus::Results;
        let mut ticket = ticket("DEMO-75", "Ticket detail", "In Progress");
        ticket.detail_loaded = true;
        app.filter_results = vec![ticket];
        app.mark_cache_changed();
        app.selected_index = 1;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();
        handle_filter_keys(&mut app, KeyCode::Enter, &tx, &mut config);

        assert_eq!(app.detail_ticket_key.as_deref(), Some("DEMO-75"));
    }

    #[tokio::test]
    async fn move_failure_stays_on_screen_until_dismissed() {
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![ticket("DEMO-2478", "Epic", "Backlog")];
        app.open_detail("DEMO-2478".to_string());
        assert!(app.moves.start("DEMO-2478", "Done"));
        app.finish_move(
            "DEMO-2478",
            Err("Jira answered 400 Bad Request.\nresolution: Resolution is required.".to_string()),
        );
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();

        for code in [KeyCode::Char('m'), KeyCode::Down, KeyCode::Tab] {
            handle_key(&mut app, KeyEvent::from(code), &tx, &mut config).await;
            assert_eq!(app.moves.failures().len(), 1);
        }
        // The popup swallowed those keys instead of the detail view underneath.
        assert!(matches!(app.detail_mode, DetailMode::View));
        assert_eq!(app.detail_scroll, 0);

        handle_key(&mut app, KeyEvent::from(KeyCode::Esc), &tx, &mut config).await;
        assert!(app.moves.failures().is_empty());
        assert!(app.is_detail_open());
        assert_eq!(app.cache.my_tickets[0].status, "Backlog");
    }

    #[test]
    fn bulk_upload_submit_is_blocked_when_preview_has_invalid_rows() {
        let mut app = App::new();
        app.bulk_upload_state = Some(BulkUploadState::Preview {
            preview: BulkUploadPreview {
                source_path: "/tmp/bulk.csv".to_string(),
                rows: vec![crate::app::BulkUploadRow {
                    row_number: 2,
                    issue_type: "Task".to_string(),
                    summary: "".to_string(),
                    assignee_email: None,
                    epic_key: None,
                    labels: vec![],
                    description: None,
                    errors: vec!["summary is required".to_string()],
                    warnings: vec![],
                }],
                total_rows: 1,
                valid_rows: 0,
                invalid_rows: 1,
                warning_count: 0,
            },
            selected: 0,
        });

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_bulk_upload_keys(&mut app, KeyCode::Enter, &tx, &sample_config());

        assert!(matches!(
            app.bulk_upload_state,
            Some(BulkUploadState::Preview { .. })
        ));
        assert!(app
            .flash
            .as_deref()
            .unwrap_or("")
            .contains("Upload blocked"));
    }
}
