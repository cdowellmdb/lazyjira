//! The ticket pane's text: where it ends beside the scrollbar, the space under the header rule,
//! that a code chip reaches the frame, and the views whose cells are pinned (History, 80 columns,
//! zoomed). The look of headings, bullets, quotes and code is `markup`'s tests.

use super::*;
use crate::app::DetailMode;
use ratatui::style::Color;

const FILLER: usize = 30;

fn description() -> String {
    let mut text = String::from(
        "h2. Problem statement\n\
         Facet queries take {{facet_counts}} over 800 ms and skip the cache \\(see DEMO-190), so \
         they make it worse and this line has to wrap before the end of the pane.\n\
         \n\
         h1. Findings\n\
         ||Query||Hits||\n\
         |title:*|12,400|\n\
         \n\
         bq. Count every facet in one pass.\n\
         \n",
    );
    for n in 1..=FILLER {
        text.push_str(&format!("* Item {n}\n"));
    }
    text
}

fn pane_app() -> App {
    let mut app = app_with_a_long_list();
    let ticket = &mut app.cache.my_tickets[29];
    ticket.detail_loaded = true;
    ticket.description = Some(description());
    ticket.activity = vec![cache::ActivityEntry {
        timestamp: "2026-09-02T10:00:00.000+0000".to_string(),
        author: "Sam Lee".to_string(),
        author_email: None,
        kind: cache::ActivityKind::Comment {
            body:
                "Agree, and {{the cache key}} is the part to test first, so this comment is long \
                   enough to wrap before the end of the pane."
                    .to_string(),
        },
    }];
    app.mark_cache_changed();
    app
}

/// The pane's app with the detail of `LAST-30` open, drawn at `width` x `height`.
async fn pane(width: u16, height: u16) -> (App, Vec<String>) {
    let mut config = sample_config();
    let mut app = pane_app();
    press(&mut app, &mut config, KeyCode::Enter).await;
    assert_eq!(app.detail_ticket_key.as_deref(), Some("LAST-30"));
    let rows = screen(&app, width, height);
    (app, rows)
}

/// `pane`'s app with the activity view open (`h`), drawn at `width` x `height`.
async fn history(width: u16, height: u16) -> (App, Vec<String>) {
    let mut config = sample_config();
    let (mut app, _) = pane(width, height).await;
    press(&mut app, &mut config, KeyCode::Char('h')).await;
    assert!(matches!(app.detail_mode, DetailMode::History { .. }));
    let rows = screen(&app, width, height);
    (app, rows)
}

/// The widths the pane sits beside the list at.
const WIDTHS: [u16; 2] = [120, 160];

/// The first text column of the pane: the list's 50 cells, the divider and the lead cell.
const TEXT_FIRST: usize = 52;

/// The last text column at `width`: the scrollbar's column is the last, and two blank cells
/// come before it.
fn text_last(width: u16) -> usize {
    width as usize - 4
}

/// The `count` cells of `row` from `from`.
fn cells(row: &str, from: usize, count: usize) -> String {
    row.chars().skip(from).take(count).collect()
}

/// The column and row of `text`'s first cell.
fn find_cell(rows: &[String], text: &str) -> (u16, u16) {
    let y = rows.iter().position(|row| row.contains(text)).unwrap();
    let byte = rows[y].find(text).unwrap();
    (rows[y][..byte].chars().count() as u16, y as u16)
}

/// The rows under the top bar and the rule, above the hints: two hint rows below 121 columns.
fn pane_rows(rows: &[String], width: u16) -> &[String] {
    let hints = if width < 121 { 2 } else { 1 };
    &rows[2..rows.len() - hints]
}

/// Every pane row has two blank cells after the text's last column, then the scrollbar's column,
/// which holds a thumb or is blank.
fn assert_gap(rows: &[String], width: u16, case: &str) {
    for row in pane_rows(rows, width) {
        assert_eq!(cells(row, text_last(width) + 1, 2), "  ", "{case}");
        assert!(
            matches!(cells(row, width as usize - 1, 1).as_str(), "█" | " "),
            "{case}"
        );
    }
}

