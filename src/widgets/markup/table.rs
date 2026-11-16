//! The table rows of a description or comment, aligned in columns.

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::{inline, muted, spans_width, text_style, Renderer};

impl Renderer {
    pub(super) fn table(&mut self, rows: &[String]) {
        let rows: Vec<Vec<(bool, Vec<Span<'static>>)>> = rows
            .iter()
            .map(|row| {
                parse_table_row(row)
                    .into_iter()
                    .map(|(header, cell)| {
                        let style = if header {
                            text_style().add_modifier(Modifier::BOLD)
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
}

/// Splits a table row into cells, marking header cells (`||`). Pipes inside
/// links (`[text|url]`) and macros don't split cells.
pub(super) fn parse_table_row(row: &str) -> Vec<(bool, String)> {
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
