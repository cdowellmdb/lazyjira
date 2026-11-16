use std::ops::Range;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, GroupSelectionState, Tab, VisibleGroup};
use crate::cache::{Epic, Status, StatusKind, StatusRules, Ticket};
use crate::delight::Delight;
use crate::style;
use crate::subtasks::Family;
use unicode_width::UnicodeWidthStr;

/// Cells before a row's content: a margin, the `›` marker (a header's glyph), and a space.
pub const GUTTER: usize = 3;
/// The column in the gutter of a row's `›` and a header's dot or fold arrow.
pub const GLYPH_COLUMN: u16 = 1;
/// A row's checkbox and the space after it, drawn only while tickets are marked.
pub const CHECKBOX_WIDTH: usize = 2;
/// Blank cells between two columns.
pub const COLUMN_GAP: usize = 2;
/// A Status cell's dot and the space after it.
pub const STATUS_DOT_WIDTH: usize = 2;
/// The `└ ` before a sub-task's key, and the two cells of indent before it.
const CHILD_PREFIX: &str = "  └ ";

/// The arrow of a folded group or parent, and of an open one.
const FOLDED: &str = "▶";
const OPEN: &str = "▼";

pub fn fold_indicator(collapsed: bool) -> &'static str {
    if collapsed {
        FOLDED
    } else {
        OPEN
    }
}

/// The columns before a row's key: the gutter, and the checkbox while tickets are `marking`.
pub fn lead_width(marking: bool) -> usize {
    GUTTER + if marking { CHECKBOX_WIDTH } else { 0 }
}

/// The `GUTTER` cells of a line: `glyph` in `GLYPH_COLUMN`, blanks around it.
pub fn gutter(glyph: Span<'static>) -> Vec<Span<'static>> {
    let margin = GLYPH_COLUMN as usize;
    vec![
        Span::raw(" ".repeat(margin)),
        glyph,
        Span::raw(" ".repeat(GUTTER - margin - 1)),
    ]
}

/// Tints the selected item's `lines`, each to `width`.
pub fn highlight_row(lines: &mut [Line<'_>], selected: Option<Range<usize>>, width: u16) {
    let Some(selected) = selected else {
        return;
    };
    for line in &mut lines[selected] {
        line.spans.push(Span::raw(
            " ".repeat((width as usize).saturating_sub(line.width())),
        ));
        line.style = line.style.patch(style::selected_row());
    }
}

/// Cells in an epic's progress bar, in the Epics headers and the epic detail.
pub const EPIC_BAR_WIDTH: usize = 24;

/// A cell of an epic's progress bar for a done child; `Delight::rainbow_bar` recolors them.
pub const DONE_CELL: &str = "█";

/// An epic's progress bar, `width` cells: a run per status in `Epic::status_counts`' order, as
/// wide as its share of the children (see `share_cells`), `DONE_CELL` when the status is done,
/// `░` when it's dropped and `▒` when it's active, in the status's color. An epic with no
/// children draws `·` in `style`.
pub fn epic_bar(
    epic: &Epic,
    rules: &StatusRules,
    width: usize,
    style: Style,
) -> Vec<Span<'static>> {
    let counts = epic.status_counts(rules);
    let sizes: Vec<usize> = counts.iter().map(|(_, count)| *count).collect();
    let mut spans = Vec::new();
    if counts.is_empty() {
        spans.push(Span::styled("·".repeat(width), style));
    }
    for ((status, _), cells) in counts.iter().zip(share_cells(&sizes, width)) {
        let mark = match rules.kind(status) {
            StatusKind::Done => DONE_CELL,
            StatusKind::Active => "▒",
            StatusKind::Dropped => "░",
        };
        spans.push(Span::styled(
            mark.repeat(cells),
            style.fg(style::status_color(status, rules)),
        ));
    }
    spans
}

/// Splits `width` cells between `counts` in proportion, the cells left after rounding down going
/// to the largest remainders (earliest first), so they sum to `width`. A nonzero count with no
/// cell then takes one from the widest run, while that run has more than one.
fn share_cells(counts: &[usize], width: usize) -> Vec<usize> {
    let total: usize = counts.iter().sum();
    if total == 0 {
        return vec![0; counts.len()];
    }
    let mut cells: Vec<usize> = counts.iter().map(|count| count * width / total).collect();
    let mut by_remainder: Vec<usize> = (0..counts.len()).collect();
    by_remainder.sort_by_key(|&at| std::cmp::Reverse(counts[at] * width % total));
    let left = width - cells.iter().sum::<usize>();
    for &at in by_remainder.iter().take(left) {
        cells[at] += 1;
    }
    for at in 0..counts.len() {
        if counts[at] > 0 && cells[at] == 0 {
            let widest = (0..cells.len()).max_by_key(|&run| cells[run]);
            if let Some(widest) = widest.filter(|&run| cells[run] > 1) {
                cells[widest] -= 1;
                cells[at] += 1;
            }
        }
    }
    cells
}

/// `s` cut to at most `max` display columns, ending "..." when cut. Widths are measured on whole
/// prefixes, as `fit` and the columns measure them: an emoji with its variation selector is two
/// columns together but one per character.
pub fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let ellipsis = &"..."[..max.min(3)];
    let room = max - ellipsis.len();
    // ponytail: re-measures each prefix, quadratic in the cut length (a few thousand width
    // lookups per cut cell); keep a running width if long lists ever redraw slowly.
    let end = s
        .char_indices()
        .map(|(at, c)| at + c.len_utf8())
        .take_while(|&end| s[..end].width() <= room)
        .last()
        .unwrap_or(0);
    s[..end].to_string() + ellipsis
}

