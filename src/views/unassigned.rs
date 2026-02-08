use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Tab};
use crate::cache::Status;

const NO_EPIC_KEY: &str = "NO-EPIC";

fn status_color(status: &Status) -> Color {
    match status {
        Status::NeedsTriage => Color::White,
        Status::ReadyForWork => Color::Blue,
        Status::InProgress => Color::Yellow,
        Status::ToDo => Color::White,
        Status::InReview => Color::Cyan,
        Status::Blocked => Color::Red,
        Status::Closed => Color::Green,
        Status::Other(_) => Color::Magenta,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let mut result: String = s.chars().take(max.saturating_sub(3)).collect();
        result.push_str("...");
        result
    } else {
        s.to_string()
    }
}

fn ticket_column_widths(area: Rect) -> (usize, usize, usize) {
    let key_w = 10usize;
    let mut status_w = 12usize;
    let mut summary_w = 48usize;
    let inner = area.width.saturating_sub(2) as usize;
    let prefix_and_separators = 4 + key_w + 3 + status_w + 3;
    let mut overflow = prefix_and_separators + summary_w;

    if overflow > inner {
        let mut to_trim = overflow - inner;
        if summary_w > 18 {
            let cut = to_trim.min(summary_w - 18);
            summary_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && status_w > 8 {
            let cut = to_trim.min(status_w - 8);
            status_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && summary_w > 10 {
            let cut = to_trim.min(summary_w - 10);
            summary_w -= cut;
        }
    }

    overflow = prefix_and_separators + summary_w;
    if overflow < inner {
        summary_w += inner - overflow;
    }

    (key_w, status_w, summary_w)
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let grouped = app.unassigned_visible_by_epic();
    let (key_w, status_w, summary_w) = ticket_column_widths(area);
    let heading_style = Style::default()
        .fg(Color::Gray)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    let mut item_idx: usize = 0;
    let mut selected_visual_line: Option<usize> = None;

    if !grouped.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(format!("    {:<key_w$}", "KEY"), heading_style),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
        ]));
        lines.push(Line::from(""));
    }

    for (epic_key, epic_summary, tickets) in grouped {
        let collapsed = app.is_collapsed(Tab::Unassigned, &epic_key);
        let indicator = if collapsed { ">" } else { "v" };

        let is_header_selected = item_idx == app.selected_index;
        if is_header_selected {
            selected_visual_line = Some(lines.len());
        }

        let header = if epic_key == NO_EPIC_KEY {
            format!("{} No Epic", indicator)
        } else {
            format!("{} {}  {}", indicator, epic_key, epic_summary)
        };
        let header_style = if is_header_selected {
            Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        lines.push(Line::from(Span::styled(header, header_style)));
        let count_style = if is_header_selected {
            Style::default().fg(Color::Gray).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("unassigned: {}", tickets.len()),
                count_style,
            ),
        ]));
        item_idx += 1;

        if collapsed {
            lines.push(Line::from(""));
            continue;
        }

        for ticket in tickets {
            let is_selected = item_idx == app.selected_index;
            if is_selected {
                selected_visual_line = Some(lines.len());
            }

            let base = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            let status_style = if is_selected {
                Style::default()
                    .fg(status_color(&ticket.status))
                    .bg(Color::DarkGray)
            } else {
                Style::default().fg(status_color(&ticket.status))
            };

            lines.push(Line::from(vec![
                Span::styled(format!("    {:<key_w$}", ticket.key), base),
                Span::styled(" | ", base),
                Span::styled(
                    format!("{:<status_w$}", ticket.status.as_str()),
                    status_style,
                ),
                Span::styled(" | ", base),
                Span::styled(
                    format!("{:<summary_w$}", truncate(&ticket.summary, summary_w)),
                    base,
                ),
            ]));

            item_idx += 1;
        }

        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        let empty_text = if let Some(search) = app.search.as_ref().filter(|s| !s.is_empty()) {
            format!("  No unassigned tickets match \"{}\"", search)
        } else {
            "  No unassigned tickets found".to_string()
        };
        lines.push(Line::from(Span::styled(
            empty_text,
            Style::default().fg(Color::DarkGray),
        )));
    }

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
