use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use std::io;
use std::time::Duration;

use crate::config::{AppConfig, JiraConfig, StatusConfig};
use crate::style;
use crate::widgets::form;

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
    config_path: String,
}

pub async fn run_setup(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<AppConfig> {
    let user_email = crate::jira_client::fetch_my_email()
        .await
        .unwrap_or_else(|_| "unknown@example.com".to_string());

    let mut state = SetupState {
        step: SetupStep::ProjectKey,
        project_key: String::new(),
        team_name: String::new(),
        user_email,
        config_path: config_path_display(),
    };

    loop {
        terminal.draw(|f| render_setup(f, &state))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if press(&mut state, key.code) {
                    let config = build_config(&state);
                    crate::config::save_config(&config)?;
                    return Ok(config);
                }
            }
        }
    }
}

/// Applies `key` to the setup. True when it saves the configuration.
fn press(state: &mut SetupState, key: KeyCode) -> bool {
    match &state.step {
        SetupStep::ProjectKey => match key {
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
        SetupStep::TeamName => match key {
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
        SetupStep::Confirm => match key {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => return true,
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                state.step = SetupStep::ProjectKey;
                state.project_key.clear();
                state.team_name.clear();
            }
            _ => {}
        },
    }
    false
}

fn build_config(state: &SetupState) -> AppConfig {
    let mut team = std::collections::BTreeMap::new();
    let user_name = crate::cache::name_from_email(&state.user_email);
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
        filters: vec![],
        preferences: Default::default(),
        themes: Default::default(),
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

    Ok(team_map
        .iter()
        .filter_map(|(name_val, email_val)| {
            let email = email_val.as_str().unwrap_or_default();
            if email.is_empty() {
                return None;
            }

            Some((
                name_val.as_str().unwrap_or_default().to_string(),
                email.to_string(),
            ))
        })
        .collect())
}

fn config_path_display() -> String {
    crate::config::config_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "~/.config/lazyjira/config.toml".to_string())
}

/// Cells of a field's label: the longest, "Project key", and two spaces.
const LABEL_WIDTH: usize = 13;

/// A field: its `label` (the accent while it has focus, muted otherwise), then `value`, then the
/// cursor while it has focus.
fn field(label: &str, value: &str, focused: bool) -> Line<'static> {
    let label_style = if focused {
        style::accent().patch(style::heading())
    } else {
        style::secondary()
    };
    let mut spans = vec![
        Span::styled(format!("{label:<LABEL_WIDTH$}"), label_style),
        Span::styled(value.to_string(), style::primary()),
    ];
    if focused {
        spans.push(Span::styled(" ", style::cursor()));
    }
    Line::from(spans)
}

/// A muted line under a field, lined up with its value.
fn note(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("{}{text}", " ".repeat(LABEL_WIDTH)),
        style::secondary(),
    ))
}

/// The row of buttons that ends a step, with `hints` after them.
fn buttons(labels: &[&str], hints: crate::footer::Pairs, width: u16) -> Line<'static> {
    let mut spans = Vec::new();
    for label in labels {
        spans.push(Span::styled(format!(" {label} "), style::button()));
        spans.push(Span::raw("  "));
    }
    let used: usize = spans.iter().map(Span::width).sum();
    let room = width.saturating_sub(used as u16);
    spans.extend(crate::footer::pairs_line(hints, room).spans);
    Line::from(spans)
}

