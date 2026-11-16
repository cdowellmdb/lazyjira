use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};

use super::form;
use crate::app::{App, ISSUE_TYPES};

/// Rows of the pickers: their label and three options.
const PICKER_ROWS: u16 = 4;

/// The rows the panel's body needs: the pickers, the summary and labels, the description (its
/// label and from 3 to 12 lines) and the buttons.
pub fn rows(app: &App) -> u16 {
    let lines = app
        .create_ticket
        .as_ref()
        .map_or(0, |s| s.description.lines().len());
    PICKER_ROWS + 1 + 1 + 1 + lines.clamp(3, 12) as u16 + 1
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let Some(state) = &app.create_ticket else {
        return;
    };
    let inner = form::render_modal_frame(f, app, area, form::title("Create Ticket", ""));
    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    let [pickers, summary, labels, description] = Layout::vertical([
        Constraint::Length(PICKER_ROWS),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(body);
    let [kind, assignee, epic] = Layout::horizontal([
        Constraint::Percentage(24),
        Constraint::Percentage(38),
        Constraint::Percentage(38),
    ])
    .spacing(2)
    .areas(pickers);
    form::render_choices(
        f,
        app,
        kind,
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
        assignee,
        (2, "Assignee"),
        &build_assignee_options(app),
        state.assignee_idx,
        &state.assignee_search,
    );
    form::render_choices(
        f,
        app,
        epic,
        (3, "Epic"),
        &build_epic_options(app),
        state.epic_idx,
        &state.epic_search,
    );
    form::render_editor(
        f,
        app,
        summary,
        "Summary",
        &state.summary,
        state.focused_field == 1,
        1,
    );
    form::render_editor(
        f,
        app,
        labels,
        "Labels · comma-separated",
        &state.labels,
        state.focused_field == 4,
        4,
    );
    form::render_editor(
        f,
        app,
        description,
        "Description",
        &state.description,
        state.focused_field == 5,
        5,
    );
    form::button_row(
        f,
        app,
        footer,
        &[
            ("Create", KeyCode::Enter),
            ("Cancel", KeyCode::Esc),
            ("Editor", KeyCode::F(4)),
        ],
        &[
            ("Tab", "next field"),
            ("↑↓", "choose"),
            ("Shift+Enter", "newline"),
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
