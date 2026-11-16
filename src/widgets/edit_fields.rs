use crossterm::event::KeyCode;
use ratatui::layout::Rect;

use super::form::{self, Row};
use crate::app::App;

/// The panel's body: the summary and labels, and the description (its label and from 3 to 12
/// lines).
fn rows(app: &App) -> [Row; 3] {
    let lines = app
        .edit_state
        .as_ref()
        .map_or(0, |s| s.description.lines().len());
    [
        Row::fixed(1),
        Row::fixed(1),
        Row::flex(1 + lines.clamp(3, 12) as u16, 1),
    ]
}

pub fn need(app: &App) -> form::Need {
    form::need(&rows(app))
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let Some(state) = &app.edit_state else {
        return;
    };
    let (body, footer) = form::render_panel_frame(
        f,
        app,
        area,
        form::title(format!("Edit {}", state.ticket_key), ""),
    );
    let [summary, labels, description] = form::split(rows(app), body);
    form::render_editor(
        f,
        app,
        summary,
        "Summary",
        &state.summary,
        state.focused_field == 0,
        0,
    );
    form::render_editor(
        f,
        app,
        labels,
        "Labels · comma-separated",
        &state.labels,
        state.focused_field == 1,
        1,
    );
    form::render_editor(
        f,
        app,
        description,
        "Description",
        &state.description,
        state.focused_field == 2,
        2,
    );
    form::button_row(
        f,
        app,
        footer,
        &[
            ("Save", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
        &[
            ("Tab", "next field"),
            ("Shift+Enter", "newline"),
            ("Ctrl+E", "external editor"),
        ],
    );
}
