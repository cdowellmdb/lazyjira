//! Renders Jira wiki markup (the format Jira keeps descriptions and comments in) as styled
//! lines, word-wrapped to a width so list items, quotes and code blocks keep their indent and
//! gutter on wrapped lines.

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::style;

mod inline;
mod table;
mod wrap;

pub use inline::inline;
pub use wrap::wrap;

/// Renders `text` as lines no wider than `width` columns.
pub fn render(text: &str, width: u16) -> Vec<Line<'static>> {
    let mut r = Renderer {
        width: (width as usize).max(10),
        lines: Vec::new(),
        list_counters: Vec::new(),
        in_quote: false,
    };
    r.render(text);
    r.finish()
}

struct Renderer {
    width: usize,
    lines: Vec<Line<'static>>,
    /// Numbered-list counters, one per nesting depth.
    list_counters: Vec<usize>,
    in_quote: bool,
}

impl Renderer {
    fn render(&mut self, text: &str) {
        let raw_lines: Vec<String> = text.lines().map(|l| l.replace('\t', "    ")).collect();
        let mut code_close: Option<&'static str> = None;
        let mut i = 0;

        while i < raw_lines.len() {
            let raw = raw_lines[i].as_str();
            let trimmed = raw.trim();
            i += 1;

            if let Some(close) = code_close {
                if let Some(before) = code_closes(raw, close) {
                    if !before.trim().is_empty() {
                        self.code_line(before);
                    }
                    code_close = None;
                } else {
                    self.code_line(raw);
                }
                continue;
            }

            if let Some((close, rest)) = code_opens(trimmed) {
                self.list_counters.clear();
                match code_closes(rest, close) {
                    Some(before) => {
                        if !before.trim().is_empty() {
                            self.code_line(before);
                        }
                    }
                    None => {
                        code_close = Some(close);
                        if !rest.trim().is_empty() {
                            self.code_line(rest);
                        }
                    }
                }
                continue;
            }

            if trimmed.starts_with('|') {
                let mut rows = vec![trimmed.to_string()];
                while i < raw_lines.len() && raw_lines[i].trim().starts_with('|') {
                    rows.push(raw_lines[i].trim().to_string());
                    i += 1;
                }
                self.list_counters.clear();
                self.lines.extend(table::lines(&rows, self.width));
                continue;
            }

            self.block_line(trimmed, raw);
        }
    }

    /// A line outside code blocks and tables.
    fn block_line(&mut self, trimmed: &str, raw: &str) {
        let mut body = trimmed;

        // {quote} may open or close a quote, alone or around text on this line.
        if let Some(rest) = body.strip_prefix("{quote}") {
            self.in_quote = !self.in_quote;
            body = rest.trim();
        }
        let mut closes_quote = false;
        if let Some(rest) = body.strip_suffix("{quote}") {
            closes_quote = true;
            body = rest.trim();
        }
        if body.is_empty() && (trimmed.starts_with("{quote}") || closes_quote) {
            if closes_quote {
                self.in_quote = !self.in_quote;
            }
            return;
        }

        if body.starts_with("{panel") {
            if let Some(title) = panel_title(body) {
                self.blank();
                self.push_wrapped(vec![], vec![], inline(title, style::heading()));
            }
            return;
        }

        if body.is_empty() {
            self.list_counters.clear();
            self.blank();
        } else if self.in_quote {
            self.quote(body);
        } else if let Some(rest) = body.strip_prefix("bq. ") {
            self.quote(rest);
        } else if body.len() >= 4 && body.chars().all(|c| c == '-') {
            self.list_counters.clear();
            self.rule();
        } else if let Some((level, text)) = parse_heading(body) {
            self.list_counters.clear();
            self.heading(level, text);
        } else if let Some((markers, item)) = parse_list_item(body) {
            self.list_item(markers, item);
        } else {
            self.list_counters.clear();
            // Keep the source's leading indent on plain paragraphs.
            let indent = raw.len() - raw.trim_start().len();
            let text = format!("{}{}", " ".repeat(indent), body);
            for segment in split_breaks(&text) {
                self.push_wrapped(vec![], vec![], inline(segment, style::primary()));
            }
        }

        if closes_quote {
            self.in_quote = !self.in_quote;
        }
    }

    fn heading(&mut self, level: u8, text: &str) {
        self.blank();
        self.push_wrapped(vec![], vec![], inline(text, style::heading()));
        if level == 1 {
            self.rule();
        }
    }

