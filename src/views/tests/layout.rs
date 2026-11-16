use super::support::*;
use crate::app::{App, FilterFocus, Tab, VisibleItem};
use crate::cache::{Epic, TeamMember, Ticket};
use crate::views::{epics, my_work};
use ratatui::{backend::TestBackend, style::Color, Terminal};

#[test]
fn rendered_selection_matches_navigation_across_tabs_and_collapsed_groups() {
    for &tab in Tab::all() {
        for search in [None, Some("needle")] {
            for collapse in [false, true] {
                let mut app = App::new();
                app.active_tab = tab;
                app.loading = false;
                app.show_done = true;
                app.filter_focus = FilterFocus::Results;
                app.search = search.map(String::from);
                let mut active = Ticket::for_test("DEMO-1", "In Progress");
                active.labels = vec!["needle".into()];
                active.assignee = Some("Alex".into());
                active.assignee_email = Some("alex@example.com".into());
                let mut done = active.clone();
                done.key = "DEMO-2".into();
                done.status = "Done".into();
                let other = Ticket::for_test("DEMO-3", "To Do");
                let mut unassigned = active.clone();
                unassigned.key = "DEMO-4".into();
                unassigned.assignee = Some("Unassigned".into());
                unassigned.assignee_email = Some("__unassigned__".into());
                app.cache.my_tickets = vec![active.clone(), done.clone(), other.clone()];
                app.filter_results = app.cache.my_tickets.clone();
                app.cache.team_tickets = vec![active.clone(), done.clone(), unassigned];
                app.cache.team_members = vec![TeamMember {
                    name: "Alex".into(),
                    email: "alex@example.com".into(),
                }];
                // The same key in two groups must highlight only one occurrence.
                app.cache.epics = vec![
                    Epic {
                        key: "DEMO-100".into(),
                        summary: "First epic".into(),
                        children: vec![active.clone(), done],
                    },
                    Epic {
                        key: "DEMO-200".into(),
                        summary: "Second epic".into(),
                        children: vec![active, other],
                    },
                ];
                if collapse {
                    let group = app.selected_header_group_id().unwrap();
                    app.toggle_group_collapse(&group);
                }

                for index in 0..app.item_count() {
                    app.selected_index = index;
                    let selected = app.selected_item().unwrap();
                    let mut terminal = Terminal::new(TestBackend::new(160, 60)).unwrap();
                    terminal.draw(|frame| render_tab(frame, &app, tab)).unwrap();
                    let buffer = terminal.backend().buffer();
                    let mut highlighted = Vec::new();
                    let mut text = String::new();
                    let mut rows = Vec::new();
                    for y in 0..60 {
                        let mut row = String::new();
                        let mut whole = String::new();
                        for x in 0..160 {
                            let cell = &buffer[(x, y)];
                            whole.push_str(cell.symbol());
                            if cell.bg == selected_bg() {
                                row.push_str(cell.symbol());
                            }
                        }
                        text.push_str(&whole);
                        rows.push(whole);
                        if !row.trim().is_empty() {
                            assert_eq!(
                                buffer[(159, y)].bg,
                                selected_bg(),
                                "{tab:?}, row {index}: highlight fills the row"
                            );
                            highlighted.push(row);
                        }
                    }
                    let expected = match selected {
                        VisibleItem::Ticket(key) => {
                            assert_eq!(
                                highlighted.len(),
                                1,
                                "{tab:?}, row {index}: {highlighted:?}"
                            );
                            key
                        }
                        VisibleItem::GroupHeader(id) => match tab {
                            Tab::Team => "Alex".into(),
                            Tab::Unassigned => "No Epic".into(),
                            _ => id,
                        },
                    };
                    // A folded status group says how to open it, and the other tabs' headers
                    // lead with an arrow.
                    match tab {
                        Tab::MyWork | Tab::Filters => {
                            assert_eq!(text.contains("(z to open)"), collapse, "{tab:?}")
                        }
                        _ => assert!(text.contains(if collapse { '▶' } else { '▼' })),
                    }
                    assert!(highlighted.iter().any(|row| row.to_uppercase().contains(&expected.to_uppercase())),
                        "{tab:?}, search {search:?}, collapse {collapse}, row {index}: expected {expected}, got {highlighted:?}");
                    if tab == Tab::Team {
                        assert!(text.contains("active 1 · done 1"));
                        let done_line = rows.iter().any(|row| row.trim_end() == "   done 1");
                        assert_eq!(done_line, !collapse, "{rows:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn sub_tasks_sit_under_their_parent_or_name_it() {
    let rows_of = |app: &App, tab: Tab| -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                match tab {
                    Tab::MyWork => my_work::render(frame, area, app),
                    _ => epics::render(frame, area, app),
                }
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..30)
            .map(|y| {
                (0..120)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    };
    let row = |rows: &[String], key: &str| rows.iter().position(|r| r.contains(key)).unwrap();
    let column = |rows: &[String], key: &str| column_of(&rows[row(rows, key)], key);

    let parent = Ticket::for_test("DEMO-1", "In Progress");
    let plain = Ticket::for_test("DEMO-2", "In Progress");
    let mut subtask = Ticket::for_test("DEMO-3", "Closed");
    subtask.parent_key = Some("DEMO-1".into());

    let mut app = App::new();
    app.loading = false;
    app.show_done = true;
    app.active_tab = Tab::Epics;
    app.cache.epics = vec![Epic {
        key: "EPIC-9".into(),
        summary: "Epic".into(),
        // Closed would sort last, but the sub-task belongs under its parent.
        children: vec![parent.clone(), plain.clone(), subtask.clone()],
    }];
    let rows = rows_of(&app, Tab::Epics);
    assert_eq!(row(&rows, "DEMO-3"), row(&rows, "DEMO-1") + 1);
    assert!(row(&rows, "DEMO-2") > row(&rows, "DEMO-3"));
    // A sub-task is indented two cells and leads with `└ `.
    assert_eq!(column(&rows, "DEMO-3"), column(&rows, "DEMO-1") + 4);
    assert_eq!(column(&rows, "└ DEMO-3"), column(&rows, "DEMO-1") + 2);
    assert!(rows[row(&rows, "DEMO-1")].contains("DEMO-1 ▼"));

    // Folded, the parent says how many sub-tasks are hidden and they have no row.
    app.collapsed_parents.insert("DEMO-1".into());
    app.mark_cache_changed();
    let rows = rows_of(&app, Tab::Epics);
    assert!(rows[row(&rows, "DEMO-1")].contains("DEMO-1 ▶"));
    assert!(rows[row(&rows, "DEMO-1")].contains("(1 sub-task) DEMO-1"));
    assert!(!rows.iter().any(|r| r.contains("DEMO-3")));
    app.collapsed_parents.clear();
    app.mark_cache_changed();

    // In My Work the parent is in another status group, so the sub-task names it instead.
    app.active_tab = Tab::MyWork;
    app.cache.my_tickets = vec![parent, plain, subtask];
    let rows = rows_of(&app, Tab::MyWork);
    assert_eq!(column(&rows, "DEMO-3"), column(&rows, "DEMO-1"));
    assert!(rows[row(&rows, "DEMO-3")].contains("DEMO-1 › DEMO-3"));
}

#[test]
fn a_long_or_wide_status_keeps_summaries_in_line_on_every_tab_with_a_status_column() {
    let ticket = |key: &str, status: &str, email: &str| {
        let mut ticket = Ticket::for_test(key, status);
        ticket.summary = format!("Summary of {key}");
        ticket.assignee_email = Some(email.into());
        ticket
    };
    let tickets = |email: &str| {
        vec![
            ticket("DEMO-1", "Waiting for Approval", email),
            ticket("DEMO-2", "In Progress", email),
            ticket("DEMO-3", "承認待ち", email),
        ]
    };
    let mut app = App::new();
    app.loading = false;
    app.cache.team_members = vec![TeamMember {
        name: "Alex".into(),
        email: "alex@example.com".into(),
    }];
    app.cache.team_tickets = tickets("alex@example.com");
    app.cache.team_tickets.extend(tickets("__unassigned__"));
    app.filter_results = tickets("alex@example.com");
    app.cache.epics = vec![Epic {
        key: "EPIC-9".into(),
        summary: "Epic".into(),
        children: tickets("alex@example.com"),
    }];
    for tab in [Tab::Team, Tab::Unassigned, Tab::Filters, Tab::Epics] {
        app.active_tab = tab;
        app.mark_cache_changed();
        let rows = draw(&app, tab, 160);
        let columns: Vec<usize> = ["DEMO-1", "DEMO-2", "DEMO-3"]
            .iter()
            .map(|key| {
                column_of(
                    &line_with(&rows, key).unwrap().0,
                    &format!("Summary of {key}"),
                )
            })
            .collect();
        assert!(
            columns.iter().all(|&c| c == columns[0]),
            "{tab:?}: {columns:?}"
        );
        let row = &line_with(&rows, "DEMO-1").unwrap().0;
        assert!(row.contains("Waiting for Approval  "), "{tab:?}: {row}");
    }

    // Narrow, Status gives up columns so the row still ends inside the screen. Team's
    // narrowest fit is 65 (its Epic and Labels keep their floors even while hidden), and
    // Filters shares its width with the list of filters.
    for (tab, width) in [(Tab::Team, 65), (Tab::Unassigned, 47), (Tab::Filters, 56)] {
        app.active_tab = tab;
        app.mark_cache_changed();
        let rows = draw(&app, tab, width);
        let row = &line_with(&rows, "DEMO-1").unwrap().0;
        assert!(
            row.contains("Wait") && !row.contains("Approval"),
            "{tab:?}: {row}"
        );
        assert!(row.trim_end().ends_with(" -"), "{tab:?}: {row}");
    }
}

#[test]
fn a_sub_tasks_parent_key_prefix_is_muted_and_readable_when_selected() {
    let parent = Ticket::for_test("DEMO-1", "In Progress");
    let mut subtask = Ticket::for_test("DEMO-3", "Closed");
    subtask.summary = "Write the docs".into();
    subtask.parent_key = Some("DEMO-1".into());
    let mut app = App::new();
    app.loading = false;
    app.show_done = true;
    app.active_tab = Tab::MyWork;
    app.cache.my_tickets = vec![parent, subtask];

    // The prefix's and the summary's (foreground, background), from the drawn buffer.
    let colors = |app: &App| {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| my_work::render(frame, frame.area(), app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..30)
            .map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        let y = rows
            .iter()
            .position(|r| r.contains("Write the docs"))
            .unwrap();
        let x_of = |text: &str| rows[y][..rows[y].find(text).unwrap()].chars().count();
        let cell = |x: usize| {
            let cell = &buffer[(x as u16, y as u16)];
            (cell.fg, cell.bg)
        };
        (cell(x_of("DEMO-1 ›")), cell(x_of("Write the docs")))
    };

    let (prefix, summary) = colors(&app);
    assert_eq!(prefix.0, Color::DarkGray);
    assert_ne!(prefix.0, summary.0);

    app.selected_index = (0..app.item_count())
        .find(|&index| {
            app.selected_index = index;
            app.selected_ticket_key().as_deref() == Some("DEMO-3")
        })
        .unwrap();
    let (prefix, summary) = colors(&app);
    assert_ne!(
        prefix.0, prefix.1,
        "the prefix must not vanish into the highlight"
    );
    assert_ne!(prefix.0, summary.0);
}

/// The mockup's My Work: DEMO-101 with its sub-task DEMO-111 under In Progress, DEMO-201
/// under Ready for Work, DEMO-102 under In Review. The epics differ, so Epic shows.
fn mockup_app() -> App {
    let ticket = |key: &str, summary: &str, status: &str, epic: &str, labels: &[&str]| {
        let mut ticket = Ticket::for_test(key, status);
        ticket.summary = summary.into();
        ticket.epic_name = Some(epic.into());
        ticket.labels = labels.iter().map(|label| label.to_string()).collect();
        ticket
    };
    let mut child = ticket(
        "DEMO-111",
        "Migrate stored cards to the new vault",
        "In Progress",
        "Checkout redesign",
        &["checkout"],
    );
    child.parent_key = Some("DEMO-101".into());
    let mut app = App::new();
    app.loading = false;
    app.cache.my_tickets = vec![
        ticket(
            "DEMO-101",
            "Add saved payment methods to checkout",
            "In Progress",
            "Checkout redesign",
            &["checkout", "frontend"],
        ),
        child,
        ticket(
            "DEMO-201",
            "Profile slow facet queries",
            "Ready for Work",
            "Search performance",
            &["search", "perf"],
        ),
        ticket(
            "DEMO-102",
            "Fix tax rounding for multi-currency carts",
            "In Review",
            "Checkout redesign",
            &["checkout", "billing"],
        ),
    ];
    app.mark_cache_changed();
    app
}

#[test]
fn my_work_at_120_columns_is_the_mockup_with_groups_rows_and_sub_tasks_in_their_cells() {
    let mut app = mockup_app();
    // The cursor on DEMO-101: its group's header is item 0.
    app.selected_index = 1;
    let rows = trimmed(&draw(&app, Tab::MyWork, 120));
    let expected = [
        "",
        " ● In Progress 2",
        " › DEMO-101 ▼    Add saved payment methods to checkout            Checkout redesign            -  checkout, frontend",
        "     └ DEMO-111  Migrate stored cards to the new vault            Checkout redesign            -  checkout",
        "",
        " ● Ready for Work 1",
        "   DEMO-201      Profile slow facet queries                       Search performance           -  search, perf",
        "",
        " ● In Review 1",
        "   DEMO-102      Fix tax rounding for multi-currency carts        Checkout redesign            -  checkout, billing",
        "",
    ];
    assert_eq!(rows[..expected.len()], expected, "{rows:#?}");
    assert!(rows[expected.len()..].iter().all(String::is_empty));
}

#[test]
fn rows_put_the_marker_the_key_and_the_summary_in_exact_columns() {
    let mut app = mockup_app();
    app.selected_index = 1;
    let rows = draw(&app, Tab::MyWork, 120);
    let at = |key: &str, text: &str| column_of(&line_with(&rows, key).unwrap().0, text);
    // Marker in column 1, key in 3, the fold arrow after it, summary after the widest key cell
    // (a sub-task's `  └ DEMO-111`, 12 cells) and a gap of 2.
    assert_eq!(at("DEMO-101", "›"), 1);
    assert_eq!(at("DEMO-101", "DEMO-101"), 3);
    assert_eq!(at("DEMO-101", "▼"), 12);
    assert_eq!(at("DEMO-101", "Add saved"), 17);
    // A sub-task is indented two cells under its parent, with the `└` before its key.
    assert_eq!(at("DEMO-111", "└"), 5);
    assert_eq!(at("DEMO-111", "DEMO-111"), 7);
    assert_eq!(at("DEMO-111", "Migrate"), 17);
    // Another row's gutter is blank: its key and summary are where the selected row's are.
    let other = &line_with(&rows, "DEMO-201").unwrap().0;
    assert_eq!(other.chars().nth(1), Some(' '));
    assert_eq!(at("DEMO-201", "DEMO-201"), 3);
    assert_eq!(at("DEMO-201", "Profile"), 17);
    // The group header's dot is in column 1 and its name in 3.
    assert_eq!(at("In Progress", "●"), 1);
    assert_eq!(at("In Progress", "In Progress"), 3);

    // Marking adds the checkbox and its space before the key, and moves the rest two over.
    app.selected_ticket_keys.insert("DEMO-201".into());
    let rows = draw(&app, Tab::MyWork, 120);
    let at = |key: &str, text: &str| column_of(&line_with(&rows, key).unwrap().0, text);
    assert_eq!(at("DEMO-101", "›"), 1);
    assert_eq!(at("DEMO-101", "☐"), 3);
    assert_eq!(at("DEMO-101", "DEMO-101"), 5);
    assert_eq!(at("DEMO-101", "Add saved"), 19);
    assert_eq!(at("DEMO-111", "└"), 7);
    assert_eq!(at("DEMO-111", "DEMO-111"), 9);
    assert_eq!(at("DEMO-201", "☒"), 3);
    assert_eq!(at("DEMO-201", "DEMO-201"), 5);
    assert_eq!(at("In Progress", "●"), 1);
    assert_eq!(at("In Progress", "☐"), 3);
    assert_eq!(at("In Progress", "In Progress"), 5);
}

#[test]
fn the_selected_row_has_an_accent_marker_a_full_width_tint_and_a_bold_key_and_summary() {
    let mut app = mockup_app();
    app.selected_index = 1;
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
    terminal
        .draw(|frame| render_tab(frame, &app, Tab::MyWork))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let y = 2;
    let bold = |x: u16| {
        buffer[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    };
    assert_eq!(buffer[(1, y)].symbol(), "›");
    assert_eq!(buffer[(1, y)].fg, Color::Cyan);
    assert!(bold(1));
    for x in 0..120 {
        assert_eq!(buffer[(x, y)].bg, selected_bg(), "column {x}");
    }
    // Key and summary are bold in the plain foreground, and the secondary cells are neither.
    for x in [3, 10, 17, 40] {
        assert!(bold(x), "column {x}");
        assert_eq!(buffer[(x, y)].fg, Color::Reset, "column {x}");
    }
    let epic = column_of(&trimmed(&draw(&app, Tab::MyWork, 120))[2], "Checkout") as u16;
    assert!(!bold(epic));
    assert_eq!(
        buffer[(epic, y)].fg,
        Color::Gray,
        "muted, lighter than the tint"
    );
    // Off the selected row, the same cell is the plain muted color with no tint.
    assert_eq!(buffer[(epic, 6)].fg, Color::DarkGray);
    assert_eq!(buffer[(epic, 6)].bg, Color::Reset);
    assert_eq!(buffer[(3, 6)].fg, Color::Reset);
    assert!(
        buffer[(3, 6)].modifier.is_empty(),
        "an unselected key isn't bold"
    );
}

#[test]
fn a_group_header_is_a_status_dot_a_bold_name_and_a_muted_count() {
    let mut app = mockup_app();
    app.selected_index = 1;
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
    terminal
        .draw(|frame| render_tab(frame, &app, Tab::MyWork))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let header = |y: u16| {
        let cell = |x| &buffer[(x, y)];
        (
            cell(1).symbol().to_string(),
            cell(1).fg,
            cell(3).modifier.contains(ratatui::style::Modifier::BOLD),
            cell(15).symbol().to_string(),
            cell(15).fg,
        )
    };
    // "In Progress" is eleven cells from column 3, so its count is in column 15.
    assert_eq!(
        header(1),
        (
            "●".to_string(),
            Color::Yellow,
            true,
            "2".to_string(),
            Color::DarkGray
        )
    );
    assert_eq!(buffer[(3, 5)].symbol(), "R");
    assert_eq!(buffer[(1, 5)].fg, Color::Blue);
    assert_eq!(buffer[(1, 8)].fg, Color::Cyan);
}

/// An app on `tab` with two groups of one ticket each, and the text that starts each group's
/// header line.
fn two_group_app(tab: Tab) -> (App, [&'static str; 2]) {
    let mut app = App::new();
    app.loading = false;
    app.active_tab = tab;
    app.filter_focus = FilterFocus::Results;
    let mine = |key: &str, status: &str, email: &str, epic_key: Option<&str>| {
        let mut ticket = Ticket::for_test(key, status);
        ticket.assignee_email = Some(email.into());
        ticket.epic_key = epic_key.map(String::from);
        ticket.epic_name = epic_key.map(|key| format!("Epic {key}"));
        ticket
    };
    app.cache.team_members = ["alex", "sam"]
        .map(|name| TeamMember {
            name: name.into(),
            email: format!("{name}@example.com"),
        })
        .to_vec();
    let tickets = [
        mine("DEMO-1", "In Progress", "alex@example.com", None),
        mine("DEMO-2", "To Do", "sam@example.com", None),
    ];
    app.cache.my_tickets = tickets.to_vec();
    app.filter_results = tickets.to_vec();
    app.cache.team_tickets = tickets.to_vec();
    let epic = |key: &str, child: &Ticket| Epic {
        key: key.into(),
        summary: "Name".into(),
        children: vec![child.clone()],
    };
    app.cache.epics = vec![epic("EPIC-1", &tickets[0]), epic("EPIC-2", &tickets[1])];
    let unassigned = [
        mine("DEMO-3", "To Do", "__unassigned__", Some("EPIC-1")),
        mine("DEMO-4", "To Do", "__unassigned__", Some("EPIC-2")),
    ];
    app.cache.team_tickets.extend(unassigned);
    app.mark_cache_changed();
    let headers = match tab {
        Tab::MyWork | Tab::Filters => ["● In Progress", "● To Do"],
        Tab::Team => ["▼ alex", "▼ sam"],
        Tab::Epics => ["▼ EPIC-1", "▼ EPIC-2"],
        Tab::Unassigned => ["▼ EPIC-1", "▼ EPIC-2"],
    };
    (app, headers)
}

#[test]
fn one_blank_line_separates_the_groups_on_every_tab() {
    for &tab in Tab::all() {
        let (app, headers) = two_group_app(tab);
        let rows = trimmed(&draw(&app, tab, 120));
        // Filters draws its sidebar beside the list, so look from where the list starts.
        let list: Vec<&str> = rows
            .iter()
            .map(|row| match tab {
                Tab::Filters => row.split_once('│').map_or("", |(_, list)| list),
                _ => row.as_str(),
            })
            .map(str::trim_end)
            .collect();
        let at = |text: &str| {
            list.iter()
                .position(|row| row.trim_start().starts_with(text))
                .unwrap_or_else(|| panic!("{tab:?}: no {text} in {list:#?}"))
        };
        let (first, second) = (at(headers[0]), at(headers[1]));
        assert_eq!(list[second - 1], "", "{tab:?}: {list:#?}");
        assert_ne!(list[second - 2], "", "{tab:?}: {list:#?}");
        assert!(second - first >= 3, "{tab:?}");
        // And one before the first group, and none between a header and its first row.
        assert_ne!(list[second + 1], "", "{tab:?}: {list:#?}");
    }
}

#[test]
fn no_row_draws_a_column_separator_or_a_header_row() {
    for &tab in Tab::all() {
        let (app, _) = two_group_app(tab);
        let rows = draw(&app, tab, 120);
        for (row, _) in &rows {
            assert!(
                !row.contains("SEL KEY") && !row.contains("SUMMARY"),
                "{row}"
            );
            assert!(!row.contains('╭') && !row.contains('╰'), "{row}");
            assert!(!row.contains("─"), "{tab:?}: {row}");
            // Only Filters has a divider, and it is the one `│` in the row.
            let bars = row.matches('│').count();
            assert_eq!(bars, usize::from(tab == Tab::Filters), "{tab:?}: {row}");
        }
    }
}
