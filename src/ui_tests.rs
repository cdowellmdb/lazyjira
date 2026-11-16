//! Whole-screen rendering tests: they draw `ui` into a `TestBackend` and read the cells.

use crate::tests::{
    app_with_a_ticket_on_every_tab, press, sample_config, ticket, two_saved_filters,
};
use crate::*;

#[tokio::test]
async fn forms_render_after_resizing_and_keep_actions_visible_at_normal_sizes() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut config = sample_config();
    for (width, height) in [(160, 55), (80, 24), (80, 20), (40, 18), (8, 4)] {
        let mut app = App::new();
        app.loading = false;
        press(&mut app, &mut config, KeyCode::Char('c')).await;
        for modal in ["Create", "Comment", "Settings"] {
            if modal == "Comment" {
                app.create_ticket = None;
                app.comment_state = Some(app::CommentState {
                    ticket_key: "DEMO-1".into(),
                    body: widgets::form::editor("First line\nSecond line"),
                });
            } else if modal == "Settings" {
                app.comment_state = None;
                app.settings = Some(settings::Settings::new(&config));
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| ui(f, &app, &config)).unwrap();
            if width >= 80 {
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                assert!(
                    text.contains(" Cancel "),
                    "{modal} at {width}x{height}: {text}"
                );
                assert!(
                    text.contains(" Editor "),
                    "{modal} at {width}x{height}: {text}"
                );
            }
        }
    }
}

#[test]
fn a_multi_line_error_reads_as_one_line_in_the_status_bar() {
    use ratatui::backend::TestBackend;

    let mut app = App::new();
    app.flash =
        Some("Refresh failed: Jira answered 400 Bad Request.\nError in the JQL Query".to_string());
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();

    terminal.draw(|f| ui(f, &app, &sample_config())).unwrap();

    let buffer = terminal.backend().buffer();
    let text: String = (0..100).map(|x| buffer[(x, 11)].symbol()).collect();
    assert!(
        text.contains("400 Bad Request.; Error in the JQL Query"),
        "{text}"
    );
}

fn footer_row(app: &App, width: u16) -> (ratatui::buffer::Buffer, String) {
    use ratatui::backend::TestBackend;

    let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
    terminal.draw(|f| ui(f, app, &sample_config())).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let text = (0..width).map(|x| buffer[(x, 11)].symbol()).collect();
    (buffer, text)
}

#[test]
fn main_footer_highlights_shortcuts_and_keeps_status_indicators_muted() {
    use ratatui::style::{Color, Modifier};

    let mut app = App::new();
    app.loading = false;
    app.show_done = false;
    app.ticket_sync_stage = Some(TicketSyncStage::Full);
    app.cache_stale_age_secs = Some(180);
    app.epics_refreshing = true;
    app.status_focus = Some("In Progress".to_string());

    for tab in Tab::all() {
        app.active_tab = *tab;
        let (buffer, text) = footer_row(&app, 300);
        let (keys, indicators) = if *tab == Tab::Filters {
            (vec!["j/k", "Enter", "Tab", "n", "x", "z", "B"], "")
        } else {
            (
                vec!["Tab", "j/k", "Space", "Enter", "B", "/", "r", "q"],
                "done hidden · focus In Progress · syncing · stale 3m · epics syncing",
            )
        };
        let at = |token: &str, offset: usize| {
            let byte = text
                .find(token)
                .unwrap_or_else(|| panic!("missing {token}: {text}"));
            &buffer[((text[..byte].chars().count() + offset) as u16, 11)]
        };
        for key in keys {
            let cell = at(&format!(" {key} "), 1);
            assert_eq!(cell.fg, Color::Cyan, "{key}");
            assert!(cell.modifier.contains(Modifier::BOLD), "{key}");
        }
        assert_eq!(at("move", 0).fg, Color::DarkGray);
        assert_eq!(at("more", 0).fg, Color::DarkGray);
        if !indicators.is_empty() {
            assert!(text.ends_with(&format!("{indicators} ")), "{text}");
            assert_eq!(at(indicators, 0).fg, Color::DarkGray);
        }
        assert!(!text.contains("sel:"), "{text}");
    }
}

#[test]
fn the_footer_at_80_and_120_columns_keeps_whole_hints_and_a_right_aligned_segment() {
    let mut app = App::new();
    app.loading = false;
    let full = " Tab switch · j/k move · Space mark · Enter open · B bulk · / search · r refresh · q quit · ? more";
    for (width, expected) in [
        (
            80,
            " Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more",
        ),
        (120, full),
    ] {
        let (_, text) = footer_row(&app, width);
        assert_eq!(text.trim_end(), expected, "{width}");
    }

    app.cache_stale_age_secs = Some(180);
    let (_, text) = footer_row(&app, 120);
    assert_eq!(text, format!("{full}{}stale 3m ", " ".repeat(13)));
    let (_, text) = footer_row(&app, 80);
    assert_eq!(
        text.trim_end(),
        " Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more",
        "the segment goes before a hint"
    );
}

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
