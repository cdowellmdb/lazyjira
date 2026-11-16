//! The status line when nothing else is on it: key hints that drop out as the terminal narrows,
//! and one muted segment, right-aligned, for what differs from the defaults. While tickets are
//! marked, the bulk bar above it counts them and names the keys that act on them.

use std::cmp::Reverse;

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, FilterFocus, Tab};
use crate::mouse::Target;
use crate::style;

#[derive(Debug, Clone, Copy)]
struct Hint {
    key: &'static str,
    label: &'static str,
    /// Lower stays longer. Zero is never dropped.
    priority: u8,
}

const fn hint(key: &'static str, label: &'static str, priority: u8) -> Hint {
    Hint {
        key,
        label,
        priority,
    }
}

const MORE: Hint = hint("?", "more", 0);

const MAIN: &[Hint] = &[
    hint("Tab", "switch", 1),
    hint("j/k", "move", 1),
    hint("Space", "mark", 3),
    hint("Enter", "open", 1),
    hint("B", "bulk", 4),
    hint("/", "search", 2),
    hint("r", "refresh", 4),
    hint("q", "quit", 3),
    MORE,
];

const FILTERS: &[Hint] = &[
    hint("j/k", "move", 1),
    hint("Enter", "open", 1),
    hint("Tab", "pane", 1),
    hint("Space", "mark", 3),
    hint("n", "new", 2),
    hint("e", "edit", 2),
    hint("x", "delete", 2),
    hint("z", "fold", 4),
    hint("B", "bulk", 4),
    hint("q", "quit", 3),
    MORE,
];

const SEPARATOR: &str = " · ";
/// Blank cells between the hints and the segment at the right.
const SEGMENT_GAP: usize = 2;

/// The idle footer for `app` in `width` columns. In spooky season, until `theme` is the spooky
/// one, a hint to it leads.
pub fn line(app: &App, theme: &str, width: u16) -> Line<'static> {
    fit(&hints(app, theme), &indicators(app), width as usize)
}

fn hints(app: &App, theme: &str) -> Vec<Hint> {
    let mut hints = Vec::new();
    if app.delight.suggests_spooky_theme(theme) {
        hints.push(hint("S", "settings", 2));
    }
    let base = if app.active_tab == Tab::Filters {
        FILTERS
    } else {
        MAIN
    };
    for hint in base {
        hints.push(*hint);
    }
    hints
}

/// What differs from the defaults, joined, or empty when nothing does.
fn indicators(app: &App) -> String {
    let mut parts = Vec::new();
    if app.active_tab == Tab::Filters {
        if app.filter_focus == FilterFocus::Results {
            parts.push("results pane".to_string());
        }
    } else {
        if !app.show_done {
            parts.push("done hidden".to_string());
        }
        if let Some(focus) = &app.status_focus {
            parts.push(format!("focus {focus}"));
        }
        if app.ticket_sync_stage.is_some() {
            parts.push("syncing".to_string());
        } else if app.loading {
            parts.push("refreshing".to_string());
        }
        if let Some(age) = app.cache_stale_age_secs {
            parts.push(format!("stale {}", age_in_minutes(age)));
        }
        if app.epics_refreshing {
            parts.push("epics syncing".to_string());
        }
    }
    parts.join(SEPARATOR)
}

/// What the bulk bar offers. `bulk_bar` drops hints from the right, so their order is the drop
/// order and every priority is 1, unread.
const BULK: &[Hint] = &[
    hint("B", "move or assign", 1),
    hint("u", "clear", 1),
    hint("A", "all", 1),
];

/// Cells between the count and the hints in the bulk bar.
const COUNT_GAP: usize = 3;

/// The bar above the footer while tickets are marked: `2 selected   B move or assign · u clear ·
/// A all`, with whole hints dropped from the right in `width` columns. `None` when nothing is
/// marked.
pub fn bulk_bar(app: &App, width: u16) -> Option<Line<'static>> {
    if !app.is_marking() {
        return None;
    }
    let selected = app.selected_ticket_count();
    let count = format!("{selected} selected");
    let mut hints: Vec<&Hint> = BULK.iter().collect();
    let room = (width as usize).saturating_sub(1 + count.width() + COUNT_GAP);
    // `hints_width` counts the cell that leads the footer's hints, which this bar's gap replaces.
    while hints_width(&hints) - 1 > room {
        hints.pop();
    }
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            selected.to_string(),
            style::accent().patch(style::heading()),
        ),
        Span::styled(" selected", style::secondary()),
    ];
    if !hints.is_empty() {
        spans.push(Span::raw(" ".repeat(COUNT_GAP)));
        spans.extend(hint_spans(&hints));
    }
    Some(Line::from(spans))
}

