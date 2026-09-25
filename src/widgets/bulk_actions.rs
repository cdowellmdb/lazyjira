use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::{App, BulkAction, BulkState, BulkSummary, BulkTarget};
use crate::bulk_plan;

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
        BulkTarget::Move {
            destination,
            resolution,
        } => match resolution {
            Some(resolution) => format!("Move to {} (resolution: {})", destination, resolution),
            None => format!("Move to {}", destination),
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

/// A titled list of "KEY: reason" lines, if there are any.
fn push_ticket_reasons(
    lines: &mut Vec<Line>,
    title: &'static str,
    color: Color,
    reasons: &[(String, String)],
) {
    if reasons.is_empty() {
        return;
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        title,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )));
    for (key, reason) in reasons {
        let text = Text::styled(format!("  {}: {}", key, reason), Style::default().fg(color));
        lines.extend(text.lines);
    }
}

fn hint(text: &'static str) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(Color::DarkGray)))
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
    lines.push(Line::from(format!("Skipped: {}", summary.skipped.len())));
    lines.push(Line::from(format!("Failed: {}", summary.failed)));

    push_ticket_reasons(lines, "Failures:", Color::Red, &summary.failed_details);
    push_ticket_reasons(lines, "Skipped:", Color::Yellow, &summary.skipped);

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[j/k] scroll  [Enter/Esc] close",
        Style::default().fg(Color::DarkGray),
    )));
}

pub fn render(f: &mut ratatui::Frame, app: &App) {
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
    let mut scroll = 0;

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
        BulkState::MoveLoading { targets, .. } => {
            lines.push(Line::from(format!(
                "Loading the transitions of {} tickets from Jira…",
                targets.len()
            )));
            lines.push(Line::from(""));
            lines.push(hint("[Esc] cancel"));
        }
        BulkState::MoveStatusPicker {
            targets,
            fetched,
            selected,
        } => {
            lines.push(Line::from(format!("Tickets: {}", targets.len())));
            lines.push(Line::from(""));
            let destinations = bulk_plan::destinations(fetched);
            if destinations.is_empty() {
                lines.push(hint("Jira offers no transitions for these tickets."));
            }
            for (i, (destination, count)) in destinations.iter().enumerate() {
                let label = format!("{} ({} of {} tickets)", destination, count, targets.len());
                render_option(&mut lines, &label, i == *selected);
            }
            let failed: Vec<(&String, &String)> = fetched
                .iter()
                .filter_map(|(key, result)| Some((key, result.as_ref().err()?)))
                .collect();
            if let Some((key, error)) = failed.first() {
                lines.push(Line::from(""));
                let text = format!(
                    "Couldn't load the transitions of {} ticket(s), so they will be skipped. {}: {}",
                    failed.len(),
                    key,
                    error
                );
                lines.extend(Text::styled(text, Style::default().fg(Color::Red)).lines);
            }
            lines.push(Line::from(""));
            lines.push(hint("[j/k] choose status  [Enter] next  [Esc] cancel"));
        }
        BulkState::MoveResolutionPicker {
            targets,
            destination,
            plan,
            selected,
        } => {
            lines.push(Line::from(format!("Tickets: {}", targets.len())));
            lines.push(Line::from(format!("Move to: {}", destination)));
            lines.push(Line::from(""));
            for (i, choice) in plan.resolution_choices().iter().enumerate() {
                let name = choice.as_ref().map_or("No resolution", |r| r.name.as_str());
                render_option(&mut lines, name, i == *selected);
            }
            lines.push(Line::from(""));
            lines.push(hint(
                "Tickets that require a different resolution will be skipped.",
            ));
            lines.push(hint("[j/k] choose resolution  [Enter] next  [Esc] cancel"));
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
        BulkState::Confirm {
            targets,
            target,
            plan,
        } => {
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
            lines.push(Line::from(format!(
                "To send: {}  Skipped: {} (reasons shown after the run)",
                plan.jobs.len(),
                plan.skipped.len()
            )));
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
        BulkState::Result {
            summary,
            scroll: offset,
        } => {
            render_result(&mut lines, summary);
            scroll = *offset;
        }
    }

    let body = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(body, inner);
}
