//! Help and settings, the full-width sheets.

use super::*;

#[test]
fn keybindings_help_adapts_scrolls_and_keeps_close_visible() {
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    fn draw(app: &App, width: u16, height: u16) -> (Buffer, Vec<String>) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| ui(f, app, &sample_config())).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let lines = (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        (buffer, lines)
    }

    let mut app = App::new();
    app.toggle_keybindings();
    let (buffer, wide) = draw(&app, 160, 55);
    let heading = wide
        .iter()
        .position(|line| line.contains("Navigation"))
        .unwrap();
    assert!(
        wide[heading].contains("Detail navigation"),
        "two columns on wide terminals"
    );
    assert!(wide
        .iter()
        .any(|line| line.contains("New / edit / delete filter")));
    assert!(wide.iter().any(|line| line.contains("Esc/? close")));
    assert_eq!(app.keybindings_scroll_max.get(), 0);
    let key_row = wide
        .iter()
        .position(|line| line.contains("Next tab"))
        .unwrap();
    let key_x = wide[key_row].find("Tab").unwrap();
    assert_eq!(buffer[(key_x as u16, key_row as u16)].fg, Color::Cyan);

    for width in [80, 40] {
        let (_, lines) = draw(&app, width, 24);
        assert!(app.keybindings_scroll_max.get() > 0);
        assert!(lines.iter().any(|line| line.contains("Esc/? close")));
        handle_keybindings_keys(&mut app, KeyCode::PageDown);
        assert_eq!(app.keybindings_scroll, app.keybindings_page_height.get());
        handle_keybindings_keys(&mut app, KeyCode::End);
        let (_, bottom) = draw(&app, width, 24);
        assert!(bottom
            .iter()
            .any(|line| line.contains("New / edit / delete filter")));
        assert!(bottom.iter().any(|line| line.contains("Esc/? close")));
        handle_keybindings_keys(&mut app, KeyCode::Home);
    }
    handle_keybindings_keys(&mut app, KeyCode::Down);
    assert_eq!(app.keybindings_scroll, 1);
    handle_keybindings_keys(&mut app, KeyCode::Up);
    assert_eq!(app.keybindings_scroll, 0);
    handle_keybindings_keys(&mut app, KeyCode::End);
    draw(&app, 160, 55); // Resizing clamps a stale scroll offset.
    handle_keybindings_keys(&mut app, KeyCode::Down);
    assert_eq!(app.keybindings_scroll, 0);
    handle_keybindings_keys(&mut app, KeyCode::Esc);
    assert!(!app.show_keybindings);
    app.toggle_keybindings();
    assert_eq!(app.keybindings_scroll, 0);
    draw(&app, 8, 4); // Tiny terminals must not panic.
}

#[tokio::test]
async fn help_and_settings_are_sheets_over_the_whole_frame_with_a_title_a_close_cell_and_no_box() {
    for (name, key, title) in [
        ("help", '?', "Keyboard shortcuts"),
        ("settings", 'S', "Settings"),
    ] {
        for (width, height) in [(80, 24), (120, 30), (160, 45)] {
            let mut config = two_saved_filters();
            let mut app = app_with_a_long_list();
            press(&mut app, &mut config, KeyCode::Char(key)).await;
            let rows = screen(&app, width, height);
            let case = format!("{name} at {width}x{height}\n{}", rows.join("\n"));
            assert!(rows[0].starts_with(&format!(" {title} ")), "{case}");
            assert!(rows[0].ends_with(" [×]  "), "{case}");
            // Nothing of the screen behind it shows: not the top bar, the list or the footer.
            for covered in [
                " lazyjira   My Work",
                "LEAK",
                "LAST",
                "Tab switch",
                "Unassigned   Filters",
            ] {
                assert!(
                    rows.iter().all(|row| !row.contains(covered)),
                    "{covered:?} shows through: {case}"
                );
            }
            for glyph in [
                '╭', '╮', '╰', '╯', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼', '│',
            ] {
                assert!(
                    rows.iter().all(|row| !row.contains(glyph)),
                    "{glyph}: {case}"
                );
            }
            let close = Rect::new(width - 6, 0, 5, 1);
            assert!(
                app.mouse_targets
                    .borrow()
                    .contains(&(close, mouse::Target::Key(KeyCode::Esc))),
                "{case}"
            );
            assert!(
                app.text_selection
                    .borrow()
                    .areas
                    .iter()
                    .all(|area| area.y >= 2 && area.x == 2 && area.right() == width - 2),
                "{case}"
            );
        }
    }
}

#[tokio::test]
async fn clicking_the_sheets_close_cell_closes_it_and_the_cell_before_it_does_not() {
    let mut config = two_saved_filters();
    for (key, open) in [
        ('?', (|app: &App| app.show_keybindings) as fn(&App) -> bool),
        ('S', |app: &App| app.settings.is_some()),
    ] {
        for (x, closes) in [(153, false), (154, true), (158, true), (159, false)] {
            let mut app = app_with_a_long_list();
            press(&mut app, &mut config, KeyCode::Char(key)).await;
            screen(&app, 160, 40);
            click(&mut app, &mut config, x, 0).await;
            assert_eq!(!open(&app), closes, "{key} clicked at {x}");
        }
    }
}
