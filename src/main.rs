mod app;
mod bulk_upload;
mod cache;
mod config;
mod jira_client;
mod setup;
mod views;
mod widgets;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::collections::HashSet;
use std::io;
use std::process::Command;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

use crate::cache::Status;
use crate::config::AppConfig;
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

enum BackgroundMessage {
    EpicsRefreshed(std::result::Result<Vec<crate::cache::Epic>, String>),
    CacheRefreshed {
        phase: CacheRefreshPhase,
        result: std::result::Result<crate::cache::Cache, String>,
    },
    TicketDetailFetched {
        key: String,
        result: std::result::Result<crate::cache::Ticket, String>,
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
    FilterResults(std::result::Result<Vec<crate::cache::Ticket>, String>),
}

fn spawn_epics_refresh(tx: &UnboundedSender<BackgroundMessage>, config: &AppConfig) {
    let tx = tx.clone();
    let config = config.clone();
    tokio::spawn(async move {
        let result = jira_client::refresh_epics_cache(&config)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(BackgroundMessage::EpicsRefreshed(result));
    });
}

fn spawn_cache_refresh(
    tx: &UnboundedSender<BackgroundMessage>,
    phase: CacheRefreshPhase,
    config: &AppConfig,
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
        let _ = tx.send(BackgroundMessage::CacheRefreshed { phase, result });
    });
}

fn spawn_ticket_detail_fetch(tx: &UnboundedSender<BackgroundMessage>, key: String) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = jira_client::fetch_ticket_detail(&key)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(BackgroundMessage::TicketDetailFetched { key, result });
    });
}

fn spawn_ticket_detail_prefetch(tx: &UnboundedSender<BackgroundMessage>, keys: Vec<String>) {
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
                let _ = tx.send(BackgroundMessage::TicketDetailFetched { key, result });
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
    spawn_ticket_detail_prefetch(bg_tx, prefetch_keys);
}

fn bulk_target_already_applied(ticket: &crate::cache::Ticket, target: &BulkTarget) -> bool {
    match target {
        BulkTarget::Move { status, .. } => &ticket.status == status,
        BulkTarget::Assign { member_email, .. } => {
            ticket.assignee_email.as_deref() == Some(member_email.as_str())
        }
    }
}

fn partition_bulk_targets(
    app: &App,
    targets: &[String],
    target: &BulkTarget,
) -> (Vec<String>, usize) {
    let mut attempt = Vec::new();
    let mut skipped = 0usize;
    for key in targets {
        match app.find_ticket(key) {
            Some(ticket) if bulk_target_already_applied(ticket, target) => skipped += 1,
            Some(_) => attempt.push(key.clone()),
            None => skipped += 1,
        }
    }
    (attempt, skipped)
}

fn summarize_bulk_results(
    action: BulkAction,
    target: BulkTarget,
    total: usize,
    attempted: usize,
    skipped: usize,
    results: Vec<(String, std::result::Result<(), String>)>,
) -> BulkSummary {
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut successful_keys = Vec::new();
    let mut failed_details = Vec::new();
    for (key, result) in results {
        match result {
            Ok(()) => {
                succeeded += 1;
                successful_keys.push(key);
            }
            Err(err) => {
                failed += 1;
                failed_details.push((key, err));
            }
        }
    }
    BulkSummary {
        action,
        target,
        total,
        attempted,
        succeeded,
        skipped,
        failed,
        successful_keys,
        failed_details,
    }
}

