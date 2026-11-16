//! Narrow terminals: Epic and Labels are drawn at their full width or not at all, Labels dropped
//! first, once Summary would keep fewer than `SUMMARY_MIN` (37) columns.

use super::support::*;
use crate::app::{App, Tab};
use crate::cache::{TeamMember, Ticket};
use crate::mouse::Target;
use crate::views::my_work;
use ratatui::{backend::TestBackend, layout::Rect, Terminal};

/// DEMO-101's row from `draw`, `width` columns wide, with the cursor on it.
fn my_work_row(app: &mut App, width: u16) -> String {
    app.selected_index = 1;
    line_with(&draw(app, Tab::MyWork, width), "DEMO-101")
        .unwrap()
        .0
        .clone()
}

/// (Epic drawn, Labels drawn) in `row`, a DEMO-101 row of `mockup_app`.
fn trailing_columns(row: &str) -> (bool, bool) {
    (
        row.contains("Checkout redesign"),
        row.contains("checkout, frontend"),
    )
}

#[test]
fn my_work_draws_epic_from_86_columns_and_labels_from_110_and_keeps_the_rest_at_summary() {
    // Lead 3, Key 12 (`  └ DEMO-111`), Updated 4 and two gaps leave 23 columns. Summary keeps 37,
    // so Epic (24 and a gap) needs 86 columns, Epic and Labels (22 and a gap) 110.
    let mut app = mockup_app();
    for (width, expected) in [
        (40, (false, false)),
        (60, (false, false)),
        (80, (false, false)),
        (85, (false, false)),
        (86, (true, false)),
        (109, (true, false)),
        (110, (true, true)),
        (120, (true, true)),
        (160, (true, true)),
    ] {
        let row = my_work_row(&mut app, width);
        assert_eq!(trailing_columns(&row), expected, "{width} columns: {row}");
        assert_eq!(row.chars().count(), width as usize, "{width}");
    }
}

#[test]
fn marking_moves_each_my_work_threshold_two_columns() {
    let mut app = mockup_app();
    app.selected_ticket_keys.insert("DEMO-201".into());
    for (width, expected) in [
        (87, (false, false)),
        (88, (true, false)),
        (111, (true, false)),
        (112, (true, true)),
    ] {
        let row = my_work_row(&mut app, width);
        assert_eq!(trailing_columns(&row), expected, "{width} columns: {row}");
    }
}

#[test]
fn a_column_every_row_shares_takes_no_room_so_labels_alone_come_at_84_columns() {
    let mut app = mockup_app();
    for ticket in &mut app.cache.my_tickets {
        ticket.epic_name = Some("Checkout redesign".into());
    }
    app.mark_cache_changed();
    for (width, labels) in [(83, false), (84, true), (120, true)] {
        let row = my_work_row(&mut app, width);
        assert_eq!(
            trailing_columns(&row),
            (false, labels),
            "{width} columns: {row}"
        );
    }
}

#[test]
fn my_work_at_80_and_100_columns_is_the_mockup_with_summary_taking_the_freed_width() {
    let mut app = mockup_app();
    app.selected_index = 1;
    for (width, expected) in [
        (
            80,
            [
                "",
                " ● In Progress 2",
                " › DEMO-101 ▼    Add saved payment methods to checkout                         -",
                "     └ DEMO-111  Migrate stored cards to the new vault                         -",
                "",
                " ● Ready for Work 1",
                "   DEMO-201      Profile slow facet queries                                    -",
                "",
                " ● In Review 1",
                "   DEMO-102      Fix tax rounding for multi-currency carts                     -",
                "",
            ],
        ),
        (
            100,
            [
                "",
                " ● In Progress 2",
                " › DEMO-101 ▼    Add saved payment methods to checkout                Checkout redesign            -",
                "     └ DEMO-111  Migrate stored cards to the new vault                Checkout redesign            -",
                "",
                " ● Ready for Work 1",
                "   DEMO-201      Profile slow facet queries                           Search performance           -",
                "",
                " ● In Review 1",
                "   DEMO-102      Fix tax rounding for multi-currency carts            Checkout redesign            -",
                "",
            ],
        ),
    ] {
        let rows = trimmed(&draw(&app, Tab::MyWork, width));
        assert_eq!(rows[..expected.len()], expected, "{width}: {rows:#?}");
        assert!(rows[expected.len()..].iter().all(String::is_empty));
    }
}

/// The 120 character summary of the long-summary tests: `x` and then a wide `漢`.
fn long_summary(wide: bool) -> String {
    if wide { "漢" } else { "x" }.repeat(if wide { 60 } else { 120 })
}

#[test]
fn a_long_summary_is_cut_with_an_ellipsis_to_57_columns_at_80_and_51_at_100() {
    for wide in [false, true] {
        let mut app = mockup_app();
        app.cache.my_tickets[0].summary = long_summary(wide);
        app.mark_cache_changed();
        // Summary starts at column 17. Its cell ends in the `...` at 71 to 73 (80 columns) and 65
        // to 67 (100 columns), then a gap, and the Updated cell is the last four columns. A wide
        // character that doesn't fit leaves its column blank, so the cell is still 57 or 51 wide.
        for (width, dots) in [(80usize, 71usize), (100, 65)] {
            let row = my_work_row(&mut app, width as u16);
            let cells: Vec<char> = row.chars().collect();
            let dots_at: String = cells[dots..dots + 3].iter().collect();
            assert_eq!(dots_at, "...", "wide {wide}, {width}: {row}");
            let tail: String = cells[width - 4..].iter().collect();
            assert_eq!(tail, "   -", "wide {wide}, {width}: {row}");
            let gap_end = if width == 80 { 76 } else { 70 };
            assert!(
                cells[dots + 3..gap_end].iter().all(|c| *c == ' '),
                "wide {wide}, {width}: {row}"
            );
        }
    }
}

