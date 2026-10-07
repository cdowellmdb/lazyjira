use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding};

use crate::app::{App, GroupSelectionState, VisibleGroup};
use crate::cache::{Status, StatusRules, Ticket};
use crate::subtasks::Family;

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

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let t: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", t)
    } else {
        s.to_string()
    }
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

/// Colors each line's selection mark: muted when unselected (gray on the highlighted row, so it
/// shows against the highlight), cyan when selected or partial. Only a line's first mark is the
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
        let highlighted = line.style.bg.or(span.style.bg) == Some(Color::DarkGray);
        let fg = if !mark.starts_with(UNSELECTED) {
            Color::Cyan
        } else if highlighted {
            Color::Gray
        } else {
            Color::DarkGray
        };
        let parts = [
            Span::styled(before.to_string(), span.style),
            Span::styled(mark.to_string(), span.style.fg(fg)),
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
    let cell = format!("{:<width$}", truncate(&(prefix.clone() + &summary), width));
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

/// What every row a tab draws has in common, so it is shown once instead of on every row.
/// Rows with no epic don't count when deciding whether an epic is shared.
pub struct Shared {
    pub epic: Option<String>,
    pub labels: Vec<String>,
    /// Some rows' epics differ, so rows need their Epic column.
    pub epic_column: bool,
    /// Some row has a label not every row has, so rows need their Labels column.
    pub labels_column: bool,
}

impl Shared {
    /// The shared values of the rows `groups` draw, counting the sub-tasks of folded parents
    /// (as group totals do) but nothing in a folded group.
    pub fn of<H>(groups: &[VisibleGroup<'_, H>]) -> Self {
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
        Shared {
            epic: (epics.len() == 1).then(|| epics[0].to_string()),
            epic_column: epics.len() > 1,
            labels_column: rows
                .iter()
                .any(|t| t.labels.iter().any(|label| !labels.contains(label))),
            labels,
        }
    }

    /// The summary's width once the hidden Epic and Labels columns, and their separators, give
    /// it theirs.
    pub fn summary_width(&self, summary_w: usize, epic_w: usize, labels_w: usize) -> usize {
        [(self.epic_column, epic_w), (self.labels_column, labels_w)]
            .into_iter()
            .filter(|(shown, _)| !shown)
            .fold(summary_w, |w, (_, hidden)| w + hidden + 3)
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

    /// Pushes a row's trailing cells: Epic (when shown), Updated, and Labels (when shown), muted
    /// in `base`, the row's style (with its background on the selected row, where labels are
    /// yellow).
    pub fn push_trailing_cells(
        &self,
        app: &App,
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
            row.push_span(Span::styled(
                format!("{:<epic_w$}", truncate(epic, epic_w)),
                muted(base),
            ));
        }
        row.push_span(separator.clone());
        row.push_span(Span::styled(updated_cell(app, ticket), muted(base)));
        if self.labels_column {
            let labels = truncate(&self.row_labels(ticket), labels_w);
            row.push_span(separator);
            row.push_span(Span::styled(
                format!("{labels:<labels_w$}"),
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

/// A row's Updated cell: how long ago Jira last updated the ticket, right-aligned, or "-" when
/// the read didn't say.
pub fn updated_cell(app: &App, ticket: &Ticket) -> String {
    let age = ticket
        .updated_secs()
        .map_or_else(|| "-".to_string(), |at| age(at, (app.clock)()));
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
