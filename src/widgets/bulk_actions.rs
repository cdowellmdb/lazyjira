use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use crate::app::App;
use crate::bulk_actions::{BulkAction, BulkState, BulkSummary, BulkTarget};
use crate::bulk_plan;
use crate::cache::Status;
use crate::mouse::Target;
use crate::views::common::{panel, status_color};

use super::form;
use super::ticket_detail::{option_style, render_footer, render_menu};

/// An option drawn like the detail overlay's menus: the selected one on a highlight bar.
fn render_option(lines: &mut Vec<Line<'static>>, label: &str, selected: bool, color: Color) {
    let prefix = if selected { "› " } else { "  " };
    lines.push(Line::from(Span::styled(
        format!("{}{}", prefix, label),
        option_style(color, selected),
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
    lines: &mut Vec<Line<'static>>,
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

fn render_result(lines: &mut Vec<Line<'static>>, summary: &BulkSummary) {
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
    // Framed like the ticket detail: a [×] that closes it, then the title.
    let area = form::centered_rect(percent_x, percent_y, f.area());
    f.render_widget(Clear, area);
    let block = panel().title(Line::from(vec![
        Span::styled(
            " [×] ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    let inner = block.inner(area);
    app.text_selection.borrow_mut().area = inner;
    f.render_widget(block, area);
    if area.width > 2 {
        app.mouse_targets.borrow_mut().push((
            Rect::new(area.x + 1, area.y, 5.min(area.width - 2), 1),
            Target::Key(KeyCode::Esc),
        ));
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut scroll = 0;
    let mut choices = Vec::new();
    let choose = [("j/k", "choose"), ("Enter", "next"), ("Esc", "cancel")];
    let hints: &[(&str, &str)] = match state {
        BulkState::AssignPicker { .. } => &[
            ("type", "filter"),
            ("↑↓", "choose"),
            ("Enter", "next"),
            ("Esc", "cancel"),
        ],
        BulkState::MoveStatusPicker { .. } => &[
            ("j/k", "choose"),
            ("Enter", "next"),
            ("p/w/n/t/v/b/c", "pick by status"),
            ("Esc", "cancel"),
        ],
        BulkState::MoveLoading { .. } => &[("Esc", "cancel")],
        BulkState::Confirm { .. } => &[("Enter", "run"), ("y", "run"), ("Esc", "cancel")],
        BulkState::Running { .. } => &[("Esc", "close")],
        BulkState::Result { .. } => &[("j/k", "scroll"), ("Enter", "close"), ("Esc", "close")],
        _ => &choose,
    };

    match state {
        BulkState::ActionPicker { targets, selected } => {
            lines.push(Line::from(format!("Selected tickets: {}", targets.len())));
            lines.push(Line::from(format!("Keys: {}", sample_keys(targets))));
            lines.push(Line::from(""));
            choices.push((lines.len(), 0));
            render_option(&mut lines, "Move tickets", *selected == 0, Color::Reset);
            choices.push((lines.len(), 1));
            render_option(&mut lines, "Assign tickets", *selected == 1, Color::Reset);
        }
        BulkState::MoveLoading { targets, .. } => {
            lines.push(Line::from(format!(
                "Loading the transitions of {} tickets from Jira…",
                targets.len()
            )));
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
                let shortcut = match Status::from_str(destination) {
                    Status::Other(_) => "    ".to_string(),
                    status => format!("[{}] ", status.move_shortcut()),
                };
                let label = format!(
                    "{}{} ({} of {} tickets)",
                    shortcut,
                    destination,
                    count,
                    targets.len()
                );
                choices.push((lines.len(), i));
                render_option(
                    &mut lines,
                    &label,
                    i == *selected,
                    status_color(destination, app.status_rules()),
                );
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
                choices.push((lines.len(), i));
                render_option(&mut lines, name, i == *selected, Color::Reset);
            }
            lines.push(Line::from(""));
            lines.push(hint(
                "Tickets that require a different resolution will be skipped.",
            ));
        }
        BulkState::AssignPicker {
            targets,
            selected,
            search,
        } => {
            lines.push(Line::from(format!("Tickets: {}", targets.len())));
            lines.push(Line::from(""));
            lines.push(Line::from(format!("Type to filter: {search}")));
            for (i, member) in app
                .cache
                .team_members
                .iter()
                .enumerate()
                .filter(|(_, member)| {
                    format!("{} ({})", member.name, member.email)
                        .to_lowercase()
                        .contains(&search.to_lowercase())
                })
            {
                choices.push((lines.len(), i));
                render_option(
                    &mut lines,
                    &format!("{} ({})", member.name, member.email),
                    i == *selected,
                    Color::Reset,
                );
            }
            if app.cache.team_members.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No team members configured",
                    Style::default().fg(Color::DarkGray),
                )));
            }
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
        }
        BulkState::Running {
            targets, target, ..
        } => {
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
            lines.push(hint(
                "Processing in background. Closing this keeps it running.",
            ));
        }
        BulkState::Result {
            summary,
            scroll: offset,
        } => {
            render_result(&mut lines, summary);
            scroll = *offset;
        }
    }

    let selected = match state {
        BulkState::ActionPicker { selected, .. }
        | BulkState::MoveStatusPicker { selected, .. }
        | BulkState::MoveResolutionPicker { selected, .. }
        | BulkState::AssignPicker { selected, .. } => Some(*selected),
        _ => None,
    };
    match selected {
        Some(selected) => render_menu(f, app, inner, lines, (&choices, selected), hints),
        None => {
            let body = render_footer(f, inner, hints, app);
            f.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll, 0)),
                body,
            );
        }
    }
}
