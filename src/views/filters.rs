use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, FilterFocus};
use crate::style;
use crate::views::common::{
    fit, highlight_row, key_width, lead_width, narrow_for_summary, status_cell, status_header,
    status_width, ticket_cells, ticket_marker, Ages, Columns, ListLines, Shared, COLUMN_GAP,
    STATUS_DOT_WIDTH, UPDATED_WIDTH,
};

/// Rows a pane's title and the blank line under it take.
const TITLE_HEIGHT: u16 = 2;

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App, config: &crate::config::AppConfig) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(area);

    render_sidebar(f, chunks[0], app, config);
    render_results(f, chunks[1], app);
}

/// A pane's title, over its list: the accent while the pane has focus.
fn render_title(f: &mut ratatui::Frame, area: Rect, title: String, focused: bool) {
    let style = if focused {
        style::accent().patch(style::heading())
    } else {
        style::secondary()
    };
    f.render_widget(
        Paragraph::new(Span::styled(format!(" {title}"), style)),
        Rect::new(area.x, area.y, area.width, 1.min(area.height)),
    );
}

/// What is under a pane's title.
fn below_title(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.y + TITLE_HEIGHT.min(area.height),
        area.width,
        area.height.saturating_sub(TITLE_HEIGHT),
    )
}

fn render_sidebar(
    f: &mut ratatui::Frame,
    area: Rect,
    app: &App,
    config: &crate::config::AppConfig,
) {
    let sidebar_focused = app.filter_focus == FilterFocus::Sidebar;
    render_title(f, area, "Saved Filters".to_string(), sidebar_focused);
    let list = below_title(area);

    let mut lines = Vec::new();

    if config.filters.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No saved filters",
            style::secondary(),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Press n to create one",
            style::secondary(),
        )));
    } else {
        for (i, filter) in config.filters.iter().enumerate() {
            let is_selected = i == app.filter_sidebar_idx;
            let name = if is_selected {
                style::heading()
            } else {
                style::primary()
            };
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    if is_selected { "›" } else { " " },
                    style::accent().patch(style::heading()),
                ),
                Span::raw(" "),
                Span::styled(filter.name.clone(), name),
            ]));
        }
    }

    highlight_row(
        &mut lines,
        if sidebar_focused && !config.filters.is_empty() {
            Some(app.filter_sidebar_idx..app.filter_sidebar_idx + 1)
        } else {
            None
        },
        list.width,
    );
    let scroll = app
        .filter_sidebar_idx
        .saturating_sub(list.height.saturating_sub(1) as usize);
    for index in scroll..config.filters.len().min(scroll + list.height as usize) {
        app.mouse_targets.borrow_mut().push((
            Rect::new(list.x, list.y + (index - scroll) as u16, list.width, 1),
            crate::mouse::Target::Filter(index),
        ));
    }
    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), list);
}

/// Summary takes what Status, Key, Updated and the gutter leave; on narrow terminals Status gives
/// up columns, down to 8, so it keeps 12.
fn results_column_widths(
    area: Rect,
    lead: usize,
    key_w: usize,
    mut status_w: usize,
) -> (usize, usize) {
    let fixed = lead + key_w + 3 * COLUMN_GAP + STATUS_DOT_WIDTH + UPDATED_WIDTH;
    let room = (area.width as usize).saturating_sub(fixed);
    let summary_w = narrow_for_summary(room, 12, &mut [(&mut status_w, 8)]);
    (status_w, summary_w)
}

fn render_results(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let results_focused = app.filter_focus == FilterFocus::Results;
    let ages = Ages::of(app);

    // The divider between the panes is the results' left border.
    let divider = Block::default()
        .borders(Borders::LEFT)
        .border_style(style::secondary());
    let inner = divider.inner(area);
    f.render_widget(divider, area);

    let groups = app.filters_visible_by_status();
    // How many results the search keeps, counting those in folded groups.
    let kept: usize = groups.iter().map(|group| group.total).sum();
    let title = if app.filter_results.is_empty() {
        "Results".to_string()
    } else {
        format!("Results ({kept})")
    };
    render_title(f, inner, title, results_focused);
    let list = below_title(inner);

    let mut lines = ListLines::new(app);

    if app.filter_loading {
        lines.push(Span::styled("  Loading...", style::warning()));
    } else if app.filter_results.is_empty() {
        lines.push(Span::styled(
            "  Select a filter and press Enter to run it",
            style::secondary(),
        ));
    } else if groups.is_empty() {
        // Every result is in a status group, so only a search leaves none.
        lines.push(Span::styled(
            format!(
                "  No results match \"{}\"",
                app.search.as_deref().unwrap_or_default()
            ),
            style::secondary(),
        ));
    } else {
        let key_w = key_width(&groups);
        let (status_w, summary_w) =
            results_column_widths(list, lead_width(app), key_w, status_width(&groups));

        let shared = Shared::of(&groups, Columns::Neither);
        lines.intro(shared.line(list.width as usize));

        for group in groups {
            status_header(
                &mut lines,
                app,
                &group,
                results_focused && group.index == app.selected_index,
            );

            let Some(tickets) = &group.tickets else {
                lines.push("");
                continue;
            };

            for (index, ticket) in tickets {
                let is_selected = results_focused && *index == app.selected_index;
                let (base, primary) = ages.row_styles(ticket, is_selected, false);
                let (key_cell, summary) = ticket_cells(
                    app,
                    group.family.get(index).copied(),
                    ticket,
                    summary_w,
                    primary,
                );

                let cells = vec![
                    vec![Span::styled(fit(&key_cell, key_w), primary)],
                    status_cell(app, ticket, base, status_w),
                ];
                let mark = ticket_marker(app.is_ticket_selected(&ticket.key));
                let row = lines.ticket(*index, is_selected, mark, cells, base, summary);
                shared.push_trailing_cells(ages, row, ticket, base, 0, 0);
            }

            lines.push("");
        }
    }

    lines.render(f, list, app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::common::GUTTER;

    #[test]
    fn narrow_results_cut_status_to_8_so_summary_keeps_12() {
        let widths = |width| results_column_widths(Rect::new(0, 0, width, 10), GUTTER, 8, 20);
        assert_eq!(widths(56), (20, 13));
        assert_eq!(widths(55), (20, 12));
        assert_eq!(widths(54), (19, 12));
        assert_eq!(widths(43), (8, 12));
        assert_eq!(widths(42), (8, 11));
    }
}
