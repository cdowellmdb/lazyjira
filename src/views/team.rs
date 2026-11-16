use ratatui::layout::Rect;
use ratatui::text::Span;

use crate::app::App;
use crate::style;
use crate::views::common::{
    cells_width, key_width, lead_width, muted, narrow_for_summary, status_cell, status_width, tint,
    Columns, ListLines, Shared, COLUMN_GAP, STATUS_DOT_WIDTH, UPDATED_WIDTH,
};

/// (status, summary, epic, labels) widths: Epic (20) and Labels (18) are drawn only where Summary
/// keeps `SUMMARY_MIN` beside them and the full Status, Labels dropped first, judged as if no
/// ticket were marked so that marking never adds or drops a column. Then Status gives up columns,
/// down to 8, so Summary keeps 14.
fn team_column_widths(
    area: Rect,
    lead: usize,
    key_w: usize,
    mut status_w: usize,
    shared: &Shared,
) -> (usize, usize, usize, usize) {
    let width = area.width as usize;
    let fixed = key_w + 3 * COLUMN_GAP + STATUS_DOT_WIDTH + UPDATED_WIDTH;
    let unmarked_room = width.saturating_sub(lead_width(false) + fixed + status_w);
    let (epic_w, labels_w) = shared.trailing_widths(unmarked_room, 20, 18);
    let room = width
        .saturating_sub(lead + fixed)
        .saturating_sub(cells_width(epic_w, labels_w));
    let summary_w = narrow_for_summary(room, 14, &mut [(&mut status_w, 8)]);
    (status_w, summary_w, epic_w, labels_w)
}

/// `compact` is the list's width beside the detail pane, which shows only keys and summaries.
pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App, compact: Option<u16>) {
    let members = app.team_visible_tickets_by_member();
    let shared = Shared::of(&members, Columns::EpicAndLabels);
    let key_w = key_width(&members);
    let mut lines = ListLines::new(app).compact(compact);
    let ages = lines.ages();
    let (status_w, summary_w, epic_w, labels_w) = team_column_widths(
        area,
        lines.lead_width(),
        key_w,
        status_width(&members),
        &shared,
    );

    if !members.is_empty() {
        lines.intro(Some(&shared), area.width as usize);
    }

    for group in members {
        let (member, active_count) = group.header;
        let done_count = group.total - active_count;
        let header_muted = muted(tint(lines.is_selected(group.index)));
        lines.group_header(
            &group,
            vec![
                Span::styled(member.name.clone(), style::heading()),
                Span::styled(
                    format!("  active {active_count} · done {done_count}"),
                    header_muted,
                ),
            ],
        );

        let Some(tickets) = &group.tickets else {
            lines.push("");
            continue;
        };
        // Folded sub-tasks have no row, so the split is by status rather than by `active_count`.
        let split =
            tickets.partition_point(|(_, ticket)| !app.status_rules().is_done(&ticket.status));
        let (active, done) = tickets.split_at(split);

        if active.is_empty() && done.is_empty() {
            lines.note("(no tickets)");
        }
        for (rows, faded) in [(active, false), (done, true)] {
            if faded && !rows.is_empty() {
                lines.note(format!("done {done_count}"));
            }
            for row in rows {
                let (line, base) = lines.ticket_row(&group, row, key_w, summary_w, faded, |base| {
                    vec![status_cell(app, row.1, base, status_w)]
                });
                shared.push_trailing_cells(ages, line, row.1, base, epic_w, labels_w);
            }
        }

        // Blank line between members
        lines.push("");
    }

    if lines.is_empty() {
        lines.note("No team members");
    }

    lines.render(f, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::common::{CHECKBOX_WIDTH, GUTTER};

    /// (status, summary) of a 20 column status and 6 column keys when Epic and Labels are shared
    /// by every row, so only Status gives up columns.
    fn widths(width: u16, lead: usize) -> (usize, usize) {
        let shared = Shared::of::<()>(&[], Columns::EpicAndLabels);
        let (status, summary, epic, labels) =
            team_column_widths(Rect::new(0, 0, width, 10), lead, 6, 20, &shared);
        assert_eq!((epic, labels), (0, 0));
        (status, summary)
    }

    #[test]
    fn narrow_team_cuts_status_to_8_so_summary_keeps_14() {
        assert_eq!(widths(56, GUTTER), (20, 15));
        assert_eq!(widths(55, GUTTER), (20, 14));
        assert_eq!(widths(54, GUTTER), (19, 14));
        assert_eq!(widths(43, GUTTER), (8, 14));
        assert_eq!(widths(42, GUTTER), (8, 13));
    }

    #[test]
    fn marking_moves_every_team_boundary_two_columns() {
        let lead = GUTTER + CHECKBOX_WIDTH;
        assert_eq!(widths(58, lead), (20, 15));
        assert_eq!(widths(57, lead), (20, 14));
        assert_eq!(widths(56, lead), (19, 14));
        assert_eq!(widths(45, lead), (8, 14));
        assert_eq!(widths(44, lead), (8, 13));
    }
}
