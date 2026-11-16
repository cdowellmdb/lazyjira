use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use std::time::Instant;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, VisibleGroup};
use crate::cache::{Epic, Ticket};
use crate::style;
use crate::views::common::{
    column_width, epic_bar, fit, key_width, muted, narrow_for_summary, status_cell, status_label,
    status_width, ListLines, COLUMN_GAP, EPIC_BAR_WIDTH, STATUS_DOT_WIDTH,
};

/// Status and Assignee are as wide as the longest value on screen (Assignee at most 24), so
/// "Waiting for Approval" or a long name doesn't push its row's summary out of line. Summary takes
/// the rest; on narrow terminals Assignee, then Status, give up columns, down to 8, so it keeps 18.
fn child_column_widths(
    area: Rect,
    lead: usize,
    key_w: usize,
    epics: &[VisibleGroup<'_, &Epic>],
) -> (usize, usize, usize) {
    let mut status_w = status_width(epics);
    let mut assignee_w = column_width(epics, "ASSIGNEE", assignee).min(24);
    let room =
        (area.width as usize).saturating_sub(lead + key_w + 3 * COLUMN_GAP + STATUS_DOT_WIDTH);
    let columns = &mut [(&mut assignee_w, 8), (&mut status_w, 8)];
    let summary_w = narrow_for_summary(room, 18, columns);
    (status_w, assignee_w, summary_w)
}

fn assignee(ticket: &Ticket) -> &str {
    ticket.assignee.as_deref().unwrap_or("-")
}

/// "97% done · 121 tickets: 108 Done · 3 In Progress · 10 Won't Do", each count in its status's
/// color, in the progress bar's order, after `Epic::percent_done` when it has one.
fn status_counts(app: &App, epic: &Epic, style: Style) -> Vec<Span<'static>> {
    let rules = app.status_rules();
    let total = epic.total();
    let noun = if total == 1 { "ticket" } else { "tickets" };
    let done = match epic.percent_done(rules) {
        Some(percent) => format!("{percent}% done · "),
        None => String::new(),
    };
    let mut spans = vec![Span::styled(format!("  {done}{total} {noun}"), style)];
    for (at, (status, count)) in epic.status_counts(rules).into_iter().enumerate() {
        spans.push(Span::styled(if at == 0 { ": " } else { " · " }, style));
        spans.push(Span::styled(
            format!("{count} {}", status_label(app, &status)),
            style.patch(style::status(&status, rules)),
        ));
    }
    spans
}

/// `compact` is the list's width beside the detail pane, which shows only keys and summaries.
pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App, compact: Option<u16>) {
    let visible_epics = app.epics_visible_epics();
    let key_w = key_width(&visible_epics);
    let mut lines = ListLines::new(app).compact(compact);
    let lead = lines.lead_width();
    let (status_w, assignee_w, summary_w) = child_column_widths(area, lead, key_w, &visible_epics);

    if !visible_epics.is_empty() {
        lines.intro(None, 0);
    }

    for group in visible_epics {
        let epic = group.header;

        let summary_w_of_header = (area.width as usize)
            .saturating_sub(lead + epic.key.width() + 2)
            .max(12);
        lines.group_header(
            &group,
            vec![
                Span::styled(epic.key.clone(), style::heading()),
                Span::raw("  "),
                Span::styled(fit(&epic.summary, summary_w_of_header), style::heading()),
            ],
        );
        lines.meta_line(Span::raw(" "), || {
            let meta_style = style::secondary();
            let bar = epic_bar(epic, app.status_rules(), EPIC_BAR_WIDTH, meta_style);
            let mut meta = app.delight.rainbow_bar(&epic.key, bar, Instant::now());
            meta.extend(status_counts(app, epic, meta_style));
            meta
        });

        let Some(children) = &group.tickets else {
            lines.push("");
            continue;
        };

        if children.is_empty() {
            lines.note("(no related tickets)");
        } else {
            for row in children {
                lines.ticket_row(&group, row, key_w, summary_w, false, |base| {
                    vec![
                        status_cell(app, row.1, base, status_w),
                        vec![Span::styled(fit(assignee(row.1), assignee_w), muted(base))],
                    ]
                });
            }
        }

        // Blank line between epics
        lines.push("");
    }

    if lines.is_empty() {
        if app.epics_refreshing {
            lines.note("Loading epics... (assembling the relationship map)");
        } else if let Some(search) = app.search.as_ref().filter(|s| !s.is_empty()) {
            lines.note(format!("No epics match \"{}\"", search));
        } else {
            lines.note("No epics found");
        }
    }

    lines.render(f, area);
}

#[cfg(test)]
mod tests {
    use crate::app::{App, Tab};
    use crate::cache::{Epic, Ticket};
    use crate::views::common::COLUMN_GAP;
    use crate::views::tests::support::{draw, line_with};
    use ratatui::style::Color;

    /// The drawn widths of the Status, Assignee and Summary cells in `row`, the line of a ticket
    /// with no assignee ("-") and `DEMO-2` as its summary, `width` columns wide: from the dot
    /// that starts Status to the "-" that starts Assignee to the summary.
    fn columns_in(row: &str, width: u16) -> [usize; 3] {
        let cells: Vec<char> = row.chars().collect();
        let dot = cells.iter().position(|c| *c == '●').unwrap();
        let dash = (dot..cells.len())
            .find(|&at| cells[at] == '-' && cells[at - 1] == ' ')
            .unwrap();
        let summary = row[..row.rfind("DEMO-2").unwrap()].chars().count();
        [
            dash - COLUMN_GAP - (dot + 2),
            summary - COLUMN_GAP - dash,
            width as usize - summary,
        ]
    }

