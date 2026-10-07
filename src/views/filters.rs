use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, FilterFocus};
use crate::views::common::{
    color_marks, fold_indicator, group_marker, highlight_row, muted, panel, status_color,
    ticket_cells, ticket_marker, truncate, updated_cell, Shared, KEY_WIDTH, UPDATED_WIDTH,
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

fn render_results(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let results_focused = app.filter_focus == FilterFocus::Results;
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
        let key_w = KEY_WIDTH;
        let status_w = 14usize;
        let inner = panel().inner(area).width as usize;
        let fixed = 2 + key_w + 3 + status_w + 3 + 3 + UPDATED_WIDTH + 3;
        let summary_w = inner.saturating_sub(fixed).max(12);

        let heading_style = Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD);

        let header = Line::from(vec![
            Span::styled(format!("  {:<key_w$}", "SEL KEY"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("UPDATED", heading_style),
        ]);
        let header_w = header.width();
        lines.push(header);
        let groups = app.filters_visible_by_status();
        lines.extend(Shared::of(&groups).line(header_w));
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
                    status.as_str().to_uppercase(),
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

                let mut row = vec![
                    Span::styled(format!("  {:<key_w$}", key_cell), base),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:<status_w$}", truncate(ticket.status.as_str(), status_w)),
                        status_style,
                    ),
                    Span::styled(" │ ", base.fg(Color::DarkGray)),
                ];
                row.extend(summary);
                row.push(Span::styled(" │ ", base.fg(Color::DarkGray)));
                row.push(Span::styled(updated_cell(app, ticket), muted(base)));
                lines.push(Line::from(row));
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
