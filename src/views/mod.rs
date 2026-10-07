pub mod common;
pub mod epics;
pub mod filters;
pub mod my_work;
pub mod team;
pub mod unassigned;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, FilterFocus, Tab, VisibleItem};
    use crate::cache::{Epic, TeamMember, Ticket};
    use ratatui::{backend::TestBackend, style::Color, Terminal};

    #[test]
    fn rendered_selection_matches_navigation_across_tabs_and_collapsed_groups() {
        let config = toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
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
                        terminal
                            .draw(|frame| {
                                let area = frame.area();
                                match tab {
                                    Tab::MyWork => my_work::render(frame, area, &app),
                                    Tab::Team => team::render(frame, area, &app),
                                    Tab::Epics => epics::render(frame, area, &app),
                                    Tab::Unassigned => unassigned::render(frame, area, &app),
                                    Tab::Filters => filters::render(frame, area, &app, &config),
                                }
                            })
                            .unwrap();
                        let buffer = terminal.backend().buffer();
                        assert_eq!(buffer[(0, 0)].symbol(), "╭");
                        let mut highlighted = Vec::new();
                        let mut text = String::new();
                        for y in 0..60 {
                            let mut row = String::new();
                            for x in 0..160 {
                                let cell = &buffer[(x, y)];
                                text.push_str(cell.symbol());
                                if cell.bg == Color::DarkGray {
                                    row.push_str(cell.symbol());
                                }
                            }
                            if !row.trim().is_empty() {
                                assert_eq!(
                                    buffer[(157, y)].bg,
                                    Color::DarkGray,
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
                        assert!(text.contains(if collapse { '▶' } else { '▼' }));
                        assert!(highlighted.iter().any(|row| row.to_uppercase().contains(&expected.to_uppercase())),
                            "{tab:?}, search {search:?}, collapse {collapse}, row {index}: expected {expected}, got {highlighted:?}");
                        if tab == Tab::Team {
                            assert!(text.contains("active: 1  done: 1"));
                            if !collapse {
                                assert!(text.contains("done (1)"));
                            }
                        }
                    }
                }
            }
        }
    }
    /// Draws `tab` and returns each row's text and the foreground color of its first glyph.
    fn draw(app: &App, tab: Tab, width: u16) -> Vec<(String, Color)> {
        let config = toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                match tab {
                    Tab::MyWork => my_work::render(frame, area, app),
                    Tab::Team => team::render(frame, area, app),
                    Tab::Epics => epics::render(frame, area, app),
                    Tab::Unassigned => unassigned::render(frame, area, app),
                    Tab::Filters => filters::render(frame, area, app, &config),
                }
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..30)
            .map(|y| {
                let text: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
                let fg = (0..width)
                    .map(|x| &buffer[(x, y)])
                    .find(|cell| !matches!(cell.symbol(), " " | "│" | "╭" | "╰" | "─"))
                    .map_or(Color::Reset, |cell| cell.fg);
                (text, fg)
            })
            .collect()
    }

    fn line_with<'a>(rows: &'a [(String, Color)], text: &str) -> Option<&'a (String, Color)> {
        rows.iter().find(|(row, _)| row.contains(text))
    }

    /// A ticket in `epic` carrying `labels`.
    fn labelled(key: &str, epic: Option<&str>, labels: &[&str]) -> Ticket {
        let mut ticket = Ticket::for_test(key, "In Progress");
        ticket.epic_name = epic.map(String::from);
        ticket.labels = labels.iter().map(|l| l.to_string()).collect();
        ticket
    }

    #[test]
    fn shared_epic_and_labels_show_once_under_the_column_headers() {
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![
            labelled("DEMO-1", Some("Grading"), &["DSCI", "FY27Q3", "tech-debt"]),
            labelled("DEMO-2", Some("Grading"), &["DSCI", "FY27Q3"]),
            // No epic: doesn't stop the epic being shared.
            labelled("DEMO-3", None, &["FY27Q3", "DSCI"]),
        ];
        let rows = draw(&app, Tab::MyWork, 120);
        let header_y = rows
            .iter()
            .position(|(r, _)| r.contains("SEL KEY"))
            .unwrap();
        let header = &rows[header_y].0;
        assert!(
            !header.contains("EPIC") && header.contains("LABELS"),
            "{header}"
        );
        let (shared, color) = &rows[header_y + 1];
        assert!(
            shared.contains("all rows · epic Grading · DSCI, FY27Q3"),
            "{shared}"
        );
        assert_eq!(*color, Color::DarkGray);
        // Rows keep only the labels that tell them apart.
        let row = &line_with(&rows, "DEMO-1").unwrap().0;
        assert!(row.contains("tech-debt") && !row.contains("DSCI"), "{row}");
        assert!(!line_with(&rows, "DEMO-2").unwrap().0.contains("Grading"));

        // A different epic brings the Epic column back, and with every label shared the Labels
        // column goes.
        app.cache.my_tickets = vec![
            labelled("DEMO-1", Some("Grading"), &["DSCI"]),
            labelled("DEMO-2", Some("Runner"), &["DSCI"]),
        ];
        app.mark_cache_changed();
        let rows = draw(&app, Tab::MyWork, 120);
        let header = &line_with(&rows, "SEL KEY").unwrap().0;
        assert!(
            header.contains("EPIC") && !header.contains("LABELS"),
            "{header}"
        );
        assert!(line_with(&rows, "all rows · DSCI").is_some());
        assert!(line_with(&rows, "DEMO-2").unwrap().0.contains("Runner"));

        // Nothing shared: no line, and both columns.
        app.cache.my_tickets = vec![
            labelled("DEMO-1", Some("Grading"), &["DSCI"]),
            labelled("DEMO-2", Some("Runner"), &[]),
        ];
        app.mark_cache_changed();
        let rows = draw(&app, Tab::MyWork, 120);
        assert!(line_with(&rows, "all rows").is_none());
        let header = &line_with(&rows, "SEL KEY").unwrap().0;
        assert!(
            header.contains("EPIC") && header.contains("LABELS"),
            "{header}"
        );
    }

    #[test]
    fn a_hidden_column_gives_its_width_to_the_summary() {
        let summary = format!("{}END", "s".repeat(67));
        let mut first = labelled("DEMO-1", Some("Grading"), &[]);
        first.summary = summary.clone();
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![first.clone(), labelled("DEMO-2", Some("Grading"), &[])];
        let rows = draw(&app, Tab::MyWork, 110);
        assert!(line_with(&rows, "DEMO-1").unwrap().0.contains(&summary));

        app.cache.my_tickets = vec![first, labelled("DEMO-2", Some("Runner"), &[])];
        app.mark_cache_changed();
        let rows = draw(&app, Tab::MyWork, 110);
        assert!(!line_with(&rows, "DEMO-1").unwrap().0.contains("END"));
    }

    #[test]
    fn shared_values_follow_search_status_focus_and_folding() {
        let epic_column = |app: &App| {
            line_with(&draw(app, Tab::MyWork, 120), "SEL KEY")
                .unwrap()
                .0
                .contains("EPIC")
        };
        let mut app = App::new();
        app.loading = false;
        let mut other = labelled("DEMO-2", Some("Runner"), &["x"]);
        other.status = "To Do".into();
        app.cache.my_tickets = vec![labelled("DEMO-1", Some("Grading"), &["x"]), other];
        assert!(epic_column(&app));

        app.search = Some("DEMO-1".into());
        app.mark_cache_changed();
        assert!(!epic_column(&app));
        app.search = None;

        app.status_focus = Some("In Progress".into());
        app.mark_cache_changed();
        assert!(!epic_column(&app));
        app.status_focus = None;

        app.toggle_group_collapse("To Do");
        assert!(!epic_column(&app));
        app.toggle_group_collapse("To Do");
        assert!(epic_column(&app));

        // A folded parent's sub-tasks still count: their missing label isn't shared.
        let mut subtask = labelled("DEMO-3", None, &[]);
        subtask.parent_key = Some("DEMO-1".into());
        app.cache.my_tickets = vec![labelled("DEMO-1", None, &["x"]), subtask];
        app.collapsed_parents.insert("DEMO-1".into());
        app.mark_cache_changed();
        let rows = draw(&app, Tab::MyWork, 120);
        assert!(line_with(&rows, "all rows").is_none());
        assert!(line_with(&rows, "SEL KEY").unwrap().0.contains("LABELS"));
    }

    #[test]
    fn team_unassigned_and_filters_show_shared_values_once() {
        let assigned = |key: &str, epic: &str, email: &str| {
            let mut ticket = labelled(key, Some(epic), &["DSCI"]);
            ticket.assignee_email = Some(email.into());
            ticket
        };
        let mut app = App::new();
        app.loading = false;
        app.cache.team_members = vec![TeamMember {
            name: "Alex".into(),
            email: "alex@example.com".into(),
        }];
        app.cache.team_tickets = vec![
            assigned("DEMO-1", "Grading", "alex@example.com"),
            assigned("DEMO-2", "Grading", "alex@example.com"),
            assigned("DEMO-3", "Grading", "__unassigned__"),
            assigned("DEMO-4", "Grading", "__unassigned__"),
        ];
        app.filter_results = app.cache.team_tickets.clone();
        for tab in [Tab::Team, Tab::Unassigned, Tab::Filters] {
            app.active_tab = tab;
            app.mark_cache_changed();
            let rows = draw(&app, tab, 160);
            let header_y = rows
                .iter()
                .position(|(r, _)| r.contains("SEL KEY"))
                .unwrap();
            let header = &rows[header_y].0;
            assert!(
                !header.contains("EPIC") && !header.contains("LABELS"),
                "{header}"
            );
            let (shared, color) = &rows[header_y + 1];
            // Unassigned's groups are its epics, so its line leaves the epic out.
            let expected = if tab == Tab::Unassigned {
                "all rows · DSCI"
            } else {
                "all rows · epic Grading · DSCI"
            };
            assert!(shared.contains(expected), "{tab:?}: {shared}");
            assert_eq!(*color, Color::DarkGray);
        }

        // A second epic in Team brings its Epic column back.
        app.active_tab = Tab::Team;
        app.cache.team_tickets[1].epic_name = Some("Runner".into());
        app.mark_cache_changed();
        let rows = draw(&app, Tab::Team, 160);
        assert!(line_with(&rows, "SEL KEY").unwrap().0.contains("EPIC"));
        assert!(line_with(&rows, "all rows · DSCI").is_some());
        assert!(line_with(&rows, "DEMO-2").unwrap().0.contains("Runner"));
    }

    #[test]
    fn team_draws_the_shared_line_once_under_the_first_column_headers() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Team;
        app.cache.team_members = ["alex", "sam"]
            .map(|name| TeamMember {
                name: name.into(),
                email: format!("{name}@example.com"),
            })
            .to_vec();
        app.cache.team_tickets = ["alex", "sam"]
            .iter()
            .enumerate()
            .map(|(n, name)| {
                let mut ticket = labelled(&format!("DEMO-{n}"), Some("Grading"), &["DSCI"]);
                ticket.assignee_email = Some(format!("{name}@example.com"));
                ticket
            })
            .collect();
        let rows = draw(&app, Tab::Team, 160);
        let shared: Vec<usize> = (0..rows.len())
            .filter(|&y| rows[y].0.contains("all rows"))
            .collect();
        let first_header = rows.iter().position(|(r, _)| r.contains("SEL KEY"));
        assert_eq!(shared, [first_header.unwrap() + 1], "{rows:?}");
    }

    #[test]
    fn unassigned_shared_line_leaves_the_epic_to_its_groups() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Unassigned;
        app.cache.team_tickets = ["DEMO-1", "DEMO-2"]
            .map(|key| {
                let mut ticket = labelled(key, Some("Grading"), &["DSCI"]);
                ticket.epic_key = Some("EPIC-1".into());
                ticket.assignee_email = Some("__unassigned__".into());
                ticket
            })
            .to_vec();
        let rows = draw(&app, Tab::Unassigned, 160);
        let (line, _) = line_with(&rows, "all rows").expect("labels are shared");
        assert!(line.contains("all rows · DSCI"), "{line}");
        assert!(!line.contains("epic"), "{line}");
    }

    #[test]
    fn done_groups_start_folded_and_stay_unfolded_across_refreshes() {
        let screen = |app: &App, tab: Tab| -> String {
            let config = toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
            terminal
                .draw(|frame| match tab {
                    Tab::MyWork => my_work::render(frame, frame.area(), app),
                    _ => filters::render(frame, frame.area(), app, &config),
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..30)
                .map(|y| {
                    (0..120)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                        + "\n"
                })
                .collect()
        };
        let tickets = vec![
            Ticket::for_test("DEMO-1", "In Progress"),
            Ticket::for_test("DEMO-2", "Closed"),
            Ticket::for_test("DEMO-3", "Closed"),
            Ticket::for_test("DEMO-4", "Resolved"),
        ];
        let mut cache = crate::cache::Cache::empty();
        cache.my_tickets = tickets.clone();

        let mut app = App::new();
        app.loading = false;
        app.replace_cache(cache.clone(), app.moves.now());
        let text = screen(&app, Tab::MyWork);
        assert!(text.contains("▼ IN PROGRESS (1)"), "{text}");
        assert!(text.contains("DEMO-1"), "{text}");
        // Folded, each done group keeps its header and count but draws no rows.
        assert!(text.contains("▶ CLOSED (2)"), "{text}");
        assert!(text.contains("▶ RESOLVED (1)"), "{text}");
        for key in ["DEMO-2", "DEMO-3", "DEMO-4"] {
            assert!(!text.contains(key), "{key} is folded away: {text}");
        }

        // Unfolded by the user, Closed stays open through a background refresh.
        app.toggle_group_collapse("Closed");
        app.replace_cache(cache.clone(), app.moves.now());
        let text = screen(&app, Tab::MyWork);
        assert!(text.contains("▼ CLOSED (2)"), "{text}");
        assert!(text.contains("DEMO-2") && text.contains("DEMO-3"), "{text}");
        assert!(text.contains("▶ RESOLVED (1)"), "{text}");

        // `d` still hides done tickets, headers included, and shows them again.
        app.toggle_show_done();
        let text = screen(&app, Tab::MyWork);
        assert!(
            !text.contains("CLOSED") && !text.contains("RESOLVED"),
            "{text}"
        );
        app.toggle_show_done();
        assert!(screen(&app, Tab::MyWork).contains("▼ CLOSED (2)"));

        // A filter's results start with their done groups folded too.
        app.active_tab = Tab::Filters;
        app.filter_focus = FilterFocus::Results;
        app.show_filter_results(tickets, app.moves.now());
        let text = screen(&app, Tab::Filters);
        assert!(text.contains("DEMO-1"), "{text}");
        assert!(
            !text.contains("DEMO-2") && !text.contains("DEMO-4"),
            "{text}"
        );
    }

    #[test]
    fn a_done_group_that_first_appears_on_a_refresh_starts_unfolded() {
        let mut cache = crate::cache::Cache::empty();
        cache.my_tickets = vec![Ticket::for_test("DEMO-1", "In Progress")];
        let mut app = App::new();
        app.loading = false;
        app.replace_cache(cache.clone(), app.moves.now());

        // Closed mid-session: the refresh brings a Closed group My Work hasn't shown before.
        cache.my_tickets[0].status = "Closed".into();
        app.replace_cache(cache, app.moves.now());
        let rows = draw(&app, Tab::MyWork, 120);
        assert!(line_with(&rows, "▼ CLOSED (1)").is_some(), "{rows:?}");
        assert!(line_with(&rows, "DEMO-1").is_some(), "{rows:?}");
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
        let column = |rows: &[String], key: &str| rows[row(rows, key)].find(key).unwrap();

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
        assert_eq!(column(&rows, "DEMO-3"), column(&rows, "DEMO-1") + 2);
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
    fn updated_column_shows_ages_and_done_groups_list_the_newest_first() {
        const NOW: i64 = 1_790_763_800; // 2026-09-30T10:23:20Z
        let ticket = |key: &str, status: &str, updated: &str, labels: &[&str]| {
            let mut ticket = labelled(key, None, labels);
            ticket.status = status.into();
            ticket.updated = Some(format!("{updated}.000+0000"));
            ticket.assignee_email = Some("alex@example.com".into());
            ticket
        };
        let mut subtask = ticket("DEMO-6", "Closed", "2026-09-30T10:23:20", &[]);
        subtask.parent_key = Some("DEMO-3".into());
        let tickets = vec![
            ticket("DEMO-1", "In Progress", "2026-09-30T10:00:00", &["a"]), // 23m
            ticket("DEMO-2", "In Progress", "2026-09-27T09:00:00", &[]),    // 3d
            ticket("DEMO-3", "Closed", "2026-09-01T00:00:00", &[]),         // 4w
            ticket("DEMO-4", "Closed", "2026-09-30T05:00:00", &[]),         // 5h
            ticket("DEMO-5", "Closed", "2026-09-20T00:00:00", &[]),         // 1w
            subtask, // newest, but follows its parent
        ];
        let mut app = App::new();
        app.loading = false;
        app.show_done = true;
        app.clock = || NOW;
        app.cache.team_members = vec![TeamMember {
            name: "Alex".into(),
            email: "alex@example.com".into(),
        }];
        app.cache.my_tickets = tickets.clone();
        app.cache.team_tickets = tickets.clone();
        app.filter_focus = FilterFocus::Results;
        app.filter_results = tickets.clone();

        for tab in [Tab::MyWork, Tab::Team, Tab::Filters] {
            app.active_tab = tab;
            app.mark_cache_changed();
            let rows = draw(&app, tab, 160);
            let header = &line_with(&rows, "SEL KEY").unwrap().0;
            let column = |title: &str| header.split('│').position(|cell| cell.contains(title));
            let updated = column("UPDATED").expect(header);
            if let Some(labels) = column("LABELS") {
                assert_eq!(updated + 1, labels, "{tab:?}: {header}");
            }
            let age_of = |key: &str| {
                let row = &line_with(&rows, key).unwrap().0;
                row.split('│').nth(updated).unwrap().trim().to_string()
            };
            for (key, age) in [("DEMO-1", "23m"), ("DEMO-2", "3d"), ("DEMO-4", "5h")] {
                assert_eq!(age_of(key), age, "{tab:?}: {key}");
            }
            // Done rows newest first, the sub-task right under its parent; active rows keep
            // their order.
            let order = vec!["DEMO-1", "DEMO-2", "DEMO-4", "DEMO-5", "DEMO-3", "DEMO-6"];
            let mut drawn = order.clone();
            drawn.sort_by_key(|key| rows.iter().position(|(r, _)| r.contains(key)).unwrap());
            assert_eq!(drawn, order, "{tab:?}");
        }

        // Unassigned has no Labels column; Updated comes last.
        let mut unassigned = ticket("DEMO-7", "To Do", "2026-09-30T09:23:20", &[]);
        unassigned.assignee_email = Some("__unassigned__".into());
        app.cache.team_tickets = vec![unassigned];
        app.active_tab = Tab::Unassigned;
        app.mark_cache_changed();
        let rows = draw(&app, Tab::Unassigned, 160);
        assert!(line_with(&rows, "SEL KEY").unwrap().0.contains("UPDATED"));
        assert!(line_with(&rows, "DEMO-7").unwrap().0.contains(" 1h"));
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

    /// The drawn cell under each mark target: (symbol, color, on the highlighted row, row text).
    /// Also checks that each mark target is one column and each fold target sits on an arrow.
    fn marks_drawn(
        terminal: &Terminal<TestBackend>,
        app: &App,
    ) -> Vec<(String, Color, bool, String)> {
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
                    assert_eq!(rect.width, 1, "a mark is one column: {}", row_text(rect.y));
                    let highlighted = cell.bg == Color::DarkGray;
                    marks.push((
                        cell.symbol().to_string(),
                        cell.fg,
                        highlighted,
                        row_text(rect.y),
                    ));
                }
                Target::Fold(_) => {
                    assert!(["▼", "▶"].contains(&cell.symbol()), "{}", row_text(rect.y))
                }
                _ => {}
            }
        }
        marks
    }

    #[test]
    fn selection_marks_show_unselected_selected_and_partial_in_their_colors() {
        let config = toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
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
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    match tab {
                        Tab::MyWork => my_work::render(frame, area, app),
                        Tab::Team => team::render(frame, area, app),
                        Tab::Epics => epics::render(frame, area, app),
                        Tab::Unassigned => unassigned::render(frame, area, app),
                        Tab::Filters => filters::render(frame, area, app, &config),
                    }
                })
                .unwrap();
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
                Tab::MyWork | Tab::Filters => "IN PROGRESS",
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

            // The mark takes one column, so the columns stay where the headings put them.
            let buffer = terminal.backend().buffer();
            let row_text = |y: u16| {
                (0..160)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            };
            let bars = |y: u16| {
                (0..160)
                    .filter(|&x| buffer[(x, y)].symbol() == "│")
                    .collect::<Vec<_>>()
            };
            let heading = (0..40).find(|&y| row_text(y).contains("SEL KEY")).unwrap();
            for y in (0..40).filter(|&y| row_text(y).contains(unpicked_key)) {
                assert_eq!(bars(y), bars(heading), "{tab:?}: {}", row_text(y));
            }

            // Everything selected: the group is selected too.
            app.toggle_selection_at_cursor();
            draw(&mut terminal, &app, tab);
            let marks = marks_drawn(&terminal, &app);
            let header_mark = marks.iter().find(|mark| mark.3.contains(header)).unwrap();
            assert_eq!((header_mark.0.as_str(), header_mark.1), ("☒", Color::Cyan));

            // Nothing selected: every mark away from the cursor is unselected and muted.
            app.clear_selected_tickets();
            draw(&mut terminal, &app, tab);
            let unhighlighted: Vec<_> = marks_drawn(&terminal, &app)
                .into_iter()
                .filter(|mark| !mark.2)
                .map(|mark| (mark.0, mark.1))
                .collect();
            assert!(unhighlighted.len() > 1, "{tab:?}");
            for mark in unhighlighted {
                assert_eq!(mark, ("☐".to_string(), Color::DarkGray), "{tab:?}");
            }
        }
    }
}