    /// The Epics tab drawn `width` wide with one child assigned to `assignee`, and the drawn
    /// widths of its Status, Assignee and Summary cells.
    fn epic_columns(width: u16, assignee: &str) -> (Vec<(String, Color)>, [usize; 3]) {
        epic_columns_marking(width, assignee, false)
    }

    /// `epic_columns`, with DEMO-1 marked when `marking`.
    fn epic_columns_marking(
        width: u16,
        assignee: &str,
        marking: bool,
    ) -> (Vec<(String, Color)>, [usize; 3]) {
        let mut waiting = Ticket::for_test("DEMO-1", "Waiting for Approval");
        waiting.assignee = Some(assignee.into());
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        app.cache.epics = vec![Epic {
            key: "EPIC-9".into(),
            summary: "Epic".into(),
            children: vec![waiting, Ticket::for_test("DEMO-2", "In Progress")],
        }];
        if marking {
            app.selected_ticket_keys.insert("DEMO-1".into());
        }
        let rows = draw(&app, Tab::Epics, width);
        let widths = columns_in(&line_with(&rows, "DEMO-2").unwrap().0, width);
        (rows, widths)
    }

    #[test]
    fn beside_the_detail_an_epic_is_its_header_and_its_children_keys_and_summaries() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        app.cache.epics = vec![Epic {
            key: "EPIC-9".into(),
            summary: "Epic".into(),
            children: vec![Ticket::for_test("DEMO-1", "In Progress")],
        }];
        let rows_at = |compact: Option<u16>| -> Vec<String> {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, 8)).unwrap();
            terminal
                .draw(|f| super::render(f, f.area(), &app, compact))
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..8)
                .map(|y| (0..50).map(|x| buffer[(x, y)].symbol()).collect())
                .collect()
        };
        let full = rows_at(None);
        assert!(full.iter().any(|row| row.contains("1 ticket")), "{full:?}");
        let compact = rows_at(Some(50));
        assert!(
            compact.iter().all(|row| !row.contains("ticket")),
            "{compact:?}"
        );
        let child = compact.iter().find(|row| row.contains("DEMO-1")).unwrap();
        assert_eq!(child.trim_end(), "   DEMO-1  DEMO-1", "{compact:?}");
    }

    #[test]
    fn epic_children_show_their_assignee_in_a_column_of_at_most_24() {
        let (rows, widths) = epic_columns(160, "Sam Doe");
        assert_eq!(widths[..2], [20, 8]);
        let row = |key| line_with(&rows, key).unwrap().0.clone();
        assert!(row("DEMO-1").contains("● Waiting for Approval  Sam Doe   D"));
        assert!(row("DEMO-2").contains(&format!(
            "● In Progress{}-{}D",
            " ".repeat(11),
            " ".repeat(9)
        )));

        let name = "Alexandria Montgomery-Jo";
        let (rows, widths) = epic_columns(160, name);
        assert_eq!(widths[1], 24);
        assert!(line_with(&rows, "DEMO-1")
            .unwrap()
            .0
            .contains(&format!("  {name}  D")));

        let (rows, widths) = epic_columns(160, "Alexandria Montgomery-Jon");
        assert_eq!(widths[1], 24);
        assert!(line_with(&rows, "DEMO-1")
            .unwrap()
            .0
            .contains("  Alexandria Montgomery...  D"));

        // Wide characters count by display width: 12 characters are 24 columns.
        let (_, widths) = epic_columns(160, "山田太郎山田太郎山田太郎");
        assert_eq!(widths[1], 24);
    }

    #[test]
    fn narrow_epics_trim_assignee_then_status_to_8_so_summary_keeps_18() {
        // 79 columns leave Summary exactly 18 beside Status 20 and Assignee 24.
        let name = "Alexandria Montgomery-Jo";
        assert_eq!(epic_columns(79, name).1, [20, 24, 18]);
        assert_eq!(epic_columns(78, name).1, [20, 23, 18]);
        // Assignee reaches 8 first, then Status gives up its columns.
        assert_eq!(epic_columns(52, name).1, [9, 8, 18]);
        assert_eq!(epic_columns(51, name).1, [8, 8, 18]);
        // Neither goes below 8, so Summary shrinks instead of the row overflowing.
        assert_eq!(epic_columns(50, name).1, [8, 8, 17]);
    }

    #[test]
    fn a_folded_epic_leaves_status_and_assignee_as_wide_as_the_drawn_rows() {
        let mut waiting = Ticket::for_test("DEMO-1", "Waiting for Approval");
        waiting.assignee = Some("Alexandria Montgomery-Jo".into());
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        app.cache.epics = vec![
            Epic {
                key: "EPIC-1".into(),
                summary: "Folded".into(),
                children: vec![waiting],
            },
            Epic {
                key: "EPIC-2".into(),
                summary: "Open".into(),
                children: vec![Ticket::for_test("DEMO-2", "Open")],
            },
        ];
        app.toggle_group_collapse("EPIC-1");
        let rows = draw(&app, Tab::Epics, 160);
        // "STATUS" and "ASSIGNEE" are the narrowest the cells go, and the folded epic's longer
        // values don't count.
        assert_eq!(
            columns_in(&line_with(&rows, "DEMO-2").unwrap().0, 160)[..2],
            [6, 8]
        );
        assert!(line_with(&rows, "DEMO-1").is_none());
    }

    #[test]
    fn marking_moves_every_epics_boundary_two_columns() {
        let name = "Alexandria Montgomery-Jo";
        let widths = |width| epic_columns_marking(width, name, true).1;
        assert_eq!(widths(81), [20, 24, 18]);
        assert_eq!(widths(80), [20, 23, 18]);
        assert_eq!(widths(54), [9, 8, 18]);
        assert_eq!(widths(53), [8, 8, 18]);
        assert_eq!(widths(52), [8, 8, 17]);
    }
}
