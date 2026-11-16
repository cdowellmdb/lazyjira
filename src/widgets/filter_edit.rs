use crossterm::event::KeyCode;
use ratatui::layout::Rect;

use super::form::{self, Row};
use crate::app::App;

/// The panel's body: the name, and the JQL (its label and from 2 to 8 lines).
fn rows(app: &App) -> [Row; 2] {
    let lines = app.filter_edit.as_ref().map_or(0, |s| s.jql.lines().len());
    [Row::fixed(1), Row::flex(1 + lines.clamp(2, 8) as u16, 1)]
}

pub fn need(app: &App) -> form::Need {
    form::need(&rows(app))
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let Some(state) = &app.filter_edit else {
        return;
    };
    let title = if state.editing_idx.is_some() {
        "Edit Filter"
    } else {
        "New Filter"
    };
    let (body, footer) = form::render_panel_frame(f, app, area, form::title(title, ""));
    let [name, jql] = form::split(rows(app), body);
    form::render_editor(
        f,
        app,
        name,
        "Name",
        &state.name,
        state.focused_field == 0,
        0,
    );
    form::render_editor(f, app, jql, "JQL", &state.jql, state.focused_field == 1, 1);
    form::button_row(
        f,
        app,
        footer,
        &[
            ("Save", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
        &[("Tab", "next field"), ("Ctrl+E", "external editor")],
    );
}
