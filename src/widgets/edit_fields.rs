use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};

use super::form;
use crate::app::App;

/// The rows the panel's body needs: the summary and labels, the description (its label and
/// from 3 to 12 lines) and the buttons.
pub fn rows(app: &App) -> u16 {
    let lines = app
        .edit_state
        .as_ref()
        .map_or(0, |s| s.description.lines().len());
    1 + 1 + 1 + lines.clamp(3, 12) as u16 + 1
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let Some(state) = &app.edit_state else {
        return;
    };
    let inner = form::render_modal_frame(
        f,
        app,
        area,
        form::title(format!("Edit {}", state.ticket_key), ""),
    );
    let [summary, labels, description, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);
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