    fn list_item(&mut self, markers: &str, item: &str) {
        let depth = markers.chars().count();
        let numbered = markers.ends_with('#');
        self.list_counters.resize(depth, 0);
        let marker = if numbered {
            self.list_counters[depth - 1] += 1;
            format!("{}.", self.list_counters[depth - 1])
        } else {
            self.list_counters[depth - 1] = 0;
            ["•", "◦", "▪"][(depth - 1) % 3].to_string()
        };

        let indent = "  ".repeat(depth - 1);
        let first = vec![Span::styled(
            format!("{}{} ", indent, marker),
            style::secondary(),
        )];
        let hang = vec![Span::raw(
            " ".repeat(indent.len() + marker.chars().count() + 1),
        )];
        for (n, segment) in split_breaks(item).into_iter().enumerate() {
            let prefix = if n == 0 { first.clone() } else { hang.clone() };
            self.push_wrapped(prefix, hang.clone(), inline(segment, style::primary()));
        }
    }

    fn quote(&mut self, text: &str) {
        let gutter = vec![Span::styled("▎ ", style::secondary())];
        let quoted = style::primary().add_modifier(Modifier::ITALIC);
        for segment in split_breaks(text) {
            self.push_wrapped(gutter.clone(), gutter.clone(), inline(segment, quoted));
        }
    }

    fn code_line(&mut self, text: &str) {
        let gutter = vec![Span::styled("│ ", style::secondary())];
        let content = vec![Span::styled(text.to_string(), style::primary())];
        self.push_wrapped(gutter.clone(), gutter, content);
    }

    fn rule(&mut self) {
        self.lines.push(Line::from(Span::styled(
            "─".repeat(self.width),
            style::secondary(),
        )));
    }

    fn blank(&mut self) {
        let last_blank = self.lines.last().map(|l| l.width() == 0).unwrap_or(true);
        if !last_blank {
            self.lines.push(Line::from(""));
        }
    }

    fn push_wrapped(
        &mut self,
        first: Vec<Span<'static>>,
        rest: Vec<Span<'static>>,
        content: Vec<Span<'static>>,
    ) {
        let wrapped = wrap(first, rest, content, self.width);
        self.lines.extend(wrapped);
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        while self.lines.last().is_some_and(|l| l.width() == 0) {
            self.lines.pop();
        }
        self.lines
    }
}

/// If `trimmed` opens a code block, returns the tag that closes it and the
/// text after the opening tag.
fn code_opens(trimmed: &str) -> Option<(&'static str, &str)> {
    if trimmed.starts_with("```") {
        return Some(("```", ""));
    }
    for (open, close) in [("{code", "{code}"), ("{noformat", "{noformat}")] {
        if let Some(rest) = trimmed.strip_prefix(open) {
            if rest.starts_with('}') || rest.starts_with(':') {
                let end = rest.find('}')?;
                return Some((close, &rest[end + 1..]));
            }
        }
    }
    None
}

/// If `line` closes a code block, returns the code before the closing tag.
fn code_closes<'a>(line: &'a str, close: &str) -> Option<&'a str> {
    if close == "```" {
        return line.trim_start().starts_with("```").then_some("");
    }
    line.find(close).map(|idx| &line[..idx])
}

fn panel_title(tag: &str) -> Option<&str> {
    let params = tag.strip_prefix("{panel:")?.split('}').next()?;
    params
        .split('|')
        .find_map(|p| p.trim().strip_prefix("title="))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

fn parse_heading(line: &str) -> Option<(u8, &str)> {
    let rest = line.strip_prefix('h')?;
    let level = rest.chars().next()?.to_digit(10)?;
    if !(1..=6).contains(&level) {
        return None;
    }
    let text = rest[1..].strip_prefix(". ")?;
    Some((level as u8, text.trim()))
}

/// Returns a list item's markers (`*`, `#`, `-`, or a mix like `#*`) and text.
fn parse_list_item(line: &str) -> Option<(&str, &str)> {
    if let Some(rest) = line.strip_prefix("- ") {
        return Some(("-", rest.trim()));
    }
    let markers_len = line.chars().take_while(|c| *c == '*' || *c == '#').count();
    if markers_len == 0 {
        return None;
    }
    let rest = line[markers_len..].strip_prefix(' ')?;
    Some((&line[..markers_len], rest.trim()))
}

/// Splits on Jira's forced line break, `\\`.
fn split_breaks(text: &str) -> Vec<&str> {
    text.split("\\\\").collect()
}

#[cfg(test)]
mod tests;
