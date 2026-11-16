use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};

use super::form;
use crate::app::App;

/// The rows the panel's body needs: the name, the JQL (its label and from 2 to 8 lines) and the
/// buttons.
pub fn rows(app: &App) -> u16 {
    let lines = app.filter_edit.as_ref().map_or(0, |s| s.jql.lines().len());
    1 + 1 + lines.clamp(2, 8) as u16 + 1
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
    let inner = form::render_modal_frame(f, app, area, form::title(title, ""));
    let [name, jql, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);
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