/// `text` cut and padded to exactly `width` display columns, so a cell holding wide characters
/// keeps the columns after it in line.
pub fn fit(text: &str, width: usize) -> String {
    let cell = truncate(text, width);
    let pad = width.saturating_sub(cell.width());
    cell + &" ".repeat(pad)
}

/// Summary's width in `room` columns beside `columns`: when Summary would get fewer than
/// `min_summary`, each column in turn gives up columns, down to its floor, until Summary has them.
/// A terminal too narrow even then leaves Summary what is left.
pub fn narrow_for_summary(
    room: usize,
    min_summary: usize,
    columns: &mut [(&mut usize, usize)],
) -> usize {
    let mut used: usize = columns.iter().map(|(width, _)| **width).sum();
    for (width, floor) in columns.iter_mut() {
        let cut = (used + min_summary)
            .saturating_sub(room)
            .min(width.saturating_sub(*floor));
        **width -= cut;
        used -= cut;
    }
    room.saturating_sub(used)
}

/// The display width of the widest `value` among the rows `groups` draw, or of `heading` when
/// that is wider: a column as wide as what is on screen.
pub fn column_width<H>(
    groups: &[VisibleGroup<'_, H>],
    heading: &str,
    value: fn(&Ticket) -> &str,
) -> usize {
    groups
        .iter()
        .flat_map(|group| group.tickets.iter().flatten())
        .map(|(_, ticket)| value(ticket).width())
        .fold(heading.width(), usize::max)
}

/// The Status column's width: the longest status drawn.
pub fn status_width<H>(groups: &[VisibleGroup<'_, H>]) -> usize {
    column_width(groups, "STATUS", |ticket| ticket.status.as_str())
}

const UNSELECTED: char = '☐';
const SELECTED: char = '☒';
const PARTIAL: char = '⊟';
/// The selection marks, each one column wide (East Asian Width "N"): unselected, selected, and a
/// group with some of its tickets selected.
#[cfg(test)]
pub const MARKS: [char; 3] = [UNSELECTED, SELECTED, PARTIAL];

pub fn ticket_marker(selected: bool) -> char {
    if selected {
        SELECTED
    } else {
        UNSELECTED
    }
}

pub fn group_marker(state: GroupSelectionState) -> char {
    match state {
        GroupSelectionState::None => UNSELECTED,
        GroupSelectionState::Partial => PARTIAL,
        GroupSelectionState::All => SELECTED,
    }
}

/// `base` in the muted color: the subtle one on the selected row (`base` has its background), so
/// it shows against the highlight, and not bold.
pub fn muted(base: Style) -> Style {
    base.remove_modifier(Modifier::BOLD)
        .patch(if base.bg.is_some() {
            style::subtle()
        } else {
            style::secondary()
        })
}

/// The base style of a row or header: the selected row's tint, or the plain foreground.
pub fn tint(selected: bool) -> Style {
    if selected {
        style::selected_row()
    } else {
        style::primary()
    }
}

/// A selection mark: muted when unselected, the accent when selected or partial.
fn mark_span(mark: char, row: Style) -> Span<'static> {
    Span::styled(
        mark.to_string(),
        if mark == UNSELECTED {
            muted(row)
        } else {
            row.patch(style::accent())
        },
    )
}

/// A ticket row's key cell and the column in it of its fold arrow, if it has one.
pub struct KeyCell {
    pub text: String,
    pub fold: Option<usize>,
}

/// A ticket row's key cell: a sub-task under its parent leads with `└` and two cells of indent,
/// and a parent ends with the arrow that says whether its sub-tasks are `folded`.
fn key_cell(family: Option<Family>, key: &str, folded: bool) -> KeyCell {
    let (text, fold) = match family {
        Some(Family::Child) => (format!("{CHILD_PREFIX}{key}"), None),
        Some(Family::Parent(_)) => (
            format!("{key} {}", fold_indicator(folded)),
            Some(key.width() + 1),
        ),
        None => (key.to_string(), None),
    };
    KeyCell { text, fold }
}

/// The Key column's width: the widest key cell among the rows `groups` draw.
pub fn key_width<H>(groups: &[VisibleGroup<'_, H>]) -> usize {
    groups
        .iter()
        .flat_map(|group| {
            group.tickets.iter().flatten().map(move |(index, ticket)| {
                key_cell(group.family.get(index).copied(), &ticket.key, false)
                    .text
                    .width()
            })
        })
        .max()
        .unwrap_or(0)
}

/// A ticket row's key and summary cells, given its place in a parent's family. A parent shows
/// whether its sub-tasks are folded, with how many are hidden. A sub-task whose parent is
/// elsewhere leads its summary with the parent's key, muted so the summary reads first. The
/// summary is drawn in `base`, one line truncated and padded to `width`, or, with
/// `App::wrap_summaries`, word-wrapped onto as many lines as it needs, each padded to `width`.
pub fn ticket_cells(
    app: &App,
    family: Option<Family>,
    ticket: &Ticket,
    width: usize,
    base: Style,
) -> (KeyCell, Vec<Vec<Span<'static>>>) {
    let folded = app.is_parent_folded(&ticket.key);
    let key = key_cell(family, &ticket.key, folded);
    let (prefix, summary) = match (family, &ticket.parent_key) {
        (Some(Family::Parent(count)), _) if folded => (
            String::new(),
            format!(
                "({} sub-task{}) {}",
                count,
                if count == 1 { "" } else { "s" },
                ticket.summary
            ),
        ),
        (None, Some(parent)) => (format!("{} › ", parent), ticket.summary.clone()),
        _ => (String::new(), ticket.summary.clone()),
    };
    // Under two columns a wide character can't fit a line, so wrapping would overflow the cell.
    // A terminal too narrow for Summary's minimum leaves it less (`narrow_for_summary`).
    if app.wrap_summaries && width >= 2 {
        let content = vec![
            Span::styled(prefix, muted(base)),
            Span::styled(summary, base),
        ];
        let lines = crate::widgets::markup::wrap(vec![], vec![], content, width)
            .into_iter()
            .map(|line| {
                let pad = width.saturating_sub(line.width());
                let mut spans = line.spans;
                spans.push(Span::styled(" ".repeat(pad), base));
                spans
            })
            .collect();
        return (key, lines);
    }
    let cell = fit(&(prefix.clone() + &summary), width);
    let split = cell
        .char_indices()
        .nth(prefix.chars().count())
        .map_or(cell.len(), |(i, _)| i);
    let (prefix, summary) = cell.split_at(split);
    (
        key,
        vec![vec![
            Span::styled(prefix.to_string(), muted(base)),
            Span::styled(summary.to_string(), base),
        ]],
    )
}

/// A row's Status cell: a dot in the status's color, then its name muted, `width` columns of it.
pub fn status_cell(app: &App, ticket: &Ticket, base: Style, width: usize) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!("{:<width$}", "●", width = STATUS_DOT_WIDTH),
            style::status(&ticket.status, app.status_rules()),
        ),
        Span::styled(fit(status_label(app, &ticket.status), width), muted(base)),
    ]
}

