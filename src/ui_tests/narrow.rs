//! The whole screen at 80x24 and 60x20.

use super::*;

/// Three groups of tickets in two epics with labels, so every column and header is in play.
fn app_with_groups() -> App {
    let ticket = |key: &str, summary: &str, status: &str, epic: &str, label: &str| {
        let mut ticket = ticket(key, summary, status);
        ticket.epic_name = Some(epic.into());
        ticket.labels = vec![label.into()];
        ticket
    };
    let mut app = App::new();
    app.loading = false;
    app.cache.my_tickets = vec![
        ticket(
            "DEMO-1",
            "Add saved payment methods to checkout and then some more words to cut",
            "In Progress",
            "Checkout redesign",
            "checkout",
        ),
        ticket(
            "DEMO-2",
            "Profile slow facet queries",
            "Ready for Work",
            "Search performance",
            "search",
        ),
        ticket(
            "DEMO-3",
            "Fix tax rounding",
            "In Review",
            "Checkout redesign",
            "billing",
        ),
    ];
    app.mark_cache_changed();
    app
}

/// Every row of `rows` after trimming trailing blanks, the empty ones at the end cut.
fn trimmed(rows: &[String]) -> Vec<&str> {
    let mut rows: Vec<&str> = rows.iter().map(|row| row.trim_end()).collect();
    while rows.last() == Some(&"") {
        rows.pop();
    }
    rows
}

#[test]
fn at_80x24_the_list_is_key_summary_and_age_with_the_footer_on_the_last_row() {
    let mut app = app_with_groups();
    app.selected_index = 1;
    let rows = screen(&app, 80, 24);
    assert_eq!(rows.len(), 24);
    assert_eq!(
        trimmed(&rows),
        [
            " lazyjira   My Work   Team   Epics   Unassigned   Filters            Settings",
            &"─".repeat(80),
            "",
            " ● In Progress 1",
            " › DEMO-1  Add saved payment methods to checkout and then some more wor...     -",
            "",
            " ● Ready for Work 1",
            "   DEMO-2  Profile slow facet queries                                          -",
            "",
            " ● In Review 1",
            "   DEMO-3  Fix tax rounding                                                    -",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            " Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more",
        ]
    );
    assert_eq!(rows[4].chars().count(), 80);

    app.selected_ticket_keys.insert("DEMO-2".into());
    let rows = screen(&app, 80, 24);
    assert_eq!(rows[3].trim_end(), " ● ☐ In Progress 1");
    assert_eq!(rows[6].trim_end(), " ● ☒ Ready for Work 1");
    assert_eq!(
        rows[22].trim_end(),
        " 1 selected   B move or assign · u clear · A all"
    );
    assert!(rows[23].trim_end().ends_with("? more"), "{}", rows[23]);
}

#[test]
fn at_60x20_nothing_overflows_and_the_footer_keeps_whole_hints_and_more() {
    use unicode_width::UnicodeWidthStr;

    let mut app = app_with_groups();
    app.selected_index = 1;
    let rows = screen(&app, 60, 20);
    assert_eq!(
        trimmed(&rows),
        [
            " lazyjira   My Work   Team   Epics   Unassigned   Filters",
            &"─".repeat(60),
            "",
            " ● In Progress 1",
            " › DEMO-1  Add saved payment methods to checkout an...     -",
            "",
            " ● Ready for Work 1",
            "   DEMO-2  Profile slow facet queries                      -",
            "",
            " ● In Review 1",
            "   DEMO-3  Fix tax rounding                                -",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            " Tab switch · j/k move · Enter open · / search · ? more",
        ]
    );

    app.selected_ticket_keys.insert("DEMO-2".into());
    let rows = screen(&app, 60, 20);
    assert_eq!(
        rows[18].trim_end(),
        " 1 selected   B move or assign · u clear · A all"
    );
    assert_eq!(rows[6].trim_end(), " ● ☒ Ready for Work 1");
    assert_eq!(
        rows[19].trim_end(),
        " Tab switch · j/k move · Enter open · / search · ? more"
    );

    // Every cell is one column wide, so no glyph is half drawn at the edge.
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(60, 20)).unwrap();
    terminal.draw(|f| ui(f, &app, &sample_config())).unwrap();
    for cell in &terminal.backend().buffer().content {
        assert_eq!(cell.symbol().width(), 1, "{cell:?}");
    }
}
