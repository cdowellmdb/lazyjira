use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::cache::{ActivityEntry, ActivityKind};

fn format_timestamp(ts: &str) -> String {
    // "2024-01-15T10:30:00.000+0000" -> "2024-01-15 10:30"
    if ts.len() >= 16 {
        ts[..16].replace('T', " ")
    } else {
        ts.to_string()
    }
}

pub fn render(f: &mut ratatui::Frame, area: Rect, entries: &[ActivityEntry], scroll: u16) {
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        "Activity History",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no activity found -- open the ticket once to load history)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    for entry in entries {
        let ts = format_timestamp(&entry.timestamp);
        let detail = match &entry.kind {
            ActivityKind::StatusChange { from, to } => format!("Status: {} -> {}", from, to),
            ActivityKind::Comment { body } => {
                let preview: String = body.chars().take(80).collect();
                let ellipsis = if body.chars().count() > 80 { "..." } else { "" };
                format!("Comment: \"{}{}\"", preview, ellipsis)
            }
            ActivityKind::AssigneeChange { from, to } => {
                let from_str = from.as_deref().unwrap_or("Unassigned");
                let to_str = to.as_deref().unwrap_or("Unassigned");
                format!("Assignee: {} -> {}", from_str, to_str)
            }
            ActivityKind::FieldChange { field, from, to } => {
                format!("{}: {} -> {}", field, from, to)
            }
        };

        lines.push(Line::from(vec![
            Span::styled(format!("{:<17}", ts), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<20}", entry.author), Style::default().fg(Color::White)),
            Span::styled(detail, Style::default().fg(Color::Gray)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[Up/Down] scroll  [Esc] back to detail",
        Style::default().fg(Color::DarkGray),
    )));

    let widget = Paragraph::new(lines)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}
