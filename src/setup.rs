use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use std::io;
use std::time::Duration;

use crate::config::{AppConfig, JiraConfig, StatusConfig};

enum SetupStep {
    ProjectKey,
    TeamName,
    Confirm,
}

struct SetupState {
    step: SetupStep,
    project_key: String,
    team_name: String,
    user_email: String,
}

pub async fn run_setup(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<AppConfig> {
    let user_email = crate::jira_client::fetch_my_email()
        .await
        .unwrap_or_else(|_| "unknown@mongodb.com".to_string());

    let mut state = SetupState {
        step: SetupStep::ProjectKey,
        project_key: String::new(),
        team_name: String::new(),
        user_email,
    };

    loop {
        terminal.draw(|f| render_setup(f, &state))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match &state.step {
                    SetupStep::ProjectKey => match key.code {
                        KeyCode::Enter if !state.project_key.is_empty() => {
                            state.step = SetupStep::TeamName;
                        }
                        KeyCode::Char(c) if c.is_ascii_alphanumeric() => {
                            state.project_key.push(c.to_ascii_uppercase());
                        }
                        KeyCode::Backspace => {
                            state.project_key.pop();
                        }
                        _ => {}
                    },
                    SetupStep::TeamName => match key.code {
                        KeyCode::Enter if !state.team_name.is_empty() => {
                            state.step = SetupStep::Confirm;
                        }
                        KeyCode::Esc => {
                            state.step = SetupStep::ProjectKey;
                        }
                        KeyCode::Char(c) => {
                            state.team_name.push(c);
                        }
                        KeyCode::Backspace => {
                            state.team_name.pop();
                        }
                        _ => {}
                    },
                    SetupStep::Confirm => match key.code {
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                            let config = build_config(&state);
                            crate::config::save_config(&config)?;
                            return Ok(config);
                        }
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                            state.step = SetupStep::ProjectKey;
                            state.project_key.clear();
                            state.team_name.clear();
                        }
                        _ => {}
                    },
                }
            }
        }
    }
}

fn build_config(state: &SetupState) -> AppConfig {
    let mut team = std::collections::BTreeMap::new();
    let user_name = crate::jira_client::name_from_email(&state.user_email);
    team.insert(user_name, state.user_email.clone());

    // Try to migrate legacy team.yml
    if let Ok(legacy_members) = migrate_legacy_team_roster() {
        for (name, email) in legacy_members {
            team.entry(name).or_insert(email);
        }
    }

    AppConfig {
        jira: JiraConfig {
            project: state.project_key.clone(),
            team_name: state.team_name.clone(),
            done_window_days: 14,
            epics_i_care_about: vec![],
        },
        team,
        statuses: StatusConfig::default(),
        resolutions: crate::config::default_resolutions(),
        filters: vec![],
    }
}

fn migrate_legacy_team_roster() -> Result<Vec<(String, String)>> {
    let home = std::env::var("HOME")?;
    let path = format!("{}/.claude/skills/jira/team.yml", home);
    let content = std::fs::read_to_string(&path)?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&content)?;
    let team_map = yaml
        .get("team")
        .and_then(|v| v.as_mapping())
        .ok_or_else(|| anyhow::anyhow!("no team mapping"))?;

    let mut members = Vec::new();
    for (name_val, email_val) in team_map {
        let name = name_val.as_str().unwrap_or_default().to_string();
        let email = email_val.as_str().unwrap_or_default().to_string();
        if !email.is_empty() {
            members.push((name, email));
        }
    }
    Ok(members)
}

fn render_setup(f: &mut ratatui::Frame, state: &SetupState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(f.area());

    let title = Paragraph::new(" lazyjira — First-Time Setup ")
        .block(Block::default().borders(Borders::ALL))
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(title, chunks[0]);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Welcome! Let's configure lazyjira for your team.",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
    ];

    match state.step {
        SetupStep::ProjectKey => {
            lines.push(Line::from(vec![
                Span::styled("Jira Project Key: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    &state.project_key,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("_", Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  (e.g., AMP, SERVER, CLOUD)  Press Enter to continue.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        SetupStep::TeamName => {
            lines.push(Line::from(vec![
                Span::styled("Project: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&state.project_key),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    "Team Name (for unassigned queries): ",
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    &state.team_name,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("_", Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Press Enter to continue, Esc to go back.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        SetupStep::Confirm => {
            lines.push(Line::from(Span::styled(
                "Configuration Summary:",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Project:    ", Style::default().fg(Color::DarkGray)),
                Span::raw(&state.project_key),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Team:       ", Style::default().fg(Color::DarkGray)),
                Span::raw(&state.team_name),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  User:       ", Style::default().fg(Color::DarkGray)),
                Span::raw(&state.user_email),
            ]));
            lines.push(Line::from(""));

            let config_path = crate::config::config_path();
            let path_str = match config_path {
                Ok(p) => p.display().to_string(),
                Err(_) => "~/.config/lazyjira/config.toml".to_string(),
            };
            lines.push(Line::from(vec![
                Span::styled("  Config:     ", Style::default().fg(Color::DarkGray)),
                Span::raw(path_str),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Press Enter or y to save.  Esc or n to start over.",
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    let body = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));
    f.render_widget(body, chunks[1]);
}
