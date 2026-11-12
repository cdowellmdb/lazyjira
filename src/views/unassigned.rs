use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::views::common::{
    fit, fold_indicator, group_marker, narrow_for_summary, panel, status_color, status_label,
    status_width, ticket_cells, ticket_marker, Ages, Columns, ListLines, Shared, KEY_WIDTH,
    SEPARATOR_WIDTH, UPDATED_WIDTH,
};

const NO_EPIC_KEY: &str = "NO-EPIC";

/// Summary takes what Status and the fixed cells leave; on narrow terminals Status gives up
/// columns, down to 8, so it keeps 18.
fn ticket_column_widths(area: Rect, mut status_w: usize) -> (usize, usize) {
    let inner = panel().inner(area).width as usize;
    let room = inner.saturating_sub(4 + KEY_WIDTH + 3 * SEPARATOR_WIDTH + UPDATED_WIDTH);
    let summary_w = narrow_for_summary(room, 18, &mut [(&mut status_w, 8)]);
    (status_w, summary_w)
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let grouped = app.unassigned_visible_by_epic();
    let shared = Shared::of(&grouped, Columns::GroupedByEpic);
    let ages = Ages::of(app);
    let key_w = KEY_WIDTH;
    let (status_w, summary_w) = ticket_column_widths(area, status_width(&grouped));
    let heading_style = Style::default()
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);

    let mut lines = ListLines::default();

    if !grouped.is_empty() {
        let mut header = Line::from(vec![
            Span::styled(format!("    {:<key_w$}", "SEL KEY"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
        ]);
        shared.push_trailing_headings(&mut header, heading_style, 0, 0);
        let header_w = header.width();
        lines.push(header);
        lines.extend(shared.line(header_w));
        lines.push("");
    }

    for group in grouped {
        let (epic_key, epic_summary) = &group.header;
        let indicator = fold_indicator(group.tickets.is_none());
        let marker = group_marker(app.group_selection_state(epic_key));

        let is_header_selected = group.index == app.selected_index;

        let header = if epic_key == NO_EPIC_KEY {
            format!("{} {} No Epic", marker, indicator)
        } else {
            format!("{} {} {}  {}", marker, indicator, epic_key, epic_summary)
        };
        let header_style = if is_header_selected {
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        lines.header(
            group.index,
            is_header_selected,
            Span::styled(header, header_style),
        );
        let count_style = if is_header_selected {
            Style::default().fg(Color::Gray)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("unassigned: {}", group.total), count_style),
        ]));

        let Some(tickets) = &group.tickets else {
            lines.push("");
            continue;
        };

        for (index, ticket) in tickets {
            let is_selected = *index == app.selected_index;

            let base = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let base = ages.fade(ticket, base);
            let status_style = if is_selected {
                Style::default()
                    .fg(status_color(&ticket.status, app.status_rules()))
                    .bg(Color::DarkGray)
            } else {
                Style::default().fg(status_color(&ticket.status, app.status_rules()))
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

            let cells = vec![
                Span::styled(format!("    {:<key_w$}", key_cell), base),
                Span::styled(" │ ", base.fg(Color::DarkGray)),
                Span::styled(
                    fit(status_label(app, &ticket.status), status_w),
                    status_style,
                ),
            ];
            let row = lines.ticket(*index, is_selected, cells, base, summary);
            shared.push_trailing_cells(ages, row, ticket, base, 0, 0);
        }

        lines.push("");
    }

    if lines.is_empty() {
        let empty_text = if let Some(search) = app.search.as_ref().filter(|s| !s.is_empty()) {
            format!("  No unassigned tickets match \"{}\"", search)
        } else {
            "  No unassigned tickets found".to_string()
        };
        lines.push(Span::styled(
            empty_text,
            Style::default().fg(Color::DarkGray),
        ));
    }

    lines.render(f, area, app, panel());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_unassigned_cuts_status_to_8_so_summary_keeps_18() {
        let widths = |width| ticket_column_widths(Rect::new(0, 0, width, 10), 20);
        assert_eq!(widths(79), (20, 19));
        assert_eq!(widths(78), (20, 18));
        assert_eq!(widths(77), (19, 18));
        assert_eq!(widths(66), (8, 18));
        assert_eq!(widths(65), (8, 17));
    }
}
