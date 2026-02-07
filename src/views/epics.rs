use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::cache::Status;

fn status_color(status: &Status) -> Color {
    match status {
        Status::InProgress => Color::Yellow,
        Status::ToDo => Color::White,
        Status::InReview => Color::Cyan,
        Status::Blocked => Color::Red,
        Status::Done => Color::Green,
    }
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > max {
        let end = max.saturating_sub(3);
        let mut result: String = chars[..end].iter().collect();
        result.push_str("...");
        result
    } else {
        s.to_string()
    }
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let search_lower = app.search.as_ref().map(|s| s.to_lowercase());

    // Filter epics by search
    let filtered_epics: Vec<_> = app
        .cache
        .epics
        .iter()
        .filter(|epic| match &search_lower {
            Some(s) if !s.is_empty() => {
                epic.key.to_lowercase().contains(s.as_str())
                    || epic.summary.to_lowercase().contains(s.as_str())
            }
            _ => true,
        })
        .collect();

    let mut lines: Vec<Line> = Vec::new();
    let mut epic_idx: usize = 0;
    let mut selected_visual_line: Option<usize> = None;

    for epic in &filtered_epics {
        let is_selected = epic_idx == app.selected_index;
        if is_selected {
            selected_visual_line = Some(lines.len());
        }

        let total = epic.total();
        let done = epic.done_count();
        let pct = epic.progress_pct();

        // Progress bar: 12 chars wide
        let filled = if total > 0 {
            ((pct / 100.0) * 12.0).round() as usize
        } else {
            0
        };
        let empty = 12usize.saturating_sub(filled);
        let bar = format!(
            "{}{}",
            "\u{2588}".repeat(filled),
            "\u{2591}".repeat(empty),
        );

        let base = if is_selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };

        let bar_style = if is_selected {
            Style::default().fg(Color::Green).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Green)
        };

        // Epic row: "AMP-200  Auth Overhaul        ████████░░░░  8/15  (53%)"
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<10}", epic.key),
                if is_selected {
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .bg(Color::DarkGray)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                },
            ),
            Span::styled(format!("{:<20}", truncate(&epic.summary, 20)), base),
            Span::styled(format!("{}  ", bar), bar_style),
            Span::styled(format!("{}/{}  ({:.0}%)", done, total, pct), base),
        ]));

        // Status breakdown: "  In Progress: 3  To Do: 4  In Review: 2  Done: 8"
        let counts = epic.count_by_status();
        let mut parts: Vec<Span> = vec![Span::raw("  ")];
        let display_order = [
            Status::InProgress,
            Status::ToDo,
            Status::InReview,
            Status::Blocked,
            Status::Done,
        ];
        let mut first = true;
        for status in &display_order {
            if let Some(&count) = counts.get(status) {
                if count > 0 {
                    if !first {
                        parts.push(Span::raw("  "));
                    }
                    parts.push(Span::styled(
                        format!("{}: {}", status.as_str(), count),
                        Style::default().fg(status_color(status)),
                    ));
                    first = false;
                }
            }
        }
        lines.push(Line::from(parts));

        // Blank line between epics
        lines.push(Line::from(""));
        epic_idx += 1;
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No epics found",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Scroll to keep selected row visible
    let visible = area.height.saturating_sub(2) as usize;
    let scroll_y = match selected_visual_line {
        Some(line) if line >= visible => (line - visible + 1) as u16,
        _ => 0,
    };

    let widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL))
        .scroll((scroll_y, 0));
    f.render_widget(widget, area);
}
