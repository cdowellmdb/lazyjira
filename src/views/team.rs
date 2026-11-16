use ratatui::layout::Rect;
use ratatui::text::Span;

use crate::app::App;
use crate::style;
use crate::views::common::{
    key_width, muted, narrow_for_summary, status_cell, status_width, tint, Columns, ListLines,
    Shared, COLUMN_GAP, STATUS_DOT_WIDTH, UPDATED_WIDTH,
};

/// Summary takes what Status, Epic, Labels and the fixed cells leave; on narrow terminals Epic,
/// then Labels, then Status give up columns so it keeps 14.
fn team_column_widths(
    area: Rect,
    lead: usize,
    key_w: usize,
    mut status_w: usize,
) -> (usize, usize, usize, usize) {
    let (mut epic_w, mut labels_w) = (20, 18);
    let fixed = lead + key_w + 5 * COLUMN_GAP + STATUS_DOT_WIDTH + UPDATED_WIDTH;
    let room = (area.width as usize).saturating_sub(fixed);
    let columns = &mut [(&mut epic_w, 10), (&mut labels_w, 8), (&mut status_w, 8)];
    let summary_w = narrow_for_summary(room, 14, columns);
    (status_w, summary_w, epic_w, labels_w)
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let members = app.team_visible_tickets_by_member();
    let shared = Shared::of(&members, Columns::EpicAndLabels);
    let key_w = key_width(&members);
    let mut lines = ListLines::new(app);
    let ages = lines.ages();
    let (status_w, summary_w, epic_w, labels_w) =
        team_column_widths(area, lines.lead_width(), key_w, status_width(&members));
    let summary_w = shared.summary_width(summary_w, epic_w, labels_w);

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

    #[test]
    fn narrow_team_cuts_epic_then_labels_then_status_so_summary_keeps_14() {
        // (status, summary, epic, labels) for a 20-column status.
        let widths = |width| team_column_widths(Rect::new(0, 0, width, 10), GUTTER, 8, 20);
        assert_eq!(widths(100), (20, 15, 20, 18));
        assert_eq!(widths(99), (20, 14, 20, 18));
        assert_eq!(widths(98), (20, 14, 19, 18));
        assert_eq!(widths(89), (20, 14, 10, 18));
        assert_eq!(widths(88), (20, 14, 10, 17));
        assert_eq!(widths(79), (20, 14, 10, 8));
        assert_eq!(widths(78), (19, 14, 10, 8));
        assert_eq!(widths(67), (8, 14, 10, 8));
        assert_eq!(widths(66), (8, 13, 10, 8));
    }

    #[test]
    fn marking_moves_every_team_boundary_two_columns() {
        let widths =
            |width| team_column_widths(Rect::new(0, 0, width, 10), GUTTER + CHECKBOX_WIDTH, 8, 20);
        assert_eq!(widths(102), (20, 15, 20, 18));
        assert_eq!(widths(101), (20, 14, 20, 18));
        assert_eq!(widths(100), (20, 14, 19, 18));
        assert_eq!(widths(91), (20, 14, 10, 18));
        assert_eq!(widths(90), (20, 14, 10, 17));
        assert_eq!(widths(81), (20, 14, 10, 8));
        assert_eq!(widths(80), (19, 14, 10, 8));
        assert_eq!(widths(69), (8, 14, 10, 8));
        assert_eq!(widths(68), (8, 13, 10, 8));
    }
}
