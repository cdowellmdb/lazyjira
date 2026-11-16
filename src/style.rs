//! The shared styles, one per role in `docs/tui-style.md`. Each uses the ANSI color `theme`
//! recolors for that role, and never `Modifier::DIM` or RGB, which a theme can't reach.

use ratatui::style::{Color, Modifier, Style};

use crate::cache::StatusRules;
use crate::views::common::status_color;

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

/// The dot before a status, in that status's color.
pub fn status_dot(status: &str, rules: &StatusRules) -> Style {
    Style::default().fg(status_color(status, rules))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use std::collections::BTreeMap;

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
            status_dot("In Progress", &rules),
            status_dot("Done", &rules),
            status_dot("Blocked", &rules),
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
