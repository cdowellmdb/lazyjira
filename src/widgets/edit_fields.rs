use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Paragraph;

use super::form;
use crate::app::App;

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let Some(state) = &app.edit_state else {
        return;
    };
    let inner = form::render_modal_frame(f, &format!("Edit {}", state.ticket_key), 85, 85);
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    form::render_editor(
        f,
        app,
        areas[0],
        "Summary",
        &state.summary,
        state.focused_field == 0,
        0,
    );
    form::render_editor(
        f,
        app,
        areas[1],
        "Labels · comma-separated",
        &state.labels,
        state.focused_field == 1,
        1,
    );
    form::render_editor(
        f,
        app,
        areas[2],
        "Description",
        &state.description,
        state.focused_field == 2,
        2,
    );
    f.render_widget(
        Paragraph::new("Tab: next field · Shift+Enter: newline · Ctrl+E/F4: external editor"),
        areas[3],
    );
    form::buttons(
        f,
        app,
        areas[4],
        &[
            ("Save", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
    );
}