/// A list tab's lines and the items among them: each group header and ticket row, with the lines
/// it takes, so `render` can highlight, scroll to and register mouse targets for whole items.
pub struct ListLines<'a> {
    app: &'a App,
    ages: Ages<'a>,
    lines: Vec<Line<'static>>,
    items: Vec<crate::mouse::Item>,
    selected_lines: Option<Range<usize>>,
    /// Whether the list has the selection: a pane without focus draws none.
    focused: bool,
    /// The list's width beside the detail pane, which shows only keys and summaries.
    compact: Option<u16>,
}

impl<'a> ListLines<'a> {
    pub fn new(app: &'a App) -> Self {
        ListLines {
            app,
            ages: Ages::of(app),
            lines: Vec::new(),
            items: Vec::new(),
            selected_lines: None,
            focused: true,
            compact: None,
        }
    }

    /// The list is `width` cells wide beside the detail pane, and draws only keys and
    /// summaries: no extra cells on a row, no meta line, and a click on a row shows its ticket.
    pub fn compact(mut self, width: Option<u16>) -> Self {
        self.compact = width;
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// The ticket ages every row measures to.
    pub fn ages(&self) -> Ages<'a> {
        self.ages
    }

    /// The columns before a row's key.
    pub fn lead_width(&self) -> usize {
        lead_width(self.app.is_marking())
    }

    /// Whether the item at `index` is the selected one, in a list that has the selection.
    pub fn is_selected(&self, index: usize) -> bool {
        self.focused && index == self.app.selected_index
    }

