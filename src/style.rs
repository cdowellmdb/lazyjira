//! The shared styles, one per role in `docs/tui-style.md`. Each uses the ANSI color `theme`
//! recolors for that role, and never `Modifier::DIM` or RGB, which a theme can't reach.

use ratatui::style::{Color, Modifier, Style};

use crate::cache::{Status, StatusKind, StatusRules};

/// Most text: the terminal's foreground.
pub fn primary() -> Style {
    Style::default().fg(Color::Reset)
}

/// Secondary text: epics, ages, labels, hint descriptions, rules.
pub fn secondary() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Focus, links, the active tab.
pub fn accent() -> Style {
    Style::default().fg(Color::Cyan)
}

/// The key in a hint like `Enter open`.
pub fn key_hint() -> Style {
    accent().add_modifier(Modifier::BOLD)
}

/// Group names and headers: weight, not color.
pub fn heading() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Behind the selected row.
pub fn selected_row() -> Style {
    Style::default().bg(Color::DarkGray)
}

/// The status's color on the dot and the text beside it.
pub fn status(status: &str, rules: &StatusRules) -> Style {
    Style::default().fg(status_color(status, rules))
}

/// Color for a status name: gray when it's dropped (readable on the selected row's dark gray, as
/// `muted` is), green when it's done, otherwise by the built-in status it reads as, and magenta
/// for workflow statuses the app doesn't know.
pub fn status_color(status: &str, rules: &StatusRules) -> Color {
    match rules.kind(status) {
        StatusKind::Dropped => return Color::Gray,
        StatusKind::Done => return Color::Green,
        StatusKind::Active => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use std::collections::BTreeMap;

    #[test]
    fn muted_text_and_the_selected_row_keep_their_terminal_colors() {
        assert_eq!(secondary().fg, Some(Color::DarkGray));
        assert_eq!(selected_row().bg, Some(Color::DarkGray));
    }

    #[test]
    fn a_dropped_status_stays_readable_on_the_selected_row() {
        // Rows draw a selected status cell on dark gray, so its text can't be dark gray too.
        let rules = StatusRules::default();
        assert_eq!(status_color("Won't Do", &rules), Color::Gray);
        assert_eq!(status_color("Done", &rules), Color::Green);
    }

    #[test]
    fn every_role_style_is_recolored_by_a_theme_and_none_is_dim() {
        let rules = StatusRules::default();
        let styles = [
            primary(),
            secondary(),
            accent(),
            key_hint(),
            heading(),
            selected_row(),
            status("In Progress", &rules),
            status("Done", &rules),
            status("Blocked", &rules),
        ];
        let nord = theme::resolve("nord", &BTreeMap::new());
        for style in styles {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
            buffer.set_string(0, 0, "x", style);
            nord.apply(&mut buffer);
            assert!(
                matches!(
                    (buffer[(0, 0)].fg, buffer[(0, 0)].bg),
                    (Color::Rgb(..), Color::Rgb(..))
                ),
                "{style:?} kept a terminal color"
            );
            assert!(!style.add_modifier.contains(Modifier::DIM), "{style:?}");
        }
        assert_eq!(key_hint().add_modifier, Modifier::BOLD);
        assert_eq!(heading().fg, None);
    }
}
