//! Renders Jira wiki markup (the format `jira issue view --raw` returns for
//! descriptions) as styled lines, word-wrapped to a width so list items,
//! quotes and code blocks keep their indent and gutter on wrapped lines.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

// Plain text uses the terminal's own foreground, so it reads in light and
// dark themes alike.
fn text_style() -> Style {
    Style::default().fg(Color::Reset)
}

fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn link_style() -> Style {
    Style::default()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::UNDERLINED)
}

fn mono_style(base: Style) -> Style {
    base.fg(Color::LightRed)
}

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
                self.table(&rows);
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
                self.push_wrapped(
                    vec![],
                    vec![],
                    inline(title, text_style().add_modifier(Modifier::BOLD)),
                );
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
            self.lines
                .push(Line::from(Span::styled("─".repeat(self.width), muted())));
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
                self.push_wrapped(vec![], vec![], inline(segment, text_style()));
            }
        }

        if closes_quote {
            self.in_quote = !self.in_quote;
        }
    }

    fn heading(&mut self, level: u8, text: &str) {
        self.blank();
        let style = match level {
            1 => Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            2 => Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            _ => Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        };
        self.push_wrapped(vec![], vec![], inline(text, style));
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
            Style::default().fg(Color::Cyan),
        )];
        let hang = vec![Span::raw(
            " ".repeat(indent.len() + marker.chars().count() + 1),
        )];
        for (n, segment) in split_breaks(item).into_iter().enumerate() {
            let prefix = if n == 0 { first.clone() } else { hang.clone() };
            self.push_wrapped(prefix, hang.clone(), inline(segment, text_style()));
        }
    }

    fn quote(&mut self, text: &str) {
        let gutter = vec![Span::styled("▎ ", Style::default().fg(Color::Blue))];
        let style = text_style().add_modifier(Modifier::ITALIC);
        for segment in split_breaks(text) {
            self.push_wrapped(gutter.clone(), gutter.clone(), inline(segment, style));
        }
    }

    fn code_line(&mut self, text: &str) {
        let gutter = vec![Span::styled("│ ", muted())];
        let content = vec![Span::styled(
            text.to_string(),
            Style::default().fg(Color::Reset),
        )];
        self.push_wrapped(gutter.clone(), gutter, content);
    }

    fn table(&mut self, rows: &[String]) {
        let rows: Vec<Vec<(bool, Vec<Span<'static>>)>> = rows
            .iter()
            .map(|row| {
                parse_table_row(row)
                    .into_iter()
                    .map(|(header, cell)| {
                        let style = if header {
                            Style::default()
                                .fg(Color::Reset)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            text_style()
                        };
                        (header, inline(cell.trim(), style))
                    })
                    .collect()
            })
            .collect();

        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut widths = vec![0usize; columns];
        for row in &rows {
            for (c, (_, spans)) in row.iter().enumerate() {
                widths[c] = widths[c].max(spans_width(spans));
            }
        }
        let separator = " │ ";
        let total: usize =
            widths.iter().sum::<usize>() + separator.len() * columns.saturating_sub(1);
        let aligned = total <= self.width;

        for (r, row) in rows.iter().enumerate() {
            let mut content: Vec<Span<'static>> = Vec::new();
            for (c, (_, spans)) in row.iter().enumerate() {
                if c > 0 {
                    content.push(Span::styled(" │ ", muted()));
                }
                content.extend(spans.iter().cloned());
                if aligned {
                    let pad = widths[c] - spans_width(spans);
                    if pad > 0 && c + 1 < row.len() {
                        content.push(Span::raw(" ".repeat(pad)));
                    }
                }
            }
            self.push_wrapped(vec![], vec![], content);

            let is_header = !row.is_empty() && row.iter().all(|(h, _)| *h);
            let next_is_header = rows
                .get(r + 1)
                .is_some_and(|next| !next.is_empty() && next.iter().all(|(h, _)| *h));
            if aligned && is_header && !next_is_header && r + 1 < rows.len() {
                let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
                self.lines
                    .push(Line::from(Span::styled(rule.join("─┼─"), muted())));
            }
        }
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

/// Splits a table row into cells, marking header cells (`||`). Pipes inside
/// links (`[text|url]`) and macros don't split cells.
fn parse_table_row(row: &str) -> Vec<(bool, String)> {
    let chars: Vec<char> = row.chars().collect();
    let mut cells = Vec::new();
    let mut current: Option<(bool, String)> = None;
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '|' && depth == 0 {
            let header = chars.get(i + 1) == Some(&'|');
            if let Some(cell) = current.take() {
                cells.push(cell);
            }
            current = Some((header, String::new()));
            i += if header { 2 } else { 1 };
            continue;
        }
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth = (depth - 1).max(0),
            _ => {}
        }
        if let Some((_, text)) = current.as_mut() {
            text.push(c);
        }
        i += 1;
    }
    if let Some(cell) = current {
        if !cell.1.trim().is_empty() {
            cells.push(cell);
        }
    }
    cells
}

