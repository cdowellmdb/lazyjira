mod app;
mod cache;
mod config;
mod jira_client;
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
use app::{App, DetailMode, Tab, TicketSyncStage};

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
}

fn spawn_epics_refresh(tx: &UnboundedSender<BackgroundMessage>) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = jira_client::refresh_epics_cache()
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(BackgroundMessage::EpicsRefreshed(result));
    });
}

fn spawn_cache_refresh(tx: &UnboundedSender<BackgroundMessage>, phase: CacheRefreshPhase) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = match phase {
            CacheRefreshPhase::ActiveOnly => jira_client::fetch_active_only().await,
            CacheRefreshPhase::Full => jira_client::fetch_all().await,
            CacheRefreshPhase::Manual => jira_client::fetch_all().await,
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

    let mut app = App::new();
    let (bg_tx, mut bg_rx) = tokio::sync::mpsc::unbounded_channel();
    let detail_cache_tx = jira_client::spawn_detail_cache_writer();

    // Fast startup: load persisted snapshot immediately, then revalidate in stages.
    if let Some(snapshot) = jira_client::load_startup_cache_snapshot() {
        app.replace_cache(snapshot.cache);
        app.loading = false;
        app.cache_stale_age_secs = Some(snapshot.age_secs);
        app.ticket_sync_stage = Some(TicketSyncStage::ActiveOnly);
        app.flash = Some("Loaded cached data. Refreshing active tickets...".to_string());
        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::ActiveOnly);
    } else {
        let cache = jira_client::fetch_active_only().await?;
        app.replace_cache(cache);
        app.loading = false;
        app.ticket_sync_stage = Some(TicketSyncStage::Full);
        app.flash = Some("Loaded active tickets. Syncing recently done...".to_string());
        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::Full);
    }

    spawn_epics_refresh(&bg_tx);
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
                        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::Full);
                    }
                    (CacheRefreshPhase::ActiveOnly, Err(e))
                        if app.ticket_sync_stage == Some(TicketSyncStage::ActiveOnly) =>
                    {
                        app.ticket_sync_stage = Some(TicketSyncStage::Full);
                        app.flash = Some(format!(
                            "Active refresh failed ({}). Trying full refresh...",
                            e
                        ));
                        spawn_cache_refresh(&bg_tx, CacheRefreshPhase::Full);
                    }
                    (CacheRefreshPhase::Full, Ok(cache))
                        if app.ticket_sync_stage == Some(TicketSyncStage::Full) =>
                    {
                        app.replace_cache(cache);
                        app.cache_stale_age_secs = None;
                        app.ticket_sync_stage = None;
                        app.clamp_selection();
                        queue_detail_prefetch(&mut app, &bg_tx);
                        if let Err(e) = jira_client::save_full_cache_snapshot(&app.cache) {
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
                        if let Err(e) = jira_client::save_full_cache_snapshot(&app.cache) {
                            app.flash = Some(format!("Refreshed (cache save failed: {})", e));
                        } else {
                            app.flash =
                                Some("Refreshed! Syncing epic relationships...".to_string());
                        }
                        if !app.epics_refreshing {
                            app.epics_refreshing = true;
                            spawn_epics_refresh(&bg_tx);
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
            }
        }

        if state_changed {
            draw_needed = true;
        }
        if draw_needed {
            terminal.draw(|f| ui(f, &app))?;
            draw_needed = false;
        }

        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Key(key) => {
                    // Clear flash on any keypress
                    app.flash = None;

                    if app.show_keybindings {
                        handle_keybindings_keys(&mut app, key.code);
                    } else if app.is_detail_open() {
                        handle_detail_keys(&mut app, key.code);
                    } else if app.search.is_some() {
                        handle_search_keys(&mut app, key.code, key.modifiers, &bg_tx).await;
                    } else {
                        handle_main_keys(&mut app, key.code, key.modifiers, &bg_tx).await;
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

fn ui(f: &mut ratatui::Frame, app: &App) {
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
    if app.show_keybindings {
        widgets::keybindings_help::render(f);
    }
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

async fn handle_main_keys(
    app: &mut App,
    key: KeyCode,
    _modifiers: KeyModifiers,
    bg_tx: &UnboundedSender<BackgroundMessage>,
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
                spawn_cache_refresh(bg_tx, CacheRefreshPhase::Manual);
            }
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