/// The row of the rule under the header fields.
fn header_rule(rows: &[String], first: usize) -> usize {
    let below_the_top_bar = &rows[2..];
    2 + below_the_top_bar
        .iter()
        .position(|row| cells(row, first, 2) == "──")
        .unwrap_or_else(|| panic!("no rule\n{}", rows.join("\n")))
}

#[tokio::test]
async fn the_pane_text_ends_two_cells_before_the_scrollbar_column_at_120_and_160_columns() {
    for width in WIDTHS {
        let last = text_last(width);
        let (_, rows) = pane(width, 40).await;
        let case = format!("{width} columns\n{}", rows.join("\n"));
        let rule = &rows[header_rule(&rows, TEXT_FIRST)];
        // The lead cell, the text, two blank cells, then the scrollbar's column.
        assert_eq!(cells(rule, TEXT_FIRST - 1, 1), " ", "{case}");
        assert_eq!(
            cells(rule, TEXT_FIRST, last - TEXT_FIRST + 1),
            "─".repeat(last - TEXT_FIRST + 1),
            "{case}"
        );
        assert_eq!(cells(rule, last + 1, 2), "  ", "{case}");

        assert_gap(&rows, width, &case);
        assert!(
            pane_rows(&rows, width)
                .iter()
                .any(|row| cells(row, width as usize - 1, 1) == "█"),
            "the scrollbar shows in the last column\n{case}"
        );
    }
}

#[tokio::test]
async fn one_blank_row_follows_the_rule_under_the_header_fields() {
    for width in WIDTHS {
        let (_, rows) = pane(width, 40).await;
        let case = format!("{width} columns\n{}", rows.join("\n"));
        let rule = header_rule(&rows, TEXT_FIRST);
        assert!(
            cells(&rows[rule - 2], 51, 20).contains("Assignee"),
            "{case}"
        );
        assert_eq!(cells(&rows[rule - 1], 51, 60).trim(), "", "{case}");
        assert_eq!(cells(&rows[rule + 1], 51, 60).trim(), "", "{case}");
        assert_eq!(
            cells(&rows[rule + 2], 51, 18),
            " Problem statement",
            "{case}"
        );
    }
}

#[tokio::test]
async fn an_escaped_paren_loses_its_backslash_and_wrapped_lines_stay_in_the_text_cells() {
    for width in WIDTHS {
        let (mut app, rows) = pane(width, 40).await;
        let case = format!("{width} columns\n{}", rows.join("\n"));
        assert!(
            rows.iter().any(|row| row.contains("(see DEMO-190)")),
            "{case}"
        );
        assert!(rows.iter().all(|row| !row.contains('\\')), "{case}");

        app.detail_scroll = u16::MAX;
        let rows = screen(&app, width, 40);
        let case = format!("{width} columns\n{}", rows.join("\n"));
        let (x, y) = find_cell(&rows, "Sam Lee  2026-09-02 10:00");
        assert_eq!(x, 52, "{case}");
        // The comment's body is indented two cells under its author.
        assert_eq!(
            cells(&rows[y as usize + 1], 52, 26),
            "  Agree, and the cache key",
            "{case}"
        );
        assert_gap(&rows, width, &case);
    }
}

#[tokio::test]
async fn code_reaches_the_frame_as_a_chip_and_the_sample_pane_has_no_red_or_cyan_text() {
    let (app, rows) = pane(160, 40).await;
    let buf = draw(&app, 160, 40);

    let (x, y) = find_cell(&rows, "facet_counts");
    for i in 0..12 {
        assert_eq!(buf[(x + i, y)].fg, Color::Reset, "x {}", x + i);
        assert_eq!(buf[(x + i, y)].bg, Color::DarkGray, "x {}", x + i);
    }
    assert_eq!(buf[(x - 1, y)].bg, Color::Reset);
    assert_eq!(buf[(x + 12, y)].bg, Color::Reset);

    // Every cell of the pane below its header line: no red or cyan text. (The sample has no
    // mention, which is cyan on purpose, and no icon.)
    for y in 3..rows.len() as u16 - 1 {
        for x in TEXT_FIRST as u16 - 1..160 {
            let fg = buf[(x, y)].fg;
            assert!(
                !matches!(fg, Color::Red | Color::LightRed | Color::Cyan),
                "({x}, {y}) {:?}",
                buf[(x, y)]
            );
        }
    }
}

