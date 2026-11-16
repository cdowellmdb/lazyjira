//! The ticket detail beside the list or in place of it.

use super::*;

/// Enter on the selected row of a long list, then the detail's screen at `width` x 30.
async fn detail_screen(width: u16) -> (App, Vec<String>) {
    let mut config = sample_config();
    let mut app = app_with_a_long_list();
    app.cache.my_tickets[29].detail_loaded = true;
    press(&mut app, &mut config, KeyCode::Enter).await;
    assert_eq!(app.detail_ticket_key.as_deref(), Some("LAST-30"));
    let rows = screen(&app, width, 30);
    (app, rows)
}

#[test]
fn the_panes_split_at_exactly_120_columns_with_a_50_cell_list_and_a_1_cell_divider() {
    let app = App::new();
    let area = |width| Rect::new(0, 2, width, 27);
    for width in [0, 1, 80, 119] {
        let panes = widgets::ticket_detail::panes(&app, area(width));
        assert_eq!(panes.list, None, "{width} columns");
        assert_eq!(panes.divider, None, "{width} columns");
        assert_eq!(panes.detail, area(width), "{width} columns");
    }
    for width in [120, 121, 160] {
        let panes = widgets::ticket_detail::panes(&app, area(width));
        assert_eq!(panes.list, Some(Rect::new(0, 2, 50, 27)), "{width} columns");
        assert_eq!(
            panes.divider,
            Some(Rect::new(50, 2, 1, 27)),
            "{width} columns"
        );
        assert_eq!(
            panes.detail,
            Rect::new(51, 2, width - 51, 27),
            "{width} columns"
        );
    }
    let mut zoomed = App::new();
    zoomed.detail_fullscreen = true;
    let panes = widgets::ticket_detail::panes(&zoomed, area(160));
    assert_eq!((panes.list, panes.detail), (None, area(160)), "z zooms");
}

#[tokio::test]
async fn at_120_columns_detail_has_the_list_on_the_left_and_at_119_it_replaces_it() {
    let (_, rows) = detail_screen(119).await;
    assert!(
        rows.iter().all(|row| !row.contains("LEAK")),
        "the list is gone:\n{}",
        rows.join("\n")
    );
    assert!(rows[2].starts_with(" LAST-30  30 of 30"), "{}", rows[2]);
    assert!(rows[2].trim_end().ends_with("×"), "{}", rows[2]);
    assert!(!column(&rows, 50).contains('│'), "no divider at 119");

    for width in [120, 160] {
        let (_, rows) = detail_screen(width).await;
        let case = format!("{width} columns\n{}", rows.join("\n"));
        // The 11 detail hints fit one row at 160 columns and take two at 120.
        let pane_rows = if width == 120 { 26 } else { 27 };
        // `column` stops above the last row, so the 120 column screen's first hint row is in it.
        assert_eq!(
            column(&rows, 50),
            format!("{}{}", "│".repeat(pane_rows), " ".repeat(27 - pane_rows)),
            "{case}"
        );
        // Only the list is left of the divider, and only the pane right of it.
        for row in &rows[2..2 + pane_rows] {
            let (list, pane) = row.split_at(row.char_indices().nth(50).unwrap().0);
            assert!(!pane.contains("LEAK"), "{case}");
            assert!(list.chars().count() == 50, "{case}");
        }
        assert!(
            rows[2]
                .chars()
                .skip(51)
                .collect::<String>()
                .starts_with(" LAST-30  30 of 30"),
            "{case}"
        );
        // The list shows only keys and summaries, and keeps the selected row in view.
        let selected = rows.iter().find(|row| row.contains('›')).expect(&case);
        assert!(selected.starts_with(" › LAST-30  Summary"), "{case}");
        let list: String = selected.chars().take(50).collect();
        assert!(
            list.trim_end().ends_with("Summary"),
            "no other column: {case}"
        );
        assert!(
            rows[2..2 + pane_rows]
                .iter()
                .any(|row| row.starts_with("   LEAK-")),
            "{case}"
        );
    }
}

#[tokio::test]
async fn z_zooms_the_detail_over_the_list_at_160_columns_and_z_again_brings_the_list_back() {
    let (mut app, _) = detail_screen(160).await;
    let mut config = sample_config();
    press(&mut app, &mut config, KeyCode::Char('z')).await;
    let rows = screen(&app, 160, 30);
    assert!(
        rows.iter().all(|row| !row.contains("LEAK")),
        "{}",
        rows.join("\n")
    );
    assert!(rows[2].starts_with(" LAST-30"), "{}", rows[2]);
    press(&mut app, &mut config, KeyCode::Char('z')).await;
    let rows = screen(&app, 160, 30);
    assert!(rows.iter().any(|row| row.contains("LEAK-")));
}

