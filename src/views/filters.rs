use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, FilterFocus};
use crate::style;
use crate::views::common::{
    gutter, highlight_row, key_width, narrow_for_summary, status_cell, status_width, Columns,
    ListLines, Shared, Trailing, COLUMN_GAP, GUTTER, STATUS_DOT_WIDTH, UPDATED_WIDTH,
};

/// Rows a pane's title and the blank line under it take.
const TITLE_HEIGHT: u16 = 2;

/// `compact` is the list's width beside the detail pane, which shows only keys and summaries.
pub fn render(
    f: &mut ratatui::Frame,
    area: Rect,
    app: &App,
    config: &crate::config::AppConfig,
    compact: Option<u16>,
) {
    // Beside the detail pane there is room for the results alone.
    if compact.is_some() {
        render_results(f, area, app, false, compact);
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(area);

    render_sidebar(f, chunks[0], app, config);
    render_results(f, chunks[1], app, true, None);
}

/// A pane's title, over its list: the accent while the pane has focus.
fn render_title(f: &mut ratatui::Frame, app: &App, area: Rect, title: String, focused: bool) {
    let style = if focused {
        style::accent().patch(style::heading())
    } else {
        style::secondary()
    };
    let row = Rect::new(area.x, area.y, area.width, 1.min(area.height));
    f.render_widget(
        Paragraph::new(Span::styled(format!(" {title}"), style)),
        row,
    );
    // The title sits over the gutter's marker, so a drag takes it from its first letter.
    crate::mouse::selectable(
        app,
        Rect::new(row.x + 1, row.y, row.width.saturating_sub(1), row.height),
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
    render_title(f, app, area, "Saved Filters".to_string(), sidebar_focused);
    let list = below_title(area);

    let mut lines = Vec::new();

    if config.filters.is_empty() {
        let note = |text: &'static str| {
            let mut row = gutter(Span::raw(" "));
            row.push(Span::styled(text, style::secondary()));
            Line::from(row)
        };
        lines.push(note("No saved filters"));
        lines.push(Line::from(""));
        lines.push(note("Press n to create one"));
    } else {
        for (i, filter) in config.filters.iter().enumerate() {
            let is_selected = i == app.filter_sidebar_idx;
            let name = if is_selected {
                style::heading()
            } else {
                style::primary()
            };
            let marker = Span::styled(
                if is_selected { "›" } else { " " },
                style::accent().patch(style::heading()),
            );
            let mut row = gutter(marker);
            row.push(Span::styled(filter.name.clone(), name));
            lines.push(Line::from(row));
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
    // A drag copies a filter's name, not its marker.
    let lead = (GUTTER as u16).min(list.width);
    crate::mouse::selectable(
        app,
        Rect::new(list.x + lead, list.y, list.width - lead, list.height),
    );
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

fn render_results(
    f: &mut ratatui::Frame,
    area: Rect,
    app: &App,
    divided: bool,
    compact: Option<u16>,
) {
    let results_focused = app.filter_focus == FilterFocus::Results;

    // The divider between the panes is the results' left border.
    let inner = if divided {
        let divider = Block::default()
            .borders(Borders::LEFT)
            .border_style(style::secondary());
        let inner = divider.inner(area);
        f.render_widget(divider, area);
        inner
    } else {
        area
    };

    let groups = app.filters_visible_by_status();
    // How many results the search keeps, counting those in folded groups.
    let kept: usize = groups.iter().map(|group| group.total).sum();
    let title = if app.filter_results.is_empty() {
        "Results".to_string()
    } else {
        format!("Results ({kept})")
    };
    render_title(f, app, inner, title, results_focused);
    let list = below_title(inner);

    let mut lines = ListLines::new(app)
        .compact(compact)
        .focused(results_focused);

    if app.filter_loading {
        lines.glyph_line(
            Span::raw(" "),
            vec![Span::styled("Loading...", style::warning())],
        );
    } else if app.filter_results.is_empty() {
        lines.note("Select a filter and press Enter to run it");
    } else if groups.is_empty() {
        // Every result is in a status group, so only a search leaves none.
        lines.note(format!(
            "No results match \"{}\"",
            app.search.as_deref().unwrap_or_default()
        ));
    } else {
        let ages = lines.ages();
        let key_w = key_width(&groups);
        let (status_w, summary_w) =
            results_column_widths(list, lines.lead_width(), key_w, status_width(&groups));

        let shared = Shared::of(&groups, Columns::Neither);
        lines.intro(Some(&shared), list.width as usize);

        for group in groups {
            lines.status_header(&group);

            let Some(tickets) = &group.tickets else {
                lines.push("");
                continue;
            };

            for row in tickets {
                let (line, base) = lines.ticket_row(&group, row, key_w, summary_w, false, |base| {
                    vec![status_cell(app, row.1, base, status_w)]
                });
                shared.push_trailing_cells(ages, line, row.1, base, Trailing::NONE);
            }

            lines.push("");
        }
    }

    lines.render(f, list);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::common::{CHECKBOX_WIDTH, GUTTER};

    #[test]
    fn narrow_results_cut_status_to_8_so_summary_keeps_12() {
        let widths = |width| results_column_widths(Rect::new(0, 0, width, 10), GUTTER, 8, 20);
        assert_eq!(widths(56), (20, 13));
        assert_eq!(widths(55), (20, 12));
        assert_eq!(widths(54), (19, 12));
        assert_eq!(widths(43), (8, 12));
        assert_eq!(widths(42), (8, 11));
    }

    #[test]
    fn marking_moves_every_results_boundary_two_columns() {
        let widths = |width| {
            results_column_widths(Rect::new(0, 0, width, 10), GUTTER + CHECKBOX_WIDTH, 8, 20)
        };
        assert_eq!(widths(58), (20, 13));
        assert_eq!(widths(57), (20, 12));
        assert_eq!(widths(56), (19, 12));
        assert_eq!(widths(45), (8, 12));
        assert_eq!(widths(44), (8, 11));
    }
}
