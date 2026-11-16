//! The docked panel: its height, what it hides and its click cells.

use super::*;

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

#[tokio::test]
async fn the_move_panel_registers_its_close_cell_each_option_row_and_its_hint_keys() {
    let mut config = two_saved_filters();
    for (width, height) in [(80, 24), (120, 40), (160, 45)] {
        let mut app = app_with_a_long_list();
        open_panel(&mut app, &mut config, "move").await;
        let rows = screen(&app, width, height);
        let case = format!("{width}x{height}\n{}", rows.join("\n"));
        let title = rows
            .iter()
            .position(|row| row.contains("Move LAST-30"))
            .unwrap();
        assert!(
            rows[title].starts_with(" Move LAST-30 from In Progress"),
            "{case}"
        );
        assert_eq!(rows[title - 1], "─".repeat(width as usize), "{case}");
        let targets = app.mouse_targets.borrow().clone();
        assert!(
            targets.contains(&(
                Rect::new(width - 6, title as u16, 5, 1),
                mouse::Target::Key(KeyCode::Esc)
            )),
            "{case}"
        );
        let options: Vec<_> = targets
            .iter()
            .filter_map(|(area, target)| match target {
                mouse::Target::Choose { index, .. } => Some((*area, *index)),
                _ => None,
            })
            .collect();
        assert!(!options.is_empty(), "{case}");
        for (i, (area, index)) in options.iter().enumerate() {
            assert_eq!(*index, i, "{case}");
            assert_eq!(
                (area.x, area.y, area.width, area.height),
                (1, title as u16 + 1 + i as u16, width - 2, 1),
                "{case}"
            );
            assert!(rows[area.y as usize].len() > 3, "{case}");
        }
        // Esc, as the hint line's key or the close cell, cancels the picker.
        click(&mut app, &mut config, width - 4, title as u16).await;
        assert!(
            !matches!(app.detail_mode, DetailMode::MovePicker(_)),
            "{case}"
        );
    }
}
