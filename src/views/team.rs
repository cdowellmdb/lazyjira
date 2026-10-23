use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::views::common::{
    color_marks, fit, fold_indicator, group_marker, highlight_row, muted, narrow_for_summary,
    panel, status_color, status_label, status_width, ticket_cells, ticket_marker, Ages, Columns,
    Shared, KEY_WIDTH, SEPARATOR_WIDTH, UPDATED_WIDTH,
};

/// Summary takes what Status, Epic, Labels and the fixed cells leave; on narrow terminals Epic,
/// then Labels, then Status give up columns so it keeps 14.
fn team_column_widths(area: Rect, mut status_w: usize) -> (usize, usize, usize, usize) {
    let (mut epic_w, mut labels_w) = (20, 18);
    let inner = panel().inner(area).width as usize;
    let room = inner.saturating_sub(2 + KEY_WIDTH + 5 * SEPARATOR_WIDTH + UPDATED_WIDTH);
    let columns = &mut [(&mut epic_w, 10), (&mut labels_w, 8), (&mut status_w, 8)];
    let summary_w = narrow_for_summary(room, 14, columns);
    (status_w, summary_w, epic_w, labels_w)
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let members = app.team_visible_tickets_by_member();
    let ages = Ages::of(app);
    let shared = Shared::of(&members, Columns::EpicAndLabels);
    let key_w = KEY_WIDTH;
    let (status_w, summary_w, epic_w, labels_w) = team_column_widths(area, status_width(&members));
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
            if !shared_line_drawn {
                lines.extend(shared.line(header_w));
                shared_line_drawn = true;
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

                let base = ages.fade(ticket, base);

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
                    Span::styled(fit(status_label(app, &ticket.status), status_w), colored),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                ]);
                row.spans.extend(summary);
                shared.push_trailing_cells(ages, &mut row, ticket, base, epic_w, labels_w);
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

                let base = ages.fade(ticket, base);

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
                    Span::styled(fit(status_label(app, &ticket.status), status_w), colored),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                ]);
                row.spans.extend(summary);
                shared.push_trailing_cells(ages, &mut row, ticket, base, epic_w, labels_w);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_team_cuts_epic_then_labels_then_status_so_summary_keeps_14() {
        // (status, summary, epic, labels) for a 20-column status.
        let widths = |width| team_column_widths(Rect::new(0, 0, width, 10), 20);
        assert_eq!(widths(117), (20, 15, 20, 18));
        assert_eq!(widths(116), (20, 14, 20, 18));
        assert_eq!(widths(115), (20, 14, 19, 18));
        assert_eq!(widths(106), (20, 14, 10, 18));
        assert_eq!(widths(105), (20, 14, 10, 17));
        assert_eq!(widths(96), (20, 14, 10, 8));
        assert_eq!(widths(95), (19, 14, 10, 8));
        assert_eq!(widths(84), (8, 14, 10, 8));
        assert_eq!(widths(83), (8, 13, 10, 8));
    }
}