#[tokio::test]
async fn the_left_and_right_keys_move_the_lists_selection_with_the_shown_ticket() {
    let (mut app, _) = detail_screen(160).await;
    let mut config = sample_config();
    press(&mut app, &mut config, KeyCode::Left).await;
    assert_eq!(app.detail_ticket_key.as_deref(), Some("LEAK-29"));
    assert_eq!(app.selected_index, 29);
    let rows = screen(&app, 160, 30);
    let selected = rows.iter().find(|row| row.contains('›')).unwrap();
    assert!(selected.starts_with(" › LEAK-29  Summary"), "{selected}");
    assert!(rows[2]
        .chars()
        .skip(51)
        .collect::<String>()
        .starts_with(" LEAK-29  29 of 30"));
    press(&mut app, &mut config, KeyCode::Right).await;
    assert_eq!(app.detail_ticket_key.as_deref(), Some("LAST-30"));
}

#[tokio::test]
async fn beside_the_detail_a_click_on_a_list_row_shows_that_ticket_and_the_close_button_closes() {
    let mut config = sample_config();
    let (mut app, mut rows) = detail_screen(160).await;
    // Any cell of the row, key or summary, shows it.
    for (x, key) in [(3, "LEAK-27"), (20, "LEAK-26"), (49, "LEAK-25")] {
        let y = rows.iter().position(|row| row.contains(key)).unwrap() as u16;
        click(&mut app, &mut config, x, y).await;
        assert_eq!(app.detail_ticket_key.as_deref(), Some(key), "column {x}");
        rows = screen(&app, 160, 30);
        let selected = rows.iter().find(|row| row.contains('›')).unwrap();
        assert!(selected.starts_with(&format!(" › {key}")), "{selected}");
    }
    assert_eq!(app.selected_index, 25);
    // The divider and the top bar aren't targets, and the pane's text isn't either.
    let y = rows.iter().position(|row| row.contains("LEAK-25")).unwrap() as u16;
    for (x, y) in [(50, y), (3, 0), (140, 0), (80, y)] {
        click(&mut app, &mut config, x, y).await;
        assert_eq!(
            app.detail_ticket_key.as_deref(),
            Some("LEAK-25"),
            "({x}, {y})"
        );
        assert_eq!(app.active_tab, Tab::MyWork);
        screen(&app, 160, 30);
    }
    // The × sits in a 5 cell click cell ending a cell before the right edge of the header.
    for (x, closes) in [(153, false), (154, true), (158, true), (159, false)] {
        let (mut app, _) = detail_screen(160).await;
        click(&mut app, &mut config, x, 2).await;
        assert_eq!(!app.is_detail_open(), closes, "click at {x}");
    }
}

/// Presses at `from`, drags to `to` and releases, drawing after each event; the text copied.
async fn drag_text(from: (u16, u16), to: (u16, u16)) -> Option<String> {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut config = sample_config();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let (mut app, _) = detail_screen(160).await;
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
        screen(&app, 160, 30);
    }
    mouse::selected_text(&mut app)
}

#[tokio::test]
async fn a_drag_in_the_detail_pane_copies_detail_text_only_and_a_drag_in_the_list_copies_row_text()
{
    let (_, rows) = detail_screen(160).await;
    let status = rows.iter().position(|row| row.contains("Status")).unwrap() as u16;
    let leak = rows.iter().position(|row| row.contains("LEAK-27")).unwrap() as u16;
    // Dragging on over the divider and the list stays in the pane the drag began in.
    let copied = drag_text((70, status), (30, status)).await.unwrap();
    assert!(copied.starts_with("Status"), "{copied:?}");
    assert!(
        !copied.contains("LEAK") && !copied.contains('│'),
        "{copied:?}"
    );
    assert_eq!(
        drag_text((52, status), (70, status)).await.unwrap(),
        "Status    ● In Pro"
    );
    // The same from the list into the pane: row text only.
    let copied = drag_text((3, leak), (159, leak)).await.unwrap();
    assert_eq!(copied, "LEAK-27  Summary");
    // The divider is selectable by neither.
    assert_eq!(drag_text((50, status), (50, status + 1)).await, None);
}

