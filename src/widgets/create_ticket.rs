use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Paragraph;

use super::form;
use crate::app::{App, ISSUE_TYPES};

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let Some(state) = &app.create_ticket else {
        return;
    };
    let inner = form::render_modal_frame(f, "Create Ticket", 90, 90);
    let areas = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let pickers = Layout::horizontal([
        Constraint::Percentage(24),
        Constraint::Percentage(38),
        Constraint::Percentage(38),
    ])
    .split(areas[0]);
    form::render_choices(
        f,
        app,
        pickers[0],
        (0, "Type"),
        &ISSUE_TYPES
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        state.issue_type_idx,
        "",
    );
    form::render_choices(
        f,
        app,
        pickers[1],
        (2, "Assignee"),
        &build_assignee_options(app),
        state.assignee_idx,
        &state.assignee_search,
    );
    form::render_choices(
        f,
        app,
        pickers[2],
        (3, "Epic"),
        &build_epic_options(app),
        state.epic_idx,
        &state.epic_search,
    );
    form::render_editor(
        f,
        app,
        areas[1],
        "Summary",
        &state.summary,
        state.focused_field == 1,
        1,
    );
    form::render_editor(
        f,
        app,
        areas[2],
        "Labels · comma-separated",
        &state.labels,
        state.focused_field == 4,
        4,
    );
    form::render_editor(
        f,
        app,
        areas[3],
        "Description",
        &state.description,
        state.focused_field == 5,
        5,
    );
    f.render_widget(
        Paragraph::new(
            "Tab: next field · type to filter pickers · ↑↓: choose · Shift+Enter: newline",
        ),
        areas[4],
    );
    form::buttons(
        f,
        app,
        areas[5],
        &[
            ("Create", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
    );
}

pub fn build_assignee_options(app: &App) -> Vec<String> {
    std::iter::once("None".to_string())
        .chain(
            app.cache
                .team_members
                .iter()
                .map(|m| format!("{} ({})", m.name, m.email)),
        )
        .collect()
}

pub fn build_epic_options(app: &App) -> Vec<String> {
    std::iter::once("None".to_string())
        .chain(
            app.cache
                .epics
                .iter()
                .map(|e| format!("{} {}", e.key, e.summary)),
        )
        .collect()
}