    /// A line that is no item: counts, blank lines, empty-state text.
    pub fn push(&mut self, line: impl Into<Line<'static>>) {
        self.lines.push(line.into());
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// A muted line among the rows, indented as they are.
    pub fn note(&mut self, text: impl Into<String>) {
        self.glyph_line(
            Span::raw(" "),
            vec![Span::styled(text.into(), style::secondary())],
        );
    }

    /// A line under a header that only a full-width list draws.
    pub fn meta_line(&mut self, glyph: Span<'static>, rest: impl FnOnce() -> Vec<Span<'static>>) {
        if self.compact.is_none() {
            self.glyph_line(glyph, rest());
        }
    }

    /// A line among the rows with `glyph` in the gutter, then `rest` after the checkbox's cells.
    pub fn glyph_line(&mut self, glyph: Span<'static>, rest: Vec<Span<'static>>) {
        let mut spans = self.lead(glyph, None);
        spans.extend(rest);
        self.lines.push(Line::from(spans));
    }

    /// The top of a list: the line `shared` has in common, if any, or a blank line, then a blank,
    /// the line cut to `width`.
    pub fn intro(&mut self, shared: Option<&Shared>, width: usize) {
        let line = shared.and_then(|shared| shared.line(self.lead_width(), width));
        self.lines.extend(line);
        self.lines.push(Line::from(""));
    }

    /// The gutter of a line: `glyph`, then the checkbox `mark` (blank for a line with none)
    /// while tickets are marked.
    fn lead(&self, glyph: Span<'static>, mark: Option<Span<'static>>) -> Vec<Span<'static>> {
        let mut lead = gutter(glyph);
        if self.app.is_marking() {
            lead.push(mark.unwrap_or_else(|| Span::raw(" ")));
            lead.push(Span::raw(" "));
        }
        lead
    }

    /// A group header, one line: `glyph` in the gutter, the group's mark while tickets are
    /// marked, then `rest`.
    fn header<H>(
        &mut self,
        group: &VisibleGroup<'_, H>,
        glyph: Span<'static>,
        rest: Vec<Span<'static>>,
    ) {
        let row = tint(self.is_selected(group.index));
        let mark = mark_span(group_marker(self.app.group_selection_state(&group.id)), row);
        let start = self.lines.len();
        let mut spans = self.lead(glyph, Some(mark));
        spans.extend(rest);
        self.lines.push(Line::from(spans));
        // An epic's header shows the epic; any other group's has nothing to show.
        let shows = self.app.active_tab == Tab::Epics;
        self.item(start, group.index, Some(GLYPH_COLUMN), shows);
    }

    /// The header of a group of one member, epic or epic key: its fold arrow in the gutter, then
    /// `rest`.
    pub fn group_header<H>(&mut self, group: &VisibleGroup<'_, H>, rest: Vec<Span<'static>>) {
        let row = tint(self.is_selected(group.index));
        let arrow = Span::styled(fold_indicator(group.tickets.is_none()), muted(row));
        self.header(group, arrow, rest);
    }

    /// A status group's header: `● Name 3`, the dot in the status's color, the name bold, the
    /// count muted. A folded group says how to open it.
    pub fn status_header(&mut self, group: &VisibleGroup<'_, String>) {
        let status = &group.header;
        let muted = muted(tint(self.is_selected(group.index)));
        let mut rest = vec![
            Span::styled(status_label(self.app, status).to_string(), style::heading()),
            Span::styled(format!(" {}", group.total), muted),
        ];
        if group.tickets.is_none() {
            rest.push(Span::styled(FOLDED_HINT, muted));
        }
        let dot = Span::styled("●", style::status(status, self.app.status_rules()));
        self.header(group, dot, rest);
    }

    /// A ticket row of `group`: the gutter (`›` when selected, and the ticket's mark while
    /// tickets are marked), the key cell fit to `key_w`, the cells `extra` makes from the row's
    /// style (Status, Assignee), then the summary's first line (from `ticket_cells`), each a
    /// `COLUMN_GAP` apart. Each further summary line goes on a line of its own, blank under the
    /// cells. A `faded` row is muted. Returns the first line, for the cells after Summary, and
    /// the row's style.
    pub fn ticket_row<H>(
        &mut self,
        group: &VisibleGroup<'_, H>,
        &(index, ticket): &(usize, &Ticket),
        key_w: usize,
        summary_w: usize,
        faded: bool,
        extra: impl FnOnce(Style) -> Vec<Vec<Span<'static>>>,
    ) -> (&mut Line<'static>, Style) {
        let selected = self.is_selected(index);
        let (base, primary) = self.ages.row_styles(ticket, selected, faded);
        let family = group.family.get(&index).copied();
        // Beside the detail pane the summary takes the width the key leaves, and no other cell
        // is drawn: what the views push after it is cut off by the pane's edge.
        let summary_w = self.compact.map_or(summary_w, |width| {
            (width as usize).saturating_sub(self.lead_width() + key_w + COLUMN_GAP)
        });
        let (key, summary) = ticket_cells(self.app, family, ticket, summary_w, primary);
        let mut cells = vec![vec![Span::styled(fit(&key.text, key_w), primary)]];
        if self.compact.is_none() {
            cells.extend(extra(base));
        }

        let glyph = if selected {
            Span::styled("›", style::accent().patch(style::heading()))
        } else {
            Span::raw(" ")
        };
        let mark = ticket_marker(self.app.is_ticket_selected(&ticket.key));
        let lead = self.lead_width();
        let fold = key.fold.map(|col| (lead + col) as u16);
        let start = self.lines.len();
        let mut row = self.lead(glyph, Some(mark_span(mark, base)));
        let gap = Span::raw(" ".repeat(COLUMN_GAP));
        let cells_width: usize =
            cells.iter().flatten().map(Span::width).sum::<usize>() + COLUMN_GAP * cells.len();
        for cell in cells {
            row.extend(cell);
            row.push(gap.clone());
        }
        let mut summary = summary.into_iter();
        row.extend(summary.next().unwrap_or_default());
        self.lines.push(Line::from(row));
        for more in summary {
            let mut line = Line::from(Span::raw(" ".repeat(lead + cells_width)));
            line.spans.extend(more);
            self.lines.push(line);
        }
        self.item(start, index, fold, true);
        (&mut self.lines[start], base)
    }