fn apply_bulk_successes(app: &mut App, summary: &BulkSummary) {
    match &summary.target {
        BulkTarget::Move { status, .. } => {
            for key in &summary.successful_keys {
                app.update_ticket_status(key, status.clone());
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

fn spawn_bulk_execution(
    tx: &UnboundedSender<BackgroundMessage>,
    targets: Vec<String>,
    attempt_keys: Vec<String>,
    target: BulkTarget,
    skipped: usize,
) {
    const MAX_CONCURRENCY: usize = 6;
    let tx = tx.clone();
    tokio::spawn(async move {
        let action = match target {
            BulkTarget::Move { .. } => BulkAction::Move,
            BulkTarget::Assign { .. } => BulkAction::Assign,
        };
        let total = targets.len();
        let attempted = attempt_keys.len();
        if attempt_keys.is_empty() {
            let summary =
                summarize_bulk_results(action, target, total, attempted, skipped, Vec::new());
            let _ = tx.send(BackgroundMessage::BulkCompleted(summary));
            return;
        }

        let mut iter = attempt_keys.into_iter();
        let mut tasks = tokio::task::JoinSet::new();
        let mut results = Vec::new();

        let initial_workers = MAX_CONCURRENCY.min(attempted);
        for _ in 0..initial_workers {
            if let Some(key) = iter.next() {
                let run_target = target.clone();
                tasks.spawn(async move {
                    let result = match run_target {
                        BulkTarget::Move { status, resolution } => {
                            jira_client::move_ticket(&key, status.as_str(), resolution.as_deref())
                                .await
                                .map_err(|e| e.to_string())
                        }
                        BulkTarget::Assign { member_email, .. } => {
                            jira_client::assign_ticket(&key, &member_email)
                                .await
                                .map_err(|e| e.to_string())
                        }
                    };
                    (key, result)
                });
            }
        }

        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok(outcome) => results.push(outcome),
                Err(err) => results.push(("unknown".to_string(), Err(err.to_string()))),
            }
            if let Some(next_key) = iter.next() {
                let run_target = target.clone();
                tasks.spawn(async move {
                    let result = match run_target {
                        BulkTarget::Move { status, resolution } => jira_client::move_ticket(
                            &next_key,
                            status.as_str(),
                            resolution.as_deref(),
                        )
                        .await
                        .map_err(|e| e.to_string()),
                        BulkTarget::Assign { member_email, .. } => {
                            jira_client::assign_ticket(&next_key, &member_email)
                                .await
                                .map_err(|e| e.to_string())
                        }
                    };
                    (next_key, result)
                });
            }
        }

        let summary = summarize_bulk_results(action, target, total, attempted, skipped, results);
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
    let (bg_tx, mut bg_rx) = tokio::sync::mpsc::unbounded_channel();
    let detail_cache_tx = jira_client::spawn_detail_cache_writer(&config.jira.project);

    // Fast startup: load persisted snapshot immediately, then revalidate in stages.
    if let Some(snapshot) = jira_client::load_startup_cache_snapshot(&config.jira.project) {
        app.replace_cache(snapshot.cache);
        app.loading = false;
        app.cache_stale_age_secs = Some(snapshot.age_secs);
        app.ticket_sync_stage = Some(TicketSyncStage::ActiveOnly);
        app.flash = Some("Loaded cached data. Refreshing active tickets...".to_string());
        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::ActiveOnly, &config);
    } else {
        let cache = jira_client::fetch_active_only(&config).await?;
        app.replace_cache(cache);
        app.loading = false;
        app.ticket_sync_stage = Some(TicketSyncStage::Full);
        app.flash = Some("Loaded active tickets. Syncing recently done...".to_string());
        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::Full, &config);
    }

    spawn_epics_refresh(&bg_tx, &config);
    app.epics_refreshing = true;
    queue_detail_prefetch(&mut app, &bg_tx);

    let mut draw_needed = true;

    // Main loop
    loop {
        let mut state_changed = false;
        while let Ok(message) = bg_rx.try_recv() {
            state_changed = true;
            match message {
                BackgroundMessage::EpicsRefreshed(result) => {
                    app.epics_refreshing = false;
                    match result {
                        Ok(epics) => {
                            jira_client::attach_epics_to_tickets(
                                &mut app.cache.my_tickets,
                                &mut app.cache.team_tickets,
                                &epics,
                            );
                            app.cache.epics = epics;
                            app.mark_cache_changed();
                            app.clamp_selection();
                            app.flash = Some("Epic relationships refreshed".to_string());
                        }
                        Err(e) => {
                            app.flash = Some(format!("Epic refresh failed: {}", e));
                        }
                    }
                }
                BackgroundMessage::CacheRefreshed { phase, result } => match (phase, result) {
                    (CacheRefreshPhase::ActiveOnly, Ok(cache))
                        if app.ticket_sync_stage == Some(TicketSyncStage::ActiveOnly) =>
                    {
                        app.replace_cache(cache);
                        app.cache_stale_age_secs = None;
                        app.ticket_sync_stage = Some(TicketSyncStage::Full);
                        app.clamp_selection();
                        queue_detail_prefetch(&mut app, &bg_tx);
                        app.flash =
                            Some("Active tickets refreshed. Syncing recently done...".to_string());
                        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::Full, &config);
                    }
                    (CacheRefreshPhase::ActiveOnly, Err(e))
                        if app.ticket_sync_stage == Some(TicketSyncStage::ActiveOnly) =>
                    {
                        app.ticket_sync_stage = Some(TicketSyncStage::Full);
                        app.flash = Some(format!(
                            "Active refresh failed ({}). Trying full refresh...",
                            e
                        ));
                        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::Full, &config);
                    }
                    (CacheRefreshPhase::Full, Ok(cache))
                        if app.ticket_sync_stage == Some(TicketSyncStage::Full) =>
                    {
                        app.replace_cache(cache);
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
                        app.replace_cache(cache);
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
                            spawn_epics_refresh(&bg_tx, &config);
                        }
                    }
                    (CacheRefreshPhase::Manual, Err(e)) => {
                        app.loading = false;
                        app.flash = Some(format!("Refresh failed: {}", e));
                    }
                    _ => {}
                },
                BackgroundMessage::TicketDetailFetched { key, result } => {
                    app.end_detail_fetch(&key);
                    match result {
                        Ok(detail) => {
                            app.enrich_ticket(&key, &detail);
                            if detail_cache_tx.send(detail).is_err() {
                                app.flash = Some(
                                    "Detail cache writer unavailable; skipping write".to_string(),
                                );
                            }
                        }
                        Err(_) => {}
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
                                spawn_cache_refresh(&bg_tx, CacheRefreshPhase::Manual, &config);
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
                        action_label, summary.succeeded, summary.failed, summary.skipped
                    ));
                    app.bulk_state = Some(BulkState::Result { summary });
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
                        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::Manual, &config);
                    }
                }
                BackgroundMessage::FilterResults(result) => {
                    app.filter_loading = false;
                    match result {
                        Ok(tickets) => {
                            let count = tickets.len();
                            app.filter_results = tickets;
                            app.mark_cache_changed();
                            app.prune_selection_to_visible();
                            app.filter_focus = FilterFocus::Results;
                            app.selected_index = 0;
                            app.flash = Some(format!("Filter returned {} tickets", count));
                        }
                        Err(e) => {
                            app.filter_results.clear();
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
                    // Clear flash on any keypress
                    app.flash = None;

                    if app.is_filter_edit_open() {
                        handle_filter_edit_keys(&mut app, key.code, &mut config);
                    } else if app.is_bulk_upload_open() {
                        handle_bulk_upload_keys(&mut app, key.code, &bg_tx, &config);
                    } else if app.is_create_ticket_open() {
                        handle_create_ticket_keys(
                            &mut app,
                            key.code,
                            key.modifiers,
                            &bg_tx,
                            &config,
                        )
                        .await;
                    } else if app.is_comment_open() {
                        handle_comment_keys(&mut app, key.code, &bg_tx);
                    } else if app.is_assign_open() {
                        handle_assign_keys(&mut app, key.code, &bg_tx);
                    } else if app.is_edit_open() {
                        handle_edit_keys(&mut app, key.code, &bg_tx);
                    } else if app.is_bulk_open() {
                        handle_bulk_keys(&mut app, key.code, &bg_tx, &config);
                    } else if app.show_keybindings {
                        handle_keybindings_keys(&mut app, key.code);
                    } else if app.is_detail_open() {
                        handle_detail_keys(&mut app, key.code, &config);
                    } else if app.search.is_some() {
                        handle_search_keys(&mut app, key.code, key.modifiers, &bg_tx).await;
                    } else if app.active_tab == Tab::Filters {
                        handle_filter_keys(&mut app, key.code, &bg_tx, &mut config);
                    } else {
                        handle_main_keys(&mut app, key.code, key.modifiers, &bg_tx, &config).await;
                    }
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
    } else {
        let selected_count = app.selected_ticket_count();
        if app.active_tab == Tab::Filters {
            let pane = match app.filter_focus {
                FilterFocus::Sidebar => "sidebar",
                FilterFocus::Results => "results",
            };
            Span::styled(
                format!(
                    " j/k: navigate  Space: mark  A: all  u: clear  B: bulk  U: upload  sel:{}  Tab/S-Tab: switch pane({})  Enter: run/open  n: new  e: edit  x: delete  ?: keys  q: quit ",
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
        widgets::ticket_detail::render(f, app, &config.resolutions);
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
        widgets::bulk_actions::render(f, app, &config.resolutions);
    }
    if app.is_bulk_upload_open() {
        widgets::bulk_upload::render(f, app);
    }
    if app.show_keybindings {
        widgets::keybindings_help::render(f);
    }
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

fn current_move_options(app: &App) -> Option<(String, Vec<Status>)> {
    let ticket_key = app.detail_ticket_key.clone()?;
    let ticket = app.find_ticket(&ticket_key)?;
    let options = ticket.status.others().into_iter().cloned().collect();
    Some((ticket_key, options))
}

fn queue_move_confirmation(app: &mut App, ticket_key: &str, selected: usize, new_status: Status) {
    let shortcut = new_status.move_shortcut().to_ascii_uppercase();
    let status_str = new_status.as_str().to_string();
    app.detail_mode = DetailMode::MovePicker {
        selected,
        confirm_target: Some(new_status),
    };
    app.flash = Some(format!(
        "Move {} to {}? Press Enter/y to confirm (or {} to move now).",
        ticket_key, status_str, shortcut
    ));
}

fn perform_ticket_move(
    app: &mut App,
    ticket_key: String,
    new_status: Status,
    resolution: Option<String>,
) {
    let status_str = new_status.as_str().to_string();
    let key_clone = ticket_key.clone();

    // Optimistic update
    app.update_ticket_status(&ticket_key, new_status);
    app.detail_mode = DetailMode::View;
    let flash_msg = match &resolution {
        Some(r) => format!(
            "Moving {} to {} (resolution: {})...",
            key_clone, status_str, r
        ),
        None => format!("Moving {} to {}...", key_clone, status_str),
    };
    app.flash = Some(flash_msg);

    // Fire and forget the CLI call
    tokio::spawn(async move {
        let _ = jira_client::move_ticket(&key_clone, &status_str, resolution.as_deref()).await;
    });
}

/// Returns true if the given status is a terminal/done status that requires a resolution.
fn is_terminal_status(status: &Status) -> bool {
    matches!(status, Status::Closed)
}

/// Either perform the move directly, or redirect to the resolution picker for terminal statuses.
fn perform_or_pick_resolution(app: &mut App, ticket_key: String, new_status: Status) {
    if is_terminal_status(&new_status) {
        app.detail_mode = DetailMode::ResolutionPicker {
            target_status: new_status,
            selected: 0,
        };
        app.flash = Some("Select a resolution:".to_string());
    } else {
        perform_ticket_move(app, ticket_key, new_status, None);
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

fn handle_bulk_keys(
    app: &mut App,
    key: KeyCode,
    bg_tx: &UnboundedSender<BackgroundMessage>,
    config: &AppConfig,
) {
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
            KeyCode::Enter => {
                app.bulk_state = Some(if selected == 0 {
                    BulkState::MoveStatusPicker {
                        targets,
                        selected: 0,
                    }
                } else {
                    BulkState::AssignPicker {
                        targets,
                        selected: 0,
                    }
                });
            }
            _ => {}
        },
        BulkState::MoveStatusPicker { targets, selected } => match key {
            KeyCode::Esc => app.bulk_state = None,
            KeyCode::Char('j') | KeyCode::Down => {
                let max = Status::all().len().saturating_sub(1);
                app.bulk_state = Some(BulkState::MoveStatusPicker {
                    targets,
                    selected: (selected + 1).min(max),
                });
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.bulk_state = Some(BulkState::MoveStatusPicker {
                    targets,
                    selected: selected.saturating_sub(1),
                });
            }
            KeyCode::Enter => {
                let Some(status) = Status::all().get(selected).cloned() else {
                    return;
                };
                if is_terminal_status(&status) {
                    app.bulk_state = Some(BulkState::MoveResolutionPicker {
                        targets,
                        status,
                        selected: 0,
                    });
                } else {
                    app.bulk_state = Some(BulkState::Confirm {
                        targets,
                        target: BulkTarget::Move {
                            status,
                            resolution: None,
                        },
                    });
                }
            }
            _ => {}
        },
        BulkState::MoveResolutionPicker {
            targets,
            status,
            selected,
        } => match key {
            KeyCode::Esc => app.bulk_state = None,
            KeyCode::Char('j') | KeyCode::Down => {
                let max = config.resolutions.len().saturating_sub(1);
                app.bulk_state = Some(BulkState::MoveResolutionPicker {
                    targets,
                    status,
                    selected: (selected + 1).min(max),
                });
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.bulk_state = Some(BulkState::MoveResolutionPicker {
                    targets,
                    status,
                    selected: selected.saturating_sub(1),
                });
            }
            KeyCode::Enter => {
                let resolution = config.resolutions.get(selected).cloned();
                app.bulk_state = Some(BulkState::Confirm {
                    targets,
                    target: BulkTarget::Move { status, resolution },
                });
            }
            _ => {}
        },
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
                    app.bulk_state = Some(BulkState::Confirm {
                        targets,
                        target: BulkTarget::Assign {
                            member_email: member.email.clone(),
                            member_name: member.name.clone(),
                        },
                    });
                }
                _ => {}
            }
        }
        BulkState::Confirm { targets, target } => match key {
            KeyCode::Esc => app.bulk_state = None,
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                let (attempt_keys, skipped) = partition_bulk_targets(app, &targets, &target);
                app.flash = Some(format!(
                    "Running bulk action on {} tickets...",
                    targets.len()
                ));
                app.bulk_state = Some(BulkState::Running {
                    targets: targets.clone(),
                    target: target.clone(),
                });
                spawn_bulk_execution(bg_tx, targets, attempt_keys, target, skipped);
            }
            _ => {}
        },
        BulkState::Running { .. } => {
            if matches!(key, KeyCode::Esc) {
                app.bulk_state = None;
            }
        }
        BulkState::Result { .. } => match key {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => app.bulk_state = None,
            _ => {}
        },
    }
}

fn handle_detail_keys(app: &mut App, key: KeyCode, config: &AppConfig) {
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
                            let url = ticket.url.clone();
                            let _ = std::process::Command::new("open").arg(&url).spawn();
                        }
                    } else if let Some(epic_key) = app.detail_epic_key.as_ref() {
                        let url = format!("https://jira.mongodb.org/browse/{}", epic_key);
                        let _ = std::process::Command::new("open").arg(&url).spawn();
                    }
                }
                KeyCode::Char('m') => {
                    if ticket_detail_key.is_some() {
                        app.detail_mode = DetailMode::MovePicker {
                            selected: 0,
                            confirm_target: None,
                        };
                    }
                }
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
        DetailMode::MovePicker {
            selected,
            confirm_target,
        } => match key {
            KeyCode::Esc => app.detail_mode = DetailMode::View,
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some((_, options)) = current_move_options(app) {
                    let new_sel = (selected + 1).min(options.len().saturating_sub(1));
                    app.detail_mode = DetailMode::MovePicker {
                        selected: new_sel,
                        confirm_target: None,
                    };
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.detail_mode = DetailMode::MovePicker {
                    selected: selected.saturating_sub(1),
                    confirm_target: None,
                };
            }
            KeyCode::Enter => {
                if let Some(target) = confirm_target {
                    if let Some((ticket_key, options)) = current_move_options(app) {
                        if options.contains(&target) {
                            perform_or_pick_resolution(app, ticket_key, target);
                        }
                    }
                } else if let Some((ticket_key, options)) = current_move_options(app) {
                    if let Some(new_status) = options.get(selected).cloned() {
                        queue_move_confirmation(app, &ticket_key, selected, new_status);
                    }
                }
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(target) = confirm_target {
                    if let Some((ticket_key, options)) = current_move_options(app) {
                        if options.contains(&target) {
                            perform_or_pick_resolution(app, ticket_key, target);
                        }
                    }
                }
            }
            KeyCode::Char(c) => {
                if let Some(target_status) = Status::from_move_shortcut(c) {
                    if let Some((ticket_key, options)) = current_move_options(app) {
                        if let Some(target_idx) = options.iter().position(|s| *s == target_status) {
                            if c.is_ascii_uppercase() {
                                perform_or_pick_resolution(app, ticket_key, target_status);
                            } else {
                                queue_move_confirmation(
                                    app,
                                    &ticket_key,
                                    target_idx,
                                    target_status,
                                );
                            }
                        } else {
                            app.flash = Some(format!(
                                "{} is already {}",
                                ticket_key,
                                target_status.as_str()
                            ));
                        }
                    }
                }
            }
            _ => {}
        },
        DetailMode::ResolutionPicker {
            target_status,
            selected,
        } => match key {
            KeyCode::Esc => {
                app.detail_mode = DetailMode::MovePicker {
                    selected: 0,
                    confirm_target: None,
                };
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let max = config.resolutions.len().saturating_sub(1);
                app.detail_mode = DetailMode::ResolutionPicker {
                    target_status,
                    selected: (selected + 1).min(max),
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.detail_mode = DetailMode::ResolutionPicker {
                    target_status,
                    selected: selected.saturating_sub(1),
                };
            }
            KeyCode::Enter => {
                if let Some(resolution) = config.resolutions.get(selected) {
                    if let Some(ticket_key) = app.detail_ticket_key.clone() {
                        perform_ticket_move(
                            app,
                            ticket_key,
                            target_status,
                            Some(resolution.clone()),
                        );
                    }
                }
            }
            _ => {}
        },
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
                    spawn_ticket_detail_fetch(bg_tx, key);
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

fn handle_comment_keys(app: &mut App, key: KeyCode, bg_tx: &UnboundedSender<BackgroundMessage>) {
    match key {
        KeyCode::Esc => {
            app.comment_state = None;
        }
        KeyCode::Enter => {
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
                    app.mark_cache_changed();
                    app.flash = Some(format!("Running filter '{}'...", filter.name));

                    let tx = bg_tx.clone();
                    let cfg = config.clone();
                    let jql = filter.jql.clone();
                    tokio::spawn(async move {
                        let result = jira_client::fetch_jql_query(&cfg, &jql)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = tx.send(BackgroundMessage::FilterResults(result));
                    });
                }
            }
            FilterFocus::Results => {
                if let Some(key) = app.selected_ticket_key() {
                    let detail_loaded = app.is_ticket_detail_loaded(&key);
                    app.open_detail(key.clone());
                    if !detail_loaded && app.begin_detail_fetch(&key) {
                        spawn_ticket_detail_fetch(bg_tx, key);
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
                spawn_cache_refresh(bg_tx, CacheRefreshPhase::Manual, config);
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
                spawn_cache_refresh(bg_tx, CacheRefreshPhase::Manual, config);
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
                    spawn_ticket_detail_fetch(bg_tx, key);
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
            },
            team: BTreeMap::new(),
            statuses: crate::config::StatusConfig::default(),
            resolutions: crate::config::default_resolutions(),
            filters: vec![],
        }
    }

    fn ticket(key: &str, summary: &str, status: Status) -> crate::cache::Ticket {
        crate::cache::Ticket {
            key: key.to_string(),
            summary: summary.to_string(),
            status,
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
            status: Status::InProgress,
            resolution: None,
        };
        let summary = summarize_bulk_results(
            BulkAction::Move,
            target.clone(),
            2,
            2,
            0,
            vec![("AMP-1".to_string(), Ok(())), ("AMP-2".to_string(), Ok(()))],
        );
        assert_eq!(summary.target, target);
        assert_eq!(summary.succeeded, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn summarize_bulk_results_partial_failure_and_skips() {
        let summary = summarize_bulk_results(
            BulkAction::Assign,
            BulkTarget::Assign {
                member_email: "dev@example.com".to_string(),
                member_name: "Dev".to_string(),
            },
            3,
            2,
            1,
            vec![
                ("AMP-1".to_string(), Ok(())),
                ("AMP-2".to_string(), Err("boom".to_string())),
            ],
        );
        assert_eq!(summary.total, 3);
        assert_eq!(summary.attempted, 2);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.failed_details.len(), 1);
    }

    #[test]
    fn summarize_bulk_results_empty_attempts() {
        let summary = summarize_bulk_results(
            BulkAction::Move,
            BulkTarget::Move {
                status: Status::Closed,
                resolution: Some("Done".to_string()),
            },
            2,
            0,
            2,
            vec![],
        );
        assert_eq!(summary.total, 2);
        assert_eq!(summary.attempted, 0);
        assert_eq!(summary.succeeded, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 2);
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