/// Presses at `from`, drags to `to` and releases, drawing after each event; the text copied.
async fn drag(width: u16, from: (u16, u16), to: (u16, u16)) -> Option<String> {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut config = sample_config();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let (mut app, _) = pane(width, 40).await;
    for (kind, (column, row)) in [
        (MouseEventKind::Down(MouseButton::Left), from),
        (MouseEventKind::Drag(MouseButton::Left), to),
        (MouseEventKind::Up(MouseButton::Left), to),
    ] {
        let event = crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        mouse::handle(&mut app, event, &tx, &mut config).await;
        screen(&app, width, 40);
    }
    mouse::selected_text(&mut app)
}

#[tokio::test]
async fn the_drag_area_and_the_scroll_page_match_the_drawn_pane_at_120_and_160_columns() {
    // The pane is the 40 rows less the top bar and its rule and the hints, which take two rows
    // at 120 columns and one at 160. The scroll page is the pane less its header line.
    for (width, last, pane_height, page) in [(120, 116, 36, 35), (160, 156, 37, 36)] {
        let (app, rows) = pane(width, 40).await;
        let case = format!("{width} columns\n{}", rows.join("\n"));
        let text_width = (last - TEXT_FIRST + 1) as u16;
        let areas = app.text_selection.borrow().areas.clone();
        assert!(
            areas.contains(&Rect::new(52, 2, text_width, pane_height)),
            "{areas:?}\n{case}"
        );
        assert_eq!(app.detail_page_height.get(), page, "{case}");

        let rule = header_rule(&rows, TEXT_FIRST) as u16;
        // Dragging across the rule, the gap and the scrollbar copies the text cells only.
        let copied = drag(width, (52, rule), (width - 1, rule)).await.unwrap();
        assert_eq!(copied, "─".repeat(text_width as usize), "{case}");
        let copied = drag(width, (width - 4, rule), (width - 1, rule)).await;
        assert_eq!(copied.as_deref(), Some("─"), "{case}");
        assert_eq!(
            drag(width, (width - 3, rule), (width - 1, rule)).await,
            None
        );
    }
}

#[tokio::test]
async fn scrolled_to_the_bottom_the_last_comment_line_sits_on_the_last_pane_row() {
    for width in WIDTHS {
        let last = text_last(width);
        let (mut app, _) = pane(width, 40).await;
        app.detail_scroll = u16::MAX;
        let rows = screen(&app, width, 40);
        let case = format!("{width} columns\n{}", rows.join("\n"));
        let pane = pane_rows(&rows, width);
        let bottom = pane.last().unwrap();
        let text = cells(bottom, TEXT_FIRST, last - TEXT_FIRST + 1);
        assert!(text.trim_end().ends_with("pane."), "{case}");
        assert_eq!(cells(bottom, width as usize - 1, 1), "█", "{case}");
    }
}

#[test]
fn the_epic_view_ends_two_cells_before_the_scrollbar_column_too() {
    for width in WIDTHS {
        let last = text_last(width);
        let mut app = App::new();
        app.loading = false;
        app.cache.epics = vec![cache::Epic::for_test("DEMO-3228", &[("In Progress", 60)])];
        app.detail_epic_key = Some("DEMO-3228".into());
        let rows = screen(&app, width, 40);
        let case = format!("{width} columns\n{}", rows.join("\n"));
        let (x, y) = find_cell(&rows, "── Related tickets (60) ");
        assert_eq!(x, 52, "{case}");
        let section = &rows[y as usize];
        assert_eq!(cells(section, last - 2, 3), "───", "{case}");
        assert_eq!(cells(section, last + 1, 2), "  ", "{case}");
        let (x, _) = find_cell(&rows, "Progress");
        assert_eq!(x, 52, "{case}");
        assert_gap(&rows, width, &case);
        assert!(
            pane_rows(&rows, width)
                .iter()
                .any(|row| cells(row, width as usize - 1, 1) == "█"),
            "{case}"
        );
    }
}

