use ratatui::layout::Rect;
use ratatui::text::Span;

use crate::app::App;
use crate::style;
use crate::views::common::{
    fit, fold_indicator, group_marker, key_width, lead_width, muted, narrow_for_summary,
    status_cell, status_width, ticket_cells, ticket_marker, tint, Ages, Columns, ListLines, Shared,
    GAP, STATUS_DOT_WIDTH, UPDATED_WIDTH,
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
    let fixed = lead + key_w + 3 * GAP + STATUS_DOT_WIDTH + UPDATED_WIDTH;
    let room = (area.width as usize).saturating_sub(fixed);
    let summary_w = narrow_for_summary(room, 18, &mut [(&mut status_w, 8)]);
    (status_w, summary_w)
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let grouped = app.unassigned_visible_by_epic();
    let shared = Shared::of(&grouped, Columns::GroupedByEpic);
    let ages = Ages::of(app);
    let key_w = key_width(&grouped);
    let lead = lead_width(app);
    let (status_w, summary_w) = ticket_column_widths(area, lead, key_w, status_width(&grouped));

    let mut lines = ListLines::new(app);
    if !grouped.is_empty() {
        lines.intro(shared.line(area.width as usize));
    }

    for group in grouped {
        let (epic_key, epic_summary) = &group.header;
        let is_header_selected = group.index == app.selected_index;

        let name = if epic_key == NO_EPIC_KEY {
            "No Epic".to_string()
        } else {
            format!("{epic_key}  {epic_summary}")
        };
        lines.header(
            group.index,
            is_header_selected,
            Span::styled(
                fold_indicator(group.tickets.is_none()),
                muted(tint(is_header_selected)),
            ),
            group_marker(app.group_selection_state(epic_key)),
            vec![Span::styled(name, style::heading())],
        );
        lines.push(Span::styled(
            format!("{}unassigned: {}", " ".repeat(lead), group.total),
            style::secondary(),
        ));

        let Some(tickets) = &group.tickets else {
            lines.push("");
            continue;
        };

        for (index, ticket) in tickets {
            let is_selected = *index == app.selected_index;
            let (base, primary) = ages.row_styles(ticket, is_selected, false);
            let (key_cell, summary) = ticket_cells(
                app,
                group.family.get(index).copied(),
                ticket,
                summary_w,
                primary,
            );

            let cells = vec![
                vec![Span::styled(fit(&key_cell, key_w), primary)],
                status_cell(app, ticket, base, status_w),
            ];
            let mark = ticket_marker(app.is_ticket_selected(&ticket.key));
            let row = lines.ticket(*index, is_selected, mark, cells, base, summary);
            shared.push_trailing_cells(ages, row, ticket, base, 0, 0);
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

    lines.render(f, area, app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::common::GUTTER;

    #[test]
    fn narrow_unassigned_cuts_status_to_8_so_summary_keeps_18() {
        let widths = |width| ticket_column_widths(Rect::new(0, 0, width, 10), GUTTER, 8, 20);
        assert_eq!(widths(62), (20, 19));
        assert_eq!(widths(61), (20, 18));
        assert_eq!(widths(60), (19, 18));
        assert_eq!(widths(49), (8, 18));
        assert_eq!(widths(48), (8, 17));
    }
}