/// A panel's or the detail's hints, `(key, label)`: `Esc` is never dropped and the others go
/// from the right, so their order is the drop order.
pub type Pairs<'a> = &'a [(&'static str, &'static str)];

fn pair_hints(pairs: Pairs) -> Vec<Hint> {
    pairs
        .iter()
        .enumerate()
        .map(|(at, &(key, label))| hint(key, label, if key == "Esc" { 0 } else { 1 + at as u8 }))
        .collect()
}

/// Draws `pairs` in `area`'s first row like the footer's hints, dropping whole hints from the
/// right to fit, and makes each kept key that is one key press (`Esc`, `Enter` or a character)
/// a click target, and each arrow of `←→`.
pub fn render_pairs(f: &mut ratatui::Frame, app: &App, area: Rect, pairs: Pairs) {
    if area.is_empty() {
        return;
    }
    let row = Rect::new(area.x, area.y, area.width, 1);
    let hints = pair_hints(pairs);
    let kept = choose(&hints, "", row.width as usize).kept;
    f.render_widget(ratatui::widgets::Paragraph::new(hints_line(&kept)), row);
    let mut x = row.x.saturating_add(1);
    let mut targets = app.mouse_targets.borrow_mut();
    for hint in kept {
        let width = hint.key.width() as u16;
        let key = match hint.key {
            "Esc" => Some(KeyCode::Esc),
            "Enter" => Some(KeyCode::Enter),
            key if key.chars().count() == 1 => key.chars().next().map(KeyCode::Char),
            _ => None,
        };
        if let Some(key) = key {
            let room = row.right().saturating_sub(x);
            targets.push((Rect::new(x, row.y, width.min(room), 1), Target::Key(key)));
        }
        if hint.key == "←→" {
            targets.push((Rect::new(x, row.y, 1, 1), Target::Key(KeyCode::Left)));
            targets.push((Rect::new(x + 1, row.y, 1, 1), Target::Key(KeyCode::Right)));
        }
        x = x.saturating_add(width + 1 + hint.label.width() as u16 + SEPARATOR.width() as u16);
    }
}

/// The line `render_pairs` draws for `pairs` in `width` columns, with no click targets.
pub fn pairs_line(pairs: Pairs, width: u16) -> Line<'static> {
    hints_line(&choose(&pair_hints(pairs), "", width as usize).kept)
}

/// `pairs` split into as few rows of at most `width` columns as hold them, in order, each hint
/// whole on one row, and the rows as even as the hints allow. A hint wider than `width` has a
/// row to itself.
pub fn wrap_pairs(pairs: Pairs, width: u16) -> Vec<Pairs> {
    let rows = fill_rows(pairs, width as usize);
    let total = pair_hints(pairs);
    let least = hints_width(&total.iter().collect::<Vec<_>>()).div_ceil(rows.len().max(1));
    // The narrowest limit that still takes the same number of rows.
    (least..=width as usize)
        .map(|limit| fill_rows(pairs, limit))
        .find(|even| even.len() == rows.len())
        .unwrap_or(rows)
}

/// `pairs` in rows filled in order up to `limit` columns each.
fn fill_rows(pairs: Pairs, limit: usize) -> Vec<Pairs> {
    let mut rows = Vec::new();
    let mut start = 0;
    for end in 1..pairs.len() {
        let row = pair_hints(&pairs[start..=end]);
        if hints_width(&row.iter().collect::<Vec<_>>()) > limit {
            rows.push(&pairs[start..end]);
            start = end;
        }
    }
    if start < pairs.len() {
        rows.push(&pairs[start..]);
    }
    rows
}

fn age_in_minutes(age_secs: u64) -> String {
    match age_secs / 60 {
        0 => "<1m".to_string(),
        mins => format!("{mins}m"),
    }
}

