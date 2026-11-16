use crossterm::event::KeyCode;
use ratatui::layout::Rect;

use super::form::{self, Row};
use crate::app::App;

/// Most options the picker shows at once.
const MAX_OPTIONS: usize = 8;

/// The panel's body: the label, then the options, down to the label and the chosen one.
fn rows(app: &App) -> [Row; 1] {
    [Row::flex(
        1 + app.cache.team_members.len().clamp(1, MAX_OPTIONS) as u16,
        2,
    )]
}

pub fn need(app: &App) -> form::Need {
    form::need(&rows(app))
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let Some(state) = &app.assign_state else {
        return;
    };
    let (body, footer) = form::render_panel_frame(
        f,
        app,
        area,
        form::title(format!("Assign {}", state.ticket_key), ""),
    );
    let [body] = form::split(rows(app), body);
    let options = app
        .cache
        .team_members
        .iter()
        .map(|m| format!("{} ({})", m.name, m.email))
        .collect::<Vec<_>>();
    form::render_choices(
        f,
        app,
        body,
        (0, "Assignee"),
        &options,
        state.selected,
        &state.search,
    );
    form::button_row(
        f,
        app,
        footer,
        &[("Assign", KeyCode::Enter), ("Cancel", KeyCode::Esc)],
        &[("type", "to filter"), ("↑↓", "choose")],
    );
}
