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
        let mut result: String = s.chars().take(max.saturating_sub(3)).collect();
        result.push_str("...");
        result
    } else {
        s.to_string()
    }
}

fn child_column_widths(area: Rect) -> (usize, usize, usize) {
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

fn progress_bar(done: usize, total: usize, width: usize) -> String {
    if total == 0 || width == 0 {
        return format!("[{}]", "-".repeat(width));
    }

    let filled = ((done * width) + (total / 2)) / total;
    format!(
        "[{}{}]",
        "#".repeat(filled.min(width)),
        "-".repeat(width.saturating_sub(filled.min(width)))
    )
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let visible_epics = app.epics_visible_epics();
    let (key_w, status_w, summary_w) = child_column_widths(area);
    let heading_style = Style::default()
        .fg(Color::Gray)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    let mut ticket_idx: usize = 0;
    let mut selected_visual_line: Option<usize> = None;

    if !visible_epics.is_empty() {
        let header_w = 4 + key_w + 3 + status_w + 3 + summary_w;
        lines.push(Line::from(vec![
            Span::styled(format!("    {:<key_w$}", "KEY"), heading_style),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
        ]));
        lines.push(Line::from(Span::styled(
            format!("{}", "-".repeat(header_w)),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
    }

    for (epic, children) in visible_epics {
        let total = epic.total();
        let done = epic.done_count();
        let pct = epic.progress_pct();
        let counts = epic.count_by_status();
        let blocked = counts.get(&Status::Blocked).copied().unwrap_or(0);
        let bar_width = 18usize;
        let progress = progress_bar(done, total, bar_width);
        let meta = if blocked > 0 {
            format!(
                "{}  ({} / {} done, {:.1}%, blocked {})",
                progress, done, total, pct, blocked
            )
        } else {
            format!("{}  ({} / {} done, {:.1}%)", progress, done, total, pct)
        };
        let inner = area.width.saturating_sub(2) as usize;
        let prefix = 10 + 2;
        let epic_summary_w = inner.saturating_sub(prefix).max(12);

        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<10}", epic.key),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!(
                    "{:<epic_summary_w$}",
                    truncate(&epic.summary, epic_summary_w)
                ),
                Style::default(),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(meta, Style::default().fg(Color::DarkGray)),
        ]));

        if children.is_empty() {
            lines.push(Line::from(Span::styled(
                "    (no related tickets)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for ticket in children {
                let is_selected = ticket_idx == app.selected_index;
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

                ticket_idx += 1;
            }
        }

        // Blank line between epics
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        let empty_text = if app.epics_refreshing {
            "  Loading epics... (assembling the relationship map)"
        } else if let Some(search) = app.search.as_ref().filter(|s| !s.is_empty()) {
            lines.push(Line::from(Span::styled(
                format!("  No epics match \"{}\"", search),
                Style::default().fg(Color::DarkGray),
            )));
            ""
        } else {
            "  No epics found"
        };

        if !empty_text.is_empty() {
            lines.push(Line::from(Span::styled(
                empty_text,
                Style::default().fg(Color::DarkGray),
            )));
        }
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