    fn item(&mut self, start: usize, index: usize, fold: Option<u16>, shows: bool) {
        let lines = start..self.lines.len();
        if self.is_selected(index) {
            self.selected_lines = Some(lines.clone());
        }
        self.items.push(crate::mouse::Item {
            lines,
            index,
            fold,
            shows,
        });
    }

    /// Highlights the selected item, scrolls so the selected item's last line is on screen (its
    /// first, when it is taller than the screen), registers mouse targets for the rows actually
    /// drawn and draws the lines in `area`.
    pub fn render(mut self, f: &mut ratatui::Frame, area: Rect) {
        highlight_row(&mut self.lines, self.selected_lines.clone(), area.width);
        let scroll = self.selected_lines.as_ref().map_or(0, |item| {
            item.end
                .saturating_sub(area.height as usize)
                .min(item.start)
        }) as u16;
        let lead = self.lead_width();
        crate::mouse::register_rows(
            self.app,
            area,
            &self.items,
            scroll,
            (lead, self.compact.is_some()),
        );
        // A drag copies a row's text, not its marker or checkbox.
        let lead = (lead as u16).min(area.width);
        crate::mouse::selectable(
            self.app,
            Rect::new(area.x + lead, area.y, area.width - lead, area.height),
        );
        f.render_widget(Paragraph::new(self.lines).scroll((scroll, 0)), area);
    }
}

impl Extend<Line<'static>> for ListLines<'_> {
    fn extend<I: IntoIterator<Item = Line<'static>>>(&mut self, lines: I) {
        self.lines.extend(lines);
    }
}

/// What `ListLines::status_header` adds to a folded group.
const FOLDED_HINT: &str = "  (z to open)";

/// The name a status cell or group header shows: Jira's, except that Blocked is Cursed in
/// spooky season.
pub fn status_label<'a>(app: &App, status: &'a str) -> &'a str {
    if app.delight.spooky() && Status::from_str(status) == Status::Blocked {
        "Cursed"
    } else {
        status
    }
}

/// The width Summary keeps before a list gives room to Epic or Labels: a summary of about seven
/// words. Below it those columns are not drawn, so an 80 column terminal shows Key, Summary and
/// Updated (and Status where a tab has it).
pub const SUMMARY_MIN: usize = 40;

/// The columns an Epic cell of `epic_w` and a Labels cell of `labels_w` take, with their gaps; a
/// width of 0 is a column not drawn and takes none.
pub fn cells_width(epic_w: usize, labels_w: usize) -> usize {
    [epic_w, labels_w]
        .into_iter()
        .filter(|&w| w > 0)
        .map(|w| COLUMN_GAP + w)
        .sum()
}

/// The columns a tab has that `Shared` may hide, which decides what its shared line names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Columns {
    /// Epic and Labels columns, each hidden while every row shares its value (My Work, Team).
    EpicAndLabels,
    /// Neither: the shared line names a shared epic and labels (Filters).
    Neither,
    /// Neither, and the groups are epics, whose headers name them, so the shared line names only
    /// labels (Unassigned).
    GroupedByEpic,
}

/// What every row a tab draws has in common, so it is shown once instead of on every row.
/// Rows with no epic don't count when deciding whether an epic is shared.
pub struct Shared {
    epic: Option<String>,
    labels: Vec<String>,
    /// Some rows' epics differ, so rows need their Epic column.
    epic_column: bool,
    /// Some row has a label not every row has, so rows need their Labels column.
    labels_column: bool,
}

