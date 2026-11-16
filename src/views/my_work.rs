use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::app::App;
use crate::style;
use crate::views::common::{key_width, Columns, ListLines, Shared, COLUMN_GAP, UPDATED_WIDTH};

/// (summary, epic, labels) widths: Summary takes what the other cells leave, and gives up columns
/// to Epic and Labels (down to 12) on narrow terminals, those to their floors only after it.
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
/// search or status focus hiding any), otherwise "No tickets found".
fn empty_lines(app: &App) -> Vec<Line<'static>> {
    let rules = app.status_rules();
    let all_done = app
        .cache
        .my_tickets
        .iter()
        .all(|t| rules.is_done(&t.status));
    let scene = (all_done && app.search.is_none() && app.status_focus.is_none())
        .then(|| app.delight.inbox_zero())
        .flatten();
    let Some((art, caption)) = scene else {
        return vec![Line::from(Span::styled(
            "  No tickets found",
            style::secondary(),
        ))];
    };
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
    lines
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let grouped = app.my_work_visible_by_status();
    let shared = Shared::of(&grouped, Columns::EpicAndLabels);
    let key_w = key_width(&grouped);
    let mut lines = ListLines::new(app);
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
        lines.extend(empty_lines(app));
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
