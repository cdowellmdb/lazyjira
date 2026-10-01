//! Color themes. Renderers draw with the terminal's ANSI colors, each used for one job (dark gray
//! for muted text and the selected row, cyan for keys and focus, and so on). A theme recolors the
//! finished frame by those jobs, the way a terminal color scheme recolors its palette. A role a
//! theme leaves unset keeps the terminal's own color, so the default theme changes nothing.

use std::collections::BTreeMap;

use ratatui::buffer::Buffer;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

pub const DEFAULT: &str = "default";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    /// Behind everything; unset keeps the terminal's background.
    pub background: Option<Color>,
    pub text: Option<Color>,
    /// Borders, separators, hints.
    pub muted: Option<Color>,
    /// Secondary text on the selected row.
    pub subtle: Option<Color>,
    /// Behind the selected row.
    pub selection: Option<Color>,
    /// Keys, focused fields, links, In Review.
    pub accent: Option<Color>,
    /// Headers, the chosen option, In Progress.
    pub highlight: Option<Color>,
    /// Errors, Blocked.
    pub error: Option<Color>,
    /// Done.
    pub success: Option<Color>,
    /// Ready for Work, info markers, selected text.
    pub info: Option<Color>,
    /// Statuses the app doesn't know.
    pub special: Option<Color>,
}

const fn rgb(hex: u32) -> Option<Color> {
    Some(Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8))
}

#[allow(clippy::too_many_arguments)]
const fn preset(
    background: u32,
    text: u32,
    muted: u32,
    subtle: u32,
    selection: u32,
    accent: u32,
    highlight: u32,
    error: u32,
    success: u32,
    info: u32,
    special: u32,
) -> Theme {
    Theme {
        background: rgb(background),
        text: rgb(text),
        muted: rgb(muted),
        subtle: rgb(subtle),
        selection: rgb(selection),
        accent: rgb(accent),
        highlight: rgb(highlight),
        error: rgb(error),
        success: rgb(success),
        info: rgb(info),
        special: rgb(special),
    }
}

pub const PRESETS: &[(&str, Theme)] = &[
    (
        "dracula",
        preset(
            0x282a36, 0xf8f8f2, 0x6272a4, 0xbfbfbf, 0x44475a, 0x8be9fd, 0xf1fa8c, 0xff5555,
            0x50fa7b, 0xbd93f9, 0xff79c6,
        ),
    ),
    (
        "gruvbox",
        preset(
            0x282828, 0xebdbb2, 0x928374, 0xa89984, 0x3c3836, 0x8ec07c, 0xfabd2f, 0xfb4934,
            0xb8bb26, 0x83a598, 0xd3869b,
        ),
    ),
    (
        "nord",
        preset(
            0x2e3440, 0xd8dee9, 0x616e88, 0xa0a8b7, 0x3b4252, 0x88c0d0, 0xebcb8b, 0xbf616a,
            0xa3be8c, 0x81a1c1, 0xb48ead,
        ),
    ),
    (
        "catppuccin",
        preset(
            0x1e1e2e, 0xcdd6f4, 0x6c7086, 0xa6adc8, 0x313244, 0x89dceb, 0xf9e2af, 0xf38ba8,
            0xa6e3a1, 0x89b4fa, 0xcba6f7,
        ),
    ),
    (
        "solarized-light",
        preset(
            0xfdf6e3, 0x586e75, 0x93a1a1, 0x657b83, 0xeee8d5, 0x2aa198, 0xb58900, 0xdc322f,
            0x859900, 0x268bd2, 0xd33682,
        ),
    ),
];

/// Every theme name to offer: the default, the presets, then custom themes that don't
/// replace a preset.
pub fn names(custom: &BTreeMap<String, Theme>) -> Vec<String> {
    let builtin: Vec<&str> = std::iter::once(DEFAULT)
        .chain(PRESETS.iter().map(|(name, _)| *name))
        .collect();
    let custom = custom
        .keys()
        .filter(|name| !builtin.contains(&name.as_str()));
    builtin
        .iter()
        .map(|name| name.to_string())
        .chain(custom.cloned())
        .collect()
}

/// The theme called `name`. A custom theme replaces a preset of the same name; an unknown name
/// is the default.
pub fn resolve(name: &str, custom: &BTreeMap<String, Theme>) -> Theme {
    custom
        .get(name)
        .copied()
        .or_else(|| {
            PRESETS
                .iter()
                .find(|(preset, _)| *preset == name)
                .map(|(_, theme)| *theme)
        })
        .unwrap_or_default()
}

impl Theme {
    fn role(&self, color: Color, background: bool) -> Option<Color> {
        match color {
            Color::Reset if background => self.background,
            Color::Reset => self.text,
            Color::DarkGray if background => self.selection,
            Color::DarkGray => self.muted,
            Color::Gray => self.subtle,
            Color::Cyan | Color::LightCyan => self.accent,
            Color::Yellow => self.highlight,
            Color::Red | Color::LightRed => self.error,
            Color::Green => self.success,
            Color::Blue | Color::LightBlue => self.info,
            Color::Magenta => self.special,
            // Text drawn on an accent or info background.
            Color::Black => self.background,
            _ => None,
        }
    }

    /// Recolors a drawn frame. Run it once, after everything is drawn.
    pub fn apply(&self, buffer: &mut Buffer) {
        if *self == Theme::default() {
            return;
        }
        for cell in &mut buffer.content {
            if let Some(fg) = self.role(cell.fg, false) {
                cell.fg = fg;
            }
            if let Some(bg) = self.role(cell.bg, true) {
                cell.bg = bg;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn themes_recolor_by_role_and_leave_unset_roles_alone() {
        let nord = resolve("nord", &BTreeMap::new());
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        buffer.set_string(0, 0, "a", Style::default().fg(Color::DarkGray));
        buffer.set_string(1, 0, "b", Style::default().bg(Color::DarkGray));
        buffer.set_string(2, 0, "c", Style::default().fg(Color::Rgb(1, 2, 3)));
        nord.apply(&mut buffer);
        assert_eq!(buffer[(0, 0)].fg, nord.muted.unwrap());
        assert_eq!(buffer[(0, 0)].bg, nord.background.unwrap());
        assert_eq!(buffer[(1, 0)].bg, nord.selection.unwrap());
        assert_eq!(buffer[(2, 0)].fg, Color::Rgb(1, 2, 3));

        let custom: BTreeMap<String, Theme> = toml::from_str(
            "[nord]\naccent = '#ff0000'\n[mine]\nmuted = 'blue'\ntext = 'light-blue'\n",
        )
        .unwrap();
        assert_eq!(resolve("nord", &custom).accent, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(resolve("nord", &custom).background, None);
        assert_eq!(resolve("missing", &custom), Theme::default());
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        buffer.set_string(0, 0, "ab", Style::default().fg(Color::DarkGray));
        resolve("mine", &custom).apply(&mut buffer);
        assert_eq!(buffer[(0, 0)].fg, Color::Blue);
        assert_eq!(custom["mine"].text, Some(Color::LightBlue));
        assert_eq!(buffer[(0, 0)].bg, Color::Reset);
        assert_eq!(names(&custom).last().map(String::as_str), Some("mine"));
        assert_eq!(names(&custom).iter().filter(|n| *n == "nord").count(), 1);
        assert!(toml::from_str::<Theme>("acent = 'red'").is_err());
    }
}
