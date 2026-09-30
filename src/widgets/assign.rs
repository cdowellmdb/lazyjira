use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Paragraph;

use super::form;
use crate::app::App;

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let Some(state) = &app.assign_state else {
        return;
    };
    let inner = form::render_modal_frame(f, &format!("Assign {}", state.ticket_key), 70, 60);
    let areas = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let options = app
        .cache
        .team_members
        .iter()
        .map(|m| format!("{} ({})", m.name, m.email))
        .collect::<Vec<_>>();
    form::render_choices(
        f,
        app,
        areas[0],
        (0, "Assignee"),
        &options,
        state.selected,
        &state.search,
    );
    f.render_widget(
        Paragraph::new("Type to filter · ↑↓: choose · Backspace: clear search"),
        areas[1],
    );
    form::buttons(
        f,
        app,
        areas[2],
        &[("Assign", KeyCode::Enter), ("Cancel", KeyCode::Esc)],
    );
}
