use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, BulkAction, BulkState, BulkSummary, BulkTarget};
use crate::cache::Status;

use super::form;

fn render_option(lines: &mut Vec<Line>, label: &str, selected: bool) {
    let prefix = if selected { "> " } else { "  " };
    let style = if selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    lines.push(Line::from(Span::styled(
        format!("{}{}", prefix, label),
        style,
    )));
}

fn target_label(target: &BulkTarget) -> String {
    match target {
        BulkTarget::Move { status, resolution } => match resolution {
            Some(resolution) => format!("Move to {} (resolution: {})", status.as_str(), resolution),
            None => format!("Move to {}", status.as_str()),
        },
        BulkTarget::Assign {
            member_name,
            member_email,
        } => format!("Assign to {} ({})", member_name, member_email),
    }
}

fn sample_keys(targets: &[String]) -> String {
    if targets.is_empty() {
        return "(none)".to_string();
    }
    let max = 5usize;
    let preview = targets
        .iter()
        .take(max)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if targets.len() > max {
        format!("{}, ...", preview)
    } else {
        preview
    }
}

fn render_result(lines: &mut Vec<Line>, summary: &BulkSummary) {
    let action = match summary.action {
        BulkAction::Move => "Bulk Move",
        BulkAction::Assign => "Bulk Assign",
    };
    lines.push(Line::from(Span::styled(
        format!("{} complete", action),
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "Target: {}",
        target_label(&summary.target)
    )));
    lines.push(Line::from(format!("Total: {}", summary.total)));
    lines.push(Line::from(format!("Attempted: {}", summary.attempted)));
    lines.push(Line::from(format!("Succeeded: {}", summary.succeeded)));
    lines.push(Line::from(format!("Skipped: {}", summary.skipped)));
    lines.push(Line::from(format!("Failed: {}", summary.failed)));

    if !summary.failed_details.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Failures:",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        for (key, err) in summary.failed_details.iter().take(4) {
            lines.push(Line::from(Span::styled(
                format!("  {}: {}", key, err),
                Style::default().fg(Color::Red),
            )));
        }
        if summary.failed_details.len() > 4 {
            lines.push(Line::from(Span::styled(
                format!(
                    "  ... and {} more",
                    summary.failed_details.len().saturating_sub(4)
                ),
                Style::default().fg(Color::Red),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[Enter/Esc] close",
        Style::default().fg(Color::DarkGray),
    )));
}

pub fn render(f: &mut ratatui::Frame, app: &App, resolutions: &[String]) {
    let Some(state) = app.bulk_state.as_ref() else {
        return;
    };

    let (title, percent_x, percent_y) = match state {
        BulkState::Result { .. } => ("Bulk Results", 72, 62),
        BulkState::Confirm { .. } => ("Confirm Bulk Action", 72, 58),
        BulkState::Running { .. } => ("Bulk Action Running", 60, 34),
        _ => ("Bulk Actions", 58, 54),
    };
    let inner = form::render_modal_frame(f, title, percent_x, percent_y);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    match state {
        BulkState::ActionPicker { targets, selected } => {
            lines.push(Line::from(format!("Selected tickets: {}", targets.len())));
            lines.push(Line::from(format!("Keys: {}", sample_keys(targets))));
            lines.push(Line::from(""));
            render_option(&mut lines, "Move tickets", *selected == 0);
            render_option(&mut lines, "Assign tickets", *selected == 1);
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[j/k] choose  [Enter] next  [Esc] cancel",
                Style::default().fg(Color::DarkGray),
            )));
        }
        BulkState::MoveStatusPicker { targets, selected } => {
            lines.push(Line::from(format!("Tickets: {}", targets.len())));
            lines.push(Line::from(""));
            for (i, status) in Status::all().iter().enumerate() {
                render_option(&mut lines, status.as_str(), i == *selected);
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[j/k] choose status  [Enter] next  [Esc] cancel",
                Style::default().fg(Color::DarkGray),
            )));
        }
        BulkState::MoveResolutionPicker {
            targets,
            status,
            selected,
        } => {
            lines.push(Line::from(format!("Tickets: {}", targets.len())));
            lines.push(Line::from(format!("Move target: {}", status.as_str())));
            lines.push(Line::from(""));
            if resolutions.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No configured resolutions. Press Enter to continue.",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for (i, resolution) in resolutions.iter().enumerate() {
                    render_option(&mut lines, resolution, i == *selected);
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[j/k] choose resolution  [Enter] next  [Esc] cancel",
                Style::default().fg(Color::DarkGray),
            )));
        }
        BulkState::AssignPicker { targets, selected } => {
            lines.push(Line::from(format!("Tickets: {}", targets.len())));
            lines.push(Line::from(""));
            for (i, member) in app.cache.team_members.iter().enumerate() {
                render_option(
                    &mut lines,
                    &format!("{} ({})", member.name, member.email),
                    i == *selected,
                );
            }
            if app.cache.team_members.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No team members configured",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[j/k] choose assignee  [Enter] next  [Esc] cancel",
                Style::default().fg(Color::DarkGray),
            )));
        }
        BulkState::Confirm { targets, target } => {
            lines.push(Line::from(Span::styled(
                "Please confirm",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(format!("Action: {}", target_label(target))));
            lines.push(Line::from(format!("Tickets: {}", targets.len())));
            lines.push(Line::from(format!("Keys: {}", sample_keys(targets))));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[Enter/y] run  [Esc] cancel",
                Style::default().fg(Color::DarkGray),
            )));
        }
        BulkState::Running { targets, target } => {
            lines.push(Line::from(Span::styled(
                "Executing bulk action...",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(format!("Action: {}", target_label(target))));
            lines.push(Line::from(format!("Tickets: {}", targets.len())));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Processing in background. Press Esc to close this modal.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        BulkState::Result { summary } => {
            render_result(&mut lines, summary);
        }
    }

    let body = Paragraph::new(lines);
    f.render_widget(body, inner);
}