#[tokio::test]
async fn beside_the_detail_the_list_registers_only_show_row_targets_for_ticket_rows() {
    use crate::mouse::Target;
    let (app, rows) = detail_screen(160).await;
    let leak = rows.iter().position(|row| row.contains("LEAK-27")).unwrap() as u16;
    // The list pane's rows, 2 to 28; the hint line below them has its own keys.
    let list: Vec<(Rect, Target)> = app
        .mouse_targets
        .borrow()
        .iter()
        .filter(|(area, _)| area.x < 50 && (2..29).contains(&area.y))
        .copied()
        .collect();
    assert!(
        list.iter()
            .all(|(_, target)| matches!(target, Target::ShowRow(_))),
        "no gutter mark, fold arrow or plain row: {list:?}"
    );
    // The long list is scrolled to its last ticket: 27 rows of tickets 4 to 30 fill the pane,
    // each one target across the pane's 50 cells.
    assert_eq!(list.len(), 27, "{list:?}");
    assert_eq!(list[0], (Rect::new(0, 2, 50, 1), Target::ShowRow(4)));
    assert!(list.contains(&(Rect::new(0, leak, 50, 1), Target::ShowRow(27))));
}

#[tokio::test]
async fn beside_the_detail_the_gutter_shows_its_row_and_a_group_header_moves_nothing() {
    let mut config = sample_config();
    let (mut app, rows) = detail_screen(160).await;
    let leak = rows.iter().position(|row| row.contains("LEAK-27")).unwrap() as u16;
    // The gutter and the cell where a fold arrow would be are part of the row.
    for x in [0, 1, 2] {
        click(&mut app, &mut config, x, leak).await;
        assert_eq!(app.detail_ticket_key.as_deref(), Some("LEAK-27"), "x {x}");
        assert_eq!(app.selected_index, 27);
        screen(&app, 160, 30);
        let (reopened, _) = detail_screen(160).await;
        app = reopened;
    }

    // A short list, whose group header is on screen: the selection and the detail stay put.
    let mut app = app_with_a_ticket_on_every_tab(Tab::MyWork);
    press(&mut app, &mut config, KeyCode::Enter).await;
    assert_eq!(app.detail_ticket_key.as_deref(), Some("DEMO-1"));
    let rows = screen(&app, 160, 30);
    let header = rows
        .iter()
        .position(|row| row.contains("In Progress"))
        .unwrap() as u16;
    for x in [0, 1, 2, 5, 30, 49] {
        click(&mut app, &mut config, x, header).await;
        assert_eq!(app.selected_index, 1, "x {x}");
        assert_eq!(app.detail_ticket_key.as_deref(), Some("DEMO-1"), "x {x}");
        screen(&app, 160, 30);
    }
}

#[tokio::test]
async fn beside_the_detail_an_epics_header_shows_the_epic() {
    let mut config = sample_config();
    let mut app = app_with_a_ticket_on_every_tab(Tab::Epics);
    press(&mut app, &mut config, KeyCode::Enter).await;
    assert_eq!(app.detail_ticket_key.as_deref(), Some("DEMO-1"));
    let rows = screen(&app, 160, 30);
    let header = rows
        .iter()
        .position(|row| row.contains("DEMO-100"))
        .unwrap() as u16;
    click(&mut app, &mut config, 20, header).await;
    assert_eq!(app.detail_epic_key.as_deref(), Some("DEMO-100"));
    assert_eq!(app.selected_index, 0);
}

