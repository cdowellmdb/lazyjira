use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Tab};
use crate::cache::Status;

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
        let t: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", t)
    } else {
        s.to_string()
    }
}

fn team_column_widths(area: Rect) -> (usize, usize, usize, usize, usize) {
    let key_w = 10usize;
    let status_w = 15usize;
    let mut summary_w = 28usize;
    let mut epic_w = 20usize;
    let mut labels_w = 18usize;
    let inner = area.width.saturating_sub(2) as usize;
    let prefix_and_separators = 2 + key_w + 3 + status_w + 3 + 3 + 3;
    let mut overflow = prefix_and_separators + summary_w + epic_w + labels_w;

    if overflow > inner {
        let mut to_trim = overflow - inner;
        if summary_w > 14 {
            let cut = to_trim.min(summary_w - 14);
            summary_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && epic_w > 10 {
            let cut = to_trim.min(epic_w - 10);
            epic_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && labels_w > 8 {
            let cut = to_trim.min(labels_w - 8);
            labels_w -= cut;
        }
    }

    overflow = prefix_and_separators + summary_w + epic_w + labels_w;
    if overflow < inner {
        summary_w += inner - overflow;
    }

    (key_w, status_w, summary_w, epic_w, labels_w)
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let members = app.team_visible_tickets_by_member();
    let (key_w, status_w, summary_w, epic_w, labels_w) = team_column_widths(area);
    let heading_style = Style::default()
        .fg(Color::Gray)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    let mut item_idx: usize = 0;
    let mut selected_visual_line: Option<usize> = None;

    for (member, active, done) in members {
        let collapsed = app.is_collapsed(Tab::Team, &member.email);
        let indicator = if collapsed { ">" } else { "v" };

        // Member header
        let is_header_selected = item_idx == app.selected_index;
        if is_header_selected {
            selected_visual_line = Some(lines.len());
        }
        let header_style = if is_header_selected {
            Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };

        if collapsed {
            let summary_style = if is_header_selected {
                Style::default().fg(Color::Gray).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} {}", indicator, member.name),
                    header_style,
                ),
                Span::styled(
                    format!("  (active: {}  done: {})", active.len(), done.len()),
                    summary_style,
                ),
            ]));
            item_idx += 1;
            lines.push(Line::from(""));
            continue;
        }

        lines.push(Line::from(Span::styled(
            format!("{} {}", indicator, member.name),
            header_style,
        )));
        item_idx += 1;

        if active.is_empty() && done.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no tickets)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<key_w$}", "KEY"), heading_style),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<epic_w$}", "EPIC"), heading_style),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<labels_w$}", "LABELS"), heading_style),
            ]));

            for ticket in &active {
                let is_selected = item_idx == app.selected_index;
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
                let epic_str = ticket.epic_name.as_deref().unwrap_or("-");
                let labels_str = if ticket.labels.is_empty() {
                    "-".to_string()
                } else {
                    ticket.labels.join(", ")
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("  {:<key_w$}", ticket.key), base),
                    Span::styled(" | ", base),
                    Span::styled(format!("{:<status_w$}", ticket.status.as_str()), colored),
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

                item_idx += 1;
            }

            if !done.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("    done ({})", done.len()),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                )));
            }

            for ticket in &done {
                let is_selected = item_idx == app.selected_index;
                if is_selected {
                    selected_visual_line = Some(lines.len());
                }

                let base = if is_selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default().add_modifier(Modifier::DIM)
                };

                let status_fg = status_color(&ticket.status);
                let colored = if is_selected {
                    Style::default().fg(status_fg).bg(Color::DarkGray)
                } else {
                    Style::default().fg(status_fg).add_modifier(Modifier::DIM)
                };
                let epic_str = ticket.epic_name.as_deref().unwrap_or("-");
                let labels_str = if ticket.labels.is_empty() {
                    "-".to_string()
                } else {
                    ticket.labels.join(", ")
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("    {:<key_w$}", ticket.key), base),
                    Span::styled(" | ", base),
                    Span::styled(format!("{:<status_w$}", ticket.status.as_str()), colored),
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
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::DIM)
                        },
                    ),
                    Span::styled(" | ", base),
                    Span::styled(
                        format!("{:<labels_w$}", truncate(&labels_str, labels_w)),
                        if is_selected {
                            Style::default().fg(Color::Yellow).bg(Color::DarkGray)
                        } else {
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::DIM)
                        },
                    ),
                ]));

                item_idx += 1;
            }

            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("active: {}  done: {}", active.len(), done.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
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
