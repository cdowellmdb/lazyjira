mod app;
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
use std::io;
use std::process::Command;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

use crate::cache::Status;
use crate::config::AppConfig;
use app::{App, DetailMode, FilterFocus, Tab, TicketSyncStage};

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

fn spawn_cache_refresh(tx: &UnboundedSender<BackgroundMessage>, phase: CacheRefreshPhase, config: &AppConfig) {
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
                        if let Err(e) = jira_client::save_full_cache_snapshot(&config.jira.project, &app.cache) {
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
                        if let Err(e) = jira_client::save_full_cache_snapshot(&config.jira.project, &app.cache) {
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
                BackgroundMessage::CommentAdded(result) => {
                    match result {
                        Ok(key) => {
                            app.flash = Some(format!("Comment added to {}", key));
                        }
                        Err(e) => {
                            app.flash = Some(format!("Comment failed: {}", e));
                        }
                    }
                }
                BackgroundMessage::TicketAssigned { key, result } => {
                    match result {
                        Ok(()) => {
                            app.flash = Some(format!("Assigned {}", key));
                        }
                        Err(e) => {
                            app.flash = Some(format!("Assign failed for {}: {}", key, e));
                        }
                    }
                }
                BackgroundMessage::TicketEdited { key, result } => {
                    match result {
                        Ok(()) => {
                            app.flash = Some(format!("Updated {}", key));
                        }
                        Err(e) => {
                            app.flash = Some(format!("Edit failed for {}: {}", key, e));
                        }
                    }
                }
                BackgroundMessage::FilterResults(result) => {
                    app.filter_loading = false;
                    match result {
                        Ok(tickets) => {
                            let count = tickets.len();
                            app.filter_results = tickets;
                            app.mark_cache_changed();
                            app.filter_focus = FilterFocus::Results;
                            app.selected_index = 0;
                            app.flash = Some(format!("Filter returned {} tickets", count));
                        }
                        Err(e) => {
                            app.filter_results.clear();
                            app.mark_cache_changed();
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
                    } else if app.is_create_ticket_open() {
                        handle_create_ticket_keys(&mut app, key.code, key.modifiers, &bg_tx, &config).await;
                    } else if app.is_comment_open() {
                        handle_comment_keys(&mut app, key.code, &bg_tx);
                    } else if app.is_assign_open() {
                        handle_assign_keys(&mut app, key.code, &bg_tx);
                    } else if app.is_edit_open() {
                        handle_edit_keys(&mut app, key.code, &bg_tx);
                    } else if app.show_keybindings {
                        handle_keybindings_keys(&mut app, key.code);
                    } else if app.is_detail_open() {
                        handle_detail_keys(&mut app, key.code);
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
                " Tab: switch  j/k: navigate  Enter: detail  d: done({})  p/w/n/v: focus({})  ?: keys  t:{}  c:{}  e:{}  r: refresh  /: search  q: quit ",
                done_state, focus_state, ticket_state, freshness_state, epic_state
            ),
            Style::default().fg(Color::DarkGray),
        )
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

fn perform_ticket_move(app: &mut App, ticket_key: String, new_status: Status) {
    let status_str = new_status.as_str().to_string();
    let key_clone = ticket_key.clone();

    // Optimistic update
    app.update_ticket_status(&ticket_key, new_status);
    app.detail_mode = DetailMode::View;
    app.flash = Some(format!("Moving {} to {}...", key_clone, status_str));

    // Fire and forget the CLI call
    tokio::spawn(async move {
        let _ = jira_client::move_ticket(&key_clone, &status_str).await;
    });
}

fn handle_detail_keys(app: &mut App, key: KeyCode) {
    match app.detail_mode.clone() {
        DetailMode::View => match key {
            KeyCode::Esc => app.close_detail(),
            KeyCode::Up => app.scroll_detail_up(),
            KeyCode::Down => app.scroll_detail_down(),
            KeyCode::Char('o') => {
                if let Some(ref key) = app.detail_ticket_key {
                    if let Some(ticket) = app.find_ticket(key) {
                        let url = ticket.url.clone();
                        let _ = std::process::Command::new("open").arg(&url).spawn();
                    }
                }
            }
            KeyCode::Char('m') => {
                app.detail_mode = DetailMode::MovePicker {
                    selected: 0,
                    confirm_target: None,
                };
            }
            KeyCode::Char('C') => {
                if let Some(ref key) = app.detail_ticket_key {
                    app.comment_state = Some(app::CommentState {
                        ticket_key: key.clone(),
                        body: String::new(),
                    });
                }
            }
            KeyCode::Char('a') => {
                if let Some(ref key) = app.detail_ticket_key {
                    app.assign_state = Some(app::AssignState {
                        ticket_key: key.clone(),
                        selected: 0,
                    });
                }
            }
            KeyCode::Char('e') => {
                if let Some(ref key) = app.detail_ticket_key {
                    let ticket = app.find_ticket(key);
                    let summary = ticket.map(|t| t.summary.clone()).unwrap_or_default();
                    let labels = ticket
                        .map(|t| t.labels.join(", "))
                        .unwrap_or_default();
                    app.edit_state = Some(app::EditFieldsState {
                        ticket_key: key.clone(),
                        focused_field: 0,
                        summary,
                        labels,
                    });
                }
            }
            KeyCode::Char('h') => {
                app.detail_mode = DetailMode::History { scroll: 0 };
            }
            _ => {}
        },
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
                            perform_ticket_move(app, ticket_key, target);
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
                            perform_ticket_move(app, ticket_key, target);
                        }
                    }
                }
            }
            KeyCode::Char(c) => {
                if let Some(target_status) = Status::from_move_shortcut(c) {
                    if let Some((ticket_key, options)) = current_move_options(app) {
                        if let Some(target_idx) = options.iter().position(|s| *s == target_status) {
                            if c.is_ascii_uppercase() {
                                perform_ticket_move(app, ticket_key, target_status);
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
        DetailMode::History { scroll } => match key {
            KeyCode::Esc => app.detail_mode = DetailMode::View,
            KeyCode::Down | KeyCode::Char('j') => {
                app.detail_mode = DetailMode::History { scroll: scroll + 1 };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.detail_mode = DetailMode::History { scroll: scroll.saturating_sub(1) };
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
            if let Some(key) = app.selected_ticket_key() {
                let detail_loaded = app.is_ticket_detail_loaded(&key);
                app.open_detail(key.clone());
                if !detail_loaded && app.begin_detail_fetch(&key) {
                    spawn_ticket_detail_fetch(bg_tx, key);
                }
            }
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
            state.focused_field = if state.focused_field == 0 { 3 } else { state.focused_field - 1 };
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
    bg_tx: &UnboundedSender<BackgroundMessage>,
) {
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

fn handle_assign_keys(
    app: &mut App,
    key: KeyCode,
    bg_tx: &UnboundedSender<BackgroundMessage>,
) {
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

fn handle_edit_keys(
    app: &mut App,
    key: KeyCode,
    bg_tx: &UnboundedSender<BackgroundMessage>,
) {
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
                    0 => { state.summary.pop(); }
                    1 => { state.labels.pop(); }
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
            0 => { state.name.pop(); }
            1 => { state.jql.pop(); }
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
        KeyCode::Char('j') | KeyCode::Down => match app.filter_focus {
            FilterFocus::Sidebar => {
                if !config.filters.is_empty()
                    && app.filter_sidebar_idx < config.filters.len() - 1
                {
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
        KeyCode::Char('c') => {
            app.create_ticket = Some(app::CreateTicketState {
                focused_field: 0,
                issue_type_idx: 0,
                summary: String::new(),
                assignee_idx: 0,
                epic_idx: 0,
            });
        }
        KeyCode::Enter => {
            if let Some(key) = app.selected_ticket_key() {
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
