mod app;
mod bulk_plan;
mod bulk_upload;
mod cache;
mod config;
mod jira_client;
mod jira_rest;
mod move_picker;
mod moves;
mod setup;
mod transitions;
mod views;
mod widgets;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
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

use crate::bulk_plan::{BulkJob, BulkPlan, FetchedTransitions};
use crate::cache::Status;
use crate::config::AppConfig;
use crate::move_picker::JiraCall;
use app::{
    App, BulkAction, BulkState, BulkSummary, BulkTarget, BulkUploadPreview, BulkUploadState,
    BulkUploadSummary, DetailMode, FilterFocus, Tab, TicketSyncStage,
};

#[derive(Debug, Clone, Copy)]
enum CacheRefreshPhase {
    ActiveOnly,
    Full,
    Manual,
}

/// Results of background work. `requested_at` is `app.moves.now()` when a Jira read was
/// requested, so a read that predates a confirmed move can't undo it.
enum BackgroundMessage {
    EpicsRefreshed {
        requested_at: u64,
        result: std::result::Result<Vec<crate::cache::Epic>, String>,
    },
    CacheRefreshed {
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
    BulkCompleted(BulkSummary),
    BulkUploadPreviewReady(std::result::Result<BulkUploadPreview, String>),
    BulkUploadCompleted(BulkUploadSummary),
    FilterResults {
        requested_at: u64,
        result: std::result::Result<Vec<crate::cache::Ticket>, String>,
    },
}

fn spawn_epics_refresh(
    tx: &UnboundedSender<BackgroundMessage>,
    config: &AppConfig,
    requested_at: u64,
) {
    let tx = tx.clone();
    let config = config.clone();
    tokio::spawn(async move {
        let result = jira_client::refresh_epics_cache(&config)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(BackgroundMessage::EpicsRefreshed {
            requested_at,
            result,
        });
    });
}

fn spawn_cache_refresh(
    tx: &UnboundedSender<BackgroundMessage>,
    phase: CacheRefreshPhase,
    config: &AppConfig,
    requested_at: u64,
) {
    let tx = tx.clone();
    let config = config.clone();
    tokio::spawn(async move {
        let result = match phase {
            CacheRefreshPhase::ActiveOnly => jira_client::fetch_active_only(&config).await,
            CacheRefreshPhase::Full => jira_client::fetch_all(&config).await,
            CacheRefreshPhase::Manual => jira_client::fetch_all(&config).await,
        }
        .map_err(|e| e.to_string());
        let _ = tx.send(BackgroundMessage::CacheRefreshed {
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
        let result = jira_client::fetch_ticket_detail(&key)
            .await
            .map_err(|e| e.to_string());
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
    const MAX_CONCURRENCY: usize = 6;
    if keys.is_empty() {
        return;
    }

    let tx = tx.clone();
    tokio::spawn(async move {
        let mut iter = keys.into_iter();
        let mut tasks = tokio::task::JoinSet::new();

        for _ in 0..MAX_CONCURRENCY {
            if let Some(key) = iter.next() {
                tasks.spawn(async move {
                    let result = jira_client::fetch_ticket_detail(&key)
                        .await
                        .map_err(|e| e.to_string());
                    (key, result)
                });
            }
        }

        while let Some(joined) = tasks.join_next().await {
            if let Ok((key, result)) = joined {
                let _ = tx.send(BackgroundMessage::TicketDetailFetched {
                    key,
                    requested_at,
                    result,
                });
            }

            if let Some(next_key) = iter.next() {
                tasks.spawn(async move {
                    let result = jira_client::fetch_ticket_detail(&next_key)
                        .await
                        .map_err(|e| e.to_string());
                    (next_key, result)
                });
            }
        }
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
            .map_err(|e| format!("{:#}", e));
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
        // `{:#}` keeps the whole error chain, e.g. why Jira could not be reached.
        let result = jira_rest::transition(&key, &transition_id, resolution_id.as_deref())
            .await
            .map_err(|e| format!("{:#}", e));
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

fn summarize_bulk_results(
    action: BulkAction,
    target: BulkTarget,
    results: Vec<(String, std::result::Result<(), String>)>,
    skipped: Vec<(String, String)>,
) -> BulkSummary {
    let attempted = results.len();
    let mut successful_keys = Vec::new();
    let mut failed_details = Vec::new();
    for (key, result) in results {
        match result {
            Ok(()) => successful_keys.push(key),
            Err(err) => failed_details.push((key, err)),
        }
    }
    BulkSummary {
        action,
        target,
        total: attempted + skipped.len(),
        attempted,
        succeeded: successful_keys.len(),
        failed: failed_details.len(),
        successful_keys,
        failed_details,
        skipped,
    }
}

fn apply_bulk_successes(app: &mut App, summary: &BulkSummary) {
    match &summary.target {
        BulkTarget::Move { destination, .. } => {
            for key in &summary.successful_keys {
                app.record_move(key, destination);
            }
        }
        BulkTarget::Assign {
            member_email,
            member_name,
        } => {
            for key in &summary.successful_keys {
                app.update_ticket_assignee(key, member_name, member_email);
            }
        }
    }
    app.clamp_selection();
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
    let mut items = items.into_iter();
    let mut tasks = tokio::task::JoinSet::new();
    for item in items.by_ref().take(MAX_BULK_CONCURRENCY) {
        tasks.spawn(task(item));
    }
    let mut results = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        results.push(joined.unwrap_or_else(|err| ("unknown".to_string(), Err(err.to_string()))));
        if let Some(item) = items.next() {
            tasks.spawn(task(item));
        }
    }
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
                .map_err(|e| format!("{:#}", e));
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
            (key, result.map_err(|e| format!("{:#}", e)))
        })
        .await;
        let summary = summarize_bulk_results(action, target, results, plan.skipped);
        let _ = tx.send(BackgroundMessage::BulkCompleted(summary));
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

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut config = match config::load_config()? {
        Some(config) => config,
        None => setup::run_setup(&mut terminal).await?,
    };

    let mut app = App::new();
    app.set_epics_i_care_about(config.epics_i_care_about_ordered());
    let (bg_tx, mut bg_rx) = tokio::sync::mpsc::unbounded_channel();
    let detail_cache_tx = jira_client::spawn_detail_cache_writer(&config.jira.project);

    // Fast startup: load persisted snapshot immediately, then revalidate in stages.
    if let Some(snapshot) = jira_client::load_startup_cache_snapshot(&config.jira.project) {
        app.replace_cache(snapshot.cache, app.moves.now());
        app.loading = false;
        app.cache_stale_age_secs = Some(snapshot.age_secs);
        app.ticket_sync_stage = Some(TicketSyncStage::ActiveOnly);
        app.flash = Some("Loaded cached data. Refreshing active tickets...".to_string());
        spawn_cache_refresh(
            &bg_tx,
            CacheRefreshPhase::ActiveOnly,
            &config,
            app.moves.now(),
        );
    } else {
        let cache = jira_client::fetch_active_only(&config).await?;
        app.replace_cache(cache, app.moves.now());
        app.loading = false;
        app.ticket_sync_stage = Some(TicketSyncStage::Full);
        app.flash = Some("Loaded active tickets. Syncing recently done...".to_string());
        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::Full, &config, app.moves.now());
    }

    spawn_epics_refresh(&bg_tx, &config, app.moves.now());
    app.epics_refreshing = true;
    queue_detail_prefetch(&mut app, &bg_tx);

    let mut draw_needed = true;

    // Main loop
    loop {
        let mut state_changed = false;
        while let Ok(message) = bg_rx.try_recv() {
            state_changed = true;
            match message {
                BackgroundMessage::EpicsRefreshed {
                    requested_at,
                    result,
                } => {
                    app.epics_refreshing = false;
                    match result {
                        Ok(epics) => {
                            jira_client::attach_epics_to_tickets(
                                &mut app.cache.my_tickets,
                                &mut app.cache.team_tickets,
                                &epics,
                            );
                            app.cache.epics = epics;
                            app.reapply_moves_since(requested_at);
                            app.mark_cache_changed();
                            app.clamp_selection();
                            app.flash = Some("Epic relationships refreshed".to_string());
                        }
                        Err(e) => {
                            app.flash = Some(format!("Epic refresh failed: {}", e));
                        }
                    }
                }
                BackgroundMessage::CacheRefreshed {
                    phase,
                    requested_at,
                    result,
                } => match (phase, result) {
                    (CacheRefreshPhase::ActiveOnly, Ok(cache))
                        if app.ticket_sync_stage == Some(TicketSyncStage::ActiveOnly) =>
                    {
                        app.replace_cache(cache, requested_at);
                        app.cache_stale_age_secs = None;
                        app.ticket_sync_stage = Some(TicketSyncStage::Full);
                        app.clamp_selection();
                        queue_detail_prefetch(&mut app, &bg_tx);
                        app.flash =
                            Some("Active tickets refreshed. Syncing recently done...".to_string());
                        spawn_cache_refresh(
                            &bg_tx,
                            CacheRefreshPhase::Full,
                            &config,
                            app.moves.now(),
                        );
                    }
                    (CacheRefreshPhase::ActiveOnly, Err(e))
                        if app.ticket_sync_stage == Some(TicketSyncStage::ActiveOnly) =>
                    {
                        app.ticket_sync_stage = Some(TicketSyncStage::Full);
                        app.flash = Some(format!(
                            "Active refresh failed ({}). Trying full refresh...",
                            e
                        ));
                        spawn_cache_refresh(
                            &bg_tx,
                            CacheRefreshPhase::Full,
                            &config,
                            app.moves.now(),
                        );
                    }
                    (CacheRefreshPhase::Full, Ok(cache))
                        if app.ticket_sync_stage == Some(TicketSyncStage::Full) =>
                    {
                        app.replace_cache(cache, requested_at);
                        app.cache_stale_age_secs = None;
                        app.ticket_sync_stage = None;
                        app.clamp_selection();
                        queue_detail_prefetch(&mut app, &bg_tx);
                        if let Err(e) =
                            jira_client::save_full_cache_snapshot(&config.jira.project, &app.cache)
                        {
                            app.flash = Some(format!("Cache snapshot write failed: {}", e));
                        } else {
                            app.flash = Some("Ticket cache is up to date".to_string());
                        }
                    }
                    (CacheRefreshPhase::Full, Err(e))
                        if app.ticket_sync_stage == Some(TicketSyncStage::Full) =>
                    {
                        app.ticket_sync_stage = None;
                        app.flash = Some(format!("Full refresh failed: {}", e));
                    }
                    (CacheRefreshPhase::Manual, Ok(cache)) => {
                        app.loading = false;
                        app.replace_cache(cache, requested_at);
                        app.cache_stale_age_secs = None;
                        app.ticket_sync_stage = None;
                        app.clamp_selection();
                        queue_detail_prefetch(&mut app, &bg_tx);
                        if let Err(e) =
                            jira_client::save_full_cache_snapshot(&config.jira.project, &app.cache)
                        {
                            app.flash = Some(format!("Refreshed (cache save failed: {})", e));
                        } else {
                            app.flash =
                                Some("Refreshed! Syncing epic relationships...".to_string());
                        }
                        if !app.epics_refreshing {
                            app.epics_refreshing = true;
                            spawn_epics_refresh(&bg_tx, &config, app.moves.now());
                        }
                    }
                    (CacheRefreshPhase::Manual, Err(e)) => {
                        app.loading = false;
                        app.flash = Some(format!("Refresh failed: {}", e));
                    }
                    _ => {}
                },
                BackgroundMessage::TicketDetailFetched {
                    key,
                    requested_at,
                    result,
                } => {
                    app.end_detail_fetch(&key);
                    if let Ok(detail) = result {
                        if app.enrich_ticket(&key, requested_at, &detail)
                            && detail_cache_tx.send(detail).is_err()
                        {
                            app.flash =
                                Some("Detail cache writer unavailable; skipping write".to_string());
                        }
                    }
                }
                BackgroundMessage::TransitionsFetched {
                    key,
                    request,
                    result,
                } => move_picker::receive(&mut app, &key, request, result),
                BackgroundMessage::BulkTransitionsFetched { request, fetched } => {
                    if let Some(BulkState::MoveLoading {
                        targets,
                        request: waiting,
                    }) = &app.bulk_state
                    {
                        if *waiting == request {
                            app.bulk_state = Some(BulkState::MoveStatusPicker {
                                targets: targets.clone(),
                                fetched,
                                selected: 0,
                            });
                        }
                    }
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
                                    &bg_tx,
                                    CacheRefreshPhase::Manual,
                                    &config,
                                    app.moves.now(),
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
                BackgroundMessage::BulkCompleted(summary) => {
                    apply_bulk_successes(&mut app, &summary);
                    let action_label = match summary.action {
                        BulkAction::Move => "move",
                        BulkAction::Assign => "assign",
                    };
                    app.flash = Some(format!(
                        "Bulk {} complete: {} succeeded, {} failed, {} skipped",
                        action_label,
                        summary.succeeded,
                        summary.failed,
                        summary.skipped.len()
                    ));
                    app.bulk_state = Some(BulkState::Result { summary, scroll: 0 });
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
                        spawn_cache_refresh(
                            &bg_tx,
                            CacheRefreshPhase::Manual,
                            &config,
                            app.moves.now(),
                        );
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
                            app.filter_results = tickets;
                            app.reapply_moves_since(requested_at);
                            app.collapsed_filters.clear();
                            app.mark_cache_changed();
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
                Event::Key(key) => {
                    handle_key(&mut app, key, &bg_tx, &mut config).await;
                    draw_needed = true;
                }
                Event::Resize(_, _) => draw_needed = true,
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
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

    if !app.moves.failures().is_empty() {
        handle_move_failure_keys(app, key.code);
    } else if app.is_filter_edit_open() {
        handle_filter_edit_keys(app, key.code, config);
    } else if app.is_bulk_upload_open() {
        handle_bulk_upload_keys(app, key.code, bg_tx, config);
    } else if app.is_create_ticket_open() {
        handle_create_ticket_keys(app, key.code, key.modifiers, bg_tx, config).await;
    } else if app.is_comment_open() {
        handle_comment_keys(app, key.code, key.modifiers, bg_tx);
    } else if app.is_assign_open() {
        handle_assign_keys(app, key.code, bg_tx);
    } else if app.is_edit_open() {
        handle_edit_keys(app, key.code, bg_tx);
    } else if app.is_bulk_open() {
        handle_bulk_keys(app, key.code, bg_tx);
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

    let mut cmd = Command::new("cargo");
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd.arg("run");

    if release {
        cmd.arg("--release");
    }

    cmd.arg("--manifest-path")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));

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

fn ui(f: &mut ratatui::Frame, app: &App, config: &AppConfig) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Tabs};

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Content
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    // Tab bar
    let tab_titles: Vec<Line> = Tab::all().iter().map(|t| Line::from(t.title())).collect();
    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title(" lazyjira "))
        .select(match app.active_tab {
            Tab::MyWork => 0,
            Tab::Team => 1,
            Tab::Epics => 2,
            Tab::Unassigned => 3,
            Tab::Filters => 4,
        })
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, chunks[0]);

    // Content area
    if app.loading {
        let loading = ratatui::widgets::Paragraph::new("Loading...")
            .block(Block::default().borders(Borders::ALL));
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
    let status_text = if let Some(ref flash) = app.flash {
        Span::styled(flash.as_str(), Style::default().fg(Color::Red))
    } else if let Some(ref search) = app.search {
        Span::styled(format!("/{}", search), Style::default().fg(Color::Yellow))
    } else if let Some(pending) = app.moves.pending_message() {
        Span::styled(pending, Style::default().fg(Color::Yellow))
    } else {
        let selected_count = app.selected_ticket_count();
        if app.active_tab == Tab::Filters {
            let pane = match app.filter_focus {
                FilterFocus::Sidebar => "sidebar",
                FilterFocus::Results => "results",
            };
            Span::styled(
                format!(
                    " j/k: navigate  Space: mark  A: all  u: clear  B: bulk  U: upload  z/Z: fold  sel:{}  Tab/S-Tab: switch pane({})  Enter: run/open  n: new  e: edit  x: delete  ?: keys  q: quit ",
                    selected_count, pane
                ),
                Style::default().fg(Color::DarkGray),
            )
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
                None => "ready",
            };
            let freshness_state = app
                .cache_stale_age_secs
                .map(|age| format!("stale {}", format_age_minutes(age)))
                .unwrap_or_else(|| "fresh".to_string());
            let focus_state = app
                .status_focus
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("all");
            Span::styled(
                format!(
                    " Tab: switch  j/k: navigate  Space: mark  A: all  u: clear  B: bulk  U: upload  sel:{}  Enter: detail  z: fold  d: done({})  p/w/n/v: focus({})  ?: keys  t:{}  c:{}  e:{}  r: refresh  /: search  q: quit ",
                    selected_count, done_state, focus_state, ticket_state, freshness_state, epic_state
                ),
                Style::default().fg(Color::DarkGray),
            )
        }
    };
    f.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(status_text)),
        chunks[2],
    );

    // Detail overlay
    if app.is_detail_open() {
        widgets::ticket_detail::render(f, app);
    }
    if app.is_create_ticket_open() {
        widgets::create_ticket::render(f, app);
    }
    if app.is_comment_open() {
        widgets::comment::render(f, app);
    }
    if app.is_assign_open() {
        widgets::assign::render(f, app);
    }
    if app.is_edit_open() {
        widgets::edit_fields::render(f, app);
    }
    if app.is_filter_edit_open() {
        render_filter_edit_modal(f, app);
    }
    if app.is_bulk_open() {
        widgets::bulk_actions::render(f, app);
    }
    if app.is_bulk_upload_open() {
        widgets::bulk_upload::render(f, app);
    }
    if app.show_keybindings {
        widgets::keybindings_help::render(f);
    }
    widgets::move_failure::render(f, app.moves.failures());
}

fn render_filter_edit_modal(f: &mut ratatui::Frame, app: &App) {
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    let state = match &app.filter_edit {
        Some(s) => s,
        None => return,
    };

    let title = if state.editing_idx.is_some() {
        "Edit Filter"
    } else {
        "New Filter"
    };

    let inner = widgets::form::render_modal_frame(f, title, 60, 30);

    let mut lines = Vec::new();
    widgets::form::render_text_input(&mut lines, "Name", &state.name, state.focused_field == 0);
    lines.push(Line::from(""));
    widgets::form::render_text_input(&mut lines, "JQL", &state.jql, state.focused_field == 1);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tab: switch field  Enter: save  Esc: cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let widget = Paragraph::new(lines);
    f.render_widget(widget, inner);
}

fn handle_keybindings_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => app.close_keybindings(),
        _ => {}
    }
}

fn handle_move_failure_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => app.moves.dismiss_failure(),
        KeyCode::Char('o') => {
            if let Some(failure) = app.moves.failures().first() {
                open_in_browser(&jira_client::browse_url(&failure.key));
            }
        }
        _ => {}
    }
}

