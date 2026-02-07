use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::{App, ISSUE_TYPES};
use super::form;

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let state = match &app.create_ticket {
        Some(s) => s,
        None => return,
    };

    let inner = form::render_modal_frame(f, "Create Ticket", 50, 60);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    // Type picker (field 0)
    let type_options: Vec<String> = ISSUE_TYPES.iter().map(|s| s.to_string()).collect();
    form::render_picker(
        &mut lines,
        "Type",
        &type_options,
        state.issue_type_idx,
        state.focused_field == 0,
    );

    lines.push(Line::from(""));

    // Summary text input (field 1)
    form::render_text_input(
        &mut lines,
        "Summary",
        &state.summary,
        state.focused_field == 1,
    );

    lines.push(Line::from(""));

    // Assignee picker (field 2)
    let assignee_options = build_assignee_options(app);
    form::render_picker(
        &mut lines,
        "Assignee",
        &assignee_options,
        state.assignee_idx,
        state.focused_field == 2,
    );

    lines.push(Line::from(""));

    // Epic picker (field 3)
    let epic_options = build_epic_options(app);
    form::render_picker(
        &mut lines,
        "Epic",
        &epic_options,
        state.epic_idx,
        state.focused_field == 3,
    );

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Footer hints
    lines.push(Line::from(ratatui::text::Span::styled(
        "[Tab] next field  [Enter] submit  [Esc] cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let body = Paragraph::new(lines);
    f.render_widget(body, inner);
}

pub fn build_assignee_options(app: &App) -> Vec<String> {
    let mut options = vec!["None".to_string()];
    for member in &app.cache.team_members {
        options.push(member.name.clone());
    }
    options
}

pub fn build_epic_options(app: &App) -> Vec<String> {
    let mut options = vec!["None".to_string()];
    for epic in &app.cache.epics {
        options.push(format!("{} {}", epic.key, epic.summary));
    }
    options
}
