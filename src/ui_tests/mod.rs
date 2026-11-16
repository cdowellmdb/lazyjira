//! Whole-screen rendering tests: they draw `ui` into a `TestBackend` and read the cells.

use crate::tests::{
    app_with_a_ticket_on_every_tab, press, sample_config, ticket, two_saved_filters,
};
use crate::*;
use ratatui::layout::Rect;

mod chrome;
mod detail;
mod dock;
mod footer;
mod narrow;
mod pane;
mod sheets;

/// A click target as (x, y, width, height, target).
type Cell = (u16, u16, u16, u16, mouse::Target);

/// An app with a fossil, spooky season and a description full of Jira's icon markers, so the
/// glyphs the app draws on its own are all on screen.
fn app_that_draws_every_glyph(tab: Tab, selected_index: usize) -> App {
    let mut app = app_with_a_ticket_on_every_tab(tab);
    app.selected_index = selected_index;
    app.delight.enabled = true;
    app.delight.today = Some(delight::Today {
        weekday: 5,
        month: 10,
        day: 23,
    });
    let lists = [
        &mut app.cache.my_tickets,
        &mut app.cache.team_tickets,
        &mut app.filter_results,
    ];
    let tickets = lists.into_iter().flatten().chain(
        app.cache
            .epics
            .iter_mut()
            .flat_map(|epic| &mut epic.children),
    );
    for ticket in tickets {
        ticket.updated = Some("2020-01-02T03:04:05.000+0000".to_string());
        ticket.detail_loaded = true;
        ticket.labels = vec!["label".to_string()];
        ticket.description = Some("(!) careful (i) note (/) ok (x) no (*) star".to_string());
    }
    app.mark_cache_changed();
    app
}

fn screen(app: &App, width: u16, height: u16) -> Vec<String> {
    use ratatui::backend::TestBackend;

    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui(f, app, &sample_config())).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

/// An app whose My Work holds 30 tickets, `LEAK-1` to `LEAK-29` and `LAST-30`, with the last one
/// selected: whatever overlay opens over it, none of the `LEAK` rows may show through.
fn app_with_a_long_list() -> App {
    let mut app = App::new();
    app.loading = false;
    app.cache.team_members = vec![
        cache::TeamMember {
            name: "Alex".into(),
            email: "alex@example.com".into(),
        },
        cache::TeamMember {
            name: "Sam".into(),
            email: "sam@example.com".into(),
        },
    ];
    app.cache.my_tickets = (1..=29)
        .map(|n| ticket(&format!("LEAK-{n}"), "Summary", "In Progress"))
        .chain([ticket("LAST-30", "Summary", "In Progress")])
        .collect();
    app.mark_cache_changed();
    app.selected_index = 30;
    app
}

fn column(rows: &[String], x: usize) -> String {
    rows.iter()
        .skip(2)
        .take(rows.len() - 3)
        .map(|row| row.chars().nth(x).unwrap())
        .collect()
}

/// A left click, pressed and released at (`x`, `y`).
async fn click(app: &mut App, config: &mut AppConfig, x: u16, y: u16) {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    for kind in [
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
    ] {
        let event = crossterm::event::MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        };
        mouse::handle(app, event, &tx, config).await;
    }
}
