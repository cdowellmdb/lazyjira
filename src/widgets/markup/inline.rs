//! Jira's inline markup: emphasis, code, links, mentions, colors, images and icons.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::style;

/// The characters a backslash escapes. `\\` is a forced line break, and a backslash before any
/// other character is kept, so `C:\dir` reads as written.
fn is_escapable(c: char) -> bool {
    "()[]{}*_+-|!^~?".contains(c)
}

/// The character a backslash at `chars[i]` escapes, if it is one of the escapable ones. A
/// backtick is not: Jira has no backtick syntax to escape.
pub(super) fn escaped_char(chars: &[char], i: usize) -> Option<char> {
    if chars.get(i) != Some(&'\\') {
        return None;
    }
    chars.get(i + 1).copied().filter(|n| is_escapable(*n))
}

/// `text` with each backslash before an escapable character dropped.
fn unescape(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match escaped_char(&chars, i) {
            Some(escaped) => {
                out.push(escaped);
                i += 2;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

/// Parses inline markup: `*bold*`, `_italic_`, `-strike-`, `+underline+`,
/// `{{monospace}}`, `{color:red}text{color}`, `[text|url]`, `[~mention]`,
/// `!image.png!`, bare URLs, `--`/`---` dashes and `(/)`-style icons.
pub fn inline(text: &str, base: Style) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    Inline {
        chars: &chars,
        out: &mut out,
        buf: String::new(),
        style: base,
    }
    .run();
    out
}

struct Inline<'a> {
    chars: &'a [char],
    out: &'a mut Vec<Span<'static>>,
    buf: String,
    style: Style,
}

impl Inline<'_> {
    fn flush(&mut self) {
        if !self.buf.is_empty() {
            self.out
                .push(Span::styled(std::mem::take(&mut self.buf), self.style));
        }
    }

    fn emit(&mut self, text: impl Into<String>, style: Style) {
        self.flush();
        self.out.push(Span::styled(text.into(), style));
    }

    fn nested(&mut self, from: usize, to: usize, style: Style) {
        self.flush();
        let inner: String = self.chars[from..to].iter().collect();
        self.out.extend(inline(&inner, style));
    }

    fn starts_with(&self, i: usize, pat: &str) -> bool {
        let mut j = i;
        for p in pat.chars() {
            if self.chars.get(j) != Some(&p) {
                return false;
            }
            j += 1;
        }
        true
    }

    /// Where `pat` next starts at or after `from`, not counting an escaped character.
    fn find(&self, from: usize, pat: &str) -> Option<usize> {
        let mut j = from;
        while j < self.chars.len() {
            if escaped_char(self.chars, j).is_some() {
                j += 2;
            } else if self.starts_with(j, pat) {
                return Some(j);
            } else {
                j += 1;
            }
        }
        None
    }

    fn in_bare_url(&self, at: usize) -> bool {
        let Some(&c) = self.chars.get(at) else {
            return false;
        };
        !c.is_whitespace() && !"<>\"|[]".contains(c) && escaped_char(self.chars, at).is_none()
    }

    /// Draws `text` as inline code: a chip in the text color on the selection background.
    fn code(&mut self, text: String) {
        self.emit(text, self.style.patch(style::code()));
    }

    fn at_word_start(&self, i: usize) -> bool {
        i == 0 || !self.chars[i - 1].is_alphanumeric()
    }

    fn run(mut self) {
        let mut i = 0;
        while i < self.chars.len() {
            match self.step(i) {
                Some(next) => i = next,
                None => {
                    self.buf.push(self.chars[i]);
                    i += 1;
                }
            }
        }
        self.flush();
    }

    /// Handles markup starting at `i`, returning where to continue, or `None`
    /// if `chars[i]` is plain text.
    fn step(&mut self, i: usize) -> Option<usize> {
        let c = self.chars[i];

        if c == '\\' {
            self.buf.push(escaped_char(self.chars, i)?);
            return Some(i + 2);
        }

        // Jira writes braces in code as `{{\{x\}}}`, so `{{ }}` code unescapes its text. A
        // backtick span is Markdown's, so its text stays as written.
        if self.starts_with(i, "{{") {
            let end = self.find(i + 2, "}}")?;
            let code: String = self.chars[i + 2..end].iter().collect();
            self.code(unescape(&code));
            return Some(end + 2);
        }

        if c == '`' {
            let end = self.closing(i, c)?;
            self.code(self.chars[i + 1..end].iter().collect());
            return Some(end + 1);
        }

        if self.starts_with(i, "{color:") {
            let tag_end = self.find(i, "}")?;
            let name: String = self.chars[i + 7..tag_end].iter().collect();
            let close = self.find(tag_end + 1, "{color}");
            let end = close.unwrap_or(self.chars.len());
            let style = match parse_color(&name) {
                Some(color) => self.style.fg(color),
                None => self.style,
            };
            self.nested(tag_end + 1, end, style);
            return Some(close.map_or(end, |e| e + 7));
        }

        if c == '[' {
            let end = self.find(i + 1, "]")?;
            let inner: String = self.chars[i + 1..end].iter().collect();
            return self.link(&inner).then_some(end + 1);
        }

        if c == '!' && self.at_word_start(i) {
            let end = self.find(i + 1, "!")?;
            let inner: String = self.chars[i + 1..end].iter().collect();
            let name = inner.split('|').next().unwrap_or("");
            if name.is_empty() || name.contains(char::is_whitespace) || !name.contains('.') {
                return None;
            }
            let name = name.rsplit('/').next().unwrap_or(name);
            self.emit(format!("[image: {}]", name), style::aside());
            return Some(end + 1);
        }

        if c == '(' && self.at_word_start(i) {
            for (pat, symbol, color) in [
                ("(/)", "✓", Color::Green),
                ("(x)", "✗", Color::Red),
                ("(!)", "!", Color::Yellow),
                ("(i)", "i", Color::LightBlue),
                ("(*)", "★", Color::Yellow),
            ] {
                if self.starts_with(i, pat) {
                    self.emit(symbol, Style::default().fg(color));
                    return Some(i + pat.len());
                }
            }
            return None;
        }

        if (c == 'h' || c == 'H')
            && self.at_word_start(i)
            && (self.starts_with(i, "https://") || self.starts_with(i, "http://"))
        {
            let mut end = i;
            while self.in_bare_url(end) {
                end += 1;
            }
            while end > i && ".,;:!?)'".contains(self.chars[end - 1]) {
                end -= 1;
            }
            let url: String = self.chars[i..end].iter().collect();
            self.emit(url, style::link());
            return Some(end);
        }

        if c == '-' && self.at_word_start(i) {
            let before_space = i == 0 || self.chars[i - 1].is_whitespace();
            for (pat, dash) in [("---", "—"), ("--", "–")] {
                let after = i + pat.len();
                let after_space = after >= self.chars.len() || self.chars[after].is_whitespace();
                if before_space && after_space && self.starts_with(i, pat) {
                    self.buf.push_str(dash);
                    return Some(after);
                }
            }
        }

        let modifier = match c {
            '*' => Modifier::BOLD,
            '_' => Modifier::ITALIC,
            '-' => Modifier::CROSSED_OUT,
            '+' => Modifier::UNDERLINED,
            _ => return None,
        };
        let end = self.closing(i, c)?;
        self.nested(i + 1, end, self.style.add_modifier(modifier));
        Some(end + 1)
    }

    /// Finds the marker closing one opened at `i`. Like Jira, a marker opens
    /// only at a word start before non-space, and closes after non-space at a
    /// word end, so `snake_case` and `open-source` stay plain.
    fn closing(&self, i: usize, marker: char) -> Option<usize> {
        if !self.at_word_start(i) {
            return None;
        }
        let next = self.chars.get(i + 1)?;
        if next.is_whitespace() || *next == marker {
            return None;
        }
        (i + 2..self.chars.len()).find(|&j| {
            self.chars[j] == marker
                && !self.chars[j - 1].is_whitespace()
                && escaped_char(self.chars, j - 1).is_none()
                && self.chars.get(j + 1).is_none_or(|n| !n.is_alphanumeric())
        })
    }

    /// Renders the inside of `[...]`, returning false if it isn't a link.
    fn link(&mut self, inner: &str) -> bool {
        if let Some(user) = inner.strip_prefix('~') {
            let user = user.strip_prefix("accountid:").unwrap_or(user);
            self.emit(format!("@{}", user), Style::default().fg(Color::Cyan));
            return true;
        }
        if let Some((text, rest)) = inner.split_once('|') {
            let url = rest.split('|').next().unwrap_or("").trim();
            self.flush();
            let text = text.trim();
            if text.is_empty() {
                self.emit(url.to_string(), style::link());
            } else {
                self.out.extend(inline(text, style::link()));
                if !url.is_empty() && url != text {
                    self.emit(format!(" ({})", url), style::secondary());
                }
            }
            return true;
        }
        let target = inner.trim();
        if target.contains("://") || target.starts_with("mailto:") {
            self.emit(target.to_string(), style::link());
            return true;
        }
        if let Some(attachment) = target.strip_prefix('^') {
            self.emit(attachment.to_string(), style::link());
            return true;
        }
        false
    }
}

fn parse_color(name: &str) -> Option<Color> {
    let name = name.trim().to_ascii_lowercase();
    if let Some(hex) = name.strip_prefix('#') {
        let hex = if hex.len() == 3 {
            hex.chars().flat_map(|c| [c, c]).collect::<String>()
        } else {
            hex.to_string()
        };
        if hex.len() != 6 {
            return None;
        }
        let value = u32::from_str_radix(&hex, 16).ok()?;
        return Some(Color::Rgb(
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        ));
    }
    Some(match name.as_str() {
        "red" => Color::Red,
        "green" => Color::Green,
        "blue" => Color::LightBlue,
        "yellow" | "orange" => Color::Yellow,
        "purple" | "magenta" => Color::Magenta,
        "cyan" | "teal" => Color::Cyan,
        "white" => Color::Reset,
        "gray" | "grey" => Color::DarkGray,
        _ => return None,
    })
}
