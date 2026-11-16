use crossterm::event::KeyCode;
use ratatui::layout::Rect;

use super::form::{self, Row};
use crate::app::App;

/// The panel's body: the label, then the editor, as many rows as the comment has lines, from 3
/// to 12.
fn rows(app: &App) -> [Row; 1] {
    let lines = app
        .comment_state
        .as_ref()
        .map_or(0, |s| s.body.lines().len());
    [Row::flex(1 + lines.clamp(3, 12) as u16, 1)]
}

pub fn need(app: &App) -> form::Need {
    form::need(&rows(app))
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let Some(state) = &app.comment_state else {
        return;
    };
    let (body, footer) = form::render_panel_frame(
        f,
        app,
        area,
        form::title(format!("Comment on {}", state.ticket_key), ""),
    );
    let [editor] = form::split(rows(app), body);
    form::render_editor(f, app, editor, "Comment", &state.body, true, 0);
    form::button_row(
        f,
        app,
        footer,
        &[
            ("Submit", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
        &[
            ("Shift+Enter/Ctrl+J", "newline"),
            ("Ctrl+E", "external editor"),
        ],
    );
}
