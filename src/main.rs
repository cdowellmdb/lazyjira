mod app;
mod cache;
mod jira_client;
mod views;
mod widgets;

use std::io;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, DetailMode, Tab};

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    // Fetch data on startup
    let cache = jira_client::fetch_all().await?;
    app.cache = cache;
    app.loading = false;

    // Main loop
    loop {
        terminal.draw(|f| ui(f, &app))?;

        if let Event::Key(key) = event::read()? {
            // Clear flash on any keypress
            app.flash = None;

            if app.is_detail_open() {
                handle_detail_keys(&mut app, key.code);
            } else if app.search.is_some() {
                handle_search_keys(&mut app, key.code);
            } else {
                handle_main_keys(&mut app, key.code, key.modifiers).await;
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

fn ui(f: &mut ratatui::Frame, app: &App) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Tabs};

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),   // Content
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    // Tab bar
    let tab_titles: Vec<Line> = Tab::all()
        .iter()
        .map(|t| Line::from(t.title()))
        .collect();
    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title(" lazyjira "))
        .select(match app.active_tab {
            Tab::MyWork => 0,
            Tab::Team => 1,
            Tab::Epics => 2,
        })
        .style(Style::default().fg(Color::Gray))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
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
        }
    }

    // Status bar
    let status_text = if let Some(ref flash) = app.flash {
        Span::styled(flash.as_str(), Style::default().fg(Color::Red))
    } else if let Some(ref search) = app.search {
        Span::styled(format!("/{}", search), Style::default().fg(Color::Yellow))
    } else {
        Span::styled(
            " Tab: switch  j/k: navigate  Enter: detail  r: refresh  /: search  q: quit ",
            Style::default().fg(Color::DarkGray),
        )
    };
    f.render_widget(ratatui::widgets::Paragraph::new(Line::from(status_text)), chunks[2]);

    // Detail overlay
    if app.is_detail_open() {
        widgets::ticket_detail::render(f, app);
    }
}

fn handle_detail_keys(app: &mut App, key: KeyCode) {
    match &app.detail_mode {
        DetailMode::View => match key {
            KeyCode::Esc => app.close_detail(),
            KeyCode::Char('o') => {
                if let Some(ref key) = app.detail_ticket_key {
                    if let Some(ticket) = app.find_ticket(key) {
                        let url = ticket.url.clone();
                        let _ = std::process::Command::new("open").arg(&url).spawn();
                    }
                }
            }
            KeyCode::Char('m') => {
                app.detail_mode = DetailMode::MovePicker { selected: 0 };
            }
            _ => {}
        },
        DetailMode::MovePicker { selected } => match key {
            KeyCode::Esc => app.detail_mode = DetailMode::View,
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(ref key) = app.detail_ticket_key {
                    if let Some(ticket) = app.find_ticket(key) {
                        let options = ticket.status.others();
                        let new_sel = (*selected + 1).min(options.len().saturating_sub(1));
                        app.detail_mode = DetailMode::MovePicker { selected: new_sel };
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let new_sel = selected.saturating_sub(1);
                app.detail_mode = DetailMode::MovePicker { selected: new_sel };
            }
            KeyCode::Enter => {
                if let Some(ticket_key) = app.detail_ticket_key.clone() {
                    if let Some(ticket) = app.find_ticket(&ticket_key) {
                        let options = ticket.status.others();
                        if let Some(new_status) = options.get(*selected) {
                            let new_status = (*new_status).clone();
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
                    }
                }
            }
            _ => {}
        },
    }
}

fn handle_search_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => app.search = None,
        KeyCode::Enter => {
            // Search is active, just close the input
            // The filter remains applied until Esc
        }
        KeyCode::Backspace => {
            if let Some(ref mut s) = app.search {
                s.pop();
                if s.is_empty() {
                    app.search = None;
                }
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut s) = app.search {
                s.push(c);
            }
        }
        _ => {}
    }
}

async fn handle_main_keys(app: &mut App, key: KeyCode, _modifiers: KeyModifiers) {
    match key {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Tab => app.next_tab(),
        KeyCode::Char('j') | KeyCode::Down => app.move_selection_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection_up(),
        KeyCode::Char('/') => app.search = Some(String::new()),
        KeyCode::Char('r') => {
            app.loading = true;
            match jira_client::fetch_all().await {
                Ok(cache) => {
                    app.cache = cache;
                    app.flash = Some("Refreshed!".to_string());
                }
                Err(e) => {
                    app.flash = Some(format!("Refresh failed: {}", e));
                }
            }
            app.loading = false;
        }
        KeyCode::Enter => {
            if let Some(key) = app.selected_ticket_key() {
                app.open_detail(key);
            }
        }
        _ => {}
    }
}
