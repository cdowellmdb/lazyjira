//! The table rows of a description or comment, aligned in columns.

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::inline::is_escapable;
use super::{bold, inline, muted, spans_width, text_style, Renderer};
use crate::views::common::fit;

type Cell = (bool, Vec<Span<'static>>);

const SEPARATOR: &str = " │ ";

impl Renderer {
    pub(super) fn table(&mut self, rows: &[String]) {
        let rows: Vec<Vec<Cell>> = rows
            .iter()
            .map(|row| {
                parse_table_row(row)
                    .into_iter()
                    .map(|(header, cell)| {
                        let style = if header { bold() } else { text_style() };
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
        let total: usize =
            widths.iter().sum::<usize>() + SEPARATOR.width() * columns.saturating_sub(1);
        if total > self.width {
            return self.records(&rows);
        }

        for (r, row) in rows.iter().enumerate() {
            let mut content: Vec<Span<'static>> = Vec::new();
            for (c, (_, spans)) in row.iter().enumerate() {
                if c > 0 {
                    content.push(Span::styled(SEPARATOR, muted()));
                }
                content.extend(spans.iter().cloned());
                let pad = widths[c] - spans_width(spans);
                if pad > 0 && c + 1 < row.len() {
                    content.push(Span::raw(" ".repeat(pad)));
                }
            }
            self.push_wrapped(vec![], vec![], content);

            let next_is_header = rows.get(r + 1).is_some_and(|next| is_header_row(next));
            if is_header_row(row) && !next_is_header && r + 1 < rows.len() {
                let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
                self.lines
                    .push(Line::from(Span::styled(rule.join("─┼─"), muted())));
            }
        }
    }

    /// A table too wide to align, one record per body row: its first cell in bold, then a line
    /// for each other cell, `  Header  value`, with the header muted and padded to the widest one
    /// and the value wrapped under itself. Without a header row the lines have no label.
    fn records(&mut self, rows: &[Vec<Cell>]) {
        let header = rows
            .first()
            .filter(|row| rows.len() > 1 && is_header_row(row));
        let body = &rows[usize::from(header.is_some())..];
        let label = |c: usize| {
            header
                .and_then(|row| row.get(c))
                .map(|(_, spans)| spans.iter().map(|s| s.content.as_ref()).collect::<String>())
                .unwrap_or_default()
        };
        let columns = body.iter().map(Vec::len).max().unwrap_or(0);
        let label_width = (1..columns)
            .map(|c| label(c).width())
            .max()
            .unwrap_or(0)
            .min(self.width / 3);
        let lead = if label_width == 0 { 2 } else { label_width + 4 };

        for (n, row) in body.iter().enumerate() {
            if n > 0 {
                self.blank();
            }
            for (c, (_, spans)) in row.iter().enumerate() {
                if spans_width(spans) == 0 {
                    continue;
                }
                if c == 0 {
                    let spans = spans
                        .iter()
                        .map(|s| {
                            Span::styled(s.content.clone(), s.style.add_modifier(Modifier::BOLD))
                        })
                        .collect();
                    self.push_wrapped(vec![], vec![], spans);
                    continue;
                }
                let first = if label_width == 0 {
                    Span::raw("  ")
                } else {
                    Span::styled(format!("  {}  ", fit(&label(c), label_width)), muted())
                };
                self.push_wrapped(
                    vec![first],
                    vec![Span::raw(" ".repeat(lead))],
                    spans.clone(),
                );
            }
        }
    }
}

fn is_header_row(row: &[Cell]) -> bool {
    !row.is_empty() && row.iter().all(|(header, _)| *header)
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
        if c == '\\' && chars.get(i + 1).is_some_and(|n| is_escapable(*n)) {
            if let Some((_, text)) = current.as_mut() {
                text.push(c);
                text.push(chars[i + 1]);
            }
            i += 2;
            continue;
        }
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
