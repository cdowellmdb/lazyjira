use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::views::common::{
    color_marks, fold_indicator, group_marker, highlight_row, muted, panel, status_color,
    ticket_cells, ticket_marker, Shared, KEY_WIDTH, UPDATED_WIDTH,
};

fn team_column_widths(area: Rect) -> (usize, usize, usize, usize, usize) {
    let key_w = KEY_WIDTH;
    let status_w = 15usize;
    let mut summary_w = 28usize;
    let mut epic_w = 20usize;
    let mut labels_w = 18usize;
    let inner = panel().inner(area).width as usize;
    let prefix_and_separators = 2 + key_w + 3 + status_w + 3 + 3 + 3 + UPDATED_WIDTH + 3;
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
    let shared = Shared::of(&members);
    let (key_w, status_w, summary_w, epic_w, labels_w) = team_column_widths(area);
    let summary_w = shared.summary_width(summary_w, epic_w, labels_w);
    let heading_style = Style::default()
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    // The shared line goes under the first column headers only: it describes every member's rows.
    let mut shared_line_drawn = false;
    let mut selected_visual_line: Option<usize> = None;
    let mut mouse_rows = Vec::new();

    for group in members {
        let (member, active_count) = group.header;
        let done_count = group.total - active_count;
        let indicator = fold_indicator(group.tickets.is_none());
        let marker = group_marker(app.group_selection_state(&member.email));

        // Member header
        mouse_rows.push((lines.len(), group.index, true));
        let is_header_selected = group.index == app.selected_index;
        if is_header_selected {
            selected_visual_line = Some(lines.len());
        }
        let header_base = if is_header_selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        let header_style = header_base.add_modifier(Modifier::BOLD);

        let Some(tickets) = &group.tickets else {
            let summary_style = muted(header_base);
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
        // Folded sub-tasks have no row, so the split is by status rather than by `active_count`.
        let split =
            tickets.partition_point(|(_, ticket)| !app.status_rules().is_done(&ticket.status));
        let (active, done) = tickets.split_at(split);

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
            let mut header = Line::from(vec![
                Span::styled(format!("  {:<key_w$}", "SEL KEY"), heading_style),
                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
            ]);
            shared.push_trailing_headings(&mut header, heading_style, epic_w, labels_w);
            let header_w = header.width();
            lines.push(header);
            if !std::mem::replace(&mut shared_line_drawn, true) {
                lines.extend(shared.line(header_w));
            }

            for (index, ticket) in active {
                mouse_rows.push((lines.len(), *index, false));
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
                let marker = ticket_marker(app.is_ticket_selected(&ticket.key));
                let (key_cell, summary) = ticket_cells(
                    app,
                    group.family.get(index).copied(),
                    ticket,
                    marker,
                    summary_w,
                    base,
                );

                let mut row = Line::from(vec![
                    Span::styled(format!("  {:<key_w$}", key_cell), base),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(format!("{:<status_w$}", ticket.status.as_str()), colored),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                ]);
                row.spans.extend(summary);
                shared.push_trailing_cells(app, &mut row, ticket, base, epic_w, labels_w);
                lines.push(row);
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
                mouse_rows.push((lines.len(), *index, false));
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
                let marker = ticket_marker(app.is_ticket_selected(&ticket.key));
                let (key_cell, summary) = ticket_cells(
                    app,
                    group.family.get(index).copied(),
                    ticket,
                    marker,
                    summary_w,
                    base,
                );

                let mut row = Line::from(vec![
                    Span::styled(format!("    {:<key_w$}", key_cell), base),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(format!("{:<status_w$}", ticket.status.as_str()), colored),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                ]);
                row.spans.extend(summary);
                shared.push_trailing_cells(app, &mut row, ticket, base, epic_w, labels_w);
                lines.push(row);
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
    color_marks(&mut lines);

    // Scroll to keep selected row visible
    let visible = area.height.saturating_sub(2) as usize;
    let scroll_y = match selected_visual_line {
        Some(line) if line >= visible => (line - visible + 1) as u16,
        _ => 0,
    };

    crate::mouse::register_rows(app, area, &mouse_rows, scroll_y, &lines);
    let widget = Paragraph::new(lines).block(panel()).scroll((scroll_y, 0));
    f.render_widget(widget, area);
}
