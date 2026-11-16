use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};

use super::form;
use crate::app::App;

/// The rows the editor takes: as many as the comment has lines, from 3 to 12.
fn editor_rows(app: &App) -> u16 {
    let lines = app
        .comment_state
        .as_ref()
        .map_or(0, |s| s.body.lines().len());
    lines.clamp(3, 12) as u16
}

/// The rows the panel's body needs: the label, the editor, and the buttons.
pub fn rows(app: &App) -> u16 {
    1 + editor_rows(app) + 1
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let Some(state) = &app.comment_state else {
        return;
    };
    let inner = form::render_modal_frame(
        f,
        app,
        area,
        form::title(format!("Comment on {}", state.ticket_key), ""),
    );
    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    form::render_editor(f, app, body, "Comment", &state.body, true, 0);
    form::button_row(
        f,
        app,
        footer,
        &[
            ("Submit", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
        &[("Shift+Enter", "newline"), ("Ctrl+E", "external editor")],
    );
}
