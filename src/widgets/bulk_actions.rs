use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};

use crate::app::App;
use crate::bulk_actions::{BulkAction, BulkState, BulkSummary, BulkTarget};
use crate::cache::{Status, MOVE_SHORTCUTS};
use crate::style;

use super::form::{self, Menu};

/// An option drawn like the move picker's: the selected one on a highlight bar.
fn render_option(lines: &mut Vec<Line<'static>>, label: &str, selected: bool, base: Style) {
    let prefix = if selected { "› " } else { "  " };
    lines.push(Line::from(Span::styled(
        format!("{}{}", prefix, label),
        style::option(base, selected),
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
    style: Style,
    reasons: &[(String, String)],
) {
    if reasons.is_empty() {
        return;
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        title,
        style.patch(style::heading()),
    )));
    for (key, reason) in reasons {
        let text = Text::styled(format!("  {}: {}", key, reason), style);
        lines.extend(text.lines);
    }
}

fn hint(text: &'static str) -> Line<'static> {
    Line::from(Span::styled(text, style::secondary()))
}

fn render_result(lines: &mut Vec<Line<'static>>, summary: &BulkSummary) {
    let action = match summary.action {
        BulkAction::Move => "Bulk Move",
        BulkAction::Assign => "Bulk Assign",
    };
    lines.push(Line::from(Span::styled(
        format!("{} complete", action),
        style::success().patch(style::heading()),
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

    push_ticket_reasons(lines, "Failures:", style::error(), &summary.failed_details);
    push_ticket_reasons(lines, "Skipped:", style::warning(), &summary.skipped);
}

/// The panel's menu for the open bulk state, if there is one.
pub fn menu(app: &App) -> Option<Menu> {
    app.bulk_state.as_ref().map(|state| menu_of(app, state))
}

fn menu_of(app: &App, state: &BulkState) -> Menu {
    let title = match state {
        BulkState::Result { .. } => "Bulk Results",
        BulkState::Confirm { .. } => "Confirm Bulk Action",
        BulkState::Running { .. } => "Bulk Action Running",
        _ => "Bulk Actions",
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut scroll = 0;
    let mut choices = Vec::new();
    const CHOOSE: &[(&str, &str)] = &[("j/k", "choose"), ("Enter", "next"), ("Esc", "cancel")];
    let hints: &'static [(&'static str, &'static str)] = match state {
        BulkState::AssignPicker { .. } => &[
            ("type", "filter"),
            ("↑↓", "choose"),
            ("Enter", "next"),
            ("Esc", "cancel"),
        ],
        BulkState::MoveStatusPicker { .. } => &[
            ("j/k", "choose"),
            ("Enter", "next"),
            (MOVE_SHORTCUTS, "pick by status"),
            ("Esc", "cancel"),
        ],
        BulkState::MoveLoading { .. } => &[("Esc", "cancel")],
        BulkState::Confirm { .. } => &[("Enter", "run"), ("y", "run"), ("Esc", "cancel")],
        BulkState::Running { .. } => &[("Esc", "close")],
        BulkState::Result { .. } => &[("j/k", "scroll"), ("Enter", "close"), ("Esc", "close")],
        _ => CHOOSE,
    };

    match state {
        BulkState::ActionPicker { targets, selected } => {
            lines.push(Line::from(format!("Selected tickets: {}", targets.len())));
            lines.push(Line::from(format!("Keys: {}", sample_keys(targets))));
            lines.push(Line::from(""));
            choices.push((lines.len(), 0));
            render_option(&mut lines, "Move tickets", *selected == 0, style::primary());
            choices.push((lines.len(), 1));
            render_option(
                &mut lines,
                "Assign tickets",
                *selected == 1,
                style::primary(),
            );
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
            only_to,
            selected,
        } => {
            lines.push(Line::from(format!("Tickets: {}", targets.len())));
            lines.push(Line::from(""));
            let destinations = crate::bulk_actions::shown_destinations(fetched, only_to.as_ref());
            if destinations.is_empty() {
                lines.push(hint("Jira offers no transitions for these tickets."));
            }
            for (i, (destination, count)) in destinations.iter().enumerate() {
                let label = format!(
                    "{}{destination} ({count} of {} tickets)",
                    Status::move_shortcut_prefix(destination),
                    targets.len()
                );
                choices.push((lines.len(), i));
                render_option(
                    &mut lines,
                    &label,
                    i == *selected,
                    style::status(destination, app.status_rules()),
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
                lines.extend(Text::styled(text, style::error()).lines);
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
                render_option(&mut lines, name, i == *selected, style::primary());
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
                    style::primary(),
                );
            }
            if app.cache.team_members.is_empty() {
                lines.push(hint("No team members configured"));
            }
        }
        BulkState::Confirm {
            targets,
            target,
            plan,
        } => {
            lines.push(Line::from(Span::styled(
                "Please confirm",
                style::warning().patch(style::heading()),
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
                style::warning().patch(style::heading()),
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
    Menu {
        title: form::title(title, ""),
        lines,
        choices,
        selected,
        scroll,
        hints,
    }
}
