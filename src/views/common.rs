use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding};

use crate::app::{App, GroupSelectionState, VisibleGroup};
use crate::cache::{Status, StatusRules, Ticket};
use crate::subtasks::Family;
use unicode_width::UnicodeWidthStr;

/// Width of a row's checkbox-and-key cell: the checkbox, a key of up to ten characters, and the
/// indent of a sub-task drawn under its parent.
pub const KEY_WIDTH: usize = 16;

pub fn panel() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title_style(Style::default().fg(Color::Reset))
        .padding(Padding::horizontal(1))
}

pub fn fold_indicator(collapsed: bool) -> &'static str {
    if collapsed {
        "▶"
    } else {
        "▼"
    }
}

pub fn highlight_row(lines: &mut [Line<'_>], index: Option<usize>, width: u16) {
    if let Some(line) = index.and_then(|index| lines.get_mut(index)) {
        line.spans.push(Span::raw(
            " ".repeat((width as usize).saturating_sub(line.width())),
        ));
        line.style = line.style.bg(Color::DarkGray);
        if let Some(key) = line.spans.first_mut() {
            key.style = key.style.add_modifier(Modifier::BOLD);
        }
    }
}

/// Color for a status name: green when it's done, otherwise by the built-in status it
/// reads as, and magenta for workflow statuses the app doesn't know.
pub fn status_color(status: &str, rules: &StatusRules) -> Color {
    if rules.is_done(status) {
        return Color::Green;
    }
    match Status::from_str(status) {
        Status::NeedsTriage => Color::Reset,
        Status::ReadyForWork => Color::Blue,
        Status::InProgress => Color::Yellow,
        Status::ToDo => Color::Reset,
        Status::InReview => Color::Cyan,
        Status::Blocked => Color::Red,
        Status::Closed => Color::Green,
        Status::Other(_) => Color::Magenta,
    }
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

/// `base` in the muted color: gray on the selected row (`base` has its background), so it shows
/// against the highlight, and dark gray elsewhere.
pub fn muted(base: Style) -> Style {
    base.fg(if base.bg.is_some() {
        Color::Gray
    } else {
        Color::DarkGray
    })
}

/// Colors each line's selection mark: `muted` when unselected, cyan when selected or partial. Only a line's first mark is the
/// selection, since a summary may contain one. Call after `highlight_row`.
pub fn color_marks(lines: &mut [Line<'_>]) {
    for line in lines {
        let Some((index, at)) = line
            .spans
            .iter()
            .enumerate()
            .find_map(|(index, span)| span.content.find(MARKS).map(|at| (index, at)))
        else {
            continue;
        };
        let span = line.spans.remove(index);
        let (before, rest) = span.content.split_at(at);
        let mark_len = rest.chars().next().map_or(0, char::len_utf8);
        let (mark, after) = rest.split_at(mark_len);
        // The mark's style with the row's, so `muted` sees the highlight.
        let row = line.style.patch(span.style);
        let mark_style = if mark.starts_with(UNSELECTED) {
            muted(row)
        } else {
            row.fg(Color::Cyan)
        };
        let parts = [
            Span::styled(before.to_string(), span.style),
            Span::styled(mark.to_string(), mark_style),
            Span::styled(after.to_string(), span.style),
        ];
        line.spans.splice(
            index..index,
            parts.into_iter().filter(|part| !part.content.is_empty()),
        );
    }
}

/// A ticket row's key and summary cells, given its place in a parent's family. A sub-task under
/// its parent is indented, and a parent shows whether its sub-tasks are folded, with how many are
/// hidden. A sub-task whose parent is elsewhere leads its summary with the parent's key, muted so
/// the summary reads first (gray on the selected row, as other muted cells are). The summary is
/// drawn in `base`, truncated and padded to `width`.
pub fn ticket_cells(
    app: &App,
    family: Option<Family>,
    ticket: &Ticket,
    marker: char,
    width: usize,
    base: Style,
) -> (String, Vec<Span<'static>>) {
    let folded = app.is_parent_folded(&ticket.key);
    let key = match family {
        Some(Family::Child) => format!("  {} {}", marker, ticket.key),
        Some(Family::Parent(_)) => {
            format!("{} {} {}", marker, ticket.key, fold_indicator(folded))
        }
        None => format!("{} {}", marker, ticket.key),
    };
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
    let cell = fit(&(prefix.clone() + &summary), width);
    let split = cell
        .char_indices()
        .nth(prefix.chars().count())
        .map_or(cell.len(), |(i, _)| i);
    let (prefix, summary) = cell.split_at(split);
    (
        key,
        vec![
            Span::styled(prefix.to_string(), muted(base)),
            Span::styled(summary.to_string(), base),
        ],
    )
}

/// Width of the ` │ ` between two columns.
pub const SEPARATOR_WIDTH: usize = 3;

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

    /// The summary's width once the hidden Epic and Labels columns, and their separators, give
    /// it theirs. For `Columns::EpicAndLabels` tabs, whose widths count both columns.
    pub fn summary_width(&self, summary_w: usize, epic_w: usize, labels_w: usize) -> usize {
        [(self.epic_column, epic_w), (self.labels_column, labels_w)]
            .into_iter()
            .filter(|(shown, _)| !shown)
            .fold(summary_w, |w, (_, hidden)| w + hidden + SEPARATOR_WIDTH)
    }

    /// Pushes the headings, in `style`, of the cells `push_trailing_cells` draws.
    pub fn push_trailing_headings(
        &self,
        header: &mut Line<'static>,
        style: Style,
        epic_w: usize,
        labels_w: usize,
    ) {
        for (shown, title, width) in [
            (self.epic_column, "EPIC", epic_w),
            (true, "UPDATED", UPDATED_WIDTH),
            (self.labels_column, "LABELS", labels_w),
        ] {
            if shown {
                header.push_span(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                header.push_span(Span::styled(format!("{title:<width$}"), style));
            }
        }
    }

    /// Pushes a row's trailing cells: Epic (when shown), Updated (its age at `now`), and Labels
    /// (when shown), muted in `base`, the row's style (with its background on the selected row, where labels are
    /// yellow).
    pub fn push_trailing_cells(
        &self,
        now: i64,
        row: &mut Line<'static>,
        ticket: &Ticket,
        base: Style,
        epic_w: usize,
        labels_w: usize,
    ) {
        let separator = Span::styled(" │ ", base.fg(Color::DarkGray));
        if self.epic_column {
            let epic = ticket.epic_name.as_deref().unwrap_or("-");
            row.push_span(separator.clone());
            row.push_span(Span::styled(fit(epic, epic_w), muted(base)));
        }
        row.push_span(separator.clone());
        row.push_span(Span::styled(updated_cell(now, ticket), muted(base)));
        if self.labels_column {
            row.push_span(separator);
            row.push_span(Span::styled(
                fit(&self.row_labels(ticket), labels_w),
                if base.bg.is_some() {
                    base.fg(Color::Yellow)
                } else {
                    muted(base)
                },
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

    /// The muted line under the column headers, cut to `width`; `None` when nothing is shared.
    pub fn line(&self, width: usize) -> Option<Line<'static>> {
        if self.epic.is_none() && self.labels.is_empty() {
            return None;
        }
        let mut text = "  all rows".to_string();
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
            Style::default().fg(Color::DarkGray),
        )))
    }
}

/// Width of the Updated column: its heading's.
pub const UPDATED_WIDTH: usize = 7;

/// A row's Updated cell: how long before `now` (Unix seconds, read once per render) Jira last
/// updated the ticket, right-aligned, or "-" when the read didn't say.
pub fn updated_cell(now: i64, ticket: &Ticket) -> String {
    let age = ticket
        .updated_secs()
        .map_or_else(|| "-".to_string(), |at| age(at, now));
    format!("{age:>UPDATED_WIDTH$}")
}

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