fn render_setup(f: &mut ratatui::Frame, state: &SetupState) {
    let [title, _, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(f.area());
    f.render_widget(Paragraph::new(form::title("First-time setup", "")), title);
    let body = Rect::new(
        body.x.saturating_add(2),
        body.y,
        body.width.saturating_sub(4),
        body.height,
    );
    let [text, last] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(body);

    let mut lines = vec![
        Line::from("Welcome! Let's configure lazyjira for your team."),
        Line::from(""),
    ];
    let row = match state.step {
        SetupStep::ProjectKey => {
            lines.push(field("Project key", &state.project_key, true));
            lines.push(note("e.g. DEMO, SERVER, CLOUD"));
            buttons(&["Continue"], &[("Enter", "continue")], last.width)
        }
        SetupStep::TeamName => {
            lines.push(field("Project key", &state.project_key, false));
            lines.push(Line::from(""));
            lines.push(field("Team name", &state.team_name, true));
            lines.push(note("finds the team's unassigned tickets"));
            buttons(
                &["Continue", "Back"],
                &[("Enter", "continue"), ("Esc", "back")],
                last.width,
            )
        }
        SetupStep::Confirm => {
            lines.push(Line::from(Span::styled(
                "Save this configuration?",
                style::heading(),
            )));
            lines.push(Line::from(""));
            lines.push(field("Project key", &state.project_key, false));
            lines.push(field("Team name", &state.team_name, false));
            lines.push(field("User", &state.user_email, false));
            lines.push(field("Config", &state.config_path, false));
            buttons(
                &["Save", "Start over"],
                &[("Enter/y", "save"), ("Esc/n", "start over")],
                last.width,
            )
        }
    };
    f.render_widget(Paragraph::new(lines), text);
    f.render_widget(Paragraph::new(row), last);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn state(step: SetupStep) -> SetupState {
        SetupState {
            step,
            project_key: "DEMO".into(),
            team_name: "Payments".into(),
            user_email: "alex.rivera@example.com".into(),
            config_path: "~/.config/lazyjira/config.toml".into(),
        }
    }

    fn screen(state: &SetupState, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| render_setup(f, state)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                let row: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
                row.trim_end().to_string()
            })
            .collect()
    }

    #[test]
    fn each_step_at_80x24_is_a_title_unboxed_fields_and_buttons() {
        let mut project = state(SetupStep::ProjectKey);
        project.team_name.clear();
        let confirm = state(SetupStep::Confirm);
        let screens = [
            (
                project,
                vec![
                    " First-time setup",
                    "",
                    "  Welcome! Let's configure lazyjira for your team.",
                    "",
                    "  Project key  DEMO",
                    "               e.g. DEMO, SERVER, CLOUD",
                ],
                "   Continue    Enter continue",
            ),
            (
                state(SetupStep::TeamName),
                vec![
                    " First-time setup",
                    "",
                    "  Welcome! Let's configure lazyjira for your team.",
                    "",
                    "  Project key  DEMO",
                    "",
                    "  Team name    Payments",
                    "               finds the team's unassigned tickets",
                ],
                "   Continue    Back    Enter continue · Esc back",
            ),
            (
                confirm,
                vec![
                    " First-time setup",
                    "",
                    "  Welcome! Let's configure lazyjira for your team.",
                    "",
                    "  Save this configuration?",
                    "",
                    "  Project key  DEMO",
                    "  Team name    Payments",
                    "  User         alex.rivera@example.com",
                    "  Config       ~/.config/lazyjira/config.toml",
                ],
                "   Save    Start over    Enter/y save · Esc/n start over",
            ),
        ];
        for (state, body, buttons) in screens {
            let rows = screen(&state, 80, 24);
            assert_eq!(rows.len(), 24);
            assert_eq!(rows[..body.len()], body, "{rows:#?}");
            assert!(
                rows[body.len()..23].iter().all(String::is_empty),
                "{rows:#?}"
            );
            assert_eq!(rows[23], buttons);
        }
    }

    #[test]
    fn the_setup_at_60x20_keeps_the_buttons_on_the_last_row_and_draws_no_box_or_inline_color() {
        use ratatui::style::Color;
        for step in [
            SetupStep::ProjectKey,
            SetupStep::TeamName,
            SetupStep::Confirm,
        ] {
            let state = state(step);
            let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
            terminal.draw(|f| render_setup(f, &state)).unwrap();
            let buffer = terminal.backend().buffer();
            let all: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            assert!(!all.contains(['─', '│', '┌', '—']), "{all}");
            // Only the roles of `style` appear: the old screen was yellow.
            assert!(
                buffer
                    .content
                    .iter()
                    .all(|cell| !matches!(cell.fg, Color::Yellow | Color::Rgb(..))),
                "{all}"
            );
            let last: String = (0..60).map(|x| buffer[(x, 19)].symbol()).collect();
            assert!(last.trim_start().starts_with(['C', 'S']), "{last}");
        }
    }

    #[test]
    fn keys_type_the_project_key_in_capitals_and_move_between_steps_as_before() {
        let mut state = state(SetupStep::ProjectKey);
        state.project_key.clear();
        state.team_name.clear();
        assert!(!press(&mut state, KeyCode::Enter));
        assert!(
            matches!(state.step, SetupStep::ProjectKey),
            "empty key stays"
        );
        for c in ['d', 'e', '-', ' ', 'm', 'o', '1'] {
            press(&mut state, KeyCode::Char(c));
        }
        assert_eq!(state.project_key, "DEMO1");
        press(&mut state, KeyCode::Backspace);
        assert_eq!(state.project_key, "DEMO");
        press(&mut state, KeyCode::Enter);
        assert!(matches!(state.step, SetupStep::TeamName));

        assert!(!press(&mut state, KeyCode::Enter));
        assert!(
            matches!(state.step, SetupStep::TeamName),
            "empty name stays"
        );
        for c in "Pay me".chars() {
            press(&mut state, KeyCode::Char(c));
        }
        press(&mut state, KeyCode::Backspace);
        assert_eq!(state.team_name, "Pay m");
        press(&mut state, KeyCode::Esc);
        assert!(matches!(state.step, SetupStep::ProjectKey));
        assert_eq!(
            (state.project_key.as_str(), state.team_name.as_str()),
            ("DEMO", "Pay m")
        );
        press(&mut state, KeyCode::Enter);
        press(&mut state, KeyCode::Enter);
        assert!(matches!(state.step, SetupStep::Confirm));

        assert!(!press(&mut state, KeyCode::Char('x')));
        for key in [KeyCode::Esc, KeyCode::Char('n'), KeyCode::Char('N')] {
            let mut again = SetupState {
                step: SetupStep::Confirm,
                ..self::state(SetupStep::Confirm)
            };
            assert!(!press(&mut again, key), "{key:?}");
            assert!(matches!(again.step, SetupStep::ProjectKey));
            assert!(again.project_key.is_empty() && again.team_name.is_empty());
        }
        for key in [KeyCode::Enter, KeyCode::Char('y'), KeyCode::Char('Y')] {
            assert!(press(&mut state, key), "{key:?} saves");
        }
    }
}
