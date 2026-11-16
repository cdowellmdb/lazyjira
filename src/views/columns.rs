use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::VisibleGroup;
use crate::cache::Ticket;
use crate::style;
use crate::views::common::{fit, muted, truncate, Ages, COLUMN_GAP};

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

/// The width Summary keeps before a list gives room to Epic or Labels. Below it those columns
/// are not drawn, so an 80 column terminal shows Key, Summary and Updated (and Status where a tab
/// has it). It is the most that still lets a 120 column Team keep Epic and Labels beside 12 wide
/// keys and 14 wide statuses, and the least that drops My Work's Epic at 80 for keys of 7
/// characters or more. Marking a ticket does not change which columns are drawn: the room is
/// judged as if nothing were marked, and the checkbox takes its cells from Summary.
pub const SUMMARY_MIN: usize = 37;

/// The Epic and Labels cells a list draws after Summary: `Some(width)` for a column drawn, `None`
/// for one not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trailing {
    pub epic: Option<usize>,
    pub labels: Option<usize>,
}

impl Trailing {
    /// Neither column: what Unassigned and Filters, which have none, draw.
    pub const NONE: Trailing = Trailing {
        epic: None,
        labels: None,
    };

    /// The columns the cells take, each with the gap before it.
    pub fn width(self) -> usize {
        [self.epic, self.labels]
            .into_iter()
            .flatten()
            .map(|width| COLUMN_GAP + width)
            .sum()
    }
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

    /// The Epic and Labels cells a list has room for, with `room` columns for Summary and these
    /// two, each at most `epic_max` and `labels_max` wide. A column is drawn at its full width or
    /// not at all: Labels is dropped first, then Epic, until Summary keeps `SUMMARY_MIN`. A column
    /// every row shares needs no room.
    pub fn trailing(&self, room: usize, epic_max: usize, labels_max: usize) -> Trailing {
        let wanted = Trailing {
            epic: self.epic_column.then_some(epic_max),
            labels: self.labels_column.then_some(labels_max),
        };
        let epic_only = Trailing {
            labels: None,
            ..wanted
        };
        [wanted, epic_only]
            .into_iter()
            .find(|cells| room >= SUMMARY_MIN + cells.width())
            .unwrap_or(Trailing::NONE)
    }

    /// Pushes a row's trailing cells, a `COLUMN_GAP` apart: Epic (when `trailing` has it), Updated
    /// (`Ages::updated_cell`), and Labels (when `trailing` has them), muted in `base`, the row's
    /// style.
    pub fn push_trailing_cells(
        &self,
        ages: Ages,
        row: &mut Line<'static>,
        ticket: &Ticket,
        base: Style,
        trailing: Trailing,
    ) {
        let gap = Span::raw(" ".repeat(COLUMN_GAP));
        if let Some(width) = trailing.epic {
            let epic = ticket.epic_name.as_deref().unwrap_or("-");
            row.push_span(gap.clone());
            row.push_span(Span::styled(fit(epic, width), muted(base)));
        }
        row.push_span(gap.clone());
        row.push_span(Span::styled(ages.updated_cell(ticket), muted(base)));
        if let Some(width) = trailing.labels {
            row.push_span(gap);
            row.push_span(Span::styled(
                fit(&self.row_labels(ticket), width),
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A `Shared` whose rows differ in both epics and labels, so each column is wanted.
    fn both_columns() -> Shared {
        Shared {
            epic: None,
            labels: vec![],
            epic_column: true,
            labels_column: true,
        }
    }

    #[test]
    fn trailing_drops_labels_then_epic_until_summary_keeps_its_minimum() {
        let (epic, labels) = (24, 22);
        let at = |room| both_columns().trailing(room, epic, labels);
        let epic_only = Trailing {
            epic: Some(24),
            labels: None,
        };
        // Epic and a gap need 26 beside Summary's 37, Labels and a gap 24 more.
        assert_eq!(at(87).width(), 26 + 24);
        assert_eq!(
            at(87),
            Trailing {
                epic: Some(24),
                labels: Some(22)
            }
        );
        assert_eq!(at(86), epic_only);
        assert_eq!(at(63), epic_only);
        assert_eq!(at(62), Trailing::NONE);
        assert_eq!(at(0), Trailing::NONE);
        assert_eq!(SUMMARY_MIN + COLUMN_GAP + epic, 63);
    }

    #[test]
    fn trailing_asks_no_room_for_a_column_every_row_shares() {
        let labels_only = Shared {
            epic_column: false,
            ..both_columns()
        };
        let cells = Trailing {
            epic: None,
            labels: Some(22),
        };
        assert_eq!(labels_only.trailing(61, 24, 22), cells);
        assert_eq!(labels_only.trailing(60, 24, 22), Trailing::NONE);
        let neither = Shared {
            labels_column: false,
            ..labels_only
        };
        assert_eq!(neither.trailing(200, 24, 22), Trailing::NONE);
    }

    #[test]
    fn trailing_width_counts_a_gap_before_each_cell_drawn() {
        let width = |epic, labels| Trailing { epic, labels }.width();
        assert_eq!(width(None, None), 0);
        assert_eq!(width(Some(24), None), 26);
        assert_eq!(width(None, Some(22)), 24);
        assert_eq!(width(Some(24), Some(22)), 50);
    }
}
