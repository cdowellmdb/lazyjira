use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::views::common::{
    fold_indicator, group_marker, highlight_row, panel, status_color, truncate,
};

fn team_column_widths(area: Rect) -> (usize, usize, usize, usize, usize) {
    let key_w = 14usize;
    let status_w = 15usize;
    let mut summary_w = 28usize;
    let mut epic_w = 20usize;
    let mut labels_w = 18usize;
    let inner = panel().inner(area).width as usize;
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
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_visual_line: Option<usize> = None;

    for group in members {
        let (member, active_count) = group.header;
        let done_count = group.total - active_count;
        let indicator = fold_indicator(group.tickets.is_none());
        let marker = group_marker(app.group_selection_state(&member.email));

        // Member header
        let is_header_selected = group.index == app.selected_index;
        if is_header_selected {
            selected_visual_line = Some(lines.len());
        }
        let header_style = if is_header_selected {
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };

        let Some(tickets) = &group.tickets else {
            let summary_style = if is_header_selected {
                Style::default().fg(Color::Gray).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} {} {}", marker, indicator, member.name),
                    header_style,
                ),
                Span::styled(
                    format!("  (active: {}  done: {})", active_count, done_count),
                    summary_style,
                ),
            ]));
            lines.push(Line::from(""));
            continue;
        };
        let (active, done) = tickets.split_at(active_count);

        lines.push(Line::from(Span::styled(
            format!("{} {} {}", marker, indicator, member.name),
            header_style,
        )));

        if active.is_empty() && done.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no tickets)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<key_w$}", "SEL KEY"), heading_style),
                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<epic_w$}", "EPIC"), heading_style),
                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<labels_w$}", "LABELS"), heading_style),
            ]));

            for (index, ticket) in active {
                let is_selected = *index == app.selected_index;
                if is_selected {
                    selected_visual_line = Some(lines.len());
                }

                let base = if is_selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };

                let status_fg = status_color(&ticket.status, app.status_rules());
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
                let marker = if app.is_ticket_selected(&ticket.key) {
                    "[x]"
                } else {
                    "[ ]"
                };

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {:<key_w$}", format!("{} {}", marker, ticket.key)),
                        base,
                    ),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(format!("{:<status_w$}", ticket.status.as_str()), colored),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:<summary_w$}", truncate(&ticket.summary, summary_w)),
                        base,
                    ),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:<epic_w$}", truncate(epic_str, epic_w)),
                        if is_selected {
                            Style::default().fg(Color::Gray).bg(Color::DarkGray)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        },
                    ),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:<labels_w$}", truncate(&labels_str, labels_w)),
                        if is_selected {
                            Style::default().fg(Color::Yellow).bg(Color::DarkGray)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        },
                    ),
                ]));
            }

            if !done.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("    done ({})", done_count),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                )));
            }

            for (index, ticket) in done {
                let is_selected = *index == app.selected_index;
                if is_selected {
                    selected_visual_line = Some(lines.len());
                }

                let base = if is_selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default().add_modifier(Modifier::DIM)
                };

                let status_fg = status_color(&ticket.status, app.status_rules());
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
                let marker = if app.is_ticket_selected(&ticket.key) {
                    "[x]"
                } else {
                    "[ ]"
                };

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("    {:<key_w$}", format!("{} {}", marker, ticket.key)),
                        base,
                    ),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(format!("{:<status_w$}", ticket.status.as_str()), colored),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:<summary_w$}", truncate(&ticket.summary, summary_w)),
                        base,
                    ),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
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
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
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
            }

            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("active: {}  done: {}", active_count, done_count),
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

    highlight_row(&mut lines, selected_visual_line, panel().inner(area).width);

    // Scroll to keep selected row visible
    let visible = area.height.saturating_sub(2) as usize;
    let scroll_y = match selected_visual_line {
        Some(line) if line >= visible => (line - visible + 1) as u16,
        _ => 0,
    };

    let widget = Paragraph::new(lines).block(panel()).scroll((scroll_y, 0));
    f.render_widget(widget, area);
}
