//! The panel for moving the ticket in the detail view: the transitions Jira offers, then the
//! resolutions the chosen one accepts.

use ratatui::text::{Line, Span};

use super::form::{self, Menu};
use crate::app::{App, DetailMode};
use crate::cache::Status;
use crate::style;

/// Whether the detail is in a mode this panel shows.
pub fn is_open(app: &App) -> bool {
    app.is_detail_open()
        && matches!(
            app.detail_mode,
            DetailMode::MoveLoading { .. }
                | DetailMode::MovePicker(_)
                | DetailMode::ResolutionPicker { .. }
        )
}

/// The panel's menu, if the detail is in a mode it shows.
pub fn menu(app: &App) -> Option<Menu> {
    let key = app.detail_ticket_key.as_ref()?;
    let ticket = app.find_ticket(key)?;
    let from = ticket.status.as_str();
    let mut lines = Vec::new();
    let mut choices = Vec::new();
    match &app.detail_mode {
        DetailMode::MoveLoading { .. } => {
            lines.push(Line::from(Span::styled(
                format!("Loading {key}'s transitions from Jira…"),
                style::warning(),
            )));
            Some(Menu {
                title: form::title(format!("Move {key}"), &format!("from {from}")),
                lines,
                choices,
                selected: None,
                scroll: 0,
                hints: &[("Esc", "cancel")],
            })
        }
        DetailMode::MovePicker(picker) => {
            let rows = picker.rows();
            if rows.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("Jira offers no transitions from {from}."),
                    style::secondary(),
                )));
            }
            for (i, transition) in rows.iter().enumerate() {
                choices.push((lines.len(), i));
                let prefix = if i == picker.selected { "› " } else { "  " };
                lines.push(Line::from(Span::styled(
                    format!(
                        "{}{}{}",
                        prefix,
                        Status::move_shortcut_prefix(&transition.to_name),
                        transition.label(&picker.transitions)
                    ),
                    style::option(
                        style::status(&transition.to_name, app.status_rules()),
                        i == picker.selected,
                    ),
                )));
            }
            if let Some(transition) = picker.selected_transition().filter(|_| picker.confirming) {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!(
                        "Move with {}? Press Enter or y. Esc cancels.",
                        transition.label(&picker.transitions)
                    ),
                    style::warning(),
                )));
            }
            let detail = match &picker.only_to {
                Some(status) => format!("from {from} to {}", status.as_str()),
                None => format!("from {from}"),
            };
            Some(Menu {
                title: form::title(format!("Move {key}"), &detail),
                lines,
                choices,
                selected: Some(picker.selected),
                scroll: 0,
                hints: if picker.confirming {
                    &PICK_CONFIRMING
                } else {
                    &PICK
                },
            })
        }
        DetailMode::ResolutionPicker { picker, selected } => {
            for (i, choice) in picker.resolution_choices().iter().enumerate() {
                choices.push((lines.len(), i));
                let prefix = if i == *selected { "› " } else { "  " };
                let name = choice.as_ref().map_or("No resolution", |r| r.name.as_str());
                lines.push(Line::from(Span::styled(
                    format!("{}{}", prefix, name),
                    style::option(style::primary(), i == *selected),
                )));
            }
            let transition = picker
                .selected_transition()
                .map(|t| t.label(&picker.transitions))
                .unwrap_or_default();
            Some(Menu {
                title: form::title(
                    format!("Move {key}"),
                    &format!("{transition}: select a resolution"),
                ),
                lines,
                choices,
                selected: Some(*selected),
                scroll: 0,
                hints: &[("j/k", "choose"), ("Enter", "move"), ("Esc", "back")],
            })
        }
        _ => None,
    }
}

const PICK: [(&str, &str); 5] = [
    ("j/k", "choose"),
    ("Enter", "select"),
    (crate::cache::MOVE_SHORTCUTS, "pick by status"),
    ("Shift+key", "move now"),
    ("Esc", "cancel"),
];
const PICK_CONFIRMING: [(&str, &str); 5] = [
    ("j/k", "choose"),
    ("Enter", "confirm"),
    (crate::cache::MOVE_SHORTCUTS, "pick by status"),
    ("Shift+key", "move now"),
    ("Esc", "cancel"),
];
