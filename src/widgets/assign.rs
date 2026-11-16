use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};

use super::form;
use crate::app::App;

/// Most options the picker shows at once.
const MAX_OPTIONS: usize = 8;

/// The rows the panel's body needs: the label, the options, and the buttons.
pub fn rows(app: &App) -> u16 {
    1 + app.cache.team_members.len().clamp(1, MAX_OPTIONS) as u16 + 1
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let Some(state) = &app.assign_state else {
        return;
    };
    let inner = form::render_modal_frame(
        f,
        app,
        area,
        form::title(format!("Assign {}", state.ticket_key), ""),
    );
    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
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
