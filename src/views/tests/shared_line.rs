use super::support::*;
use crate::app::{App, Tab};
use crate::cache::TeamMember;
use ratatui::style::Color;

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
