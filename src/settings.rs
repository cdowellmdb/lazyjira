use anyhow::{bail, Result};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use std::collections::BTreeMap;
use tui_textarea::TextArea;

use crate::{
    app::{App, Tab},
    config::AppConfig,
    theme,
    widgets::form,
};

/// Teammates, pinned epics, starting tab, Done tickets, theme.
pub const FIELDS: usize = 5;

pub struct Settings {
    pub focused_field: usize,
    pub team: TextArea<'static>,
    pub epics: TextArea<'static>,
    pub start_tab: usize,
    pub show_done: bool,
    /// Every theme name to offer; the chosen one previews while settings are open.
    pub themes: Vec<String>,
    pub theme: usize,
}

impl Settings {
    pub fn new(config: &AppConfig) -> Self {
        let themes = theme::names(&config.themes);
        Self {
            focused_field: 0,
            team: form::editor(
                &config
                    .team
                    .iter()
                    .map(|(name, email)| format!("{name} = {email}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            epics: form::editor(&config.epics_i_care_about_ordered().join(", ")),
            start_tab: Tab::all()
                .iter()
                .position(|tab| tab.title() == config.preferences.start_tab)
                .unwrap_or(0),
            show_done: config.preferences.show_done,
            theme: themes
                .iter()
                .position(|name| name.eq_ignore_ascii_case(&config.preferences.theme))
                .unwrap_or(0),
            themes,
        }
    }

    fn config(&self, current: &AppConfig) -> Result<AppConfig> {
        let mut config = current.clone();
        let mut team = BTreeMap::new();
        for line in self
            .team
            .lines()
            .iter()
            .filter(|line| !line.trim().is_empty())
        {
            let Some((name, email)) = line.split_once('=') else {
                bail!("Use one teammate per line: Name = email");
            };
            let (name, email) = (name.trim(), email.trim());
            let valid_email = email
                .split_once('@')
                .is_some_and(|(user, host)| !user.is_empty() && !host.is_empty());
            if name.is_empty() || !valid_email || email.chars().any(char::is_whitespace) {
                bail!("Each teammate needs a name and email: {line}");
            }
            if team.insert(name.to_string(), email.to_string()).is_some() {
                bail!("Duplicate teammate name: {name}");
            }
        }
        let epics: Vec<String> = form::text(&self.epics)
            .split([',', '\n'])
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(str::to_ascii_uppercase)
            .collect();
        for key in &epics {
            if !key.split_once('-').is_some_and(|(project, number)| {
                !project.is_empty()
                    && project
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !number.is_empty()
                    && number.chars().all(|c| c.is_ascii_digit())
            }) {
                bail!("Invalid epic key: {key}");
            }
        }
        config.team = team;
        config.jira.epics_i_care_about = epics;
        config.preferences.start_tab = Tab::all()[self.start_tab].title().into();
        config.preferences.show_done = self.show_done;
        config.preferences.theme = self.themes[self.theme].clone();
        Ok(config)
    }
}

/// True when settings were saved and the team cache needs refreshing.
pub fn handle_key(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    config: &mut AppConfig,
) -> bool {
    let Some(state) = app.settings.as_mut() else {
        return false;
    };
    match key {
        KeyCode::Esc => app.settings = None,
        KeyCode::Tab => state.focused_field = (state.focused_field + 1) % FIELDS,
        KeyCode::BackTab => state.focused_field = (state.focused_field + FIELDS - 1) % FIELDS,
        KeyCode::Enter if !modifiers.contains(KeyModifiers::SHIFT) => {
            match state.config(config).and_then(|next| {
                crate::config::save_config(&next)?;
                Ok(next)
            }) {
                Ok(next) => {
                    *config = next;
                    app.show_done = config.preferences.show_done;
                    app.theme = theme::resolve(&config.preferences.theme, &config.themes);
                    app.set_epics_i_care_about(config.epics_i_care_about_ordered());
                    app.settings = None;
                    app.flash = Some("Preferences saved. Refreshing team...".into());
                    return true;
                }
                Err(error) => app.flash = Some(error.to_string()),
            }
        }
        _ => match state.focused_field {
            0 => form::input(&mut state.team, key, modifiers, true),
            1 => form::input(&mut state.epics, key, modifiers, false),
            2 if matches!(key, KeyCode::Left | KeyCode::Up) => {
                state.start_tab = state.start_tab.saturating_sub(1)
            }
            2 if matches!(key, KeyCode::Right | KeyCode::Down) => {
                state.start_tab = (state.start_tab + 1).min(4)
            }
            3 if matches!(
                key,
                KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
            ) =>
            {
                state.show_done = !state.show_done
            }
            4 if matches!(key, KeyCode::Up | KeyCode::Left) => {
                state.theme = state.theme.saturating_sub(1)
            }
            4 if matches!(key, KeyCode::Down | KeyCode::Right) => {
                state.theme = (state.theme + 1).min(state.themes.len() - 1)
            }
            _ => {}
        },
    }
    false
}

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let Some(state) = &app.settings else {
        return;
    };
    let inner = form::render_modal_frame(f, app, "Settings", 85, 90);
    let areas = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Length(4),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let [start_tab, theme] = Layout::horizontal([Constraint::Fill(1); 2])
        .spacing(1)
        .areas(areas[2]);
    form::render_editor(
        f,
        app,
        areas[0],
        "Teammates · Name = email, one per line",
        &state.team,
        state.focused_field == 0,
        0,
    );
    form::render_editor(
        f,
        app,
        areas[1],
        "Pinned epics · comma-separated; empty shows all",
        &state.epics,
        state.focused_field == 1,
        1,
    );
    form::render_choices(
        f,
        app,
        start_tab,
        (2, "Starting tab"),
        &Tab::all()
            .iter()
            .map(|tab| tab.title().into())
            .collect::<Vec<_>>(),
        state.start_tab,
        "",
    );
    form::render_choices(
        f,
        app,
        areas[3],
        (3, "Done tickets"),
        &["Hide".into(), "Show".into()],
        usize::from(state.show_done),
        "",
    );
    form::render_choices(f, app, theme, (4, "Theme"), &state.themes, state.theme, "");
    f.render_widget(
        ratatui::widgets::Paragraph::new(
            "Tab: field · ↑↓: choose · Shift+Enter: newline · Enter: save",
        ),
        areas[4],
    );
    form::buttons(
        f,
        app,
        areas[5],
        &[
            ("Save", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_validate_and_preserve_other_configuration() {
        let config: AppConfig = toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n[[filters]]\nname = 'Bugs'\njql = 'type = Bug'\n").unwrap();
        let mut settings = Settings::new(&config);
        settings.team = form::editor("Alex = alex@example.com\nPriya = priya@example.com");
        settings.epics = form::editor("demo-2, DEMO-1");
        settings.start_tab = Tab::Team.index();
        settings.show_done = false;
        let updated = settings.config(&config).unwrap();
        assert_eq!(updated.team.len(), 2);
        assert_eq!(updated.epics_i_care_about_ordered(), ["DEMO-2", "DEMO-1"]);
        assert_eq!(updated.preferences.start_tab, "Team");
        assert!(!updated.preferences.show_done);
        assert_eq!(updated.filters[0].jql, "type = Bug");
        settings.team = form::editor("Alex = wrong-email");
        assert!(settings.config(&config).is_err());
    }

    #[test]
    fn choosing_a_theme_previews_it_and_only_save_keeps_it() {
        use ratatui::style::Color;
        let mut config: AppConfig = toml::from_str(
            "[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n[themes.mine]\naccent = 'red'\n",
        )
        .unwrap();
        let mut app = App::new();
        let draw = |app: &App, config: &AppConfig| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
            terminal.draw(|f| crate::ui(f, app, config)).unwrap();
            terminal.backend().buffer().clone()
        };
        for save in [false, true] {
            app.settings = Some(Settings::new(&config));
            handle_key(&mut app, KeyCode::BackTab, KeyModifiers::NONE, &mut config);
            assert_eq!(app.focused_field(), Some(4));
            for _ in 0..crate::theme::PRESETS.len() + 1 {
                handle_key(&mut app, KeyCode::Down, KeyModifiers::NONE, &mut config);
            }
            let buffer = draw(&app, &config);
            let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            for shown in ["Hide", "Show", "› My Work", "› mine", " Save "] {
                assert!(text.contains(shown), "{shown} isn't shown at 80x24");
            }
            assert!(buffer.content.iter().any(|cell| cell.fg == Color::Red));
            assert!(!buffer.content.iter().any(|cell| cell.fg == Color::Cyan));

            let key = if save { KeyCode::Enter } else { KeyCode::Esc };
            handle_key(&mut app, key, KeyModifiers::NONE, &mut config);
            assert!(app.settings.is_none());
            let kept = if save { "mine" } else { crate::theme::DEFAULT };
            assert_eq!(config.preferences.theme, kept);
            assert_eq!(app.theme, crate::theme::resolve(kept, &config.themes));
        }
        assert_eq!(app.theme.accent, Some(Color::Red));
    }
}
