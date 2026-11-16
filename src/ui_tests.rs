//! Whole-screen rendering tests: they draw `ui` into a `TestBackend` and read the cells.

use crate::tests::{
    app_with_a_ticket_on_every_tab, press, sample_config, ticket, two_saved_filters,
};
use crate::*;
use ratatui::layout::Rect;

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

fn screen(app: &App, width: u16, height: u16) -> Vec<String> {
    use ratatui::backend::TestBackend;

    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui(f, app, &sample_config())).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
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

/// Opens the panel called `name` over `app`.
async fn open_panel(app: &mut App, config: &mut AppConfig, name: &str) {
    match name {
        "create" => press(app, config, KeyCode::Char('c')).await,
        "comment" => {
            app.comment_state = Some(app::CommentState {
                ticket_key: "DEMO-9".into(),
                body: widgets::form::editor(""),
            })
        }
        "assign" => {
            app.assign_state = Some(app::AssignState {
                ticket_key: "DEMO-9".into(),
                selected: 0,
                search: String::new(),
            })
        }
        "edit" => {
            app.edit_state = Some(app::EditFieldsState {
                ticket_key: "DEMO-9".into(),
                focused_field: 0,
                summary: widgets::form::editor("Summary"),
                labels: widgets::form::editor(""),
                description: widgets::form::editor(""),
            })
        }
        "filter" => {
            app.filter_edit = Some(app::FilterEditState {
                focused_field: 0,
                name: widgets::form::editor(""),
                jql: widgets::form::editor(""),
                editing_idx: None,
            })
        }
        "bulk" => {
            press(app, config, KeyCode::Char(' ')).await;
            press(app, config, KeyCode::Char('B')).await;
        }
        "upload" => press(app, config, KeyCode::Char('U')).await,
        "move" => {
            app.open_detail("LAST-30".to_string());
            let Some(move_picker::JiraCall::FetchTransitions { key, request }) =
                move_picker::open(app)
            else {
                panic!("m should ask Jira for transitions");
            };
            let transitions =
                transitions::parse_transitions(transitions::tests::BACKLOG_EPIC).unwrap();
            move_picker::receive(app, &key, request, Ok(transitions));
        }
        "failure" => {
            assert!(app.moves.start("DEMO-9", "Done"));
            app.finish_move("DEMO-9", Err("Jira answered 400 Bad Request.".to_string()));
        }
        other => panic!("no panel called {other}"),
    }
    assert!(widgets::dock::open(app).is_some(), "{name} did not open");
}

const PANELS: [&str; 9] = [
    "create", "comment", "assign", "edit", "filter", "bulk", "upload", "move", "failure",
];

#[tokio::test]
async fn a_panel_takes_the_rows_it_needs_up_to_60_percent_of_the_content_at_80x24_120x40_and_160x45(
) {
    // The content is the screen less the top bar's two rows and the status line, and less the
    // bulk bar while a ticket is marked. A panel is its rule and title, then the rows its body
    // asks for.
    let wants = [
        ("create", 2 + 11, 0),
        ("comment", 2 + 5, 0),
        ("assign", 2 + 4, 0),
        ("edit", 2 + 7, 0),
        ("filter", 2 + 5, 0),
        ("bulk", 2 + 6, 1),
        ("upload", 2 + 3, 0),
        ("move", 2 + 5, 0),
        ("failure", 2 + 4, 0),
    ];
    for (width, height) in [(80, 24), (120, 40), (160, 45)] {
        for (name, want, bulk_bar) in wants {
            let mut config = two_saved_filters();
            let mut app = app_with_a_long_list();
            open_panel(&mut app, &mut config, name).await;
            let content = height - 3 - bulk_bar;
            let expected = want.min(content * 60 / 100);
            let rows = screen(&app, width, height);
            let rules: Vec<usize> = rows
                .iter()
                .enumerate()
                .skip(2)
                .filter(|(_, row)| **row == "─".repeat(width as usize))
                .map(|(y, _)| y)
                .collect();
            let bottom = 2 + content as usize;
            assert_eq!(
                rules,
                [bottom - expected as usize],
                "{name} at {width}x{height}: a panel of {expected} rows"
            );
            let panel = widgets::dock::open(&app).unwrap();
            let area = Rect::new(0, 2, width, content);
            assert_eq!(widgets::dock::height(&app, panel, area), expected, "{name}");
        }
    }
}

#[tokio::test]
async fn a_panel_hides_the_list_under_it_but_leaves_the_list_above_with_its_selection() {
    for (width, height) in [(80, 24), (120, 40), (160, 45)] {
        for name in PANELS.into_iter().filter(|name| *name != "move") {
            let mut config = two_saved_filters();
            let mut app = app_with_a_long_list();
            open_panel(&mut app, &mut config, name).await;
            let rows = screen(&app, width, height);
            let rule = rows
                .iter()
                .skip(2)
                .position(|row| *row == "─".repeat(width as usize))
                .unwrap()
                + 2;
            let case = format!("{name} at {width}x{height}");
            assert!(
                rows[rule..].iter().all(|row| !row.contains("LEAK")),
                "{case}: list text under the panel\n{}",
                rows.join("\n")
            );
            assert!(
                rows[2..rule]
                    .iter()
                    .any(|row| row.contains('›') && row.contains("LAST-30")),
                "{case}: the selected row stays above the panel\n{}",
                rows.join("\n")
            );
            let junction = |row: &String| {
                row.contains([
                    '┐', '┘', '┌', '└', '┬', '┴', '├', '┤', '╭', '╮', '╯', '╰', '│',
                ])
            };
            assert!(
                rows.iter().all(|row| !junction(row)),
                "{case}: a border glyph"
            );
        }
    }
}

#[tokio::test]
async fn the_comment_panel_registers_exact_cells_for_its_buttons_close_and_selectable_text() {
    use crate::mouse::Target;
    let mut config = sample_config();
    let mut app = app_with_a_long_list();
    open_panel(&mut app, &mut config, "comment").await;
    screen(&app, 80, 24);
    // 80x24: content rows 2 to 22, a 7-row panel from row 16. The rule is row 16, the title
    // row 17, the label row 18, three editor rows, then the buttons on row 22.
    let targets = app.mouse_targets.borrow().clone();
    let has = |rect: Rect, target: Target| {
        assert!(targets.contains(&(rect, target)), "{rect:?} {target:?}")
    };
    has(Rect::new(1, 22, 8, 1), Target::Key(KeyCode::Enter));
    has(Rect::new(11, 22, 8, 1), Target::Key(KeyCode::Esc));
    has(Rect::new(21, 22, 8, 1), Target::Key(KeyCode::F(4)));
    has(Rect::new(74, 17, 5, 1), Target::Key(KeyCode::Esc));
    has(Rect::new(1, 18, 78, 4), Target::Field(0));
    assert_eq!(
        app.text_selection.borrow().areas,
        [Rect::new(1, 18, 78, 5)],
        "a drag selects the panel's text, not the list's"
    );
}

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

fn column(rows: &[String], x: usize) -> String {
    rows.iter()
        .skip(2)
        .take(rows.len() - 3)
        .map(|row| row.chars().nth(x).unwrap())
        .collect()
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