pub fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.width()).sum()
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

    fn find(&self, from: usize, pat: &str) -> Option<usize> {
        (from..self.chars.len()).find(|&j| self.starts_with(j, pat))
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

        if self.starts_with(i, "{{") {
            let end = self.find(i + 2, "}}")?;
            let code: String = self.chars[i + 2..end].iter().collect();
            self.emit(code, mono_style(self.style));
            return Some(end + 2);
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
            self.emit(
                format!("[image: {}]", name),
                muted().add_modifier(Modifier::ITALIC),
            );
            return Some(end + 1);
        }

        if c == '(' && self.at_word_start(i) {
            for (pat, symbol, color) in [
                ("(/)", "✓", Color::Green),
                ("(x)", "✗", Color::Red),
                ("(!)", "⚠", Color::Yellow),
                ("(i)", "ℹ", Color::LightBlue),
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
            while end < self.chars.len()
                && !self.chars[end].is_whitespace()
                && !"<>\"|[]".contains(self.chars[end])
            {
                end += 1;
            }
            while end > i && ".,;:!?)'".contains(self.chars[end - 1]) {
                end -= 1;
            }
            let url: String = self.chars[i..end].iter().collect();
            self.emit(url, link_style());
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
            '`' => Modifier::empty(),
            _ => return None,
        };
        let end = self.closing(i, c)?;
        if c == '`' {
            let code: String = self.chars[i + 1..end].iter().collect();
            self.emit(code, mono_style(self.style));
        } else {
            self.nested(i + 1, end, self.style.add_modifier(modifier));
        }
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
                self.emit(url.to_string(), link_style());
            } else {
                self.out.extend(inline(text, link_style()));
                if !url.is_empty() && url != text {
                    self.emit(format!(" ({})", url), muted());
                }
            }
            return true;
        }
        let target = inner.trim();
        if target.contains("://") || target.starts_with("mailto:") {
            self.emit(target.to_string(), link_style());
            return true;
        }
        if let Some(attachment) = target.strip_prefix('^') {
            self.emit(attachment.to_string(), link_style());
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

/// Word-wraps `content` to `width`, starting the first line with `first` and
/// later lines with `rest`. Words too long for a line are broken. Lines break
/// only at plain spaces, so text joined by non-breaking spaces stays together.
pub fn wrap(
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
    content: Vec<Span<'static>>,
    width: usize,
) -> Vec<Line<'static>> {
    let chars: Vec<(char, Style)> = content
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c, s.style)))
        .collect();

    // Alternate runs of whitespace and non-whitespace.
    let mut runs: Vec<(bool, &[(char, Style)])> = Vec::new();
    let mut start = 0;
    for i in 1..=chars.len() {
        if i == chars.len() || (chars[i].0 == ' ') != (chars[start].0 == ' ') {
            runs.push((chars[start].0 == ' ', &chars[start..i]));
            start = i;
        }
    }

    let rest_width = spans_width(&rest);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut line = LineBuilder::new(first);
    let mut pending_space: &[(char, Style)] = &[];

    for (is_space, run) in runs {
        if is_space {
            pending_space = run;
            continue;
        }
        let word_width = run_width(run);
        if line.width + run_width(pending_space) + word_width <= width {
            line.push_all(pending_space);
            line.push_all(run);
        } else if !line.empty && rest_width + word_width <= width {
            lines.push(line.finish());
            line = LineBuilder::new(rest.clone());
            line.push_all(run);
        } else {
            // Too long for any line: break it where the line fills up.
            let space = if line.empty { &[][..] } else { pending_space };
            for &(c, style) in space.iter().chain(run) {
                if line.width + c.width().unwrap_or(0) > width && !line.empty {
                    lines.push(line.finish());
                    line = LineBuilder::new(rest.clone());
                    if c == ' ' {
                        continue;
                    }
                }
                line.push(c, style);
            }
        }
        pending_space = &[];
    }
    lines.push(line.finish());
    lines
}

