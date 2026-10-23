use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, FilterFocus};
use crate::views::common::{
    color_marks, fit, fold_indicator, group_marker, highlight_row, narrow_for_summary, panel,
    status_color, status_label, status_width, ticket_cells, ticket_marker, Ages, Columns, Shared,
    KEY_WIDTH, SEPARATOR_WIDTH, UPDATED_WIDTH,
};

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App, config: &crate::config::AppConfig) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(area);

    render_sidebar(f, chunks[0], app, config);
    render_results(f, chunks[1], app);
}

fn render_sidebar(
    f: &mut ratatui::Frame,
    area: Rect,
    app: &App,
    config: &crate::config::AppConfig,
) {
    let sidebar_focused = app.filter_focus == FilterFocus::Sidebar;
    let border_style = if sidebar_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut lines = Vec::new();

    if config.filters.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No saved filters",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Press n to create one",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, filter) in config.filters.iter().enumerate() {
            let is_selected = i == app.filter_sidebar_idx;
            let prefix = if is_selected { "› " } else { "  " };

            let style = if is_selected && sidebar_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray)
            } else if is_selected {
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Reset)
            };

            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, filter.name),
                style,
            )));
        }
    }

    highlight_row(
        &mut lines,
        if sidebar_focused && !config.filters.is_empty() {
            Some(app.filter_sidebar_idx)
        } else {
            None
        },
        panel().inner(area).width,
    );
    let inner = panel().inner(area);
    let scroll = app
        .filter_sidebar_idx
        .saturating_sub(inner.height.saturating_sub(1) as usize);
    for index in scroll..config.filters.len().min(scroll + inner.height as usize) {
        app.mouse_targets.borrow_mut().push((
            Rect::new(inner.x, inner.y + (index - scroll) as u16, inner.width, 1),
            crate::mouse::Target::Filter(index),
        ));
    }
    let block = panel().title(" Saved Filters ").border_style(border_style);
    let widget = Paragraph::new(lines)
        .block(block)
        .scroll((scroll as u16, 0));
    f.render_widget(widget, area);
}

/// Summary takes what Status, Key, Updated and the 3-cell margin the results keep leave; on
/// narrow terminals Status gives up columns, down to 8, so it keeps 12.
fn results_column_widths(area: Rect, mut status_w: usize) -> (usize, usize) {
    let inner = panel().inner(area).width as usize;
    let room = inner.saturating_sub(2 + KEY_WIDTH + 3 * SEPARATOR_WIDTH + UPDATED_WIDTH + 3);
    let summary_w = narrow_for_summary(room, 12, &mut [(&mut status_w, 8)]);
    (status_w, summary_w)
}

fn render_results(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let results_focused = app.filter_focus == FilterFocus::Results;
    let ages = Ages::now(app);
    let border_style = if results_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut lines = Vec::new();
    let mut selected_visual_line: Option<usize> = None;
    let mut mouse_rows = Vec::new();

    if app.filter_loading {
        lines.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(Color::Yellow),
        )));
    } else if app.filter_results.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Select a filter and press Enter to run it",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let groups = app.filters_visible_by_status();
        let key_w = KEY_WIDTH;
        let (status_w, summary_w) = results_column_widths(area, status_width(&groups));

        let heading_style = Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD);

        let shared = Shared::of(&groups, Columns::Neither);
        let mut header = Line::from(vec![
            Span::styled(format!("  {:<key_w$}", "SEL KEY"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
        ]);
        shared.push_trailing_headings(&mut header, heading_style, 0, 0);
        let header_w = header.width();
        lines.push(header);
        lines.extend(shared.line(header_w));
        lines.push(Line::from(Span::styled(
            "─".repeat(header_w),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));

        for group in groups {
            let status = &group.header;
            mouse_rows.push((lines.len(), group.index, true));
            let is_header_selected = results_focused && group.index == app.selected_index;
            if is_header_selected {
                selected_visual_line = Some(lines.len());
            }

            let indicator = fold_indicator(group.tickets.is_none());
            let marker = group_marker(app.group_selection_state(status.as_str()));
            let header_style = if is_header_selected {
                Style::default()
                    .fg(status_color(status, app.status_rules()))
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray)
            } else {
                Style::default()
                    .fg(status_color(status, app.status_rules()))
                    .add_modifier(Modifier::BOLD)
            };

            lines.push(Line::from(Span::styled(
                format!(
                    "{} {} {} ({})",
                    marker,
                    indicator,
                    status_label(app, status).to_uppercase(),
                    group.total
                ),
                header_style,
            )));

            let Some(tickets) = &group.tickets else {
                lines.push(Line::from(""));
                continue;
            };

            for (index, ticket) in tickets {
                mouse_rows.push((lines.len(), *index, false));
                let is_selected = results_focused && *index == app.selected_index;
                if is_selected {
                    selected_visual_line = Some(lines.len());
                }

                let marker = ticket_marker(app.is_ticket_selected(&ticket.key));
                let base = if is_selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                let (key_cell, summary) = ticket_cells(
                    app,
                    ages,
                    group.family.get(index).copied(),
                    ticket,
                    marker,
                    summary_w,
                    base,
                );

                let status_style = if is_selected {
                    Style::default()
                        .fg(status_color(&ticket.status, app.status_rules()))
                        .bg(Color::DarkGray)
                } else {
                    Style::default().fg(status_color(&ticket.status, app.status_rules()))
                };

                let mut row = Line::from(vec![
                    Span::styled(format!("  {:<key_w$}", key_cell), base),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(
                        fit(status_label(app, &ticket.status), status_w),
                        status_style,
                    ),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                ]);
                row.spans.extend(summary);
                shared.push_trailing_cells(ages, &mut row, ticket, base, 0, 0);
                lines.push(row);
            }

            lines.push(Line::from(""));
        }
    }

    highlight_row(&mut lines, selected_visual_line, panel().inner(area).width);
    color_marks(&mut lines);

    // Scroll to keep selected row visible
    let visible = area.height.saturating_sub(2) as usize;
    let scroll_y = match selected_visual_line {
        Some(line) if line >= visible => (line - visible + 1) as u16,
        _ => 0,
    };

    let title = if app.filter_results.is_empty() {
        " Results ".to_string()
    } else {
        format!(" Results ({}) ", app.filter_results.len())
    };

    let block = panel().title(title).border_style(border_style);
    crate::mouse::register_rows(app, area, &mouse_rows, scroll_y, &lines);
    let widget = Paragraph::new(lines).block(block).scroll((scroll_y, 0));
    f.render_widget(widget, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_results_cut_status_to_8_so_summary_keeps_12() {
        let widths = |width| results_column_widths(Rect::new(0, 0, width, 10), 20);
        assert_eq!(widths(74), (20, 13));
        assert_eq!(widths(73), (20, 12));
        assert_eq!(widths(72), (19, 12));
        assert_eq!(widths(61), (8, 12));
        assert_eq!(widths(60), (8, 11));
    }
}
