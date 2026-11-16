//! The status line and the footer, drawn through `ui`.

use super::*;

#[test]
fn a_multi_line_error_reads_as_one_line_in_the_status_bar() {
    use ratatui::backend::TestBackend;

    let mut app = App::new();
    app.flash =
        Some("Refresh failed: Jira answered 400 Bad Request.\nError in the JQL Query".to_string());
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();

    terminal.draw(|f| ui(f, &app, &sample_config())).unwrap();

    let buffer = terminal.backend().buffer();
    let text: String = (0..100).map(|x| buffer[(x, 11)].symbol()).collect();
    assert!(
        text.contains("400 Bad Request.; Error in the JQL Query"),
        "{text}"
    );
}

fn footer_row(app: &App, width: u16) -> (ratatui::buffer::Buffer, String) {
    use ratatui::backend::TestBackend;

    let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
    terminal.draw(|f| ui(f, app, &sample_config())).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let text = (0..width).map(|x| buffer[(x, 11)].symbol()).collect();
    (buffer, text)
}

#[test]
fn main_footer_highlights_shortcuts_and_keeps_status_indicators_muted() {
    use ratatui::style::{Color, Modifier};

    let mut app = App::new();
    app.loading = false;
    app.show_done = false;
    app.ticket_sync_stage = Some(TicketSyncStage::Full);
    app.cache_stale_age_secs = Some(180);
    app.epics_refreshing = true;
    app.status_focus = Some("In Progress".to_string());

    for tab in Tab::all() {
        app.active_tab = *tab;
        let (buffer, text) = footer_row(&app, 300);
        let (keys, indicators) = if *tab == Tab::Filters {
            (vec!["j/k", "Enter", "Tab", "n", "x", "z", "B"], "")
        } else {
            (
                vec!["Tab", "j/k", "Space", "Enter", "B", "/", "r", "q"],
                "done hidden · focus In Progress · syncing · stale 3m · epics syncing",
            )
        };
        let at = |token: &str, offset: usize| {
            let byte = text
                .find(token)
                .unwrap_or_else(|| panic!("missing {token}: {text}"));
            &buffer[((text[..byte].chars().count() + offset) as u16, 11)]
        };
        for key in keys {
            let cell = at(&format!(" {key} "), 1);
            assert_eq!(cell.fg, Color::Cyan, "{key}");
            assert!(cell.modifier.contains(Modifier::BOLD), "{key}");
        }
        assert_eq!(at("move", 0).fg, Color::DarkGray);
        assert_eq!(at("more", 0).fg, Color::DarkGray);
        if !indicators.is_empty() {
            assert!(text.ends_with(&format!("{indicators} ")), "{text}");
            assert_eq!(at(indicators, 0).fg, Color::DarkGray);
        }
        assert!(!text.contains("sel:"), "{text}");
    }
}

#[test]
fn the_footer_at_80_and_120_columns_keeps_whole_hints_and_a_right_aligned_segment() {
    let mut app = App::new();
    app.loading = false;
    let full = " Tab switch · j/k move · Space mark · Enter open · B bulk · / search · r refresh · q quit · ? more";
    for (width, expected) in [
        (
            80,
            " Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more",
        ),
        (120, full),
    ] {
        let (_, text) = footer_row(&app, width);
        assert_eq!(text.trim_end(), expected, "{width}");
    }

    app.cache_stale_age_secs = Some(180);
    let (_, text) = footer_row(&app, 120);
    assert_eq!(text, format!("{full}{}stale 3m ", " ".repeat(13)));
    let (_, text) = footer_row(&app, 80);
    assert_eq!(
        text.trim_end(),
        " Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more",
        "the segment goes before a hint"
    );
}