fn open_in_browser(url: &str) {
    let _ = Command::new("open").arg(url).spawn();
}

/// The list row after `key`: j/Down moves down, k/Up moves up, other keys leave it.
fn list_step(selected: usize, key: KeyCode, rows: usize) -> usize {
    match key {
        KeyCode::Char('j') | KeyCode::Down => (selected + 1).min(rows.saturating_sub(1)),
        KeyCode::Char('k') | KeyCode::Up => selected.saturating_sub(1),
        _ => selected,
    }
}

fn begin_bulk_from_selection(app: &mut App) {
    let mut targets = app.selected_visible_ticket_keys_in_order();
    if targets.is_empty() {
        if let Some(key) = app.selected_ticket_key() {
            targets.push(key);
        }
    }
    if targets.is_empty() {
        app.flash = Some("No tickets selected".to_string());
        return;
    }
    app.bulk_state = Some(BulkState::ActionPicker {
        targets,
        selected: 0,
    });
}

fn handle_bulk_keys(app: &mut App, key: KeyCode, bg_tx: &UnboundedSender<BackgroundMessage>) {
    let state = match app.bulk_state.clone() {
        Some(state) => state,
        None => return,
    };
    match state {
        BulkState::ActionPicker { targets, selected } => match key {
            KeyCode::Esc => app.bulk_state = None,
            KeyCode::Char('j') | KeyCode::Down => {
                let new_sel = (selected + 1).min(1);
                app.bulk_state = Some(BulkState::ActionPicker {
                    targets,
                    selected: new_sel,
                });
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.bulk_state = Some(BulkState::ActionPicker {
                    targets,
                    selected: selected.saturating_sub(1),
                });
            }
            KeyCode::Enter if selected == 0 => {
                let request = app.next_request_id();
                spawn_bulk_transitions_fetch(bg_tx, targets.clone(), request);
                app.bulk_state = Some(BulkState::MoveLoading { targets, request });
            }
            KeyCode::Enter => {
                app.bulk_state = Some(BulkState::AssignPicker {
                    targets,
                    selected: 0,
                });
            }
            _ => {}
        },
        BulkState::MoveLoading { .. } => {
            if key == KeyCode::Esc {
                app.bulk_state = None;
            }
        }
        BulkState::MoveStatusPicker {
            targets,
            fetched,
            selected,
        } => {
            let destinations = bulk_plan::destinations(&fetched);
            match key {
                KeyCode::Esc => app.bulk_state = None,
                KeyCode::Enter => {
                    let Some((destination, _)) = destinations.into_iter().nth(selected) else {
                        return;
                    };
                    let plan = bulk_plan::plan_move(app, &fetched, &destination);
                    app.bulk_state =
                        Some(if plan.resolution_choices().iter().any(Option::is_some) {
                            BulkState::MoveResolutionPicker {
                                targets,
                                destination,
                                plan,
                                selected: 0,
                            }
                        } else {
                            BulkState::Confirm {
                                targets,
                                target: BulkTarget::Move {
                                    destination,
                                    resolution: None,
                                },
                                plan: plan.with_resolution(None),
                            }
                        });
                }
                _ => {
                    app.bulk_state = Some(BulkState::MoveStatusPicker {
                        targets,
                        fetched,
                        selected: list_step(selected, key, destinations.len()),
                    });
                }
            }
        }
        BulkState::MoveResolutionPicker {
            targets,
            destination,
            plan,
            selected,
        } => {
            let choices = plan.resolution_choices();
            match key {
                KeyCode::Esc => app.bulk_state = None,
                KeyCode::Enter => {
                    let Some(choice) = choices.get(selected) else {
                        return;
                    };
                    let target = BulkTarget::Move {
                        destination,
                        resolution: choice.as_ref().map(|r| r.name.clone()),
                    };
                    app.bulk_state = Some(BulkState::Confirm {
                        targets,
                        target,
                        plan: plan.with_resolution(choice.as_ref()),
                    });
                }
                _ => {
                    app.bulk_state = Some(BulkState::MoveResolutionPicker {
                        targets,
                        destination,
                        plan,
                        selected: list_step(selected, key, choices.len()),
                    });
                }
            }
        }
        BulkState::AssignPicker { targets, selected } => {
            let max = app.cache.team_members.len().saturating_sub(1);
            match key {
                KeyCode::Esc => app.bulk_state = None,
                KeyCode::Char('j') | KeyCode::Down => {
                    app.bulk_state = Some(BulkState::AssignPicker {
                        targets,
                        selected: (selected + 1).min(max),
                    });
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.bulk_state = Some(BulkState::AssignPicker {
                        targets,
                        selected: selected.saturating_sub(1),
                    });
                }
                KeyCode::Enter => {
                    let Some(member) = app.cache.team_members.get(selected) else {
                        app.flash = Some("No team members configured".to_string());
                        return;
                    };
                    let target = BulkTarget::Assign {
                        member_email: member.email.clone(),
                        member_name: member.name.clone(),
                    };
                    let plan = bulk_plan::plan_assign(app, &targets, &member.email);
                    app.bulk_state = Some(BulkState::Confirm {
                        targets,
                        target,
                        plan,
                    });
                }
                _ => {}
            }
        }
        BulkState::Confirm {
            targets,
            target,
            plan,
        } => match key {
            KeyCode::Esc => app.bulk_state = None,
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.flash = Some(format!(
                    "Running bulk action on {} tickets...",
                    targets.len()
                ));
                app.bulk_state = Some(BulkState::Running {
                    targets,
                    target: target.clone(),
                });
                spawn_bulk_execution(bg_tx, target, plan);
            }
            _ => {}
        },
        BulkState::Running { .. } => {
            if matches!(key, KeyCode::Esc) {
                app.bulk_state = None;
            }
        }
        BulkState::Result { summary, scroll } => match key {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => app.bulk_state = None,
            KeyCode::Char('j') | KeyCode::Down => {
                app.bulk_state = Some(BulkState::Result {
                    summary,
                    scroll: scroll.saturating_add(1),
                });
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.bulk_state = Some(BulkState::Result {
                    summary,
                    scroll: scroll.saturating_sub(1),
                });
            }
            _ => {}
        },
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
                KeyCode::Up => app.scroll_detail_up(),
                KeyCode::Down => app.scroll_detail_down(),
                KeyCode::Char('o') => {
                    if let Some(key) = ticket_detail_key.as_ref() {
                        if let Some(ticket) = app.find_ticket(key) {
                            open_in_browser(&ticket.url);
                        }
                    } else if let Some(epic_key) = app.detail_epic_key.as_ref() {
                        open_in_browser(&format!("https://jira.mongodb.org/browse/{}", epic_key));
                    }
                }
                KeyCode::Char('m') => start_picker_call(bg_tx, move_picker::open(app)),
                KeyCode::Char('C') => {
                    if let Some(key) = ticket_detail_key {
                        app.comment_state = Some(app::CommentState {
                            ticket_key: key,
                            body: String::new(),
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
                            summary,
                            labels,
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
                let detail_loaded = app.is_ticket_detail_loaded(&key);
                app.open_detail(key.clone());
                if !detail_loaded && app.begin_detail_fetch(&key) {
                    spawn_ticket_detail_fetch(bg_tx, key, app.moves.now());
                }
            }
        }
        KeyCode::Char('U') => {
            app.search = None;
            app.bulk_upload_state = Some(BulkUploadState::PathInput {
                path: String::new(),
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
                let trimmed = path.trim().to_string();
                if trimmed.is_empty() {
                    app.flash = Some("CSV path is required".to_string());
                    return;
                }
                app.bulk_upload_state = Some(BulkUploadState::PathInput {
                    path: trimmed.clone(),
                    loading: true,
                });
                let context = build_bulk_upload_context(app);
                spawn_bulk_upload_preview(bg_tx, trimmed, context);
            }
            KeyCode::Backspace => {
                path.pop();
                app.bulk_upload_state = Some(BulkUploadState::PathInput { path, loading });
            }
            KeyCode::Char(c) => {
                path.push(c);
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
                    path: preview.source_path.clone(),
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
                    path: summary.source_path.clone(),
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
    _modifiers: KeyModifiers,
    bg_tx: &UnboundedSender<BackgroundMessage>,
    config: &AppConfig,
) {
    let state = match &mut app.create_ticket {
        Some(s) => s,
        None => return,
    };

    match key {
        KeyCode::Esc => {
            app.create_ticket = None;
        }
        KeyCode::Tab => {
            let state = app.create_ticket.as_mut().unwrap();
            state.focused_field = (state.focused_field + 1) % 4;
        }
        KeyCode::BackTab => {
            let state = app.create_ticket.as_mut().unwrap();
            state.focused_field = if state.focused_field == 0 {
                3
            } else {
                state.focused_field - 1
            };
        }
        KeyCode::Enter => {
            if state.summary.trim().is_empty() {
                app.flash = Some("Summary is required".to_string());
                return;
            }

            let issue_type = app::ISSUE_TYPES[state.issue_type_idx].to_string();
            let summary = state.summary.clone();

            let assignee_email = if state.assignee_idx == 0 {
                None
            } else {
                app.cache
                    .team_members
                    .get(state.assignee_idx - 1)
                    .map(|m| m.email.clone())
            };

            let epic_key = if state.epic_idx == 0 {
                None
            } else {
                app.cache
                    .epics
                    .get(state.epic_idx - 1)
                    .map(|e| e.key.clone())
            };

            app.create_ticket = None;
            app.flash = Some("Creating ticket...".to_string());

            let project = config.jira.project.clone();
            let tx = bg_tx.clone();
            tokio::spawn(async move {
                let result = jira_client::create_ticket(
                    &project,
                    &issue_type,
                    &summary,
                    assignee_email.as_deref(),
                    epic_key.as_deref(),
                )
                .await
                .map_err(|e| e.to_string());
                let _ = tx.send(BackgroundMessage::TicketCreated(result));
            });
        }
        KeyCode::Char(c) if state.focused_field == 1 => {
            state.summary.push(c);
        }
        KeyCode::Backspace if state.focused_field == 1 => {
            state.summary.pop();
        }
        KeyCode::Char('j') | KeyCode::Down => match state.focused_field {
            0 => {
                if state.issue_type_idx < app::ISSUE_TYPES.len() - 1 {
                    state.issue_type_idx += 1;
                }
            }
            2 => {
                let max = app.cache.team_members.len(); // options are 0..=max
                if state.assignee_idx < max {
                    state.assignee_idx += 1;
                }
            }
            3 => {
                let max = app.cache.epics.len(); // options are 0..=max
                if state.epic_idx < max {
                    state.epic_idx += 1;
                }
            }
            _ => {}
        },
        KeyCode::Char('k') | KeyCode::Up => match state.focused_field {
            0 => {
                state.issue_type_idx = state.issue_type_idx.saturating_sub(1);
            }
            2 => {
                state.assignee_idx = state.assignee_idx.saturating_sub(1);
            }
            3 => {
                state.epic_idx = state.epic_idx.saturating_sub(1);
            }
            _ => {}
        },
        _ => {}
    }
}

fn handle_comment_keys(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    bg_tx: &UnboundedSender<BackgroundMessage>,
) {
    match key {
        KeyCode::Esc => {
            app.comment_state = None;
        }
        KeyCode::Enter => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                if let Some(ref mut state) = app.comment_state {
                    state.body.push('\n');
                }
                return;
            }

            let state = match &app.comment_state {
                Some(s) => s,
                None => return,
            };
            if state.body.trim().is_empty() {
                app.flash = Some("Comment body is required".to_string());
                return;
            }
            let ticket_key = state.ticket_key.clone();
            let body = state.body.clone();
            app.comment_state = None;
            app.flash = Some(format!("Adding comment to {}...", ticket_key));

            let tx = bg_tx.clone();
            let key_clone = ticket_key.clone();
            tokio::spawn(async move {
                let result = jira_client::add_comment(&key_clone, &body)
                    .await
                    .map(|_| key_clone)
                    .map_err(|e| e.to_string());
                let _ = tx.send(BackgroundMessage::CommentAdded(result));
            });
        }
        // Some terminals encode Shift+Enter (or modified Enter) as Ctrl+J.
        // Treat it as newline in the comment editor.
        KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(ref mut state) = app.comment_state {
                state.body.push('\n');
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut state) = app.comment_state {
                state.body.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut state) = app.comment_state {
                state.body.push(c);
            }
        }
        _ => {}
    }
}

fn handle_assign_keys(app: &mut App, key: KeyCode, bg_tx: &UnboundedSender<BackgroundMessage>) {
    let member_count = app.cache.team_members.len();
    match key {
        KeyCode::Esc => {
            app.assign_state = None;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut state) = app.assign_state {
                if member_count > 0 && state.selected < member_count - 1 {
                    state.selected += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut state) = app.assign_state {
                state.selected = state.selected.saturating_sub(1);
            }
        }
        KeyCode::Enter => {
            let state = match &app.assign_state {
                Some(s) => s,
                None => return,
            };
            let member = match app.cache.team_members.get(state.selected) {
                Some(m) => m,
                None => return,
            };
            let ticket_key = state.ticket_key.clone();
            let email = member.email.clone();
            let name = member.name.clone();

            // Optimistic cache update
            for ticket in &mut app.cache.my_tickets {
                if ticket.key == ticket_key {
                    ticket.assignee = Some(name.clone());
                    ticket.assignee_email = Some(email.clone());
                }
            }
            for ticket in &mut app.cache.team_tickets {
                if ticket.key == ticket_key {
                    ticket.assignee = Some(name.clone());
                    ticket.assignee_email = Some(email.clone());
                }
            }
            for epic in &mut app.cache.epics {
                for ticket in &mut epic.children {
                    if ticket.key == ticket_key {
                        ticket.assignee = Some(name.clone());
                        ticket.assignee_email = Some(email.clone());
                    }
                }
            }
            app.mark_cache_changed();

            app.assign_state = None;
            app.flash = Some(format!("Assigning {} to {}...", ticket_key, name));

            let tx = bg_tx.clone();
            let key_clone = ticket_key.clone();
            let email_clone = email.clone();
            tokio::spawn(async move {
                let result = jira_client::assign_ticket(&key_clone, &email_clone)
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx.send(BackgroundMessage::TicketAssigned {
                    key: key_clone,
                    result,
                });
            });
        }
        _ => {}
    }
}

fn handle_edit_keys(app: &mut App, key: KeyCode, bg_tx: &UnboundedSender<BackgroundMessage>) {
    match key {
        KeyCode::Esc => {
            app.edit_state = None;
        }
        KeyCode::Tab => {
            if let Some(ref mut state) = app.edit_state {
                state.focused_field = (state.focused_field + 1) % 2;
            }
        }
        KeyCode::BackTab => {
            if let Some(ref mut state) = app.edit_state {
                state.focused_field = if state.focused_field == 0 { 1 } else { 0 };
            }
        }
        KeyCode::Enter => {
            let state = match &app.edit_state {
                Some(s) => s,
                None => return,
            };

            let ticket_key = state.ticket_key.clone();
            let new_summary = state.summary.clone();
            let new_labels_str = state.labels.clone();
            let new_labels: Vec<String> = new_labels_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            // Optimistic cache update
            for ticket in &mut app.cache.my_tickets {
                if ticket.key == ticket_key {
                    ticket.summary = new_summary.clone();
                    ticket.labels = new_labels.clone();
                }
            }
            for ticket in &mut app.cache.team_tickets {
                if ticket.key == ticket_key {
                    ticket.summary = new_summary.clone();
                    ticket.labels = new_labels.clone();
                }
            }
            for epic in &mut app.cache.epics {
                for ticket in &mut epic.children {
                    if ticket.key == ticket_key {
                        ticket.summary = new_summary.clone();
                        ticket.labels = new_labels.clone();
                    }
                }
            }
            app.mark_cache_changed();

            app.edit_state = None;
            app.flash = Some(format!("Updating {}...", ticket_key));

            let tx = bg_tx.clone();
            let key_clone = ticket_key.clone();
            let summary_clone = new_summary.clone();
            let labels_clone = new_labels.clone();
            tokio::spawn(async move {
                let summary_opt = if summary_clone.is_empty() {
                    None
                } else {
                    Some(summary_clone.as_str())
                };
                let labels_opt = if labels_clone.is_empty() {
                    None
                } else {
                    Some(labels_clone.as_slice())
                };
                let result = jira_client::edit_ticket(&key_clone, summary_opt, labels_opt)
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx.send(BackgroundMessage::TicketEdited {
                    key: key_clone,
                    result,
                });
            });
        }
        KeyCode::Backspace => {
            if let Some(ref mut state) = app.edit_state {
                match state.focused_field {
                    0 => {
                        state.summary.pop();
                    }
                    1 => {
                        state.labels.pop();
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut state) = app.edit_state {
                match state.focused_field {
                    0 => state.summary.push(c),
                    1 => state.labels.push(c),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn handle_filter_edit_keys(app: &mut App, key: KeyCode, config: &mut AppConfig) {
    let state = match &mut app.filter_edit {
        Some(s) => s,
        None => return,
    };

    match key {
        KeyCode::Esc => {
            app.filter_edit = None;
        }
        KeyCode::Tab | KeyCode::BackTab => {
            state.focused_field = if state.focused_field == 0 { 1 } else { 0 };
        }
        KeyCode::Enter => {
            if state.name.trim().is_empty() || state.jql.trim().is_empty() {
                app.flash = Some("Both name and JQL are required".to_string());
                return;
            }
            let filter = crate::config::SavedFilter {
                name: state.name.trim().to_string(),
                jql: state.jql.trim().to_string(),
            };

            if let Some(idx) = state.editing_idx {
                if idx < config.filters.len() {
                    config.filters[idx] = filter;
                }
            } else {
                config.filters.push(filter);
                app.filter_sidebar_idx = config.filters.len() - 1;
            }

            match crate::config::save_config(config) {
                Ok(()) => {
                    app.flash = Some("Filter saved".to_string());
                }
                Err(e) => {
                    app.flash = Some(format!("Failed to save filter: {}", e));
                }
            }
            app.filter_edit = None;
        }
        KeyCode::Backspace => match state.focused_field {
            0 => {
                state.name.pop();
            }
            1 => {
                state.jql.pop();
            }
            _ => {}
        },
        KeyCode::Char(c) => match state.focused_field {
            0 => state.name.push(c),
            1 => state.jql.push(c),
            _ => {}
        },
        _ => {}
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
                name: String::new(),
                jql: String::new(),
                editing_idx: None,
            });
        }
        KeyCode::Char('e') => {
            if app.filter_focus == FilterFocus::Sidebar {
                if let Some(filter) = config.filters.get(app.filter_sidebar_idx) {
                    app.filter_edit = Some(app::FilterEditState {
                        focused_field: 0,
                        name: filter.name.clone(),
                        jql: filter.jql.clone(),
                        editing_idx: Some(app.filter_sidebar_idx),
                    });
                }
            }
        }
        KeyCode::Char('x') => {
            if app.filter_focus == FilterFocus::Sidebar && !config.filters.is_empty() {
                if app.filter_sidebar_idx < config.filters.len() {
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
        }
        KeyCode::Char('U') => {
            app.bulk_upload_state = Some(BulkUploadState::PathInput {
                path: String::new(),
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
                begin_bulk_from_selection(app);
            }
        }
        KeyCode::Char('z') => {
            if app.filter_focus == FilterFocus::Results {
                if let Some(group_id) = app.selected_group_id() {
                    app.toggle_group_collapse(&group_id);
                }
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
                        let result = jira_client::fetch_jql_query(&cfg, &jql)
                            .await
                            .map_err(|e| e.to_string());
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
                    let detail_loaded = app.is_ticket_detail_loaded(&key);
                    app.open_detail(key.clone());
                    if !detail_loaded && app.begin_detail_fetch(&key) {
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
                spawn_cache_refresh(bg_tx, CacheRefreshPhase::Manual, config, app.moves.now());
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
        KeyCode::Tab => app.next_tab(),
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
        KeyCode::Char('B') => begin_bulk_from_selection(app),
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
        KeyCode::Char('p') => {
            app.toggle_status_focus(Status::InProgress);
            app.flash = Some(
                app.status_focus
                    .as_ref()
                    .map(|s| format!("Focus: {}", s.as_str()))
                    .unwrap_or_else(|| "Focus: all".to_string()),
            );
        }
        KeyCode::Char('w') => {
            app.toggle_status_focus(Status::ReadyForWork);
            app.flash = Some(
                app.status_focus
                    .as_ref()
                    .map(|s| format!("Focus: {}", s.as_str()))
                    .unwrap_or_else(|| "Focus: all".to_string()),
            );
        }
        KeyCode::Char('n') => {
            app.toggle_status_focus(Status::NeedsTriage);
            app.flash = Some(
                app.status_focus
                    .as_ref()
                    .map(|s| format!("Focus: {}", s.as_str()))
                    .unwrap_or_else(|| "Focus: all".to_string()),
            );
        }
        KeyCode::Char('v') => {
            app.toggle_status_focus(Status::InReview);
            app.flash = Some(
                app.status_focus
                    .as_ref()
                    .map(|s| format!("Focus: {}", s.as_str()))
                    .unwrap_or_else(|| "Focus: all".to_string()),
            );
        }
        KeyCode::Char('r') => {
            if app.loading {
                app.flash = Some("Refresh already in progress".to_string());
            } else {
                app.loading = true;
                app.ticket_sync_stage = None;
                app.flash = Some("Refreshing tickets...".to_string());
                spawn_cache_refresh(bg_tx, CacheRefreshPhase::Manual, config, app.moves.now());
            }
        }
        KeyCode::Char('z') => {
            if let Some(group_id) = app.selected_group_id() {
                app.toggle_group_collapse(&group_id);
            }
        }
        KeyCode::Char('Z') => {
            app.toggle_all_groups_collapse();
        }
        KeyCode::Char('c') => {
            app.create_ticket = Some(app::CreateTicketState {
                focused_field: 0,
                issue_type_idx: 0,
                summary: String::new(),
                assignee_idx: 0,
                epic_idx: 0,
            });
        }
        KeyCode::Char('U') => {
            app.bulk_upload_state = Some(BulkUploadState::PathInput {
                path: String::new(),
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
                let detail_loaded = app.is_ticket_detail_loaded(&key);
                app.open_detail(key.clone());
                if !detail_loaded && app.begin_detail_fetch(&key) {
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
    use std::collections::BTreeMap;

    fn sample_config() -> AppConfig {
        AppConfig {
            jira: crate::config::JiraConfig {
                project: "AMP".to_string(),
                team_name: "Code Generation".to_string(),
                done_window_days: 14,
                epics_i_care_about: vec![],
            },
            team: BTreeMap::new(),
            statuses: crate::config::StatusConfig::default(),
            filters: vec![],
        }
    }

    fn ticket(key: &str, summary: &str, status: Status) -> crate::cache::Ticket {
        crate::cache::Ticket {
            key: key.to_string(),
            summary: summary.to_string(),
            status,
            jira_status: None,
            assignee: None,
            assignee_email: None,
            reporter: None,
            description: None,
            labels: Vec::new(),
            epic_key: None,
            epic_name: None,
            detail_loaded: false,
            url: format!("https://jira.mongodb.org/browse/{}", key),
            activity: Vec::new(),
        }
    }

    #[test]
    fn summarize_bulk_results_all_success() {
        let target = BulkTarget::Move {
            destination: "In Progress".to_string(),
            resolution: None,
        };
        let summary = summarize_bulk_results(
            BulkAction::Move,
            target.clone(),
            vec![("AMP-1".to_string(), Ok(())), ("AMP-2".to_string(), Ok(()))],
            Vec::new(),
        );
        assert_eq!(summary.target, target);
        assert_eq!(summary.succeeded, 2);
        assert_eq!(summary.failed, 0);
        assert!(summary.skipped.is_empty());
    }

    #[test]
    fn summarize_bulk_results_partial_failure_and_skips() {
        let summary = summarize_bulk_results(
            BulkAction::Assign,
            BulkTarget::Assign {
                member_email: "dev@example.com".to_string(),
                member_name: "Dev".to_string(),
            },
            vec![
                ("AMP-1".to_string(), Ok(())),
                ("AMP-2".to_string(), Err("boom".to_string())),
            ],
            vec![("AMP-3".to_string(), "already assigned".to_string())],
        );
        assert_eq!(summary.total, 3);
        assert_eq!(summary.attempted, 2);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failed_details.len(), 1);
        assert_eq!(
            summary.skipped,
            [("AMP-3".to_string(), "already assigned".to_string())]
        );
    }

    #[test]
    fn summarize_bulk_results_empty_attempts() {
        let summary = summarize_bulk_results(
            BulkAction::Move,
            BulkTarget::Move {
                destination: "Closed".to_string(),
                resolution: None,
            },
            vec![],
            vec![
                ("AMP-1".to_string(), "already Closed".to_string()),
                (
                    "AMP-2".to_string(),
                    "no transition to Closed from Open".to_string(),
                ),
            ],
        );
        assert_eq!(summary.total, 2);
        assert_eq!(summary.attempted, 0);
        assert_eq!(summary.succeeded, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped.len(), 2);
    }

    // No Tokio runtime: nothing here may reach Jira.
    #[test]
    fn bulk_move_asks_once_for_a_resolution_then_confirms_the_plan() {
        use crate::transitions::tests::{transition, with_resolution};

        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![
            ticket("DEMO-1", "A", Status::InProgress),
            ticket("DEMO-2", "B", Status::InProgress),
        ];
        let done = transition("805", "Done", "Done");
        app.bulk_state = Some(BulkState::MoveStatusPicker {
            targets: vec!["DEMO-1".to_string(), "DEMO-2".to_string()],
            fetched: vec![
                (
                    "DEMO-1".to_string(),
                    Ok(vec![with_resolution(
                        done.clone(),
                        true,
                        &[("101", "Fixed")],
                    )]),
                ),
                (
                    "DEMO-2".to_string(),
                    Ok(vec![
                        transition("803", "Stop Progress", "Open"),
                        with_resolution(done, true, &[("102", "Won't Fix")]),
                    ]),
                ),
            ],
            selected: 0,
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        // "Done" (2 tickets) sorts before "Open" (1 ticket).
        handle_bulk_keys(&mut app, KeyCode::Enter, &tx);
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::MoveResolutionPicker { ref destination, .. }) if destination == "Done"
        ));
        handle_bulk_keys(&mut app, KeyCode::Enter, &tx);

        let Some(BulkState::Confirm { target, plan, .. }) = &app.bulk_state else {
            panic!("expected the confirm step");
        };
        assert_eq!(
            target,
            &BulkTarget::Move {
                destination: "Done".to_string(),
                resolution: Some("Fixed".to_string()),
            }
        );
        assert_eq!(plan.jobs.len(), 1);
        assert_eq!(plan.jobs[0].0, "DEMO-1");
        assert_eq!(plan.skipped[0].0, "DEMO-2");
    }

    #[tokio::test]
    async fn keybindings_regression_main_navigation_still_works() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::MyWork;
        app.cache.my_tickets = vec![ticket("AMP-1", "A", Status::InProgress)];
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
    fn comment_shift_enter_inserts_newline() {
        let mut app = App::new();
        app.comment_state = Some(crate::app::CommentState {
            ticket_key: "AMP-1".to_string(),
            body: "hello".to_string(),
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        handle_comment_keys(&mut app, KeyCode::Enter, KeyModifiers::SHIFT, &tx);

        let state = app.comment_state.expect("comment modal should remain open");
        assert_eq!(state.body, "hello\n");
    }

    #[test]
    fn comment_enter_requires_non_empty_body() {
        let mut app = App::new();
        app.comment_state = Some(crate::app::CommentState {
            ticket_key: "AMP-1".to_string(),
            body: "   ".to_string(),
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
            ticket_key: "AMP-1".to_string(),
            body: "hello".to_string(),
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        handle_comment_keys(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL, &tx);

        let state = app.comment_state.expect("comment modal should remain open");
        assert_eq!(state.body, "hello\n");
    }

    #[tokio::test]
    async fn bulk_menu_falls_back_to_current_ticket_when_nothing_selected() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::MyWork;
        app.cache.my_tickets = vec![ticket("AMP-1", "A", Status::InProgress)];
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
                assert_eq!(targets, vec!["AMP-1".to_string()]);
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
            key: "AMP-500".to_string(),
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

        assert_eq!(app.detail_epic_key.as_deref(), Some("AMP-500"));
        assert!(app.detail_ticket_key.is_none());
    }

    #[tokio::test]
    async fn enter_on_epic_header_opens_epic_detail_in_search_mode() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        app.search = Some(String::new());
        app.cache.epics = vec![crate::cache::Epic {
            key: "AMP-501".to_string(),
            summary: "Epic Header Search".to_string(),
            children: vec![],
        }];
        app.selected_index = 0;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        handle_search_keys(&mut app, KeyCode::Enter, KeyModifiers::NONE, &tx).await;

        assert_eq!(app.detail_epic_key.as_deref(), Some("AMP-501"));
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
            ticket("AMP-70", "Grouped", Status::InProgress),
            ticket("AMP-71", "Grouped too", Status::InProgress),
        ];
        app.mark_cache_changed();
        app.selected_index = 1;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();
        handle_filter_keys(&mut app, KeyCode::Char('z'), &tx, &mut config);

        assert!(app.collapsed_filters.contains(Status::InProgress.as_str()));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn uppercase_z_toggles_all_filter_groups() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        app.filter_focus = FilterFocus::Results;
        app.filter_results = vec![
            ticket("AMP-72", "In progress", Status::InProgress),
            ticket("AMP-73", "Ready", Status::ReadyForWork),
        ];
        app.mark_cache_changed();

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();
        handle_filter_keys(&mut app, KeyCode::Char('Z'), &tx, &mut config);

        assert!(app
            .collapsed_filters
            .contains(Status::ReadyForWork.as_str()));
        assert!(!app.collapsed_filters.contains(Status::InProgress.as_str()));

        handle_filter_keys(&mut app, KeyCode::Char('Z'), &tx, &mut config);
        assert!(app.collapsed_filters.is_empty());
    }

    #[test]
    fn enter_on_collapsed_filter_header_expands_group() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        app.filter_focus = FilterFocus::Results;
        app.filter_results = vec![ticket("AMP-74", "Grouped", Status::InProgress)];
        app.collapsed_filters
            .insert(Status::InProgress.as_str().to_string());
        app.mark_cache_changed();
        app.selected_index = 0;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();
        handle_filter_keys(&mut app, KeyCode::Enter, &tx, &mut config);

        assert!(!app.collapsed_filters.contains(Status::InProgress.as_str()));
        assert!(app.detail_ticket_key.is_none());
    }

    #[test]
    fn enter_on_filter_ticket_opens_detail() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        app.filter_focus = FilterFocus::Results;
        let mut ticket = ticket("AMP-75", "Ticket detail", Status::InProgress);
        ticket.detail_loaded = true;
        app.filter_results = vec![ticket];
        app.mark_cache_changed();
        app.selected_index = 1;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = sample_config();
        handle_filter_keys(&mut app, KeyCode::Enter, &tx, &mut config);

        assert_eq!(app.detail_ticket_key.as_deref(), Some("AMP-75"));
    }

    #[tokio::test]
    async fn move_failure_stays_on_screen_until_dismissed() {
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![ticket("DSCI-2478", "Epic", Status::from_str("Backlog"))];
        app.open_detail("DSCI-2478".to_string());
        assert!(app.moves.start("DSCI-2478", "Done"));
        app.finish_move(
            "DSCI-2478",
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
        assert_eq!(app.cache.my_tickets[0].status, Status::from_str("Backlog"));
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
