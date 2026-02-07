use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::cache::Status;

fn status_color(status: &Status) -> Color {
    match status {
        Status::NeedsTriage => Color::White,
        Status::ReadyForWork => Color::Blue,
        Status::InProgress => Color::Yellow,
        Status::ToDo => Color::White,
        Status::InReview => Color::Cyan,
        Status::Blocked => Color::Red,
        Status::Done => Color::Green,
        Status::Other(_) => Color::Magenta,
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

fn my_work_column_widths(area: Rect) -> (usize, usize, usize, usize) {
    let key_w = 10usize;
    let mut summary_w = 34usize;
    let mut epic_w = 24usize;
    let mut labels_w = 22usize;
    let inner = area.width.saturating_sub(2) as usize;
    let prefix_and_separators = 2 + key_w + 3 + 3 + 3;
    let mut overflow = prefix_and_separators + summary_w + epic_w + labels_w;

    if overflow > inner {
        let mut to_trim = overflow - inner;
        if summary_w > 16 {
            let cut = to_trim.min(summary_w - 16);
            summary_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && epic_w > 12 {
            let cut = to_trim.min(epic_w - 12);
            epic_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && labels_w > 10 {
            let cut = to_trim.min(labels_w - 10);
            labels_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && summary_w > 12 {
            let cut = to_trim.min(summary_w - 12);
            summary_w -= cut;
        }
    }

    overflow = prefix_and_separators + summary_w + epic_w + labels_w;
    if overflow < inner {
        summary_w += inner - overflow;
    }

    (key_w, summary_w, epic_w, labels_w)
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let grouped = app.my_work_visible_by_status();
    let (key_w, summary_w, epic_w, labels_w) = my_work_column_widths(area);
    let heading_style = Style::default()
        .fg(Color::Gray)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    let mut ticket_idx: usize = 0;
    let mut selected_visual_line: Option<usize> = None;
    let mut has_rows = false;

    for (status, tickets) in &grouped {
        if !has_rows {
            let header_w = 2 + key_w + 3 + summary_w + 3 + epic_w + 3 + labels_w;
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<key_w$}", "KEY"), heading_style),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<epic_w$}", "EPIC"), heading_style),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<labels_w$}", "LABELS"), heading_style),
            ]));
            lines.push(Line::from(Span::styled(
                format!("{}", "-".repeat(header_w)),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
        }

        let total_count = tickets.len();
        has_rows = true;

        // Status header
        let header = format!("{} ({})", status.as_str().to_uppercase(), total_count);
        lines.push(Line::from(Span::styled(
            header,
            Style::default()
                .fg(status_color(status))
                .add_modifier(Modifier::BOLD),
        )));

        // Ticket rows
        for ticket in tickets {
            let is_selected = ticket_idx == app.selected_index;
            if is_selected {
                selected_visual_line = Some(lines.len());
            }

            let base = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let epic_str = ticket.epic_name.as_deref().unwrap_or("-");
            let labels_str = if ticket.labels.is_empty() {
                "-".to_string()
            } else {
                ticket.labels.join(", ")
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {:<key_w$}", ticket.key), base),
                Span::styled(" | ", base),
                Span::styled(
                    format!("{:<summary_w$}", truncate(&ticket.summary, summary_w)),
                    base,
                ),
                Span::styled(" | ", base),
                Span::styled(
                    format!("{:<epic_w$}", truncate(epic_str, epic_w)),
                    if is_selected {
                        Style::default().fg(Color::Gray).bg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::styled(" | ", base),
                Span::styled(
                    format!("{:<labels_w$}", truncate(&labels_str, labels_w)),
                    if is_selected {
                        Style::default().fg(Color::Yellow).bg(Color::DarkGray)
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
