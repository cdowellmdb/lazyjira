use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Paragraph;

use super::form;
use crate::app::App;

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let Some(state) = &app.comment_state else {
        return;
    };
    let inner =
        form::render_modal_frame(f, app, &format!("Comment on {}", state.ticket_key), 85, 75);
    let areas = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    form::render_editor(f, app, areas[0], "Comment", &state.body, true, 0);
    f.render_widget(
        Paragraph::new("Shift+Enter/Ctrl+J: newline · Ctrl+E/F4: external editor"),
        areas[1],
    );
    form::buttons(
        f,
        app,
        areas[2],
        &[
            ("Submit", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
    );
}
