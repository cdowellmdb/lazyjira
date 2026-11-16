//! The whole screen: the top bar, the lists, theming and glyph widths on every tab.

use super::*;

#[test]
fn presets_theme_every_cell_on_every_tab_and_overlay() {
    use ratatui::{backend::TestBackend, style::Color};
    let mut config = sample_config();
    let mut app = App::new();
    app.loading = false;
    app.cache.my_tickets = [
        "In Progress",
        "Ready for Work",
        "Blocked",
        "In Review",
        "Done",
        "Odd",
    ]
    .iter()
    .enumerate()
    .map(|(i, status)| {
        let mut ticket = ticket(&format!("DEMO-{i}"), "Summary", status);
        ticket.description =
            Some("*bold* [link|https://example.com] {color:red}red{color} @alex".into());
        ticket.labels = vec!["label".into()];
        ticket
    })
    .collect();
    for (name, theme) in theme::PRESETS {
        config.preferences.theme = name.to_string();
        app.theme = *theme;
        for overlay in 0..4 {
            for tab in Tab::all() {
                app.active_tab = *tab;
                // A ticket row, so the selected row's colors are drawn too.
                app.selected_index = 1;
                app.close_detail();
                app.show_keybindings = overlay == 1;
                app.settings = (overlay == 2).then(|| settings::Settings::new(&config));
                if overlay == 3 {
                    app.open_detail("DEMO-0".into());
                }
                let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
                terminal.draw(|f| ui(f, &app, &config)).unwrap();
                for cell in &terminal.backend().buffer().content {
                    assert!(
                        matches!((cell.fg, cell.bg), (Color::Rgb(..), Color::Rgb(..))),
                        "{name}, {tab:?}, overlay {overlay}: {cell:?} kept a terminal color"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn every_cell_on_every_tab_and_overlay_is_one_column_wide_at_80_120_and_160_columns() {
    use ratatui::backend::TestBackend;
    use unicode_width::UnicodeWidthStr;

    fn assert_one_column_cells(app: &App, config: &AppConfig, scene: &str) {
        for width in [80, 120, 160] {
            let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
            terminal.draw(|f| ui(f, app, config)).unwrap();
            for cell in &terminal.backend().buffer().content {
                assert_eq!(
                    cell.symbol().width(),
                    1,
                    "{scene} at {width} columns drew {:?}",
                    cell.symbol()
                );
            }
        }
    }

    type Scene = (&'static str, Tab, usize, Vec<KeyCode>, fn(&App) -> bool);
    let on_ticket = |scene, tab, keys: &[char], is_open| -> Scene {
        let presses = keys.iter().map(|key| KeyCode::Char(*key)).collect();
        (scene, tab, 1, presses, is_open)
    };
    let in_detail = |scene, keys: &[char], is_open| -> Scene {
        let mut scene = on_ticket(scene, Tab::MyWork, keys, is_open);
        scene.3.insert(0, KeyCode::Enter);
        scene
    };
    let scenes: Vec<Scene> = vec![
        on_ticket("My Work", Tab::MyWork, &[], |_| true),
        on_ticket("Team", Tab::Team, &[], |_| true),
        on_ticket("Epics", Tab::Epics, &[], |_| true),
        on_ticket("Unassigned", Tab::Unassigned, &[], |_| true),
        on_ticket("Filters", Tab::Filters, &[], |_| true),
        on_ticket("My Work marking", Tab::MyWork, &[' '], |app| {
            app.selected_ticket_count() == 1
        }),
        on_ticket("Team marking", Tab::Team, &[' '], |app| {
            app.selected_ticket_count() == 1
        }),
        on_ticket("Epics marking", Tab::Epics, &[' '], |app| {
            app.selected_ticket_count() == 1
        }),
        on_ticket("Unassigned marking", Tab::Unassigned, &[' '], |app| {
            app.selected_ticket_count() == 1
        }),
        on_ticket("Filters marking", Tab::Filters, &[' '], |app| {
            app.selected_ticket_count() == 1
        }),
        in_detail("ticket detail", &[], App::is_detail_open),
        in_detail("comment", &['C'], App::is_comment_open),
        in_detail("assign", &['a'], App::is_assign_open),
        in_detail("edit", &['e'], App::is_edit_open),
        in_detail("activity", &['h'], |app| {
            matches!(app.detail_mode, DetailMode::History { .. })
        }),
        ("epic detail", Tab::Epics, 0, vec![KeyCode::Enter], |app| {
            app.detail_epic_key.is_some()
        }),
        on_ticket(
            "create ticket",
            Tab::MyWork,
            &['c'],
            App::is_create_ticket_open,
        ),
        on_ticket("bulk actions", Tab::MyWork, &['B'], App::is_bulk_open),
        on_ticket("bulk upload", Tab::MyWork, &['U'], App::is_bulk_upload_open),
        on_ticket(
            "filter editor",
            Tab::Filters,
            &['n'],
            App::is_filter_edit_open,
        ),
        on_ticket("help", Tab::MyWork, &['?'], |app| app.show_keybindings),
        on_ticket("settings", Tab::MyWork, &['S'], |app| {
            app.settings.is_some()
        }),
    ];
    for (scene, tab, index, keys, is_open) in scenes {
        let mut config = two_saved_filters();
        let mut app = app_that_draws_every_glyph(tab, index);
        for key in keys {
            press(&mut app, &mut config, key).await;
        }
        assert!(is_open(&app), "{scene} did not open");
        assert_one_column_cells(&app, &config, scene);
    }

    let config = two_saved_filters();
    let mut app = app_that_draws_every_glyph(Tab::MyWork, 1);
    app.open_detail("DEMO-1".to_string());
    let Some(move_picker::JiraCall::FetchTransitions { key, request }) =
        move_picker::open(&mut app)
    else {
        panic!("m should ask Jira for transitions");
    };
    let transitions = transitions::parse_transitions(transitions::tests::BACKLOG_EPIC).unwrap();
    move_picker::receive(&mut app, &key, request, Ok(transitions));
    assert!(matches!(app.detail_mode, DetailMode::MovePicker(_)));
    assert_one_column_cells(&app, &config, "move picker");

    let mut app = app_that_draws_every_glyph(Tab::MyWork, 1);
    assert!(app.moves.start("DEMO-1", "Done"));
    app.finish_move("DEMO-1", Err("Jira answered 400 Bad Request.".to_string()));
    assert!(!app.moves.failures().is_empty());
    assert_one_column_cells(&app, &config, "move failure");

    let mut app = app_that_draws_every_glyph(Tab::MyWork, 1);
    app.record_move("DEMO-1", "Done");
    assert_one_column_cells(&app, &config, "after a move to done");
}

#[test]
fn the_screen_is_the_top_bar_and_its_rule_the_list_the_bulk_bar_while_marking_and_the_footer() {
    let mut app = app_with_a_ticket_on_every_tab(Tab::MyWork);
    let rows = screen(&app, 120, 14);
    assert!(
        rows[0].starts_with(" lazyjira   My Work   Team"),
        "{}",
        rows[0]
    );
    assert!(rows[0].trim_end().ends_with("Settings"), "{}", rows[0]);
    assert_eq!(rows[1], "─".repeat(120));
    assert_eq!(rows[3].trim_end(), " ● In Progress 1");
    assert!(
        rows[13].starts_with(" Tab switch · j/k move"),
        "{}",
        rows[13]
    );
    assert!(
        !rows[12].contains("selected"),
        "no bulk bar until a ticket is marked"
    );

    // Marking one ticket adds the bulk bar over the footer, which keeps its row.
    app.selected_ticket_keys.insert("DEMO-1".into());
    let rows = screen(&app, 120, 14);
    assert_eq!(
        rows[12].trim_end(),
        " 1 selected   B move or assign · u clear · A all"
    );
    assert!(
        rows[13].starts_with(" Tab switch · j/k move"),
        "{}",
        rows[13]
    );
    assert!(
        !rows[13].contains("u clear"),
        "the keys are named once: {}",
        rows[13]
    );
    assert!(
        !rows[13].contains("1 selected"),
        "so is the count: {}",
        rows[13]
    );
    assert_eq!(rows[3].chars().nth(3), Some('☒'), "{}", rows[3]);
    assert_eq!(rows[4].chars().nth(3), Some('☒'), "{}", rows[4]);

    // Unmarking takes the bar and the checkboxes away.
    app.clear_selected_tickets();
    let rows = screen(&app, 120, 14);
    assert!(!rows[12].contains("selected"));
    assert!(rows
        .iter()
        .all(|row| !row.contains('☒') && !row.contains('☐')));
}

#[test]
fn the_error_and_search_lines_are_red_and_yellow_and_the_theme_recolors_them() {
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    let mut app = App::new();
    app.loading = false;
    let mut config = sample_config();
    let color_of_footer = |app: &App, config: &AppConfig| {
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|f| ui(f, app, config)).unwrap();
        terminal.backend().buffer()[(1, 7)].fg
    };
    app.flash = Some("Refresh failed".into());
    assert_eq!(color_of_footer(&app, &config), Color::Red);
    app.flash = None;
    app.search = Some("abc".into());
    assert_eq!(color_of_footer(&app, &config), Color::Yellow);
    app.search = None;
    app.theme = theme::resolve("nord", &config.themes);
    config.preferences.theme = "nord".into();
    app.flash = Some("Refresh failed".into());
    assert!(matches!(color_of_footer(&app, &config), Color::Rgb(..)));
}

#[test]
fn the_whole_my_work_screen_at_80_columns_is_the_top_bar_the_list_and_the_footer() {
    let mut app = app_with_a_ticket_on_every_tab(Tab::MyWork);
    let rows = screen(&app, 80, 12);
    let expected = [
        " lazyjira   My Work   Team   Epics   Unassigned   Filters            Settings",
        &"─".repeat(80),
        "",
        " ● In Progress 1",
        " › DEMO-1  Mine                                                                -",
    ];
    for (row, expected) in rows.iter().zip(expected) {
        assert_eq!(row.trim_end(), expected);
    }
    assert!(
        rows[5..11].iter().all(|row| row.trim().is_empty()),
        "{rows:?}"
    );
    assert_eq!(
        rows[11].trim_end(),
        " Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more"
    );

    app.selected_ticket_keys.insert("DEMO-1".into());
    let rows = screen(&app, 80, 12);
    assert_eq!(
        rows[10].trim_end(),
        " 1 selected   B move or assign · u clear · A all"
    );
    assert_eq!(
        rows[4].trim_end(),
        " › ☒ DEMO-1  Mine                                                              -"
    );
}
