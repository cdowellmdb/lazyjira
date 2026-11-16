//! Word wrapping of styled spans, keeping a prefix on every line.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

use crate::views::common::text_width;

/// The columns `spans` take, counted per grapheme as ratatui draws each span.
pub(super) fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| text_width(&s.content)).sum()
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
    let chars: Vec<(&str, Style)> = content
        .iter()
        .flat_map(|s| s.content.graphemes(true).map(move |g| (g, s.style)))
        .collect();

    // Alternate runs of whitespace and non-whitespace.
    let mut runs: Vec<(bool, &[(&str, Style)])> = Vec::new();
    let mut start = 0;
    for i in 1..=chars.len() {
        if i == chars.len() || (chars[i].0 == " ") != (chars[start].0 == " ") {
            runs.push((chars[start].0 == " ", &chars[start..i]));
            start = i;
        }
    }

    let rest_width = spans_width(&rest);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut line = LineBuilder::new(first);
    let mut pending_space: &[(&str, Style)] = &[];

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
            for &(g, style) in space.iter().chain(run) {
                if line.width + text_width(g) > width && !line.empty {
                    lines.push(line.finish());
                    line = LineBuilder::new(rest.clone());
                    if g == " " {
                        continue;
                    }
                }
                line.push(g, style);
            }
        }
        pending_space = &[];
    }
    lines.push(line.finish());
    lines
}

fn run_width(run: &[(&str, Style)]) -> usize {
    run.iter().map(|(g, _)| text_width(g)).sum()
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

    fn push(&mut self, grapheme: &str, style: Style) {
        self.width += text_width(grapheme);
        // Don't append to the prefix's last span.
        let merge = !self.empty;
        self.empty = false;
        match self.spans.last_mut() {
            Some(last) if merge && last.style == style => last.content.to_mut().push_str(grapheme),
            _ => self.spans.push(Span::styled(grapheme.to_string(), style)),
        }
    }

    fn push_all(&mut self, run: &[(&str, Style)]) {
        for &(g, style) in run {
            self.push(g, style);
        }
    }

    fn finish(self) -> Line<'static> {
        Line::from(self.spans)
    }
}
