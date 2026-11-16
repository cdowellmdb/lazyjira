use super::support::*;
use crate::app::{App, FilterFocus, Tab};
use crate::cache::{Epic, Ticket};
use ratatui::{backend::TestBackend, Terminal};

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