impl Shared {
    /// The shared values of the rows `groups` draw, in a tab with `columns`, counting the
    /// sub-tasks of folded parents (as group totals do) but nothing in a folded group.
    pub fn of<H>(groups: &[VisibleGroup<'_, H>], columns: Columns) -> Self {
        let rows: Vec<&Ticket> = groups
            .iter()
            .flat_map(|group| {
                let drawn = group.tickets.iter().flatten().map(|(_, ticket)| *ticket);
                drawn.chain(group.folded_subtasks.iter().copied())
            })
            .collect();
        let mut epics: Vec<&str> = rows.iter().filter_map(|t| t.epic_name.as_deref()).collect();
        epics.sort_unstable();
        epics.dedup();
        let labels: Vec<String> = rows.first().map_or_else(Vec::new, |first| {
            first
                .labels
                .iter()
                .filter(|label| rows.iter().all(|t| t.labels.contains(label)))
                .cloned()
                .collect()
        });
        let has_columns = columns == Columns::EpicAndLabels;
        Shared {
            epic: (epics.len() == 1 && columns != Columns::GroupedByEpic)
                .then(|| epics[0].to_string()),
            epic_column: has_columns && epics.len() > 1,
            labels_column: has_columns
                && rows
                    .iter()
                    .any(|t| t.labels.iter().any(|label| !labels.contains(label))),
            labels,
        }
    }

    /// The Epic and Labels widths (0 for a column not drawn) a list has room for, with `room`
    /// columns for Summary and these two. A column is drawn at its full width or not at all:
    /// Labels is dropped first, then Epic, until Summary keeps `SUMMARY_MIN`. A column every row
    /// shares needs no room, and a width of 0 in `epic_w` or `labels_w` has none to give.
    pub fn trailing_widths(&self, room: usize, epic_w: usize, labels_w: usize) -> (usize, usize) {
        let wanted = |shown: bool, width: usize| if shown { width } else { 0 };
        let (epic, labels) = (
            wanted(self.epic_column, epic_w),
            wanted(self.labels_column, labels_w),
        );
        let fits = |extra: usize| room >= SUMMARY_MIN + extra;
        if fits(cells_width(epic, labels)) {
            (epic, labels)
        } else if fits(cells_width(epic, 0)) {
            (epic, 0)
        } else {
            (0, 0)
        }
    }

    /// Pushes a row's trailing cells, a `COLUMN_GAP` apart: Epic (when `epic_w` is not 0), Updated
    /// (`Ages::updated_cell`), and Labels (when `labels_w` is not 0), muted in `base`, the row's
    /// style.
    pub fn push_trailing_cells(
        &self,
        ages: Ages,
        row: &mut Line<'static>,
        ticket: &Ticket,
        base: Style,
        epic_w: usize,
        labels_w: usize,
    ) {
        let gap = Span::raw(" ".repeat(COLUMN_GAP));
        if epic_w > 0 {
            let epic = ticket.epic_name.as_deref().unwrap_or("-");
            row.push_span(gap.clone());
            row.push_span(Span::styled(fit(epic, epic_w), muted(base)));
        }
        row.push_span(gap.clone());
        row.push_span(Span::styled(ages.updated_cell(ticket), muted(base)));
        if labels_w > 0 {
            row.push_span(gap);
            row.push_span(Span::styled(
                fit(&self.row_labels(ticket), labels_w),
                muted(base),
            ));
        }
    }

    /// A row's Labels cell: its labels that not every row has.
    pub fn row_labels(&self, ticket: &Ticket) -> String {
        let own: Vec<&str> = ticket
            .labels
            .iter()
            .filter(|label| !self.labels.contains(label))
            .map(String::as_str)
            .collect();
        if own.is_empty() {
            "-".to_string()
        } else {
            own.join(", ")
        }
    }

    /// The muted line over the list, indented `lead` cells as rows are and cut to `width`; `None`
    /// when nothing is shared.
    pub fn line(&self, lead: usize, width: usize) -> Option<Line<'static>> {
        if self.epic.is_none() && self.labels.is_empty() {
            return None;
        }
        let mut text = format!("{}all rows", " ".repeat(lead));
        if let Some(epic) = &self.epic {
            text.push_str(" · epic ");
            text.push_str(epic);
        }
        if !self.labels.is_empty() {
            text.push_str(" · ");
            text.push_str(&self.labels.join(", "));
        }
        Some(Line::from(Span::styled(
            truncate(&text, width),
            style::secondary(),
        )))
    }
}

/// Ticket ages at one moment, read once per render so every age on screen is measured to it,
/// and what `delight` makes of them: a fossil's faded row.
#[derive(Clone, Copy)]
pub struct Ages<'a> {
    /// Unix seconds.
    now: i64,
    delight: &'a Delight,
    rules: &'a StatusRules,
}

impl<'a> Ages<'a> {
    pub fn of(app: &'a App) -> Self {
        Ages {
            now: crate::local_cache::now_unix_secs() as i64,
            delight: &app.delight,
            rules: app.status_rules(),
        }
    }

