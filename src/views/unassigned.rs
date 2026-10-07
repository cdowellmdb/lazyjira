use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::views::common::{
    color_marks, fold_indicator, group_marker, highlight_row, panel, status_color, ticket_cells,
    ticket_marker, truncate, KEY_WIDTH,
};

const NO_EPIC_KEY: &str = "NO-EPIC";

fn ticket_column_widths(area: Rect) -> (usize, usize, usize) {
    let key_w = KEY_WIDTH;
    let mut status_w = 12usize;
    let mut summary_w = 48usize;
    let inner = panel().inner(area).width as usize;
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
        .fg(Color::Reset)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_visual_line: Option<usize> = None;
    let mut mouse_rows = Vec::new();

    if !grouped.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(format!("    {:<key_w$}", "SEL KEY"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
        ]));
        lines.push(Line::from(""));
    }

    for group in grouped {
        let (epic_key, epic_summary) = &group.header;
        let indicator = fold_indicator(group.tickets.is_none());
        let marker = group_marker(app.group_selection_state(epic_key));

        mouse_rows.push((lines.len(), group.index, true));
        let is_header_selected = group.index == app.selected_index;
        if is_header_selected {
            selected_visual_line = Some(lines.len());
        }

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
        lines.push(Line::from(Span::styled(header, header_style)));
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
            lines.push(Line::from(""));
            continue;
        };

        for (index, ticket) in tickets {
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
            let status_style = if is_selected {
                Style::default()
                    .fg(status_color(&ticket.status, app.status_rules()))
                    .bg(Color::DarkGray)
            } else {
                Style::default().fg(status_color(&ticket.status, app.status_rules()))
            };
            let marker = ticket_marker(app.is_ticket_selected(&ticket.key));
            let (key_cell, summary) =
                ticket_cells(app, group.family.get(index).copied(), ticket, marker);

            lines.push(Line::from(vec![
                Span::styled(format!("    {:<key_w$}", key_cell), base),
                Span::styled(" │ ", base.fg(Color::DarkGray)),
                Span::styled(
                    format!("{:<status_w$}", ticket.status.as_str()),
                    status_style,
                ),
                Span::styled(" │ ", base.fg(Color::DarkGray)),
                Span::styled(
                    format!("{:<summary_w$}", truncate(&summary, summary_w)),
                    base,
                ),
            ]));
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

    highlight_row(&mut lines, selected_visual_line, panel().inner(area).width);
    color_marks(&mut lines);

    let visible = area.height.saturating_sub(2) as usize;
    let scroll_y = match selected_visual_line {
        Some(line) if line >= visible => (line - visible + 1) as u16,
        _ => 0,
    };

    crate::mouse::register_rows(app, area, &mouse_rows, scroll_y, &lines);
    let widget = Paragraph::new(lines).block(panel()).scroll((scroll_y, 0));
    f.render_widget(widget, area);
}