/// The column after the last text cell of any pane row: the activity view's widest line.
fn text_end(rows: &[String]) -> usize {
    rows[2..rows.len() - 1]
        .iter()
        .map(|row| cells(row, 52, 200).trim_end().chars().count() + 52)
        .max()
        .unwrap()
}

#[tokio::test]
async fn the_history_view_keeps_the_drag_area_and_text_cells_it_had_before_stage_5() {
    // Pinned to what `feat/tui-narrow-docs` draws: the pane less one cell on each side, so the
    // text runs from column 52 to the column before the scrollbar's.
    for (width, area, last) in [
        (120, Rect::new(52, 2, 67, 37), 118),
        (160, Rect::new(52, 2, 107, 37), 158),
    ] {
        let (mut app, _) = history(width, 40).await;
        // An author no wider line holds, so it is broken at the text's last cell.
        app.cache.my_tickets[29].activity[0].author = "x".repeat(300);
        app.mark_cache_changed();
        let rows = screen(&app, width, 40);
        let case = format!("{width} columns\n{}", rows.join("\n"));
        let areas = app.text_selection.borrow().areas.clone();
        assert!(areas.contains(&area), "{areas:?}\n{case}");
        assert_eq!(cells(&rows[3], 51, 12), " Activity: c", "{case}");
        assert_eq!(text_end(&rows), last + 1, "{case}");
        assert!(
            rows.iter()
                .any(|row| cells(row, 52, last - 51) == "x".repeat(last - 51)),
            "{case}"
        );
        for row in &rows[2..rows.len() - 1] {
            assert_eq!(cells(row, last + 1, 1), " ", "{case}");
        }
    }
}

#[tokio::test]
async fn at_80_columns_the_pane_fills_the_area_and_its_text_ends_before_the_scrollbar() {
    let (app, rows) = pane(80, 24).await;
    let case = rows.join("\n");
    assert!(rows.iter().all(|row| !row.contains("LEAK-")), "{case}");
    assert!(rows.iter().all(|row| cells(row, 50, 1) != "│"), "{case}");
    let areas = app.text_selection.borrow().areas.clone();
    assert!(
        areas.contains(&Rect::new(1, 2, 76, 20)),
        "{areas:?}\n{case}"
    );
    assert_eq!(app.detail_page_height.get(), 19, "{case}");
    let rule = header_rule(&rows, 1);
    assert_eq!(
        cells(&rows[rule], 0, 79),
        format!(" {}  ", "─".repeat(76)),
        "{case}"
    );
    for row in pane_rows(&rows, 80) {
        assert_eq!(cells(row, 77, 2), "  ", "{case}");
        assert!(matches!(cells(row, 79, 1).as_str(), "█" | " "), "{case}");
    }
}

#[tokio::test]
async fn zoomed_at_160_columns_the_pane_text_runs_from_column_1_to_156() {
    let mut config = sample_config();
    let (mut app, _) = pane(160, 40).await;
    press(&mut app, &mut config, KeyCode::Char('z')).await;
    let rows = screen(&app, 160, 40);
    let case = rows.join("\n");
    assert!(rows.iter().all(|row| !row.contains("LEAK-")), "{case}");
    let areas = app.text_selection.borrow().areas.clone();
    assert!(
        areas.contains(&Rect::new(1, 2, 156, 37)),
        "{areas:?}\n{case}"
    );
    assert_eq!(app.detail_page_height.get(), 36, "{case}");
    let rule = header_rule(&rows, 1);
    assert_eq!(
        cells(&rows[rule], 0, 159),
        format!(" {}  ", "─".repeat(156)),
        "{case}"
    );
    for row in pane_rows(&rows, 160) {
        assert_eq!(cells(row, 157, 2), "  ", "{case}");
        assert!(matches!(cells(row, 159, 1).as_str(), "█" | " "), "{case}");
    }
}
