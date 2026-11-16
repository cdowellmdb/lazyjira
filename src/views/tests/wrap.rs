use super::support::*;
use crate::app::{App, FilterFocus, Tab};
use crate::cache::{Epic, TeamMember, Ticket};
use ratatui::{backend::TestBackend, Terminal};

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
