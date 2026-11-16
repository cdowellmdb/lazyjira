use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::cache::{ActivityEntry, ActivityKind};
use crate::style;

pub fn format_timestamp(ts: &str) -> String {
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
        "Activity: comments, newest first",
        style::heading(),
    )));
    lines.push(Line::from(""));

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no comments found -- open the ticket once to load them)",
            style::secondary(),
        )));
    }

    for entry in entries {
        let ts = format_timestamp(&entry.timestamp);
        let ActivityKind::Comment { body } = &entry.kind;
        let preview: String = body.chars().take(80).collect();
        let ellipsis = if body.chars().count() > 80 { "..." } else { "" };
        let detail = format!("Comment: \"{}{}\"", preview, ellipsis);

        lines.push(Line::from(vec![
            Span::styled(format!("{:<17}", ts), style::secondary()),
            Span::styled(format!("{:<20}", entry.author), style::primary()),
            Span::styled(detail, style::primary()),
        ]));
    }

    let widget = Paragraph::new(lines)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}
