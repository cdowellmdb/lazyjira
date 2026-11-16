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

    /// The background `style::selected_row` puts behind the selected row.
    fn selected_bg() -> Color {
        crate::style::selected_row().bg.unwrap()
    }

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
    /// Renders `tab` over the whole frame, with a config that has no saved filters.
    fn render_tab(frame: &mut ratatui::Frame, app: &App, tab: Tab) {
        let config = toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
        let area = frame.area();
        match tab {
            Tab::MyWork => my_work::render(frame, area, app),
            Tab::Team => team::render(frame, area, app),
            Tab::Epics => epics::render(frame, area, app),
            Tab::Unassigned => unassigned::render(frame, area, app),
            Tab::Filters => filters::render(frame, area, app, &config),
        }
    }

    /// Draws `tab` and returns each row's text and the foreground color of its first glyph.
    pub(super) fn draw(app: &App, tab: Tab, width: u16) -> Vec<(String, Color)> {
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|frame| render_tab(frame, app, tab)).unwrap();
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

    pub(super) fn line_with<'a, T>(rows: &'a [(String, T)], text: &str) -> Option<&'a (String, T)> {
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
            labelled(
                "DEMO-1",
                Some("PAY: Payments Platform Revamp"),
                &["infra", "Q3", "tech-debt"],
            ),
            labelled(
                "DEMO-2",
                Some("PAY: Payments Platform Revamp"),
                &["infra", "Q3"],
            ),
            // No epic: doesn't stop the epic being shared.
            labelled("DEMO-3", None, &["Q3", "infra"]),
        ];
        let rows = draw(&app, Tab::MyWork, 120);
        // The shared line leads the list, over the first group.
        let (shared, color) = &rows[0];
        assert!(
            shared.contains("all rows · epic PAY: Payments Platform Revamp · infra, Q3"),
            "{shared}"
        );
        assert_eq!(*color, Color::DarkGray);
        assert!(rows[2].0.contains("In Progress"), "{rows:?}");
        // Rows keep only the labels that tell them apart.
        let row = &line_with(&rows, "DEMO-1").unwrap().0;
        assert!(row.contains("tech-debt") && !row.contains("infra"), "{row}");
        assert!(!line_with(&rows, "DEMO-2").unwrap().0.contains("Payments"));

        // A different epic brings the Epic column back, and with every label shared the Labels
        // column goes.
        app.cache.my_tickets = vec![
            labelled("DEMO-1", Some("PAY: Payments Platform Revamp"), &["infra"]),
            labelled("DEMO-2", Some("Runner"), &["infra"]),
        ];
        app.mark_cache_changed();
        let rows = draw(&app, Tab::MyWork, 120);
        assert!(line_with(&rows, "all rows · infra").is_some());
        assert!(line_with(&rows, "DEMO-2").unwrap().0.contains("Runner"));
        // Epic is 24 columns, so the long name ends "...".
        assert!(line_with(&rows, "DEMO-1")
            .unwrap()
            .0
            .contains("PAY: Payments Platfor..."));
        assert!(!line_with(&rows, "DEMO-1").unwrap().0.contains("infra"));

        // Nothing shared: no line, and both columns.
        app.cache.my_tickets = vec![
            labelled("DEMO-1", Some("PAY: Payments Platform Revamp"), &["infra"]),
            labelled("DEMO-2", Some("Runner"), &[]),
        ];
        app.mark_cache_changed();
        let rows = draw(&app, Tab::MyWork, 120);
        assert!(line_with(&rows, "all rows").is_none());
        let row = &line_with(&rows, "DEMO-1").unwrap().0;
        assert!(row.contains("PAY: Payments Platfor...") && row.contains("infra"));
        let row = &line_with(&rows, "DEMO-2").unwrap().0;
        assert!(
            row.contains("Runner") && row.trim_end().ends_with(" -"),
            "{row}"
        );
    }

    #[test]
    fn a_hidden_column_gives_its_width_to_the_summary() {
        let summary = format!("{}END", "s".repeat(67));
        let mut first = labelled("DEMO-1", Some("PAY: Payments Platform Revamp"), &[]);
        first.summary = summary.clone();
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![
            first.clone(),
            labelled("DEMO-2", Some("PAY: Payments Platform Revamp"), &[]),
        ];
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
            line_with(&draw(app, Tab::MyWork, 120), "DEMO-1")
                .unwrap()
                .0
                .contains("PAY: Payments")
        };
        let mut app = App::new();
        app.loading = false;
        let mut other = labelled("DEMO-2", Some("Runner"), &["x"]);
        other.status = "To Do".into();
        app.cache.my_tickets = vec![
            labelled("DEMO-1", Some("PAY: Payments Platform Revamp"), &["x"]),
            other,
        ];
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
        let row = &line_with(&rows, "DEMO-1").unwrap().0;
        assert!(row.trim_end().ends_with(" x"), "the Labels column: {row}");
    }

    #[test]
    fn team_unassigned_and_filters_show_shared_values_once() {
        let assigned = |key: &str, epic: &str, email: &str| {
            let mut ticket = labelled(key, Some(epic), &["infra"]);
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
            assigned(
                "DEMO-1",
                "PAY: Payments Platform Revamp",
                "alex@example.com",
            ),
            assigned(
                "DEMO-2",
                "PAY: Payments Platform Revamp",
                "alex@example.com",
            ),
            assigned("DEMO-3", "PAY: Payments Platform Revamp", "__unassigned__"),
            assigned("DEMO-4", "PAY: Payments Platform Revamp", "__unassigned__"),
        ];
        app.filter_results = app.cache.team_tickets.clone();
        for tab in [Tab::Team, Tab::Unassigned, Tab::Filters] {
            app.active_tab = tab;
            app.mark_cache_changed();
            let rows = draw(&app, tab, 160);
            let key = if tab == Tab::Unassigned {
                "DEMO-3"
            } else {
                "DEMO-1"
            };
            let row = &line_with(&rows, key).unwrap().0;
            assert!(
                !row.contains("Payments") && !row.contains("infra"),
                "{tab:?}: {row}"
            );
            let (shared, color) = line_with(&rows, "all rows").unwrap();
            // Unassigned's groups are its epics, so its line leaves the epic out.
            let expected = if tab == Tab::Unassigned {
                "all rows · infra"
            } else {
                "all rows · epic PAY: Payments Platform Revamp · infra"
            };
            assert!(shared.contains(expected), "{tab:?}: {shared}");
            assert_eq!(*color, Color::DarkGray);
        }

        // A second epic in Team brings its Epic column back.
        app.active_tab = Tab::Team;
        app.cache.team_tickets[1].epic_name = Some("Runner".into());
        app.mark_cache_changed();
        let rows = draw(&app, Tab::Team, 160);
        assert!(line_with(&rows, "all rows · infra").is_some());
        assert!(line_with(&rows, "DEMO-2").unwrap().0.contains("Runner"));

        // Filters and Unassigned have no Epic or Labels column to bring back.
        app.cache.team_tickets[3].epic_name = Some("Runner".into());
        app.cache.team_tickets[3].labels.push("tech-debt".into());
        app.filter_results = app.cache.team_tickets.clone();
        for tab in [Tab::Unassigned, Tab::Filters] {
            app.active_tab = tab;
            app.mark_cache_changed();
            let rows = draw(&app, tab, 160);
            let row = &line_with(&rows, "DEMO-4").unwrap().0;
            assert!(
                !row.contains("Runner") && !row.contains("tech-debt"),
                "{tab:?}: {row}"
            );
        }
    }

    #[test]
    fn filters_title_and_empty_state_follow_the_search() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        let mut folded = labelled("DEMO-2", None, &["needle"]);
        folded.status = "To Do".into();
        app.filter_results = vec![
            labelled("DEMO-1", None, &["needle"]),
            folded,
            labelled("DEMO-3", None, &[]),
        ];
        app.toggle_group_collapse("To Do");
        let draw_with = |app: &mut App, search: Option<&str>| {
            app.search = search.map(String::from);
            app.mark_cache_changed();
            draw(app, Tab::Filters, 120)
        };

        let rows = draw_with(&mut app, None);
        assert!(line_with(&rows, "Results (3)").is_some());

        // The count is what the search keeps, a folded group's ticket included.
        let rows = draw_with(&mut app, Some("needle"));
        assert!(line_with(&rows, "Results (2)").is_some());
        assert!(line_with(&rows, "DEMO-1").is_some());
        assert!(line_with(&rows, "DEMO-2").is_none());
        assert!(line_with(&rows, "DEMO-3").is_none());

        let rows = draw_with(&mut app, Some("zzz"));
        assert!(line_with(&rows, "Results (0)").is_some());
        assert!(line_with(&rows, "No results match \"zzz\"").is_some());
        assert!(line_with(&rows, "DEMO-").is_none());

        // With no results at all, the search has nothing to explain.
        app.filter_results.clear();
        let rows = draw_with(&mut app, Some("zzz"));
        assert!(line_with(&rows, "Results").is_some());
        assert!(line_with(&rows, "Results (").is_none());
        assert!(line_with(&rows, "Select a filter and press Enter to run it").is_some());
        assert!(line_with(&rows, "No results match").is_none());
    }

    #[test]
    fn team_draws_the_shared_line_once_at_the_top() {
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
                let mut ticket = labelled(
                    &format!("DEMO-{n}"),
                    Some("PAY: Payments Platform Revamp"),
                    &["infra"],
                );
                ticket.assignee_email = Some(format!("{name}@example.com"));
                ticket
            })
            .collect();
        let rows = draw(&app, Tab::Team, 160);
        let shared: Vec<usize> = (0..rows.len())
            .filter(|&y| rows[y].0.contains("all rows"))
            .collect();
        assert_eq!(shared, [0], "{rows:?}");
    }

    #[test]
    fn unassigned_shared_line_leaves_the_epic_to_its_groups() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Unassigned;
        app.cache.team_tickets = ["DEMO-1", "DEMO-2"]
            .map(|key| {
                let mut ticket = labelled(key, Some("PAY: Payments Platform Revamp"), &["infra"]);
                ticket.epic_key = Some("EPIC-1".into());
                ticket.assignee_email = Some("__unassigned__".into());
                ticket
            })
            .to_vec();
        let rows = draw(&app, Tab::Unassigned, 160);
        let (line, _) = line_with(&rows, "all rows").expect("labels are shared");
        assert!(line.contains("all rows · infra"), "{line}");
        assert!(!line.contains("epic"), "{line}");
    }

    #[test]
    fn done_groups_start_folded_and_stay_unfolded_across_refreshes() {
        let screen = |app: &App, tab: Tab| -> String {
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
            terminal.draw(|frame| render_tab(frame, app, tab)).unwrap();
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
        app.replace_cache_full_scope(cache.clone(), app.moves.now());
        let text = screen(&app, Tab::MyWork);
        let has_line = |text: &str, line: &str| text.lines().any(|row| row.trim_end() == line);
        assert!(has_line(&text, " ● In Progress 1"), "{text}");
        assert!(text.contains("DEMO-1"), "{text}");
        // Folded, each done group keeps its header and count, says how to open, and draws no rows.
        assert!(text.contains("● Closed 2  (z to open)"), "{text}");
        assert!(text.contains("● Resolved 1  (z to open)"), "{text}");
        for key in ["DEMO-2", "DEMO-3", "DEMO-4"] {
            assert!(!text.contains(key), "{key} is folded away: {text}");
        }

        // Unfolded by the user, Closed stays open through a background refresh.
        app.toggle_group_collapse("Closed");
        app.replace_cache(cache.clone(), app.moves.now());
        let text = screen(&app, Tab::MyWork);
        assert!(has_line(&text, " ● Closed 2"), "{text}");
        assert!(text.contains("DEMO-2") && text.contains("DEMO-3"), "{text}");
        assert!(text.contains("● Resolved 1  (z to open)"), "{text}");

        // `d` still hides done tickets, headers included, and says how many and how to show them.
        app.toggle_show_done();
        let text = screen(&app, Tab::MyWork);
        assert!(
            !text.contains("Closed") && !text.contains("Resolved"),
            "{text}"
        );
        assert!(text.contains("● Done 3  (d to show)"), "{text}");
        app.toggle_show_done();
        let text = screen(&app, Tab::MyWork);
        assert!(has_line(&text, " ● Closed 2"), "{text}");
        assert!(!text.contains("(d to show)"), "{text}");

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
    fn done_groups_fold_until_the_first_full_read_and_never_after() {
        let mut cache = crate::cache::Cache::empty();
        cache.my_tickets = vec![Ticket::for_test("DEMO-1", "In Progress")];
        let mut app = App::new();
        app.loading = false;
        // No snapshot: the first read is active-only, so it has no done groups to fold.
        app.replace_cache(cache.clone(), app.moves.now());
        // The full read then fails: nothing is applied, so folding goes on.

        // A manual refresh is the first full read to land: its done groups start folded.
        cache.my_tickets.push(Ticket::for_test("DEMO-2", "Closed"));
        app.replace_cache_full_scope(cache.clone(), app.moves.now());
        let rows = draw(&app, Tab::MyWork, 120);
        assert!(
            line_with(&rows, "● Closed 1  (z to open)").is_some(),
            "{rows:?}"
        );
        assert!(line_with(&rows, "DEMO-2").is_none(), "{rows:?}");

        // Resolved mid-session: a done group that first appears after the full read shows open.
        cache.my_tickets[0].status = "Resolved".into();
        app.replace_cache_full_scope(cache, app.moves.now());
        let rows = draw(&app, Tab::MyWork, 120);
        assert!(line_with(&rows, "● Resolved 1").is_some(), "{rows:?}");
        assert!(
            line_with(&rows, "Resolved 1  (z to open)").is_none(),
            "{rows:?}"
        );
        assert!(line_with(&rows, "DEMO-1").is_some(), "{rows:?}");
    }

    #[test]
    fn a_done_group_opened_during_startup_is_not_folded_again() {
        let mut cache = crate::cache::Cache::empty();
        cache.my_tickets = vec![Ticket::for_test("DEMO-1", "Closed")];
        let mut app = App::new();
        app.loading = false;
        // The snapshot folds Closed; the user opens it before the full read lands.
        app.replace_cache(cache.clone(), app.moves.now());
        assert!(line_with(&draw(&app, Tab::MyWork, 120), "● Closed 1  (z to open)").is_some());
        app.toggle_group_collapse("Closed");
        app.replace_cache_full_scope(cache, app.moves.now());
        let rows = draw(&app, Tab::MyWork, 120);
        assert!(line_with(&rows, "● Closed 1").is_some(), "{rows:?}");
        assert!(line_with(&rows, "(z to open)").is_none(), "{rows:?}");
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
    fn epic_headers_show_percent_done_without_the_dropped_children() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        let header = |app: &mut App, children: &[(&str, usize)]| {
            app.cache.epics = vec![Epic::for_test("EPIC-9", children)];
            let rows = draw(app, Tab::Epics, 160);
            let header = rows
                .iter()
                .position(|(row, _)| row.contains("EPIC-9"))
                .unwrap()
                + 1;
            rows[header].0.clone()
        };
        // 108 of the 111 not dropped is 97.3%, shown rounded down.
        let line = header(
            &mut app,
            &[("Won't Do", 10), ("In Progress", 3), ("Closed", 108)],
        );
        assert!(
            line.contains("97% done · 121 tickets: 108 Closed · 3 In Progress · 10 Won't Do"),
            "{line}"
        );
        let line = header(&mut app, &[("Closed", 199), ("In Progress", 1)]);
        assert!(line.contains("  99% done · 200 tickets"), "{line}");
        let line = header(&mut app, &[("Closed", 200)]);
        assert!(line.contains("  100% done · 200 tickets"), "{line}");
        // With no child in scope, there's nothing to measure.
        let line = header(&mut app, &[("Won't Do", 2)]);
        assert!(line.contains("  2 tickets: 2 Won't Do"), "{line}");
        assert!(!line.contains("% done"), "{line}");
        let line = header(&mut app, &[]);
        assert!(line.contains("  0 tickets"), "{line}");
        assert!(!line.contains("% done"), "{line}");
    }

    #[test]
    fn d_hides_done_children_in_epics_but_progress_still_counts_them() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        app.cache.epics = vec![Epic {
            key: "EPIC-9".into(),
            summary: "Epic".into(),
            children: vec![
                Ticket::for_test("DEMO-1", "In Progress"),
                Ticket::for_test("DEMO-2", "Closed"),
            ],
        }];
        assert!(line_with(&draw(&app, Tab::Epics, 120), "DEMO-2").is_some());
        app.toggle_show_done();
        let rows = draw(&app, Tab::Epics, 120);
        assert!(line_with(&rows, "DEMO-1").is_some(), "{rows:?}");
        assert!(line_with(&rows, "DEMO-2").is_none(), "{rows:?}");
        assert!(
            line_with(&rows, "50% done · 2 tickets: 1 Closed · 1 In Progress").is_some(),
            "{rows:?}"
        );
    }

    #[test]
    fn f_focuses_one_status_in_epics_and_drops_epics_with_no_child_in_it() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        let epic = |key: &str, children: Vec<Ticket>| Epic {
            key: key.into(),
            summary: "Epic".into(),
            children,
        };
        app.cache.epics = vec![
            epic(
                "EPIC-1",
                vec![
                    Ticket::for_test("DEMO-1", "In Progress"),
                    Ticket::for_test("DEMO-2", "On Deck"),
                    Ticket::for_test("DEMO-3", "Closed"),
                ],
            ),
            epic("EPIC-2", vec![Ticket::for_test("DEMO-4", "On Deck")]),
            epic("EPIC-3", vec![Ticket::for_test("DEMO-5", "Closed")]),
        ];
        // Done is left to `d`.
        assert_eq!(app.focusable_statuses(), ["In Progress", "On Deck"]);

        app.cycle_status_focus(true);
        let rows = draw(&app, Tab::Epics, 120);
        assert!(line_with(&rows, "DEMO-1").is_some(), "{rows:?}");
        assert!(line_with(&rows, "DEMO-2").is_none(), "{rows:?}");
        // A kept epic shows its done children, and its progress counts every child.
        assert!(line_with(&rows, "DEMO-3").is_some(), "{rows:?}");
        assert!(
            line_with(
                &rows,
                "33% done · 3 tickets: 1 Closed · 1 In Progress · 1 On Deck"
            )
            .is_some(),
            "{rows:?}"
        );
        // An epic with only done children has none in the focus.
        assert!(line_with(&rows, "EPIC-2").is_none(), "{rows:?}");
        assert!(line_with(&rows, "EPIC-3").is_none(), "{rows:?}");

        app.cycle_status_focus(true);
        let rows = draw(&app, Tab::Epics, 120);
        assert!(line_with(&rows, "DEMO-1").is_none(), "{rows:?}");
        assert!(line_with(&rows, "DEMO-2").is_some(), "{rows:?}");
        assert!(line_with(&rows, "DEMO-4").is_some(), "{rows:?}");

        // An epic matching the search by key still needs a child in the focus.
        app.search = Some("epic-2".into());
        assert_eq!(app.focusable_statuses(), ["On Deck"]);
        app.cycle_status_focus(false);
        app.cycle_status_focus(true);
        assert_eq!(app.status_focus.as_deref(), Some("On Deck"));
        assert!(line_with(&draw(&app, Tab::Epics, 120), "DEMO-4").is_some());
        app.search = Some("epic-1".into());
        let rows = draw(&app, Tab::Epics, 120);
        assert!(line_with(&rows, "DEMO-2").is_some(), "{rows:?}");
        assert!(line_with(&rows, "DEMO-1").is_none(), "{rows:?}");
        assert!(line_with(&rows, "EPIC-2").is_none(), "{rows:?}");
    }

    /// The screen column `text` starts at in `row`, a line from `draw`: one symbol per cell, so a
    /// wide character is followed by the blank cell it covers.
    pub(super) fn column_of(row: &str, text: &str) -> usize {
        let at = row
            .find(text)
            .unwrap_or_else(|| panic!("{text} not in {row}"));
        row[..at].chars().count()
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

    /// Jira's `updated` for `secs` (Unix seconds), in UTC: Howard Hinnant's civil_from_days.
    fn jira_time(secs: i64) -> String {
        let (days, time) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        let (h, min, sec) = (time / 3600, time / 60 % 60, time % 60);
        format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{sec:02}.000+0000")
    }

    #[test]
    fn jira_time_round_trips_through_updated_secs() {
        for secs in [0, 1_709_251_199, 1_790_763_800] {
            let mut ticket = Ticket::for_test("DEMO-1", "Closed");
            ticket.updated = Some(jira_time(secs));
            assert_eq!(ticket.updated_secs(), Some(secs), "{:?}", ticket.updated);
        }
    }

    #[test]
    fn updated_column_shows_ages_and_done_groups_list_the_newest_first() {
        const MIN: i64 = 60;
        const HOUR: i64 = 60 * MIN;
        const DAY: i64 = 24 * HOUR;
        // Ages sit mid-unit, so the clock moving on while the test runs can't change them.
        let now = crate::local_cache::now_unix_secs() as i64;
        let ticket = |key: &str, status: &str, ago: i64, labels: &[&str]| {
            let mut ticket = labelled(key, None, labels);
            ticket.status = status.into();
            ticket.updated = Some(jira_time(now - ago));
            ticket.assignee_email = Some("alex@example.com".into());
            ticket
        };
        let mut subtask = ticket("DEMO-6", "Closed", 10 * MIN + 30, &[]);
        subtask.parent_key = Some("DEMO-3".into());
        let tickets = vec![
            ticket("DEMO-1", "In Progress", 23 * MIN + 30, &["a"]), // 23m
            ticket("DEMO-2", "In Progress", 3 * DAY + 12 * HOUR, &[]), // 3d
            ticket("DEMO-3", "Closed", 31 * DAY, &[]),              // 4w
            ticket("DEMO-4", "Closed", 5 * HOUR + 30 * MIN, &[]),   // 5h
            ticket("DEMO-5", "Closed", 10 * DAY + 12 * HOUR, &[]),  // 1w
            subtask, // newest, but follows its parent
        ];
        let mut app = App::new();
        app.loading = false;
        app.show_done = true;
        app.cache.team_members = vec![TeamMember {
            name: "Alex".into(),
            email: "alex@example.com".into(),
        }];
        app.cache.my_tickets = tickets.clone();
        app.cache.team_tickets = tickets.clone();
        app.filter_focus = FilterFocus::Results;
        app.filter_results = tickets.clone();

        for tab in [Tab::MyWork, Tab::Team, Tab::Filters, Tab::Unassigned] {
            if tab == Tab::Unassigned {
                for ticket in &mut app.cache.team_tickets {
                    ticket.assignee_email = Some("__unassigned__".into());
                }
            }
            app.active_tab = tab;
            app.mark_cache_changed();
            let rows = draw(&app, tab, 160);
            // Every age ends in the same column, and Labels, where a tab has it, follows.
            let ends: Vec<(usize, String)> =
                [("DEMO-1", "23m"), ("DEMO-2", "3d"), ("DEMO-4", "5h")]
                    .iter()
                    .map(|(key, age)| {
                        let row = &line_with(&rows, key).unwrap().0;
                        let at = row
                            .rfind(&format!(" {age}"))
                            .unwrap_or_else(|| panic!("{tab:?}: no {age} on {row}"));
                        (row[..at].chars().count() + 1 + age.len(), row.clone())
                    })
                    .collect();
            assert!(
                ends.iter().all(|(end, _)| *end == ends[0].0),
                "{tab:?}: {ends:?}"
            );
            let first = &ends[0].1;
            if matches!(tab, Tab::MyWork | Tab::Team) {
                assert!(first.trim_end().ends_with("23m  a"), "{tab:?}: {first}");
            } else {
                assert!(first.trim_end().ends_with("23m"), "{tab:?}: {first}");
            }
            if tab == Tab::Unassigned {
                continue; // grouped by epic, not status
            }
            // Done rows newest first, the sub-task right under its parent; active rows keep
            // their order.
            let order = vec!["DEMO-1", "DEMO-2", "DEMO-4", "DEMO-5", "DEMO-3", "DEMO-6"];
            let mut drawn = order.clone();
            drawn.sort_by_key(|key| rows.iter().position(|(r, _)| r.contains(key)).unwrap());
            assert_eq!(drawn, order, "{tab:?}");
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

    /// The mark under each mark target: (symbol, color, on the highlighted row, row text).
    /// Also checks that each fold target sits on an arrow.
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

    /// Spooky season, for `App::delight.today`.
    const HALLOWEEN_WEEK: crate::delight::Today = crate::delight::Today {
        weekday: 5,
        month: 10,
        day: 23,
    };

    #[test]
    fn stale_active_tickets_show_a_plain_age_and_fossils_fade() {
        const DAY: i64 = 24 * 60 * 60;
        // Ages sit mid-unit, so the clock moving on while the test runs can't change them.
        let now = crate::local_cache::now_unix_secs() as i64;
        let ticket = |key: &str, status: &str, ago: i64| {
            let mut ticket = labelled(key, None, &[]);
            ticket.summary = format!("Summary of {key}");
            ticket.status = status.into();
            ticket.updated = Some(jira_time(now - ago));
            ticket
        };
        // Just under three weeks: fresh.
        const FRESH_AGO: i64 = 21 * DAY - DAY / 2;
        let mut app = App::new();
        app.loading = false;
        app.delight.enabled = true;
        app.cache.my_tickets = vec![
            ticket("DEMO-1", "In Progress", FRESH_AGO),
            ticket("DEMO-2", "In Progress", 21 * DAY + DAY / 2),
            ticket("DEMO-3", "To Do", 91 * DAY + DAY / 2),
            ticket("DEMO-4", "Closed", 91 * DAY + DAY / 2),
        ];
        // A label on one row keeps the Labels column, which follows Updated.
        app.cache.my_tickets[0].labels = vec!["a".into()];
        app.mark_cache_changed();

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| render_tab(frame, &app, Tab::MyWork))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..30)
            .map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        let row = |text: &str| rows.iter().position(|row| row.contains(text)).unwrap();
        let mut ends = Vec::new();
        for (key, age) in [
            ("DEMO-1", "2w"),
            ("DEMO-2", "3w"),
            ("DEMO-3", "13w"),
            ("DEMO-4", "13w"),
        ] {
            let drawn = &rows[row(key)];
            let at = drawn
                .rfind(&format!(" {age}"))
                .unwrap_or_else(|| panic!("{key}: no {age} on {drawn}"));
            ends.push(drawn[..at].chars().count() + 1 + age.len());
        }
        assert!(ends.iter().all(|end| *end == ends[0]), "{ends:?}");
        let fg = |key: &str, text: &str| {
            let y = row(key);
            buffer[(column_of(&rows[y], text) as u16, y as u16)].fg
        };
        let row_fg = |key: &str| (fg(key, key), fg(key, &format!("Summary of {key}")));
        assert_eq!(row_fg("DEMO-2"), (Color::Reset, Color::Reset));
        assert_eq!(
            row_fg("DEMO-3"),
            (Color::DarkGray, Color::DarkGray),
            "a fossil fades"
        );
        assert_eq!(
            row_fg("DEMO-4"),
            (Color::Reset, Color::Reset),
            "done is never stale"
        );

        app.delight.enabled = false;
        let rows = draw(&app, Tab::MyWork, 120);
        assert!(line_with(&rows, "DEMO-3").unwrap().0.contains(" 13w"));
    }

    #[test]
    fn blocked_is_cursed_in_spooky_season() {
        let mut app = App::new();
        app.loading = false;
        app.delight.enabled = true;
        app.delight.today = Some(HALLOWEEN_WEEK);
        let mut ticket = Ticket::for_test("DEMO-1", "Blocked");
        ticket.assignee_email = Some("alex@example.com".into());
        app.cache.team_members = vec![TeamMember {
            name: "Alex".into(),
            email: "alex@example.com".into(),
        }];
        app.cache.my_tickets = vec![ticket.clone()];
        app.cache.team_tickets = vec![ticket];
        app.mark_cache_changed();
        assert!(line_with(&draw(&app, Tab::MyWork, 120), "Cursed 1").is_some());
        app.active_tab = Tab::Team;
        app.mark_cache_changed();
        let team = draw(&app, Tab::Team, 120);
        assert!(line_with(&team, "DEMO-1").unwrap().0.contains("● Cursed "));

        app.delight.today = None;
        app.active_tab = Tab::MyWork;
        app.mark_cache_changed();
        assert!(line_with(&draw(&app, Tab::MyWork, 120), "Blocked 1").is_some());
    }

    #[test]
    fn an_empty_my_work_celebrates_unless_a_search_focus_or_delight_off_explains_it() {
        let mut app = App::new();
        app.loading = false;
        app.delight.enabled = true;
        app.delight.today = Some(HALLOWEEN_WEEK);
        app.mark_cache_changed();
        let has = |app: &App, text: &str| line_with(&draw(app, Tab::MyWork, 120), text).is_some();
        assert!(has(&app, "Spooky quiet"), "nothing assigned");

        app.show_done = false;
        app.cache.my_tickets = vec![Ticket::for_test("DEMO-1", "Closed")];
        app.mark_cache_changed();
        assert!(has(&app, "Spooky quiet"), "only done tickets, hidden");

        app.cache.my_tickets = vec![Ticket::for_test("DEMO-1", "In Progress")];
        app.search = Some("nothing matches this".into());
        app.mark_cache_changed();
        assert!(has(&app, "No tickets found"), "a search hides the rows");

        app.cache.my_tickets.clear();
        app.mark_cache_changed();
        assert!(has(&app, "No tickets found"), "a search is still on");
        app.search = None;
        app.delight.enabled = false;
        app.mark_cache_changed();
        assert!(has(&app, "No tickets found"), "delight off");
    }

    /// `count` words ("word01" onward), a summary far wider than any Summary column.
    fn words(count: usize) -> Vec<String> {
        (1..=count).map(|n| format!("word{n:02}")).collect()
    }

    /// An app on `tab`, wrapping summaries, whose every tab lists `tickets` (unassigned on
    /// Unassigned, Alex's elsewhere) under one epic whose summary is too long for its header.
    fn wrapping_app(tab: Tab, tickets: Vec<Ticket>) -> App {
        let email = if tab == Tab::Unassigned {
            "__unassigned__"
        } else {
            "alex@example.com"
        };
        let tickets: Vec<Ticket> = tickets
            .into_iter()
            .map(|mut ticket| {
                ticket.assignee_email = Some(email.into());
                ticket
            })
            .collect();
        let mut app = App::new();
        app.loading = false;
        app.active_tab = tab;
        app.wrap_summaries = true;
        app.filter_focus = FilterFocus::Results;
        app.cache.team_members = vec![TeamMember {
            name: "Alex".into(),
            email: "alex@example.com".into(),
        }];
        app.cache.my_tickets = tickets.clone();
        app.cache.team_tickets = tickets.clone();
        app.filter_results = tickets.clone();
        app.cache.epics = vec![Epic {
            key: "EPIC-9".into(),
            summary: format!("Epic {}", words(40).join(" ")),
            children: tickets,
        }];
        app
    }

    fn select_ticket(app: &mut App, key: &str) {
        for index in 0..app.item_count() {
            app.selected_index = index;
            if matches!(app.selected_item(), Some(VisibleItem::Ticket(k)) if k == key) {
                return;
            }
        }
        panic!("{key} isn't listed");
    }

    /// Draws `tab` `width` by `height`: each row's text and whether it is highlighted, after
    /// checking a highlighted row's highlight reaches the screen's right edge.
    fn draw_highlights(app: &App, tab: Tab, width: u16, height: u16) -> Vec<(String, bool)> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render_tab(frame, app, tab)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                let text: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
                let lit = (0..width).any(|x| buffer[(x, y)].bg == selected_bg());
                if lit {
                    assert_eq!(buffer[(width - 1, y)].bg, selected_bg(), "{tab:?}: {text}");
                }
                (text, lit)
            })
            .collect()
    }

    #[test]
    fn wrapped_summaries_show_every_word_and_highlight_every_line_on_every_tab() {
        let summary = words(30);
        for &tab in Tab::all() {
            let mut long = Ticket::for_test("DEMO-1", "In Progress");
            long.summary = summary.join(" ");
            let mut short = Ticket::for_test("DEMO-2", "In Progress");
            short.summary = "Short one".into();
            let mut app = wrapping_app(tab, vec![long, short]);
            select_ticket(&mut app, "DEMO-1");
            let rows = draw_highlights(&app, tab, 120, 40);
            let lit: Vec<usize> = (0..rows.len()).filter(|&y| rows[y].1).collect();
            let first = lit[0];
            assert!(lit.len() > 1, "{tab:?}: {rows:#?}");
            assert_eq!(
                lit,
                (first..first + lit.len()).collect::<Vec<_>>(),
                "{tab:?}"
            );
            assert!(rows[first].0.contains("DEMO-1"), "{tab:?}: {rows:#?}");
            // Every word, in order, on the ticket's lines and nowhere else; none cut.
            let text: String = lit.iter().map(|&y| rows[y].0.as_str()).collect();
            let mut from = 0;
            for word in &summary {
                let at = text[from..]
                    .find(word.as_str())
                    .unwrap_or_else(|| panic!("{tab:?}: {word} missing from {text}"));
                from += at + word.len();
            }
            assert!(!text.contains("..."), "{tab:?}: {text}");
            assert!(line_with(&rows, "Short one").is_some_and(|(_, lit)| !*lit));
            if tab == Tab::Epics {
                let header = rows
                    .iter()
                    .position(|(row, _)| row.contains("EPIC-9"))
                    .unwrap();
                assert!(rows[header].0.contains("..."), "{}", rows[header].0);
                assert!(
                    rows[header + 1].0.contains("2 tickets"),
                    "{}",
                    rows[header + 1].0
                );
            }
        }
    }

    #[test]
    fn selecting_a_wrapped_ticket_below_the_screen_scrolls_to_its_last_line() {
        for &tab in Tab::all() {
            let tickets = (1..=5)
                .map(|n| {
                    let mut ticket = Ticket::for_test(&format!("DEMO-{n}"), "In Progress");
                    ticket.summary = format!("{} last{n}", words(30).join(" "));
                    ticket
                })
                .collect();
            let mut app = wrapping_app(tab, tickets);
            assert!(line_with(&draw_highlights(&app, tab, 120, 14), "last5").is_none());
            select_ticket(&mut app, "DEMO-5");
            let rows = draw_highlights(&app, tab, 120, 14);
            assert!(
                line_with(&rows, "DEMO-5").is_some_and(|(_, lit)| *lit),
                "{tab:?}"
            );
            assert!(
                rows[13].0.contains("last5") && rows[13].1,
                "{tab:?}: {rows:#?}"
            );
        }
    }

    #[test]
    fn a_wrapped_ticket_taller_than_the_list_keeps_its_key_line_on_screen() {
        for &tab in Tab::all() {
            let mut tall = Ticket::for_test("DEMO-2", "In Progress");
            tall.summary = words(200).join(" ");
            let mut app = wrapping_app(tab, vec![Ticket::for_test("DEMO-1", "In Progress"), tall]);
            select_ticket(&mut app, "DEMO-2");
            let rows = draw_highlights(&app, tab, 100, 8);
            // The list starts at the top, under Filters' two-row pane title.
            let top = if tab == Tab::Filters { 2 } else { 0 };
            assert!(
                rows[top].0.contains("DEMO-2") && rows[top].1,
                "{tab:?}: {rows:#?}"
            );
            assert!(rows[top].0.contains("word01"), "{tab:?}: {rows:#?}");
        }
    }

    #[test]
    fn wrapped_summaries_draw_on_terminals_too_narrow_for_summarys_minimum() {
        for &tab in Tab::all() {
            for width in [20, 30, 40, 60] {
                let mut long = Ticket::for_test("DEMO-1", "In Progress");
                long.summary = words(10).join(" ");
                let app = wrapping_app(tab, vec![long]);
                let rows = draw_highlights(&app, tab, width, 30);
                assert!(
                    rows.iter().any(|(text, _)| text.contains("DEMO-1")),
                    "{tab:?} at {width}: {rows:#?}"
                );
            }
        }
    }

    #[test]
    fn a_wrapped_summarys_later_lines_are_blank_before_the_summary() {
        for &tab in Tab::all() {
            let mut long = Ticket::for_test("DEMO-1", "In Progress");
            long.summary = words(30).join(" ");
            let app = wrapping_app(tab, vec![long]);
            let rows: Vec<Vec<char>> = draw_highlights(&app, tab, 120, 30)
                .into_iter()
                .map(|(text, _)| text.chars().collect())
                .collect();
            let key = rows
                .iter()
                .position(|row| String::from_iter(row).contains("DEMO-1"))
                .unwrap();
            let text = String::from_iter(&rows[key]);
            let column = |word: &str| text[..text.find(word).unwrap()].chars().count();
            let summary = column("word01");
            // The list's left edge: Filters draws its sidebar and the divider to the left of it.
            let left = rows[key][..column("DEMO-1")]
                .iter()
                .rposition(|c| *c == '│')
                .map_or(0, |divider| divider + 1);
            let later = &rows[key + 1];
            assert!(
                later[left..summary].iter().all(|c| *c == ' '),
                "{tab:?}: {}",
                String::from_iter(later)
            );
            assert!(String::from_iter(later).contains("word"), "{tab:?}");
            assert_ne!(
                later[summary], ' ',
                "{tab:?}: the summary continues in line"
            );
        }
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

    /// The text of each row of `draw`, trailing blanks cut.
    fn trimmed(rows: &[(String, Color)]) -> Vec<String> {
        rows.iter()
            .map(|(row, _)| row.trim_end().to_string())
            .collect()
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
}
