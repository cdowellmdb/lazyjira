use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding};

use crate::app::App;
use crate::app::GroupSelectionState;
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
/// The selection marks, each one column wide (East Asian Width "N"): unselected, selected, and a
/// group with some of its tickets selected.
pub const MARKS: [char; 3] = [UNSELECTED, '☒', '⊟'];

pub fn ticket_marker(selected: bool) -> &'static str {
    if selected {
        "☒"
    } else {
        "☐"
    }
}

pub fn group_marker(state: GroupSelectionState) -> &'static str {
    match state {
        GroupSelectionState::None => "☐",
        GroupSelectionState::Partial => "⊟",
        GroupSelectionState::All => "☒",
    }
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
/// hidden. A sub-task whose parent is elsewhere leads its summary with the parent's key.
pub fn ticket_cells(
    app: &App,
    family: Option<Family>,
    ticket: &Ticket,
    marker: &str,
) -> (String, String) {
    let folded = app.is_parent_folded(&ticket.key);
    let key = match family {
        Some(Family::Child) => format!("  {} {}", marker, ticket.key),
        Some(Family::Parent(_)) => {
            format!("{} {} {}", marker, ticket.key, fold_indicator(folded))
        }
        None => format!("{} {}", marker, ticket.key),
    };
    let summary = match (family, &ticket.parent_key) {
        (Some(Family::Parent(count)), _) if folded => format!(
            "({} sub-task{}) {}",
            count,
            if count == 1 { "" } else { "s" },
            ticket.summary
        ),
        (None, Some(parent)) => format!("{} › {}", parent, ticket.summary),
        _ => ticket.summary.clone(),
    };
    (key, summary)
}
