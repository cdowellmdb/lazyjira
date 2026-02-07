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

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let search_lower = app.search.as_ref().map(|s| s.to_lowercase());
    let searching = matches!(&search_lower, Some(s) if !s.is_empty());

    // Sort members: those with more active tickets first
    let mut members: Vec<_> = app.cache.team_members.iter().collect();
    members.sort_by(|a, b| {
        let ac = app.cache.active_tickets_for(&a.email).len();
        let bc = app.cache.active_tickets_for(&b.email).len();
        bc.cmp(&ac)
    });

    let mut lines: Vec<Line> = Vec::new();
    let mut ticket_idx: usize = 0;
    let mut selected_visual_line: Option<usize> = None;

    for member in &members {
        let active = app.cache.active_tickets_for(&member.email);

        // Apply search filter
        let filtered: Vec<_> = if searching {
            let s = search_lower.as_ref().unwrap();
            active
                .into_iter()
                .filter(|t| {
                    t.key.to_lowercase().contains(s.as_str())
                        || t.summary.to_lowercase().contains(s.as_str())
                })
                .collect()
        } else {
            active
        };

        // When searching, skip members with no matching tickets
        if searching && filtered.is_empty() {
            continue;
        }

        // Member header
        lines.push(Line::from(Span::styled(
            member.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )));

        if filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no active tickets)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for ticket in &filtered {
                let is_selected = ticket_idx == app.selected_index;
                if is_selected {
                    selected_visual_line = Some(lines.len());
                }

                let base = if is_selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };

                let status_fg = status_color(&ticket.status);
                let colored = if is_selected {
                    Style::default().fg(status_fg).bg(Color::DarkGray)
                } else {
                    Style::default().fg(status_fg)
                };

                lines.push(Line::from(vec![
                    Span::styled("  \u{25CF} ", colored),
                    Span::styled(format!("{} ", ticket.key), base),
                    Span::styled(format!("{:<14}", ticket.status.as_str()), colored),
                    Span::styled(ticket.summary.clone(), base),
                ]));

                ticket_idx += 1;
            }
        }

        // Blank line between members
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No team members",
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
