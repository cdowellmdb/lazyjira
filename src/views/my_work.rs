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
    if s.chars().count() > max {
        let t: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", t)
    } else {
        s.to_string()
    }
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let grouped = app.cache.my_tickets_by_status();
    let search_lower = app.search.as_ref().map(|s| s.to_lowercase());

    let mut lines: Vec<Line> = Vec::new();
    let mut ticket_idx: usize = 0;
    let mut selected_visual_line: Option<usize> = None;

    for (status, tickets) in &grouped {
        // Apply search filter
        let filtered: Vec<_> = match &search_lower {
            Some(s) if !s.is_empty() => tickets
                .iter()
                .filter(|t| {
                    t.key.to_lowercase().contains(s.as_str())
                        || t.summary.to_lowercase().contains(s.as_str())
                })
                .copied()
                .collect(),
            _ => tickets.to_vec(),
        };

        if filtered.is_empty() {
            continue;
        }

        // Cap Done section at 5 most recent
        let total_count = filtered.len();
        let display: Vec<_> = if **status == Status::Done {
            filtered.into_iter().take(5).collect()
        } else {
            filtered
        };

        // Status header
        let header = format!("{} ({})", status.as_str().to_uppercase(), total_count);
        lines.push(Line::from(Span::styled(
            header,
            Style::default()
                .fg(status_color(status))
                .add_modifier(Modifier::BOLD),
        )));

        // Ticket rows
        for ticket in &display {
            let is_selected = ticket_idx == app.selected_index;
            if is_selected {
                selected_visual_line = Some(lines.len());
            }

            let base = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let epic_str = match &ticket.epic_name {
                Some(name) => format!("Epic: {}", name),
                None => String::new(),
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {:<10}", ticket.key), base),
                Span::styled(format!("{:<30}", truncate(&ticket.summary, 30)), base),
                Span::styled(
                    epic_str,
                    if is_selected {
                        Style::default().fg(Color::Gray).bg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
            ]));

            ticket_idx += 1;
        }

        // Blank line between groups
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No tickets found",
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
