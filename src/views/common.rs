use ratatui::style::Color;

use crate::app::GroupSelectionState;
use crate::cache::Status;

pub fn status_color(status: &Status) -> Color {
    match status {
        Status::NeedsTriage => Color::White,
        Status::ReadyForWork => Color::Blue,
        Status::InProgress => Color::Yellow,
        Status::ToDo => Color::White,
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

pub fn group_marker(state: GroupSelectionState) -> &'static str {
    match state {
        GroupSelectionState::None => "[ ]",
        GroupSelectionState::Partial => "[~]",
        GroupSelectionState::All => "[x]",
    }
}
