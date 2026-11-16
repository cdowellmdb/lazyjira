pub mod common;
pub mod epics;
pub mod filters;
pub mod my_work;
pub mod team;
#[cfg(test)]
mod tests;
pub mod unassigned;

use ratatui::layout::Rect;
use ratatui::text::Span;

use crate::app::{App, Tab};
use crate::config::AppConfig;
use crate::style;

/// Draws the active tab's list in `area`, or "Loading..." while the first read has nothing to
/// show. `compact` is the list's width beside the detail pane, if it is beside one.
pub fn render_active(
    f: &mut ratatui::Frame,
    area: Rect,
    app: &App,
    config: &AppConfig,
    compact: Option<u16>,
) {
    if app.loading
        && app.cache.my_tickets.is_empty()
        && app.cache.team_tickets.is_empty()
        && app.cache.epics.is_empty()
        && app.filter_results.is_empty()
    {
        let mut loading = common::ListLines::new(app);
        loading.glyph_line(
            Span::raw(" "),
            vec![Span::styled("Loading...", style::warning())],
        );
        loading.render(f, area);
        return;
    }
    match app.active_tab {
        Tab::MyWork => my_work::render(f, area, app, compact),
        Tab::Team => team::render(f, area, app, compact),
        Tab::Epics => epics::render(f, area, app, compact),
        Tab::Unassigned => unassigned::render(f, area, app, compact),
        Tab::Filters => filters::render(f, area, app, config, compact),
    }
}
