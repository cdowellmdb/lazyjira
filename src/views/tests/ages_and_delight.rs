use super::support::*;
use crate::app::{App, FilterFocus, Tab};
use crate::cache::{TeamMember, Ticket};
use ratatui::{backend::TestBackend, style::Color, Terminal};

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
        subtask,                                                // newest, but follows its parent
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
        let ends: Vec<(usize, String)> = [("DEMO-1", "23m"), ("DEMO-2", "3d"), ("DEMO-4", "5h")]
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
