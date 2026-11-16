use crate::app::{App, Tab, VisibleItem};
use crate::cache::Ticket;
use crate::views::{epics, filters, my_work, team, unassigned};
use ratatui::{backend::TestBackend, style::Color, Terminal};

/// The background `style::selected_row` puts behind the selected row.
pub fn selected_bg() -> Color {
    crate::style::selected_row().bg.unwrap()
}

/// Renders `tab` over the whole frame, with a config that has no saved filters.
pub fn render_tab(frame: &mut ratatui::Frame, app: &App, tab: Tab) {
    let config = toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
    let area = frame.area();
    match tab {
        Tab::MyWork => my_work::render(frame, area, app, None),
        Tab::Team => team::render(frame, area, app, None),
        Tab::Epics => epics::render(frame, area, app, None),
        Tab::Unassigned => unassigned::render(frame, area, app, None),
        Tab::Filters => filters::render(frame, area, app, &config, None),
    }
}

/// Draws `tab` and returns each row's text and the foreground color of its first glyph.
pub fn draw(app: &App, tab: Tab, width: u16) -> Vec<(String, Color)> {
    let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
    terminal.draw(|frame| render_tab(frame, app, tab)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..30)
        .map(|y| {
            let text: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
            let fg = (0..width)
                .map(|x| &buffer[(x, y)])
                .find(|cell| !matches!(cell.symbol(), " " | "│" | "╭" | "╰" | "─"))
                .map_or(Color::Reset, |cell| cell.fg);
            (text, fg)
        })
        .collect()
}

pub fn line_with<'a, T>(rows: &'a [(String, T)], text: &str) -> Option<&'a (String, T)> {
    rows.iter().find(|(row, _)| row.contains(text))
}

/// The screen column `text` starts at in `row`, a line from `draw`: one symbol per cell, so a
/// wide character is followed by the blank cell it covers.
pub fn column_of(row: &str, text: &str) -> usize {
    let at = row
        .find(text)
        .unwrap_or_else(|| panic!("{text} not in {row}"));
    row[..at].chars().count()
}

pub fn select_ticket(app: &mut App, key: &str) {
    for index in 0..app.item_count() {
        app.selected_index = index;
        if matches!(app.selected_item(), Some(VisibleItem::Ticket(k)) if k == key) {
            return;
        }
    }
    panic!("{key} isn't listed");
}

/// The text of each row of `draw`, trailing blanks cut.
pub fn trimmed(rows: &[(String, Color)]) -> Vec<String> {
    rows.iter()
        .map(|(row, _)| row.trim_end().to_string())
        .collect()
}

/// A ticket in `epic` carrying `labels`.
pub fn labelled(key: &str, epic: Option<&str>, labels: &[&str]) -> Ticket {
    let mut ticket = Ticket::for_test(key, "In Progress");
    ticket.epic_name = epic.map(String::from);
    ticket.labels = labels.iter().map(|l| l.to_string()).collect();
    ticket
}

/// The mockup's My Work: DEMO-101 with its sub-task DEMO-111 under In Progress, DEMO-201
/// under Ready for Work, DEMO-102 under In Review. The epics differ, so Epic shows.
pub fn mockup_app() -> App {
    let ticket = |key: &str, summary: &str, status: &str, epic: &str, labels: &[&str]| {
        let mut ticket = Ticket::for_test(key, status);
        ticket.summary = summary.into();
        ticket.epic_name = Some(epic.into());
        ticket.labels = labels.iter().map(|label| label.to_string()).collect();
        ticket
    };
    let mut child = ticket(
        "DEMO-111",
        "Migrate stored cards to the new vault",
        "In Progress",
        "Checkout redesign",
        &["checkout"],
    );
    child.parent_key = Some("DEMO-101".into());
    let mut app = App::new();
    app.loading = false;
    app.cache.my_tickets = vec![
        ticket(
            "DEMO-101",
            "Add saved payment methods to checkout",
            "In Progress",
            "Checkout redesign",
            &["checkout", "frontend"],
        ),
        child,
        ticket(
            "DEMO-201",
            "Profile slow facet queries",
            "Ready for Work",
            "Search performance",
            &["search", "perf"],
        ),
        ticket(
            "DEMO-102",
            "Fix tax rounding for multi-currency carts",
            "In Review",
            "Checkout redesign",
            &["checkout", "billing"],
        ),
    ];
    app.mark_cache_changed();
    app
}
