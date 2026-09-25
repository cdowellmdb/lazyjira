use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use super::form;
use crate::moves::MoveFailure;

/// Shows a move Jira rejected, with jira-cli's full error. `more` counts failures queued behind it.
pub fn render(f: &mut ratatui::Frame, failure: &MoveFailure, more: usize) {
    let inner = form::render_modal_frame(f, "Move failed", 70, 50);

    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "Jira did not move {} to {}. Its status is unchanged.",
                failure.key,
                failure.target.as_str()
            ),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    lines.extend(Text::raw(failure.error.as_str()).lines);
    lines.push(Line::from(""));
    let footer = match more {
        0 => "[Enter/Esc] dismiss".to_string(),
        n => format!("[Enter/Esc] dismiss ({} more)", n),
    };
    lines.push(Line::from(Span::styled(
        footer,
        Style::default().fg(Color::DarkGray),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}
