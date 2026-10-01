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
}
