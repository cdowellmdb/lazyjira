use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::style;
use crate::views::common::{
    cells_width, key_width, lead_width, Columns, ListLines, Shared, COLUMN_GAP, UPDATED_WIDTH,
};

/// The least Summary keeps on a terminal too narrow for it, the row running off the edge.
const SUMMARY_FLOOR: usize = 12;

/// (summary, epic, labels) widths: Epic (24) and Labels (22) are drawn only where Summary keeps
/// `SUMMARY_MIN` beside them, Labels dropped first, judged as if no ticket were marked so that
/// marking never adds or drops a column. Summary takes the rest, and never less than
/// `SUMMARY_FLOOR`.
pub(super) fn my_work_column_widths(
    area: Rect,
    lead: usize,
    key_w: usize,
    shared: &Shared,
) -> (usize, usize, usize) {
    let width = area.width as usize;
    let fixed = key_w + 2 * COLUMN_GAP + UPDATED_WIDTH;
    let unmarked_room = width.saturating_sub(lead_width(false) + fixed);
    let (epic_w, labels_w) = shared.trailing_widths(unmarked_room, 24, 22);
    let room = width.saturating_sub(lead + fixed);
    (
        room.saturating_sub(cells_width(epic_w, labels_w))
            .max(SUMMARY_FLOOR),
        epic_w,
        labels_w,
    )
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
    let (summary_w, epic_w, labels_w) =
        my_work_column_widths(area, lines.lead_width(), key_w, &shared);

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
