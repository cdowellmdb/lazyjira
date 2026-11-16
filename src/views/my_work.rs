use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::style;
use crate::views::common::{key_width, Columns, ListLines, Shared, COLUMN_GAP, UPDATED_WIDTH};

/// (summary, epic, labels) widths: Summary takes what the other cells leave. On a narrow terminal
/// it gives up columns first, down to 16, then Epic gives up its down to 12, then Labels its down
/// to 10, and last Summary goes on down to 12. Below that a row is wider than the terminal.
fn my_work_column_widths(area: Rect, lead: usize, key_w: usize) -> (usize, usize, usize) {
    let mut summary_w = 34usize;
    let mut epic_w = 24usize;
    let mut labels_w = 22usize;
    let inner = area.width as usize;
    let fixed = lead + key_w + 4 * COLUMN_GAP + UPDATED_WIDTH;
    let mut overflow = fixed + summary_w + epic_w + labels_w;

    if overflow > inner {
        let mut to_trim = overflow - inner;
        if summary_w > 16 {
            let cut = to_trim.min(summary_w - 16);
            summary_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && epic_w > 12 {
            let cut = to_trim.min(epic_w - 12);
            epic_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && labels_w > 10 {
            let cut = to_trim.min(labels_w - 10);
            labels_w -= cut;
            to_trim -= cut;
        }
        if to_trim > 0 && summary_w > 12 {
            let cut = to_trim.min(summary_w - 12);
            summary_w -= cut;
        }
    }

    overflow = fixed + summary_w + epic_w + labels_w;
    if overflow < inner {
        summary_w += inner - overflow;
    }

    (summary_w, epic_w, labels_w)
}

/// What My Work shows with no rows: a scene when nothing is left to do (no active ticket, and no
/// search or status focus hiding any), otherwise none, and the list says "No tickets found".
fn empty_scene(app: &App) -> Option<Vec<Line<'static>>> {
    let rules = app.status_rules();
    let all_done = app
        .cache
        .my_tickets
        .iter()
        .all(|t| rules.is_done(&t.status));
    let scene = (all_done && app.search.is_none() && app.status_focus.is_none())
        .then(|| app.delight.inbox_zero())
        .flatten();
    let (art, caption) = scene?;
    let mut lines = vec![Line::from("")];
    lines.extend(
        art.iter()
            .map(|row| Line::from(Span::styled(format!("    {row}"), style::accent()))),
    );
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("    {caption}"),
        style::warning().add_modifier(Modifier::BOLD),
    )));
    Some(lines)
}

/// `compact` is the list's width beside the detail pane, which shows only keys and summaries.
pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App, compact: Option<u16>) {
    let grouped = app.my_work_visible_by_status();
    let shared = Shared::of(&grouped, Columns::EpicAndLabels);
    let key_w = key_width(&grouped);
    let mut lines = ListLines::new(app).compact(compact);
    let ages = lines.ages();
    let (summary_w, epic_w, labels_w) = my_work_column_widths(area, lines.lead_width(), key_w);
    let summary_w = shared.summary_width(summary_w, epic_w, labels_w);

    if !grouped.is_empty() {
        lines.intro(Some(&shared), area.width as usize);
    }

    for group in &grouped {
        lines.status_header(group);

        let Some(tickets) = &group.tickets else {
            lines.push("");
            continue;
        };

        for row in tickets {
            let (line, base) = lines.ticket_row(group, row, key_w, summary_w, false, |_| vec![]);
            shared.push_trailing_cells(ages, line, row.1, base, epic_w, labels_w);
        }

        // Blank line between groups
        lines.push("");
    }

    if lines.is_empty() {
        match empty_scene(app) {
            Some(scene) => lines.extend(scene),
            None => lines.note("No tickets found"),
        }
    }

    let hidden_done = app.hidden_done_count();
    if hidden_done > 0 {
        lines.glyph_line(
            Span::styled("●", style::status("Done", app.status_rules())),
            vec![
                Span::styled("Done", style::heading()),
                Span::styled(format!(" {hidden_done}  (d to show)"), style::secondary()),
            ],
        );
    }

    lines.render(f, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::common::{CHECKBOX_WIDTH, GUTTER};

    /// Lead 3, Key 8: the cells beside the three columns take 23, so their 34, 24 and 22 want 103.
    fn widths(width: u16, lead: usize) -> (usize, usize, usize) {
        my_work_column_widths(Rect::new(0, 0, width, 10), lead, 8)
    }

    #[test]
    fn narrow_my_work_trims_summary_to_16_then_epic_to_12_then_labels_to_10_then_summary_to_12() {
        for (width, expected) in [
            // Wider than the columns want: Summary takes the rest.
            (104, (35, 24, 22)),
            (103, (34, 24, 22)),
            // One cell narrower: Summary gives it up, and goes on to 16.
            (102, (33, 24, 22)),
            (86, (17, 24, 22)),
            (85, (16, 24, 22)),
            // Epic next, to 12.
            (84, (16, 23, 22)),
            (73, (16, 12, 22)),
            // Then Labels, to 10.
            (72, (16, 12, 21)),
            (61, (16, 12, 10)),
            // Then Summary again, to 12.
            (60, (15, 12, 10)),
            (58, (13, 12, 10)),
            (57, (12, 12, 10)),
            // Nothing is left to give: the row is wider than the terminal.
            (56, (12, 12, 10)),
            (0, (12, 12, 10)),
        ] {
            assert_eq!(widths(width, GUTTER), expected, "{width} columns");
        }
    }

    #[test]
    fn marking_moves_each_my_work_boundary_two_columns() {
        let lead = GUTTER + CHECKBOX_WIDTH;
        for (width, expected) in [
            (105, (34, 24, 22)),
            (104, (33, 24, 22)),
            (87, (16, 24, 22)),
            (86, (16, 23, 22)),
            (75, (16, 12, 22)),
            (74, (16, 12, 21)),
            (63, (16, 12, 10)),
            (62, (15, 12, 10)),
            (60, (13, 12, 10)),
            (59, (12, 12, 10)),
            (58, (12, 12, 10)),
        ] {
            assert_eq!(widths(width, lead), expected, "{width} columns");
        }
    }
}