/// The cells of every key hint on the rows from `from` down, as (x, y, width) and the key.
fn hint_cells(app: &App, from: u16) -> Vec<(u16, u16, u16, KeyCode)> {
    app.mouse_targets
        .borrow()
        .iter()
        .filter(|(area, _)| area.y >= from)
        .filter_map(|(area, target)| match target {
            mouse::Target::Key(key) => Some((area.x, area.y, area.width, *key)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn every_detail_hint_keeps_its_click_cell_at_80_120_and_160_columns() {
    // 11 hints take 121 columns, so below that they wrap onto a second row. The two rows are as
    // even as whole hints allow, so 80 and 120 columns wrap alike.
    let wrapped = [
        (13, 28, 1, KeyCode::Left),
        (14, 28, 1, KeyCode::Right),
        (28, 28, 1, KeyCode::Char('m')),
        (37, 28, 1, KeyCode::Char('C')),
        (49, 28, 1, KeyCode::Char('a')),
        (1, 29, 1, KeyCode::Char('e')),
        (10, 29, 1, KeyCode::Char('h')),
        (23, 29, 1, KeyCode::Char('o')),
        (35, 29, 1, KeyCode::Char('z')),
        (44, 29, 1, KeyCode::Char('?')),
        (53, 29, 3, KeyCode::Esc),
    ];
    let one_row = [
        (13, 29, 1, KeyCode::Left),
        (14, 29, 1, KeyCode::Right),
        (28, 29, 1, KeyCode::Char('m')),
        (37, 29, 1, KeyCode::Char('C')),
        (49, 29, 1, KeyCode::Char('a')),
        (60, 29, 1, KeyCode::Char('e')),
        (69, 29, 1, KeyCode::Char('h')),
        (82, 29, 1, KeyCode::Char('o')),
        (94, 29, 1, KeyCode::Char('z')),
        (103, 29, 1, KeyCode::Char('?')),
        (112, 29, 3, KeyCode::Esc),
    ];
    for (width, cells) in [(80, &wrapped), (120, &wrapped), (160, &one_row)] {
        let (app, rows) = detail_screen(width).await;
        assert_eq!(
            hint_cells(&app, 27),
            *cells,
            "{width} columns\n{}",
            rows.join("\n")
        );
    }
    let (_, rows) = detail_screen(80).await;
    assert_eq!(
        rows[28].trim_end(),
        " ↑↓ scroll · ←→ prev/next · m move · C comment · a assign"
    );
    assert_eq!(
        rows[29].trim_end(),
        " e edit · h activity · o browser · z zoom · ? help · Esc close"
    );
}

#[tokio::test]
async fn clicking_the_hints_the_narrow_layout_wrapped_acts_on_them() {
    let mut config = sample_config();
    let (mut app, _) = detail_screen(80).await;
    // `z` is the fourth hint of the second row, `h` the second, `?` the fifth.
    click(&mut app, &mut config, 35, 29).await;
    assert!(app.detail_fullscreen, "z");
    click(&mut app, &mut config, 35, 29).await;
    assert!(!app.detail_fullscreen, "z again");
    click(&mut app, &mut config, 44, 29).await;
    assert!(app.show_keybindings, "?");
    press(&mut app, &mut config, KeyCode::Esc).await;
    screen(&app, 80, 30);
    click(&mut app, &mut config, 10, 29).await;
    assert!(matches!(app.detail_mode, DetailMode::History { .. }), "h");
}

#[tokio::test]
async fn the_detail_hints_show_while_a_search_is_still_set_after_enter() {
    let mut config = sample_config();
    let mut app = app_with_a_long_list();
    app.cache.my_tickets[29].detail_loaded = true;
    press(&mut app, &mut config, KeyCode::Char('/')).await;
    for c in "LAST".chars() {
        press(&mut app, &mut config, KeyCode::Char(c)).await;
    }
    press(&mut app, &mut config, KeyCode::Enter).await;
    assert!(app.search.is_some(), "the search stays after Enter");
    assert_eq!(app.detail_ticket_key.as_deref(), Some("LAST-30"));
    let rows = screen(&app, 80, 30);
    assert_eq!(
        rows[29].trim_end(),
        " e edit · h activity · o browser · z zoom · ? help · Esc close"
    );
    assert!(rows[28].starts_with(" ↑↓ scroll"), "{}", rows[28]);
    assert!(
        rows.iter().all(|row| !row.starts_with("/LAST")),
        "{}",
        rows.join("\n")
    );
    let keys: Vec<KeyCode> = hint_cells(&app, 28)
        .into_iter()
        .map(|cell| cell.3)
        .collect();
    assert_eq!(keys.len(), 11);
    assert!(keys.contains(&KeyCode::Esc));
    // Esc closes the detail and the search shows again.
    press(&mut app, &mut config, KeyCode::Esc).await;
    let rows = screen(&app, 80, 30);
    assert_eq!(rows[29].trim_end(), "/LAST");
}

#[tokio::test]
async fn a_flash_or_a_pending_move_takes_a_row_above_the_detail_hints_and_hides_none() {
    let (mut app, _) = detail_screen(80).await;
    app.flash = Some("Couldn't open browser".to_string());
    let rows = screen(&app, 80, 30);
    assert_eq!(
        rows[27].trim_end(),
        "Couldn't open browser",
        "{}",
        rows.join("\n")
    );
    assert_eq!(
        rows[28].trim_end(),
        " ↑↓ scroll · ←→ prev/next · m move · C comment · a assign"
    );
    assert_eq!(hint_cells(&app, 28).len(), 11);
    app.flash = None;
    assert!(app.moves.start("LAST-30", "Done"));
    let rows = screen(&app, 80, 30);
    let pending = app.moves.pending_message().unwrap();
    assert_eq!(rows[27].trim_end(), pending, "{}", rows.join("\n"));
    assert_eq!(hint_cells(&app, 28).len(), 11);
    assert_eq!(
        rows[29].trim_end(),
        " e edit · h activity · o browser · z zoom · ? help · Esc close"
    );
}

#[tokio::test]
async fn a_detail_draws_in_tiny_terminals_without_panicking() {
    let (app, _) = detail_screen(80).await;
    for width in [0, 1, 3, 8, 20, 40] {
        for height in [0, 1, 2, 4, 6, 10] {
            screen(&app, width, height);
        }
    }
}