    /// A row's Updated cell: how long before `now` Jira last updated the ticket, right-aligned
    /// by display width, or "-" when the read didn't say.
    pub fn updated_cell(&self, ticket: &Ticket) -> String {
        let age = ticket
            .updated_secs()
            .map_or_else(|| "-".to_string(), |at| age(at, self.now));
        format!("{age:>width$}", width = UPDATED_WIDTH)
    }

    /// A ticket row's styles: the one its cells share, and the one of its key and summary, bold
    /// when `selected`. The row is the selected row's tint, and muted when `faded` or when
    /// `ticket` is a fossil.
    pub fn row_styles(&self, ticket: &Ticket, selected: bool, faded: bool) -> (Style, Style) {
        let base = tint(selected);
        let base = if faded || self.delight.is_fossil(self.rules, ticket, self.now) {
            muted(base)
        } else {
            base
        };
        let primary = if selected {
            base.add_modifier(Modifier::BOLD)
        } else {
            base
        };
        (base, primary)
    }
}

/// Width of the Updated column: the widest age, "100w", right-aligned.
pub const UPDATED_WIDTH: usize = 4;

/// How long ago `updated` was at `now` (both Unix seconds), in its largest whole unit: "59m",
/// "23h", "6d", "3w". A time ahead of `now` (clocks disagree) is "0m".
pub fn age(updated: i64, now: i64) -> String {
    let minutes = (now - updated).max(0) / 60;
    match minutes {
        m if m < 60 => format!("{m}m"),
        m if m < 60 * 24 => format!("{}h", m / 60),
        m if m < 60 * 24 * 7 => format!("{}d", m / (60 * 24)),
        m => format!("{}w", m / (60 * 24 * 7)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn rules() -> StatusRules {
        let names = |list: &[&str]| list.iter().map(|name| name.to_string()).collect::<Vec<_>>();
        StatusRules::new(
            &names(&["To Do", "In Progress"]),
            &names(&["Done"]),
            &names(&["Won't Do"]),
        )
    }

    #[test]
    fn share_cells_fill_the_width_in_proportion() {
        // 108 Done, 3 In Progress, 10 Won't Do over 24 cells: floors 21, 0, 1, and the two
        // cells left go to the largest remainders, Won't Do's then In Progress's.
        assert_eq!(share_cells(&[108, 3, 10], 24), vec![21, 1, 2]);
        assert_eq!(share_cells(&[1, 1, 1], 24), vec![8, 8, 8]);
        assert_eq!(share_cells(&[1, 1], 3), vec![2, 1]);
        assert_eq!(share_cells(&[], 24), Vec::<usize>::new());
    }

    #[test]
    fn every_status_gets_a_cell_while_the_widest_can_spare_one() {
        // 1 of 1000 rounds to no cell, so it takes one from the widest run.
        assert_eq!(share_cells(&[999, 1], 24), vec![23, 1]);
        // With fewer cells than statuses, a run of one is never taken from.
        assert_eq!(share_cells(&[5, 1, 1], 2), vec![1, 1, 0]);
    }

    #[test]
    fn epic_bar_puts_done_first_and_dropped_last() {
        let epic = Epic::for_test(
            "DEMO-1",
            &[("In Progress", 3), ("Won't Do", 10), ("Done", 108)],
        );
        let bar = epic_bar(&epic, &rules(), EPIC_BAR_WIDTH, Style::default());
        let text: String = bar.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, format!("{}▒{}", DONE_CELL.repeat(21), "░".repeat(2)));
        let colors: Vec<_> = bar.iter().map(|span| span.style.fg).collect();
        assert_eq!(
            colors,
            vec![Some(Color::Green), Some(Color::Yellow), Some(Color::Gray)]
        );
    }

    #[test]
    fn an_empty_epic_draws_an_empty_bar() {
        let bar = epic_bar(
            &Epic::for_test("DEMO-1", &[]),
            &rules(),
            EPIC_BAR_WIDTH,
            Style::default(),
        );
        let text: String = bar.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "·".repeat(24));
    }

    #[test]
    fn each_selection_mark_is_one_column_wide() {
        for mark in MARKS {
            assert_eq!(
                unicode_width::UnicodeWidthChar::width(mark),
                Some(1),
                "{mark}"
            );
        }
    }

    #[test]
    fn fit_pads_or_cuts_to_exactly_width_display_columns() {
        assert_eq!(fit("Sam", 5), "Sam  ");
        assert_eq!(fit("abcdef", 6), "abcdef");
        assert_eq!(fit("abcdefg", 6), "abc...");
        // 山 is two columns: "山田" fits 4 exactly, and in 5 a cut can't split a character.
        assert_eq!(fit("山田", 4), "山田");
        assert_eq!(fit("山田太郎", 6), "山... ");
        assert_eq!(fit("山田太郎", 5), "山...");
        assert_eq!(fit("abcdef", 2), "..");
        assert_eq!(fit("承認待ち", 20), format!("承認待ち{}", " ".repeat(12)));
        // "❤️" (heart and variation selector) is two columns together, one per character.
        assert_eq!(fit("❤️❤️xyzw", 5), "❤️...");
        assert_eq!(fit("✔️✔️✔️✔️", 6), "✔️✔...");
        for (text, width) in [
            ("山田太郎", 6),
            ("山田太郎", 5),
            ("Waiting for Approval", 8),
            ("❤️❤️xyzw", 5),
            ("✔️✔️✔️✔️", 6),
        ] {
            assert_eq!(fit(text, width).width(), width, "{text}");
        }
    }

    #[test]
    fn a_summary_wraps_to_lines_of_exactly_width_display_columns_only_when_asked() {
        let lines = |wrap: bool, summary: &str, width: usize| {
            let mut app = App::new();
            app.wrap_summaries = wrap;
            let mut ticket = Ticket::for_test("DEMO-1", "To Do");
            ticket.summary = summary.into();
            let (_, lines) = ticket_cells(&app, None, &ticket, width, Style::default());
            lines
                .iter()
                .map(|spans| spans.iter().map(|span| span.content.as_ref()).collect())
                .collect::<Vec<String>>()
        };
        let summary = "alpha bravo charlie delta";
        assert_eq!(lines(false, summary, 12), ["alpha bra..."]);
        assert_eq!(
            lines(true, summary, 12),
            ["alpha bravo ", "charlie     ", "delta       "]
        );
        // 山 is two columns: three would be six, so five columns hold two and a pad.
        assert_eq!(
            lines(true, "山田太郎山田太郎", 5),
            ["山田 ", "太郎 ", "山田 ", "太郎 "]
        );
        assert_eq!(lines(true, "", 8), ["        "]);
        // Two columns is the narrowest that wraps: one wide character a line.
        assert_eq!(lines(true, "山田", 2), ["山", "田"]);
        assert_eq!(lines(true, "山田", 1), ["."]);
    }

    #[test]
    fn narrowing_cuts_columns_in_order_to_their_floors_only_until_summary_has_its_minimum() {
        let narrow = |room: usize| {
            let (mut epic, mut status) = (20, 20);
            let summary = narrow_for_summary(room, 14, &mut [(&mut epic, 10), (&mut status, 8)]);
            [epic, status, summary]
        };
        // 54 is exactly enough: nothing narrows, and wider rooms go to Summary.
        assert_eq!(narrow(55), [20, 20, 15]);
        assert_eq!(narrow(54), [20, 20, 14]);
        assert_eq!(narrow(53), [19, 20, 14]);
        // Epic stops at its floor before Status gives up anything.
        assert_eq!(narrow(44), [10, 20, 14]);
        assert_eq!(narrow(43), [10, 19, 14]);
        assert_eq!(narrow(32), [10, 8, 14]);
        // Too narrow even with every column at its floor: Summary gets what is left.
        assert_eq!(narrow(31), [10, 8, 13]);
        assert_eq!(narrow(0), [10, 8, 0]);
    }

    #[test]
    fn only_a_parents_key_cell_has_a_fold_arrow_and_names_its_column() {
        let cell = |family, folded| {
            let cell = key_cell(family, "DEMO-1", folded);
            (cell.text, cell.fold)
        };
        assert_eq!(cell(None, false), ("DEMO-1".to_string(), None));
        assert_eq!(
            cell(Some(Family::Child), false),
            ("  └ DEMO-1".to_string(), None)
        );
        assert_eq!(
            cell(Some(Family::Parent(2)), false),
            (format!("DEMO-1 {OPEN}"), Some(7))
        );
        assert_eq!(
            cell(Some(Family::Parent(2)), true),
            (format!("DEMO-1 {FOLDED}"), Some(7))
        );
        assert_eq!(
            key_cell(Some(Family::Parent(1)), "山田-1", false).fold,
            Some(7)
        );
    }

    #[test]
    fn age_uses_the_largest_whole_unit() {
        const MIN: i64 = 60;
        const HOUR: i64 = 60 * MIN;
        const DAY: i64 = 24 * HOUR;
        let now = 1_790_763_800;
        for (ago, expected) in [
            (-5 * MIN, "0m"), // a clock running behind Jira's
            (0, "0m"),
            (59, "0m"),
            (59 * MIN, "59m"),
            (HOUR - 1, "59m"),
            (HOUR, "1h"),
            (23 * HOUR, "23h"),
            (DAY - 1, "23h"),
            (DAY, "1d"),
            (6 * DAY, "6d"),
            (7 * DAY - 1, "6d"),
            (7 * DAY, "1w"),
            (20 * DAY, "2w"),
            (400 * DAY, "57w"),
        ] {
            assert_eq!(age(now - ago, now), expected, "{ago}s ago");
        }
    }
}
