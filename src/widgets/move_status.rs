//! The panel for moving the ticket in the detail view: the transitions Jira offers, then the
//! resolutions the chosen one accepts.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::form;
use crate::app::{App, DetailMode};
use crate::cache::Status;
use crate::style;

/// What the panel shows: the title's muted words, the lines, the line each option is on, the
/// chosen option, and the hints under it.
struct Body {
    detail: String,
    lines: Vec<Line<'static>>,
    choices: Vec<(usize, usize)>,
    selected: Option<usize>,
    hints: &'static [(&'static str, &'static str)],
}

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

fn body(app: &App) -> Option<Body> {
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
            Some(Body {
                detail: format!("from {from}"),
                lines,
                choices,
                selected: None,
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
            Some(Body {
                detail,
                lines,
                choices,
                selected: Some(picker.selected),
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
            Some(Body {
                detail: format!("{transition}: select a resolution"),
                lines,
                choices,
                selected: Some(*selected),
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

/// The rows the panel's body needs: the lines and the hint line.
pub fn rows(app: &App) -> u16 {
    body(app).map_or(0, |body| body.lines.len() as u16 + 1)
}

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let (Some(key), Some(body)) = (app.detail_ticket_key.as_ref(), body(app)) else {
        return;
    };
    let inner = form::render_modal_frame(
        f,
        app,
        area,
        form::title(format!("Move {key}"), &body.detail),
    );
    match body.selected {
        Some(selected) => form::menu(
            f,
            app,
            inner,
            body.lines,
            (&body.choices, selected),
            body.hints,
        ),
        None => {
            let text = form::hint_row(f, app, inner, body.hints);
            f.render_widget(Paragraph::new(body.lines).wrap(Wrap { trim: false }), text);
        }
    }
}