#[test]
fn a_wide_epic_name_keeps_the_labels_column_in_line_at_120_columns() {
    let mut app = mockup_app();
    app.cache.my_tickets[0].epic_name = Some("決済画面の再設計決済画面の再設計決済".into());
    app.cache.my_tickets[0].labels = vec!["wide-row".into()];
    app.cache.my_tickets[3].labels = vec!["plain-row".into()];
    app.mark_cache_changed();
    let rows = draw(&app, Tab::MyWork, 120);
    let at = |key: &str, text: &str| column_of(&line_with(&rows, key).unwrap().0, text);
    assert_eq!(at("DEMO-101", "wide-row"), at("DEMO-102", "plain-row"));
    assert_eq!(at("DEMO-101", "wide-row"), 98);
}

/// Alex's DEMO-1 and DEMO-2 on the Team tab, in different epics with labels of their own.
fn team_app() -> App {
    let ticket = |key: &str, epic: &str, label: &str| {
        let mut ticket = Ticket::for_test(key, "In Progress");
        ticket.summary = format!("Summary of {key}");
        ticket.assignee_email = Some("alex@example.com".into());
        ticket.epic_name = Some(epic.into());
        ticket.labels = vec![label.into()];
        ticket
    };
    let mut app = App::new();
    app.loading = false;
    app.active_tab = Tab::Team;
    app.cache.team_members = vec![TeamMember {
        name: "Alex".into(),
        email: "alex@example.com".into(),
    }];
    app.cache.team_tickets = vec![
        ticket("DEMO-1", "Alpha epic", "alpha-label"),
        ticket("DEMO-2", "Beta epic", "beta-label"),
    ];
    app.mark_cache_changed();
    app
}

#[test]
fn team_draws_epic_from_91_columns_and_labels_from_111_and_always_keeps_status_and_updated() {
    // Lead 3, Key 6, the Status dot and "In Progress" (11), Updated 4 and three gaps leave 32.
    // Summary keeps 37, so Epic (20 and a gap) needs 94 columns, Epic and Labels (18 and a gap) 114.
    let app = team_app();
    for (width, expected) in [
        (60, (false, false)),
        (80, (false, false)),
        (90, (false, false)),
        (91, (true, false)),
        (110, (true, false)),
        (111, (true, true)),
        (120, (true, true)),
    ] {
        let rows = draw(&app, Tab::Team, width);
        let row = &line_with(&rows, "DEMO-1").unwrap().0;
        assert_eq!(
            (row.contains("Alpha epic"), row.contains("alpha-label")),
            expected,
            "{width} columns: {row}"
        );
        assert!(row.contains("● In Progress"), "{width}: {row}");
    }
    let rows = draw(&app, Tab::Team, 80);
    assert_eq!(
        line_with(&rows, "DEMO-1").unwrap().0.trim_end(),
        "   DEMO-1  ● In Progress  Summary of DEMO-1                                    -"
    );
}

#[test]
fn rows_register_the_same_click_and_drag_cells_at_80_and_120_columns() {
    let mut app = mockup_app();
    app.selected_index = 1;
    for width in [80u16, 120] {
        app.mouse_targets.borrow_mut().clear();
        app.text_selection.borrow_mut().areas.clear();
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal
            .draw(|f| my_work::render(f, f.area(), &app, None))
            .unwrap();
        // Item 1 is DEMO-101 on screen row 2, item 2 DEMO-111 on row 3, item 3 the Ready for Work
        // header on row 5: each row is selectable across the whole width, its mark is the three
        // gutter cells, and DEMO-101's fold arrow is the cell after its key.
        let row = |index, y| (Rect::new(0, y, width, 1), Target::Row(index));
        let mark = |index, y| (Rect::new(0, y, 3, 1), Target::Mark(index));
        let fold = |index, x, y| (Rect::new(x, y, 1, 1), Target::Fold(index));
        let targets = app.mouse_targets.borrow();
        assert_eq!(
            targets[..9],
            [
                row(0, 1),
                mark(0, 1),
                fold(0, 1, 1),
                row(1, 2),
                mark(1, 2),
                fold(1, 12, 2),
                row(2, 3),
                mark(2, 3),
                row(3, 5),
            ],
            "{width} columns"
        );
        // A drag copies from the key on: the gutter is not selectable.
        let areas = &app.text_selection.borrow().areas;
        assert_eq!(areas[..], [Rect::new(3, 0, width - 3, 12)], "{width}");
    }
}

#[test]
fn team_keeps_epic_and_labels_at_120_columns_beside_12_wide_keys_and_14_wide_statuses() {
    // The demo's Team: `DEMO-101 ▼` and `  └ DEMO-111` (12) and "Ready for Work" (14) leave
    // 3 + 12 + 14 + 2 + 4 + three gaps = 41 columns, so Epic and Labels need 41 + 37 + 42 = 120.
    let mut app = team_app();
    let mut child = app.cache.team_tickets[0].clone();
    child.key = "DEMO-111".into();
    child.parent_key = Some("DEMO-101".into());
    app.cache.team_tickets[0].key = "DEMO-101".into();
    app.cache.team_tickets[0].status = "Ready for Work".into();
    app.cache.team_tickets.push(child);
    app.mark_cache_changed();
    for (width, labels) in [(119, false), (120, true)] {
        let rows = draw(&app, Tab::Team, width);
        let row = &line_with(&rows, "DEMO-101").unwrap().0;
        assert!(row.contains("Alpha epic"), "{width}: {row}");
        assert_eq!(row.contains("alpha-label"), labels, "{width}: {row}");
    }
}
