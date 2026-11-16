//! The status line when nothing else is on it: key hints that drop out as the terminal narrows,
//! and one muted segment, right-aligned, for what differs from the defaults.

use std::cmp::Reverse;

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, FilterFocus, Tab};
use crate::style;

#[derive(Debug, Clone, Copy)]
pub struct Hint {
    pub key: &'static str,
    pub label: &'static str,
    /// Lower stays longer. Zero is never dropped.
    pub priority: u8,
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
const GAP: usize = 2;

/// The idle footer for `app` in `width` columns. In spooky season, until `theme` is the spooky
/// one, a hint to it leads.
pub fn line(app: &App, theme: &str, width: u16) -> Line<'static> {
    let mut hints = Vec::new();
    if app.delight.suggests_spooky_theme(theme) {
        hints.push(hint("S", "spooky theme", 2));
    }
    hints.extend_from_slice(if app.active_tab == Tab::Filters {
        FILTERS
    } else {
        MAIN
    });
    fit(&hints, &indicators(app), width as usize)
}

/// What differs from the defaults, joined, or empty when nothing does.
fn indicators(app: &App) -> String {
    let mut parts = Vec::new();
    let selected = app.selected_ticket_count();
    if selected > 0 {
        parts.push(format!("{selected} selected"));
    }
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

/// `hints` and `indicators` in `width` columns. The indicators go first when they don't fit
/// beside the hints, then hints from the highest priority number, rightmost first, so a hint
/// is whole or gone. A priority of zero stays even when nothing fits.
fn fit(hints: &[Hint], indicators: &str, width: usize) -> Line<'static> {
    let mut keep = vec![true; hints.len()];
    let shown = |keep: &[bool]| -> Vec<&Hint> {
        hints
            .iter()
            .zip(keep)
            .filter_map(|(hint, keep)| keep.then_some(hint))
            .collect()
    };
    // One cell trails the segment, as one leads the hints.
    let segment = GAP + indicators.width() + 1;
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
    let kept = shown(&keep);
    let mut spans = vec![Span::raw(" ")];
    for (at, hint) in kept.iter().enumerate() {
        if at > 0 {
            spans.push(Span::styled(SEPARATOR, style::secondary()));
        }
        spans.push(Span::styled(hint.key, style::key_hint()));
        spans.push(Span::styled(format!(" {}", hint.label), style::secondary()));
    }
    if !indicators.is_empty() {
        let pad = width - hints_width(&kept) - indicators.width() - 1;
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(format!("{indicators} "), style::secondary()));
    }
    Line::from(spans)
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

    /// Each footer `MAIN` makes as the terminal narrows, from the full line down to `? more`,
    /// one hint fewer each time, with the narrowest width that holds it.
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

    #[test]
    fn hints_are_dropped_lowest_priority_first_then_from_the_right_at_exact_widths() {
        let dropped = [
            "r refresh",
            "B bulk",
            "q quit",
            "Space mark",
            "/ search",
            "Enter open",
            "j/k move",
            "Tab switch",
        ];
        for (at, (width, expected)) in MAIN_STEPS.iter().enumerate() {
            assert_eq!(expected.width(), *width, "{expected}");
            assert_eq!(text(&fit(MAIN, "", *width)), *expected);
            if let Some((_, narrower)) = MAIN_STEPS.get(at + 1) {
                let line = text(&fit(MAIN, "", width - 1));
                assert_eq!(
                    line, *narrower,
                    "one column less than {width} drops {}",
                    dropped[at]
                );
                assert!(!line.contains(dropped[at]));
            }
        }
    }

    #[test]
    fn a_hint_is_whole_or_gone_at_every_width_and_more_always_stays() {
        for width in 0..=200 {
            let line = text(&fit(MAIN, "", width));
            assert!(line.ends_with("? more"), "{width}: {line}");
            let expected = MAIN_STEPS
                .iter()
                .find(|(needed, _)| *needed <= width)
                .map_or(MAIN_STEPS[8].1, |(_, step)| step);
            assert_eq!(line, expected, "{width}");
        }
    }

    #[test]
    fn the_indicators_are_dropped_before_any_hint_and_sit_flush_right() {
        let full = MAIN_STEPS[0].0;
        let indicators = "2 selected · stale 3m";
        let needed = full + GAP + indicators.width() + 1;
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
        assert!(text(&line(&app, "default", 200)).starts_with(" S spooky theme · Tab switch"));
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

    #[test]
    fn every_footer_key_is_in_the_help() {
        let help: std::collections::HashSet<&str> = crate::widgets::keybindings_help::MAIN
            .iter()
            .chain(crate::widgets::keybindings_help::DETAIL)
            .flat_map(|(keys, _)| keys.split([' ', '·']).filter(|key| !key.is_empty()))
            .flat_map(|key| std::iter::once(key).chain(key.split('/')))
            .collect();
        for hint in MAIN.iter().chain(FILTERS) {
            let parts: Vec<&str> = hint.key.split('/').collect();
            assert!(
                help.contains(hint.key) || parts.iter().all(|part| help.contains(part)),
                "{} is not in the help",
                hint.key
            );
        }
    }
}