fn run_width(run: &[(char, Style)]) -> usize {
    run.iter().map(|(c, _)| c.width().unwrap_or(0)).sum()
}

struct LineBuilder {
    spans: Vec<Span<'static>>,
    width: usize,
    /// Nothing but the prefix yet.
    empty: bool,
}

impl LineBuilder {
    fn new(prefix: Vec<Span<'static>>) -> Self {
        let width = spans_width(&prefix);
        LineBuilder {
            spans: prefix,
            width,
            empty: true,
        }
    }

    fn push(&mut self, c: char, style: Style) {
        self.width += c.width().unwrap_or(0);
        // Don't append to the prefix's last span.
        let merge = !self.empty;
        self.empty = false;
        match self.spans.last_mut() {
            Some(last) if merge && last.style == style => last.content.to_mut().push(c),
            _ => self.spans.push(Span::styled(c.to_string(), style)),
        }
    }

    fn push_all(&mut self, run: &[(char, Style)]) {
        for &(c, style) in run {
            self.push(c, style);
        }
    }

    fn finish(self) -> Line<'static> {
        Line::from(self.spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn span<'a>(spans: &'a [Span<'static>], content: &str) -> &'a Span<'static> {
        spans
            .iter()
            .find(|s| s.content == content)
            .unwrap_or_else(|| panic!("no span {:?} in {:?}", content, spans))
    }

    #[test]
    fn inline_emphasis() {
        let spans = inline("*Stakeholders:* a _b_ -c- +d+ {{e}}", text_style());
        assert!(span(&spans, "Stakeholders:")
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(span(&spans, "b")
            .style
            .add_modifier
            .contains(Modifier::ITALIC));
        assert!(span(&spans, "c")
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT));
        assert!(span(&spans, "d")
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
        assert_eq!(span(&spans, "e").style.fg, Some(Color::LightRed));
    }

    #[test]
    fn markers_inside_words_stay_plain() {
        for s in [
            "snake_case_name",
            "open-source repos",
            "a * b * c",
            "C++ and x+y+z",
        ] {
            let spans = inline(s, text_style());
            assert_eq!(text(&[Line::from(spans.clone())]), vec![s.to_string()]);
            assert!(spans.iter().all(|s| s.style == text_style()), "{:?}", spans);
        }
    }

    #[test]
    fn nested_emphasis() {
        let spans = inline("*bold _both_*", text_style());
        let both = span(&spans, "both").style.add_modifier;
        assert!(both.contains(Modifier::BOLD | Modifier::ITALIC));
    }

    #[test]
    fn links_and_mentions() {
        let spans = inline(
            "at [10gen/mage|https://github.com/10gen/mage] by [~jdoe], see https://x.io/a.",
            text_style(),
        );
        assert_eq!(
            text(&[Line::from(spans.clone())]),
            vec!["at 10gen/mage (https://github.com/10gen/mage) by @jdoe, see https://x.io/a."]
        );
        assert_eq!(span(&spans, "10gen/mage").style, link_style());
        assert_eq!(span(&spans, "https://x.io/a").style, link_style());
        assert_eq!(span(&spans, "@jdoe").style.fg, Some(Color::Cyan));
    }

    #[test]
    fn brackets_that_are_not_links_stay() {
        let spans = inline("[WIP] thing", text_style());
        assert_eq!(text(&[Line::from(spans)]), vec!["[WIP] thing"]);
    }

    #[test]
    fn colors_icons_images_dashes() {
        let spans = inline(
            "{color:red}hot{color} (/) !shot.png|thumbnail! a -- b --- c",
            text_style(),
        );
        assert_eq!(span(&spans, "hot").style.fg, Some(Color::Red));
        assert_eq!(span(&spans, "✓").style.fg, Some(Color::Green));
        assert_eq!(
            text(&[Line::from(spans)]),
            vec!["hot ✓ [image: shot.png] a – b — c"]
        );
    }

    #[test]
    fn headings_lists_and_rules() {
        let lines = render(
            "h1. Title\nintro\n# one\n# two\n## nested\n# three\n* dot\n** sub\n----",
            30,
        );
        assert_eq!(
            text(&lines),
            vec![
                "Title",
                "intro",
                "1. one",
                "2. two",
                "  1. nested",
                "3. three",
                "• dot",
                "  ◦ sub",
                &"─".repeat(30),
            ]
        );
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn list_items_wrap_with_hanging_indent() {
        let lines = render("* alpha beta gamma delta", 14);
        assert_eq!(text(&lines), vec!["• alpha beta", "  gamma delta"]);
    }

    #[test]
    fn paragraphs_wrap_and_break_long_words() {
        let lines = render(&format!("hi {}", "x".repeat(25)), 20);
        assert_eq!(text(&lines), vec!["hi xxxxxxxxxxxxxxxxx", "xxxxxxxx"]);
    }

    #[test]
    fn code_blocks_are_not_parsed() {
        let lines = render("{code:rust}\nlet *x* = [a|b];\n{code}\nafter", 40);
        assert_eq!(text(&lines), vec!["│ let *x* = [a|b];", "after"]);

        let lines = render("{noformat}raw _text_{noformat}", 40);
        assert_eq!(text(&lines), vec!["│ raw _text_"]);

        let lines = render("```\n*x*\n```", 40);
        assert_eq!(text(&lines), vec!["│ *x*"]);
    }

    #[test]
    fn quotes() {
        let lines = render("bq. quoted\n{quote}\nmulti\n{quote}\nplain", 40);
        assert_eq!(text(&lines), vec!["▎ quoted", "▎ multi", "plain"]);
    }

    #[test]
    fn tables_align_columns() {
        let lines = render("||Name||Owner||\n|MAGE|[~cdowell]|\n|x|y|", 40);
        assert_eq!(
            text(&lines),
            vec![
                "Name │ Owner",
                "─────┼─────────",
                "MAGE │ @cdowell",
                "x    │ y",
            ]
        );
    }

    #[test]
    fn table_cells_keep_link_pipes() {
        let cells = parse_table_row("|[a|http://b]|c|");
        assert_eq!(
            cells,
            vec![
                (false, "[a|http://b]".to_string()),
                (false, "c".to_string())
            ]
        );
    }

    #[test]
    fn blank_lines_collapse_and_trail_trimmed() {
        let lines = render("a\n\n\n\nb\n\n", 40);
        assert_eq!(text(&lines), vec!["a", "", "b"]);
    }

    #[test]
    fn forced_line_breaks() {
        let lines = render("one\\\\two", 40);
        assert_eq!(text(&lines), vec!["one", "two"]);
    }
}
