use ratatui::layout::Rect;
use ratatui::text::Span;

use crate::app::App;
use crate::style;
use crate::views::common::{
    key_width, narrow_for_summary, status_cell, status_width, Columns, ListLines, Shared,
    COLUMN_GAP, STATUS_DOT_WIDTH, UPDATED_WIDTH,
};

const NO_EPIC_KEY: &str = "NO-EPIC";

/// Summary takes what Status and the fixed cells leave; on narrow terminals Status gives up
/// columns, down to 8, so it keeps 18.
fn ticket_column_widths(
    area: Rect,
    lead: usize,
    key_w: usize,
    mut status_w: usize,
) -> (usize, usize) {
    let fixed = lead + key_w + 3 * COLUMN_GAP + STATUS_DOT_WIDTH + UPDATED_WIDTH;
    let room = (area.width as usize).saturating_sub(fixed);
    let summary_w = narrow_for_summary(room, 18, &mut [(&mut status_w, 8)]);
    (status_w, summary_w)
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let grouped = app.unassigned_visible_by_epic();
    let shared = Shared::of(&grouped, Columns::GroupedByEpic);
    let key_w = key_width(&grouped);
    let mut lines = ListLines::new(app);
    let ages = lines.ages();
    let (status_w, summary_w) =
        ticket_column_widths(area, lines.lead_width(), key_w, status_width(&grouped));

    if !grouped.is_empty() {
        lines.intro(Some(&shared), area.width as usize);
    }

    for group in grouped {
        let (epic_key, epic_summary) = &group.header;

        let name = if epic_key == NO_EPIC_KEY {
            "No Epic".to_string()
        } else {
            format!("{epic_key}  {epic_summary}")
        };
        lines.group_header(&group, vec![Span::styled(name, style::heading())]);
        lines.note(format!("unassigned: {}", group.total));

        let Some(tickets) = &group.tickets else {
            lines.push("");
            continue;
        };

        for row in tickets {
            let (line, base) = lines.ticket_row(&group, row, key_w, summary_w, false, |base| {
                vec![status_cell(app, row.1, base, status_w)]
            });
            shared.push_trailing_cells(ages, line, row.1, base, 0, 0);
        }

        lines.push("");
    }

    if lines.is_empty() {
        let empty_text = if let Some(search) = app.search.as_ref().filter(|s| !s.is_empty()) {
            format!("  No unassigned tickets match \"{}\"", search)
        } else {
            "  No unassigned tickets found".to_string()
        };
        lines.push(Span::styled(empty_text, style::secondary()));
    }

    lines.render(f, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::common::{CHECKBOX_WIDTH, GUTTER};

    #[test]
    fn narrow_unassigned_cuts_status_to_8_so_summary_keeps_18() {
        let widths = |width| ticket_column_widths(Rect::new(0, 0, width, 10), GUTTER, 8, 20);
        assert_eq!(widths(62), (20, 19));
        assert_eq!(widths(61), (20, 18));
        assert_eq!(widths(60), (19, 18));
        assert_eq!(widths(49), (8, 18));
        assert_eq!(widths(48), (8, 17));
    }

    #[test]
    fn marking_moves_every_unassigned_boundary_two_columns() {
        let widths = |width| {
            ticket_column_widths(Rect::new(0, 0, width, 10), GUTTER + CHECKBOX_WIDTH, 8, 20)
        };
        assert_eq!(widths(64), (20, 19));
        assert_eq!(widths(63), (20, 18));
        assert_eq!(widths(62), (19, 18));
        assert_eq!(widths(51), (8, 18));
        assert_eq!(widths(50), (8, 17));
    }
}