fn hints_width(hints: &[&Hint]) -> usize {
    let text: usize = hints
        .iter()
        .map(|hint| hint.key.width() + 1 + hint.label.width())
        .sum();
    1 + text + SEPARATOR.width() * hints.len().saturating_sub(1)
}

/// `hints` as spans: each key bold and its label muted, a separator between.
fn hint_spans(hints: &[&Hint]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (at, hint) in hints.iter().enumerate() {
        if at > 0 {
            spans.push(Span::styled(SEPARATOR, style::secondary()));
        }
        spans.push(Span::styled(hint.key, style::key_hint()));
        spans.push(Span::styled(format!(" {}", hint.label), style::secondary()));
    }
    spans
}

/// What `choose` kept: the hints that fit, and the indicators if they fit beside them.
struct Fitted<'a> {
    kept: Vec<&'a Hint>,
    indicators: &'a str,
}

/// Which of `hints` and `indicators` fit in `width` columns. The indicators go first when they
/// don't fit beside the hints, then hints from the highest priority number, rightmost first, so
/// a hint is whole or gone. A priority of zero stays even when nothing fits.
fn choose<'a>(hints: &'a [Hint], indicators: &'a str, width: usize) -> Fitted<'a> {
    let mut keep = vec![true; hints.len()];
    let shown = |keep: &[bool]| -> Vec<&Hint> {
        hints
            .iter()
            .zip(keep)
            .filter_map(|(hint, keep)| keep.then_some(hint))
            .collect()
    };
    // One cell trails the segment, as one leads the hints.
    let segment = SEGMENT_GAP + indicators.width() + 1;
    let indicators = if !indicators.is_empty() && hints_width(&shown(&keep)) + segment <= width {
        indicators
    } else {
        ""
    };
    let mut drop_order: Vec<usize> = (0..hints.len())
        .filter(|&at| hints[at].priority > 0)
        .collect();
    drop_order.sort_by_key(|&at| (Reverse(hints[at].priority), Reverse(at)));
    for at in drop_order {
        if hints_width(&shown(&keep)) <= width {
            break;
        }
        keep[at] = false;
    }
    Fitted {
        kept: shown(&keep),
        indicators,
    }
}

/// `kept` as a line: a cell, then each hint.
fn hints_line(kept: &[&Hint]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    spans.extend(hint_spans(kept));
    Line::from(spans)
}

