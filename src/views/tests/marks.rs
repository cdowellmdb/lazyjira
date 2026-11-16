use super::support::*;
use crate::app::{App, FilterFocus, Tab, VisibleItem};
use crate::cache::{Epic, TeamMember, Ticket};
use crate::views::{common, filters};
use ratatui::{backend::TestBackend, style::Color, Terminal};

/// The mark under each mark target: (symbol, color, on the highlighted row, row text).
/// Also checks that each fold target sits on an arrow.
fn marks_drawn(terminal: &Terminal<TestBackend>, app: &App) -> Vec<(String, Color, bool, String)> {
    use crate::mouse::Target;
    let buffer = terminal.backend().buffer();
    let row_text = |y: u16| {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    let mut marks = Vec::new();
    for &(rect, target) in app.mouse_targets.borrow().iter() {
        let cell = &buffer[(rect.x, rect.y)];
        match target {
            Target::Mark(_) => {
                // The target reaches a cell either side of the mark; report the mark's cell.
                let cell = (rect.x..rect.right())
                    .map(|x| &buffer[(x, rect.y)])
                    .find(|cell| cell.symbol().starts_with(common::MARKS))
                    .unwrap_or_else(|| panic!("no mark under {rect:?}: {}", row_text(rect.y)));
                let highlighted = cell.bg == selected_bg();
                marks.push((
                    cell.symbol().to_string(),
                    cell.fg,
                    highlighted,
                    row_text(rect.y),
                ));
            }
            Target::Fold(_) => {
                assert!(
                    ["●", "▼", "▶"].contains(&cell.symbol()),
                    "{}",
                    row_text(rect.y)
                )
            }
            _ => {}
        }
    }
    marks
}

#[test]
fn selection_marks_show_unselected_selected_and_partial_in_their_colors() {
    let mut picked = Ticket::for_test("DEMO-1", "In Progress");
    picked.assignee = Some("Alex".into());
    picked.assignee_email = Some("alex@example.com".into());
    let mut unpicked = picked.clone();
    unpicked.key = "DEMO-2".into();
    let mut picked_unassigned = picked.clone();
    picked_unassigned.key = "DEMO-3".into();
    picked_unassigned.assignee = Some("Unassigned".into());
    picked_unassigned.assignee_email = Some("__unassigned__".into());
    let mut unpicked_unassigned = picked_unassigned.clone();
    unpicked_unassigned.key = "DEMO-4".into();
    let draw = |terminal: &mut Terminal<TestBackend>, app: &App, tab: Tab| {
        terminal.draw(|frame| render_tab(frame, app, tab)).unwrap();
    };
    let cursor_on = |app: &mut App, keys: &[&str]| {
        app.selected_index = (0..app.item_count())
            .find(|&index| {
                app.selected_index = index;
                matches!(app.selected_item(), Some(VisibleItem::Ticket(key)) if keys.contains(&key.as_str()))
            })
            .unwrap();
    };

    for &tab in Tab::all() {
        let mut app = App::new();
        app.active_tab = tab;
        app.loading = false;
        app.filter_focus = FilterFocus::Results;
        app.cache.my_tickets = vec![picked.clone(), unpicked.clone()];
        app.filter_results = app.cache.my_tickets.clone();
        app.cache.team_tickets = vec![
            picked.clone(),
            unpicked.clone(),
            picked_unassigned.clone(),
            unpicked_unassigned.clone(),
        ];
        app.cache.team_members = vec![TeamMember {
            name: "Alex".into(),
            email: "alex@example.com".into(),
        }];
        app.cache.epics = vec![Epic {
            key: "EPIC-9".into(),
            summary: "Epic".into(),
            children: vec![picked.clone(), unpicked.clone()],
        }];
        let (picked_key, unpicked_key) = if tab == Tab::Unassigned {
            ("DEMO-3", "DEMO-4")
        } else {
            ("DEMO-1", "DEMO-2")
        };
        cursor_on(&mut app, &[picked_key]);
        app.toggle_selection_at_cursor();
        // The cursor sits on the unselected row, so its mark must show against the highlight.
        cursor_on(&mut app, &[unpicked_key]);

        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        draw(&mut terminal, &app, tab);
        let marks = marks_drawn(&terminal, &app);
        let mark_of = |needle: &str| {
            marks
                .iter()
                .find(|mark| mark.3.contains(needle))
                .unwrap_or_else(|| panic!("{tab:?}: no mark on a row with {needle}: {marks:?}"))
                .clone()
        };
        let header = match tab {
            Tab::MyWork | Tab::Filters => "In Progress",
            Tab::Team => "Alex",
            Tab::Epics => "EPIC-9",
            Tab::Unassigned => "No Epic",
        };
        let (mark, fg, _, _) = mark_of(picked_key);
        assert_eq!((mark.as_str(), fg), ("☒", Color::Cyan), "{tab:?}");
        let (mark, fg, highlighted, _) = mark_of(unpicked_key);
        assert!(highlighted, "{tab:?}");
        // Muted, but lighter than the highlight behind it.
        assert_eq!((mark.as_str(), fg), ("☐", Color::Gray), "{tab:?}");
        let (mark, fg, _, _) = mark_of(header);
        assert_eq!((mark.as_str(), fg), ("⊟", Color::Cyan), "{tab:?}");

        // The checkbox is two cells, so every key starts two after it.
        let buffer = terminal.backend().buffer();
        let row_text = |y: u16| {
            (0..160)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        for y in (0..40).filter(|&y| row_text(y).contains(unpicked_key)) {
            let row = row_text(y);
            assert_eq!(
                column_of(&row, unpicked_key),
                column_of(&row, "☐") + 2,
                "{tab:?}: {row}"
            );
        }

        // Everything selected: the group is selected too.
        app.toggle_selection_at_cursor();
        draw(&mut terminal, &app, tab);
        let marks = marks_drawn(&terminal, &app);
        let header_mark = marks.iter().find(|mark| mark.3.contains(header)).unwrap();
        assert_eq!((header_mark.0.as_str(), header_mark.1), ("☒", Color::Cyan));

        // Nothing selected: marking is over, so no checkbox is drawn anywhere.
        app.clear_selected_tickets();
        draw(&mut terminal, &app, tab);
        assert!(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .all(|cell| !cell.symbol().starts_with(common::MARKS)),
            "{tab:?}"
        );
    }
}

#[test]
fn checkboxes_appear_on_every_row_and_header_only_while_a_ticket_is_marked() {
    let boxes = |terminal: &Terminal<TestBackend>| {
        let buffer = terminal.backend().buffer();
        buffer
            .content
            .iter()
            .filter(|cell| cell.symbol().starts_with(common::MARKS))
            .count()
    };
    // (tab, key of a ticket, how many rows and headers draw a checkbox): each ticket and
    // group, so a header counts as one.
    for (tab, key, items) in [
        (Tab::MyWork, "DEMO-1", 4),
        (Tab::Team, "DEMO-1", 2),
        (Tab::Epics, "DEMO-1", 3),
        (Tab::Unassigned, "DEMO-3", 3),
        (Tab::Filters, "DEMO-1", 4),
    ] {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = tab;
        app.filter_focus = FilterFocus::Results;
        let mine = |key: &str, status: &str, email: &str| {
            let mut ticket = Ticket::for_test(key, status);
            ticket.assignee_email = Some(email.into());
            ticket
        };
        app.cache.my_tickets = vec![
            mine("DEMO-1", "In Progress", "alex@example.com"),
            mine("DEMO-2", "To Do", "alex@example.com"),
        ];
        app.filter_results = app.cache.my_tickets.clone();
        app.cache.team_tickets = vec![
            mine("DEMO-1", "In Progress", "alex@example.com"),
            mine("DEMO-3", "To Do", "__unassigned__"),
            mine("DEMO-4", "To Do", "__unassigned__"),
        ];
        app.cache.team_members = vec![TeamMember {
            name: "Alex".into(),
            email: "alex@example.com".into(),
        }];
        app.cache.epics = vec![Epic {
            key: "EPIC-9".into(),
            summary: "Epic".into(),
            children: app.cache.my_tickets.clone(),
        }];
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        terminal.draw(|frame| render_tab(frame, &app, tab)).unwrap();
        assert_eq!(boxes(&terminal), 0, "{tab:?}: none while nothing is marked");

        app.selected_ticket_keys.insert(key.into());
        terminal.draw(|frame| render_tab(frame, &app, tab)).unwrap();
        assert_eq!(
            boxes(&terminal),
            items,
            "{tab:?}: one on every row and header"
        );
    }
}

/// An app whose every tab has a row, a group header and a line of its own among the rows.
fn lead_app(show_done: bool) -> App {
    let mut app = App::new();
    app.loading = false;
    app.show_done = show_done;
    app.filter_focus = FilterFocus::Results;
    let ticket = |key: &str, status: &str, email: &str| {
        let mut ticket = Ticket::for_test(key, status);
        ticket.assignee_email = Some(email.into());
        ticket.labels = vec!["shared".into()];
        ticket
    };
    app.cache.my_tickets = vec![
        ticket("DEMO-1", "In Progress", "alex@example.com"),
        ticket("DEMO-2", "Done", "alex@example.com"),
    ];
    app.filter_results = app.cache.my_tickets.clone();
    app.cache.team_tickets = vec![
        ticket("DEMO-1", "In Progress", "alex@example.com"),
        ticket("DEMO-2", "Done", "alex@example.com"),
        ticket("DEMO-3", "To Do", "__unassigned__"),
    ];
    app.cache.team_members = ["Alex", "Sam"]
        .map(|name| TeamMember {
            name: name.into(),
            email: format!("{}@example.com", name.to_lowercase()),
        })
        .into();
    app.cache.epics = vec![
        Epic {
            key: "EPIC-9".into(),
            summary: "Epic".into(),
            children: app.cache.my_tickets.clone(),
        },
        Epic {
            key: "EPIC-8".into(),
            summary: "Empty".into(),
            children: vec![],
        },
    ];
    app
}

#[test]
fn rows_headers_and_the_lines_among_them_start_at_the_same_lead_with_marking_off_and_on() {
    let lead_of = |marking: bool| common::GUTTER + if marking { common::CHECKBOX_WIDTH } else { 0 };
    for marking in [false, true] {
        let lead = lead_of(marking);
        // (tab, show_done, texts a row or header draws, texts a line among the rows starts with)
        for (tab, show_done, texts, notes) in [
            (
                Tab::MyWork,
                false,
                &["In Progress", "DEMO-1", "Done 1  (d to show)"][..],
                &["all rows"][..],
            ),
            (
                Tab::Team,
                true,
                &["Alex", "DEMO-1", "Sam"][..],
                &["done 1", "(no tickets)"][..],
            ),
            (
                Tab::Epics,
                true,
                &["EPIC-9", "EPIC-8", "DEMO-1"][..],
                &["(no related tickets)", "█"][..],
            ),
            (
                Tab::Unassigned,
                true,
                &["No Epic", "DEMO-3"][..],
                &["unassigned: 1"][..],
            ),
            (
                Tab::Filters,
                true,
                &["In Progress", "DEMO-1"][..],
                &["all rows"][..],
            ),
        ] {
            let mut app = lead_app(show_done);
            if marking {
                app.selected_ticket_keys.insert("DEMO-1".into());
            }
            assert_eq!(common::ListLines::new(&app).lead_width(), lead);
            // Filters' results sit right of the sidebar (a quarter of 160 columns) and the divider.
            let left = if tab == Tab::Filters { 41 } else { 0 };
            let rows: Vec<(String, Color)> = draw(&app, tab, 160)
                .into_iter()
                .map(|(row, color)| (row.chars().skip(left).collect(), color))
                .collect();
            for text in texts {
                let (row, _) = line_with(&rows, text).unwrap_or_else(|| panic!("{tab:?}: {text}"));
                assert_eq!(
                    column_of(row, text),
                    lead,
                    "{tab:?}, marking {marking}: {text}"
                );
            }
            for text in notes {
                let (row, _) = rows
                    .iter()
                    .find(|(row, _)| row.trim_start().starts_with(text))
                    .unwrap_or_else(|| panic!("{tab:?}: {text}"));
                let blanks = row.chars().take_while(|c| *c == ' ').count();
                assert_eq!(blanks, lead, "{tab:?}, marking {marking}: {text}");
            }
        }

        let mut app = lead_app(true);
        app.filter_focus = FilterFocus::Sidebar;
        if marking {
            app.selected_ticket_keys.insert("DEMO-1".into());
        }
        let config = toml::from_str(
            "[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n[[filters]]\nname = 'Mine'\njql = 'x'\n",
        )
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(160, 30)).unwrap();
        terminal
            .draw(|f| filters::render(f, f.area(), &app, &config, None))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row: String = (0..40).map(|x| buffer[(x, 2)].symbol()).collect();
        assert_eq!(
            column_of(&row, "Mine"),
            common::GUTTER,
            "the sidebar marks nothing: {row}"
        );
    }
}
