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
    let (app, rows) = detail_screen(119).await;
    assert!(
        rows.iter().all(|row| !row.contains("LEAK")),
        "the list is gone:\n{}",
        rows.join("\n")
    );
    assert!(rows[2].starts_with(" LAST-30  30 of 30"), "{}", rows[2]);
    assert!(rows[2].trim_end().ends_with("[×]"), "{}", rows[2]);
    assert!(!column(&rows, 50).contains('│'), "no divider at 119");
    assert_eq!(app.compact_list_width.get(), None);

    for width in [120, 160] {
        let (app, rows) = detail_screen(width).await;
        let case = format!("{width} columns\n{}", rows.join("\n"));
        assert_eq!(column(&rows, 50), "│".repeat(27), "{case}");
        assert_eq!(app.compact_list_width.get(), Some(50));
        // Only the list is left of the divider, and only the pane right of it.
        for row in &rows[2..29] {
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
            rows[2..29].iter().any(|row| row.starts_with("   LEAK-")),
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
    // [×] is 5 cells ending a cell before the right edge of the header.
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