/// `hints` and `indicators` in `width` columns, the indicators right-aligned.
fn fit(hints: &[Hint], indicators: &str, width: usize) -> Line<'static> {
    let Fitted { kept, indicators } = choose(hints, indicators, width);
    let mut line = hints_line(&kept);
    if !indicators.is_empty() {
        let pad = width.saturating_sub(hints_width(&kept) + indicators.width() + 1);
        line.spans.push(Span::raw(" ".repeat(pad)));
        line.spans
            .push(Span::styled(format!("{indicators} "), style::secondary()));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn app() -> App {
        let mut app = App::new();
        app.loading = false;
        app
    }

    /// Each footer `MAIN` makes as the terminal narrows, from the full line down to
    /// `? more`, one hint fewer each time, with the narrowest width that holds it.
    const MAIN_STEPS: [(usize, &str); 9] = [
        (98, " Tab switch · j/k move · Space mark · Enter open · B bulk · / search · r refresh · q quit · ? more"),
        (86, " Tab switch · j/k move · Space mark · Enter open · B bulk · / search · q quit · ? more"),
        (77, " Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more"),
        (68, " Tab switch · j/k move · Space mark · Enter open · / search · ? more"),
        (55, " Tab switch · j/k move · Enter open · / search · ? more"),
        (44, " Tab switch · j/k move · Enter open · ? more"),
        (31, " Tab switch · j/k move · ? more"),
        (20, " Tab switch · ? more"),
        (7, " ? more"),
    ];
    const MAIN_DROPPED: [&str; 8] = [
        "r refresh",
        "B bulk",
        "q quit",
        "Space mark",
        "/ search",
        "Enter open",
        "j/k move",
        "Tab switch",
    ];

    /// Each footer `FILTERS` makes as the terminal narrows, from the full line down to
    /// `? more`, one hint fewer each time, with the narrowest width that holds it.
    const FILTERS_STEPS: [(usize, &str); 11] = [
        (110, " j/k move · Enter open · Tab pane · Space mark · n new · e edit · x delete · z fold · B bulk · q quit · ? more"),
        (101, " j/k move · Enter open · Tab pane · Space mark · n new · e edit · x delete · z fold · q quit · ? more"),
        (92, " j/k move · Enter open · Tab pane · Space mark · n new · e edit · x delete · q quit · ? more"),
        (83, " j/k move · Enter open · Tab pane · Space mark · n new · e edit · x delete · ? more"),
        (70, " j/k move · Enter open · Tab pane · n new · e edit · x delete · ? more"),
        (59, " j/k move · Enter open · Tab pane · n new · e edit · ? more"),
        (50, " j/k move · Enter open · Tab pane · n new · ? more"),
        (42, " j/k move · Enter open · Tab pane · ? more"),
        (31, " j/k move · Enter open · ? more"),
        (18, " j/k move · ? more"),
        (7, " ? more"),
    ];
    const FILTERS_DROPPED: [&str; 10] = [
        "B bulk",
        "z fold",
        "q quit",
        "Space mark",
        "x delete",
        "e edit",
        "n new",
        "Tab pane",
        "Enter open",
        "j/k move",
    ];

    /// The footer when spooky season puts its hint ahead of `MAIN`, from the full line down to `? more`.
    /// Each step is the narrowest width that holds its line.
    const SPOOKY_STEPS: [(usize, &str); 10] = [
        (111, " S settings · Tab switch · j/k move · Space mark · Enter open · B bulk · / search · r refresh · q quit · ? more"),
        (99, " S settings · Tab switch · j/k move · Space mark · Enter open · B bulk · / search · q quit · ? more"),
        (90, " S settings · Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more"),
        (81, " S settings · Tab switch · j/k move · Space mark · Enter open · / search · ? more"),
        (68, " S settings · Tab switch · j/k move · Enter open · / search · ? more"),
        (57, " S settings · Tab switch · j/k move · Enter open · ? more"),
        (44, " Tab switch · j/k move · Enter open · ? more"),
        (31, " Tab switch · j/k move · ? more"),
        (20, " Tab switch · ? more"),
        (7, " ? more"),
    ];
    const SPOOKY_DROPPED: [&str; 9] = [
        "r refresh",
        "B bulk",
        "q quit",
        "Space mark",
        "/ search",
        "S settings",
        "Enter open",
        "j/k move",
        "Tab switch",
    ];

    fn spooky() -> Vec<Hint> {
        let mut app = app();
        app.delight.enabled = true;
        app.delight.today = Some(crate::delight::Today {
            weekday: 5,
            month: 10,
            day: 23,
        });
        hints(&app, "default")
    }

    /// Every step is the narrowest width that holds its line, one column less drops exactly the
    /// next hint, and every width from 0 to 200 draws the step it falls in, ending in `? more`.
    fn assert_steps(hints: &[Hint], steps: &[(usize, &str)], dropped: &[&str]) {
        for (at, (width, expected)) in steps.iter().enumerate() {
            assert_eq!(expected.width(), *width, "{expected}");
            assert_eq!(text(&fit(hints, "", *width)), *expected);
            if let Some((_, narrower)) = steps.get(at + 1) {
                let line = text(&fit(hints, "", width - 1));
                assert_eq!(
                    line, *narrower,
                    "one column less than {width} drops {}",
                    dropped[at]
                );
                assert!(!line.contains(dropped[at]), "{line}");
            }
        }
        for width in 0..=200 {
            let line = text(&fit(hints, "", width));
            assert!(line.ends_with("? more"), "{width}: {line}");
            let expected = steps
                .iter()
                .find(|(needed, _)| *needed <= width)
                .map_or(steps[steps.len() - 1].1, |(_, step)| step);
            assert_eq!(line, expected, "{width}");
        }
    }

    #[test]
    fn main_hints_are_dropped_lowest_priority_first_then_from_the_right_at_exact_widths() {
        assert_steps(MAIN, &MAIN_STEPS, &MAIN_DROPPED);
    }

    #[test]
    fn filters_hints_drop_at_exact_widths() {
        assert_steps(FILTERS, &FILTERS_STEPS, &FILTERS_DROPPED);
    }

    #[test]
    fn the_spooky_hint_drops_after_the_priority_two_hints_to_its_right() {
        assert_steps(&spooky(), &SPOOKY_STEPS, &SPOOKY_DROPPED);
    }

    #[test]
    fn overlay_hints_drop_from_the_right_but_esc_stays_and_each_key_is_clickable() {
        let pairs = [
            ("↑↓", "scroll"),
            ("←→", "prev/next"),
            ("m", "move"),
            ("Esc", "close"),
        ];
        for (width, expected) in [
            (50, " ↑↓ scroll · ←→ prev/next · m move · Esc close"),
            (46, " ↑↓ scroll · ←→ prev/next · m move · Esc close"),
            (45, " ↑↓ scroll · ←→ prev/next · Esc close"),
            (37, " ↑↓ scroll · ←→ prev/next · Esc close"),
            (36, " ↑↓ scroll · Esc close"),
            (22, " ↑↓ scroll · Esc close"),
            (21, " Esc close"),
            (10, " Esc close"),
        ] {
            let app = app();
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 8)).unwrap();
            terminal
                .draw(|f| render_pairs(f, &app, Rect::new(0, 0, width, 1), &pairs))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let row: String = (0..width).map(|x| buffer[(x, 0)].symbol()).collect();
            assert_eq!(row.trim_end(), expected, "{width}");
        }
        let app = app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 8)).unwrap();
        terminal
            .draw(|f| render_pairs(f, &app, Rect::new(10, 7, 50, 1), &pairs))
            .unwrap();
        let targets: Vec<(Rect, Target)> = app.mouse_targets.borrow().clone();
        assert_eq!(
            targets,
            [
                (Rect::new(23, 7, 1, 1), Target::Key(KeyCode::Left)),
                (Rect::new(24, 7, 1, 1), Target::Key(KeyCode::Right)),
                (Rect::new(38, 7, 1, 1), Target::Key(KeyCode::Char('m'))),
                (Rect::new(47, 7, 3, 1), Target::Key(KeyCode::Esc)),
            ]
        );
    }

    #[test]
    fn marked_tickets_add_no_hint_to_the_footer_because_the_bulk_bar_names_the_keys() {
        let mut app = app();
        let idle = text(&line(&app, "default", 200));
        app.selected_ticket_keys.insert("DEMO-1".into());
        assert_eq!(text(&line(&app, "default", 200)), idle);
        assert!(!idle.contains("u clear"), "{idle}");
        assert_eq!(indicators(&app), "", "no duplicate count either");
    }

    fn bulk_text(selected: usize, width: u16) -> Option<String> {
        let mut app = app();
        app.selected_ticket_keys = (1..=selected).map(|n| format!("DEMO-{n}")).collect();
        bulk_bar(&app, width).map(|line| text(&line))
    }

    #[test]
    fn the_bulk_bar_counts_the_marked_tickets_and_drops_whole_hints_from_the_right_at_exact_widths()
    {
        assert_eq!(bulk_text(0, 200), None);
        let full = " 2 selected   B move or assign · u clear · A all";
        assert_eq!(full.width(), 48);
        assert_eq!(bulk_text(2, 200).as_deref(), Some(full));
        assert_eq!(
            bulk_text(2, 48).as_deref(),
            Some(full),
            "exactly the room it needs"
        );
        for (width, expected) in [
            (47, " 2 selected   B move or assign · u clear"),
            (40, " 2 selected   B move or assign · u clear"),
            (39, " 2 selected   B move or assign"),
            (30, " 2 selected   B move or assign"),
            (29, " 2 selected"),
            (0, " 2 selected"),
        ] {
            assert_eq!(bulk_text(2, width).as_deref(), Some(expected), "{width}");
        }
        // The count takes its own width: 12 is a column wider than 2.
        assert_eq!(
            bulk_text(12, 49).as_deref(),
            Some(" 12 selected   B move or assign · u clear · A all")
        );
        assert_eq!(
            bulk_text(12, 48).as_deref(),
            Some(" 12 selected   B move or assign · u clear")
        );
    }

    #[test]
    fn the_bulk_bar_has_an_accent_count_bold_accent_keys_and_muted_words() {
        let mut app = app();
        app.selected_ticket_keys.insert("DEMO-1".into());
        let line = bulk_bar(&app, 80).unwrap();
        let style_of = |content: &str| {
            line.spans
                .iter()
                .find(|span| span.content == content)
                .unwrap_or_else(|| panic!("no {content:?} in {line:?}"))
                .style
        };
        assert_eq!(style_of("1").fg, Some(Color::Cyan));
        assert!(style_of("1").add_modifier.contains(Modifier::BOLD));
        assert_eq!(style_of(" selected").fg, Some(Color::DarkGray));
        for key in ["B", "u", "A"] {
            assert_eq!(style_of(key).fg, Some(Color::Cyan), "{key}");
            assert!(style_of(key).add_modifier.contains(Modifier::BOLD), "{key}");
        }
        assert_eq!(style_of(" move or assign").fg, Some(Color::DarkGray));
        assert_eq!(style_of(" · ").fg, Some(Color::DarkGray));
    }

    #[test]
    fn the_stale_age_changes_unit_at_exact_seconds() {
        for (secs, expected) in [
            (0, "<1m"),
            (59, "<1m"),
            (60, "1m"),
            (119, "1m"),
            (120, "2m"),
        ] {
            assert_eq!(age_in_minutes(secs), expected, "{secs}s");
        }
        let mut app = app();
        for (secs, expected) in [
            (59, "stale <1m"),
            (60, "stale 1m"),
            (119, "stale 1m"),
            (120, "stale 2m"),
        ] {
            app.cache_stale_age_secs = Some(secs);
            assert_eq!(indicators(&app), expected, "{secs}s");
        }
    }

    #[test]
    fn the_indicators_are_dropped_before_any_hint_and_sit_flush_right() {
        let full = MAIN_STEPS[0].0;
        let indicators = "2 selected · stale 3m";
        let needed = full + SEGMENT_GAP + indicators.width() + 1;
        let line = text(&fit(MAIN, indicators, needed));
        assert_eq!(line, format!("{}{}{indicators} ", MAIN_STEPS[0].1, "  "));
        assert_eq!(line.width(), needed);
        let wider = text(&fit(MAIN, indicators, needed + 5));
        assert_eq!(wider.width(), needed + 5);
        assert!(wider.ends_with(&format!("{indicators} ")));

        let narrower = text(&fit(MAIN, indicators, needed - 1));
        assert_eq!(
            narrower, MAIN_STEPS[0].1,
            "every hint stays, the segment goes"
        );
        assert_eq!(text(&fit(MAIN, indicators, full - 1)), MAIN_STEPS[1].1);
    }

    #[test]
    fn indicators_show_only_what_differs_from_the_defaults() {
        let mut app = app();
        assert_eq!(indicators(&app), "");
        app.show_done = false;
        assert_eq!(indicators(&app), "done hidden");
        app.status_focus = Some("In Progress".to_string());
        app.loading = true;
        assert_eq!(
            indicators(&app),
            "done hidden · focus In Progress · refreshing"
        );
        app.ticket_sync_stage = Some(crate::app::TicketSyncStage::ActiveOnly);
        app.cache_stale_age_secs = Some(59);
        app.epics_refreshing = true;
        assert_eq!(
            indicators(&app),
            "done hidden · focus In Progress · syncing · stale <1m · epics syncing"
        );
        app.cache_stale_age_secs = Some(180);
        assert!(indicators(&app).contains("stale 3m"));

        app.active_tab = Tab::Filters;
        assert_eq!(indicators(&app), "");
        app.filter_focus = FilterFocus::Results;
        assert_eq!(indicators(&app), "results pane");
    }

    #[test]
    fn filters_keep_their_own_hints_and_the_spooky_theme_hint_leads_in_season() {
        let mut app = app();
        app.active_tab = Tab::Filters;
        assert_eq!(
            text(&line(&app, "default", 200)),
            " j/k move · Enter open · Tab pane · Space mark · n new · e edit · x delete · z fold · B bulk · q quit · ? more"
        );
        assert_eq!(
            text(&line(&app, "default", 43)),
            " j/k move · Enter open · Tab pane · ? more"
        );

        app.active_tab = Tab::MyWork;
        app.delight.enabled = true;
        app.delight.today = Some(crate::delight::Today {
            weekday: 5,
            month: 10,
            day: 23,
        });
        assert!(text(&line(&app, "default", 200)).starts_with(" S settings · Tab switch"));
        assert!(text(&line(&app, "spooky", 200)).starts_with(" Tab switch"));
    }

    #[test]
    fn keys_are_bold_accent_and_labels_and_indicators_are_muted() {
        let line = fit(MAIN, "2 selected", 200);
        let style_of = |content: &str| {
            line.spans
                .iter()
                .find(|span| span.content.contains(content))
                .unwrap()
                .style
        };
        assert_eq!(style_of("Tab").fg, Some(Color::Cyan));
        assert!(style_of("Tab").add_modifier.contains(Modifier::BOLD));
        assert_eq!(style_of(" switch").fg, Some(Color::DarkGray));
        assert_eq!(style_of("2 selected").fg, Some(Color::DarkGray));
        assert_eq!(style_of(" · ").fg, Some(Color::DarkGray));
    }

    /// The keys in one help key column: ` · ` and ` / ` separate alternatives, and a `/` with no
    /// spaces either joins two keys (`j/k`, listed whole and as `j` and `k`) or is the key.
    fn tokens(column: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for key in column.split(" · ").flat_map(|keys| keys.split(" / ")) {
            keys.push(key.to_string());
            if key.len() > 1 && key.contains('/') {
                keys.extend(
                    key.split('/')
                        .filter(|part| !part.is_empty())
                        .map(String::from),
                );
            }
        }
        keys
    }

    #[test]
    fn help_columns_split_into_exact_keys() {
        assert_eq!(tokens("A / u"), ["A", "u"]);
        assert_eq!(tokens("/"), ["/"]);
        assert_eq!(tokens("j/k · ↑/↓"), ["j/k", "j", "k", "↑/↓", "↑", "↓"]);
        assert_eq!(tokens("Space · A/u · B"), ["Space", "A/u", "A", "u", "B"]);
    }

    #[test]
    fn every_footer_key_is_in_the_help() {
        let help: std::collections::HashSet<String> = crate::widgets::keybindings_help::keys()
            .flat_map(tokens)
            .collect();
        assert!(help.contains("/"), "the search row lists /");
        let mut keys: Vec<&str> = MAIN.iter().chain(FILTERS).map(|hint| hint.key).collect();
        keys.extend(BULK.iter().map(|hint| hint.key));
        keys.push("S");
        for key in keys {
            assert!(help.contains(key), "{key} is not in the help");
        }
    }

    #[test]
    fn wrapped_pairs_fill_the_fewest_rows_whole_and_as_even_as_they_allow() {
        // Three hints of 6 cells and `Esc close` of 9: 37 columns with the leading cell and the
        // 3 cell separators.
        let pairs: Pairs = &[
            ("a", "aaaa"),
            ("b", "bbbb"),
            ("c", "cccc"),
            ("Esc", "close"),
        ];
        let keys = |width| -> Vec<Vec<&str>> {
            wrap_pairs(pairs, width)
                .iter()
                .map(|row| row.iter().map(|(key, _)| *key).collect())
                .collect()
        };
        assert_eq!(keys(40), [["a", "b", "c", "Esc"]]);
        assert_eq!(keys(37), [["a", "b", "c", "Esc"]]);
        // 36 columns need two rows. Filling in order would leave `Esc` alone, so they even out.
        assert_eq!(keys(36), [vec!["a", "b"], vec!["c", "Esc"]]);
        assert_eq!(keys(19), [vec!["a", "b"], vec!["c", "Esc"]]);
        // `c` and `Esc close` need 19 columns together.
        assert_eq!(keys(18), [vec!["a", "b"], vec!["c"], vec!["Esc"]]);
        assert_eq!(keys(16), [vec!["a", "b"], vec!["c"], vec!["Esc"]]);
        assert_eq!(keys(15), [vec!["a"], vec!["b"], vec!["c"], vec!["Esc"]]);
        // A hint wider than the width has its row to itself, whole.
        assert_eq!(keys(5), [vec!["a"], vec!["b"], vec!["c"], vec!["Esc"]]);
        assert!(wrap_pairs(&[], 80).is_empty());
    }
}
