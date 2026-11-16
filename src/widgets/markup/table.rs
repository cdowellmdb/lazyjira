//! The table rows of a description or comment, aligned in columns, or one record per row when
//! they don't fit.

use ratatui::style::Stylize;
use ratatui::text::{Line, Span};

use super::inline::{escaped_char, inline};
use super::wrap::{spans_width, text_width, wrap};
use crate::style;
use crate::views::common::fit;

const SEPARATOR: &str = " │ ";

/// A cell as the row's text split it: `||` makes it a header cell.
#[derive(Debug, PartialEq)]
pub(super) struct Cell {
    pub header: bool,
    pub text: String,
}

/// A row as drawn: the spans of each cell, and whether every cell is a header.
struct Row {
    header: bool,
    cells: Vec<Vec<Span<'static>>>,
}

/// Draws the table `rows` (each starting with `|`) in `width` columns: aligned when every column
/// fits, otherwise as records.
pub(super) fn lines(rows: &[String], width: usize) -> Vec<Line<'static>> {
    let rows: Vec<Row> = rows
        .iter()
        .map(|row| {
            let cells = parse_table_row(row);
            Row {
                header: !cells.is_empty() && cells.iter().all(|cell| cell.header),
                cells: cells
                    .into_iter()
                    .map(|cell| {
                        let spans = inline(cell.text.trim(), style::primary());
                        if cell.header {
                            bold(spans)
                        } else {
                            spans
                        }
                    })
                    .collect(),
            }
        })
        .collect();

    let columns = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
    let mut widths = vec![0usize; columns];
    for row in &rows {
        for (c, spans) in row.cells.iter().enumerate() {
            widths[c] = widths[c].max(spans_width(spans));
        }
    }
    let total: usize =
        widths.iter().sum::<usize>() + text_width(SEPARATOR) * columns.saturating_sub(1);
    if total > width {
        records(&rows, width)
    } else {
        aligned(&rows, &widths, width)
    }
}

fn bold(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    spans.into_iter().map(Stylize::bold).collect()
}

fn aligned(rows: &[Row], widths: &[usize], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let mut content: Vec<Span<'static>> = Vec::new();
        for (c, spans) in row.cells.iter().enumerate() {
            if c > 0 {
                content.push(Span::styled(SEPARATOR, style::secondary()));
            }
            content.extend(spans.iter().cloned());
            let pad = widths[c] - spans_width(spans);
            if pad > 0 && c + 1 < row.cells.len() {
                content.push(Span::raw(" ".repeat(pad)));
            }
        }
        lines.extend(wrap(vec![], vec![], content, width));

        let next_is_header = rows.get(r + 1).is_some_and(|next| next.header);
        if row.header && !next_is_header && r + 1 < rows.len() {
            let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
            lines.push(Line::from(Span::styled(
                rule.join("─┼─"),
                style::secondary(),
            )));
        }
    }
    lines
}

/// A table too wide to align, one record per body row: its first cell in bold, then a line for
/// each other cell, `  Header  value`, with the header muted and padded to the widest one and the
/// value wrapped under itself. Without a header row the lines have no label. A header of one
/// cell has no labels to give, so it heads the records on a line of its own.
fn records(rows: &[Row], width: usize) -> Vec<Line<'static>> {
    let header = rows.first().filter(|row| rows.len() > 1 && row.header);
    let body = &rows[usize::from(header.is_some())..];
    let labels: Vec<String> = header
        .map(|row| {
            row.cells
                .iter()
                .map(|spans| spans.iter().map(|s| s.content.as_ref()).collect())
                .collect()
        })
        .unwrap_or_default();
    let label = |c: usize| labels.get(c).map_or("", String::as_str);
    let columns = body.iter().map(|row| row.cells.len()).max().unwrap_or(0);
    let label_width = (1..columns)
        .map(|c| text_width(label(c)))
        .max()
        .unwrap_or(0)
        .min(width / 3);
    let lead = if label_width == 0 { 2 } else { label_width + 4 };

    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some([title]) = header.map(|row| row.cells.as_slice()) {
        lines.extend(wrap(vec![], vec![], title.clone(), width));
    }
    for row in body {
        // A blank line sets each record off from what comes before it.
        if lines.last().is_some_and(|last| last.width() > 0) {
            lines.push(Line::from(""));
        }
        for (c, spans) in row.cells.iter().enumerate() {
            if spans_width(spans) == 0 {
                continue;
            }
            if c == 0 {
                lines.extend(wrap(vec![], vec![], bold(spans.clone()), width));
                continue;
            }
            let first = if label_width == 0 {
                Span::raw("  ")
            } else {
                Span::styled(
                    format!("  {}  ", fit(label(c), label_width)),
                    style::secondary(),
                )
            };
            lines.extend(wrap(
                vec![first],
                vec![Span::raw(" ".repeat(lead))],
                spans.clone(),
                width,
            ));
        }
    }
    lines
}

/// Splits a table row into cells, marking header cells (`||`). Pipes inside links
/// (`[text|url]`) and macros don't split cells, and neither does an escaped `\|`.
pub(super) fn parse_table_row(row: &str) -> Vec<Cell> {
    let chars: Vec<char> = row.chars().collect();
    let mut cells = Vec::new();
    let mut current: Option<Cell> = None;
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(escaped) = escaped_char(&chars, i) {
            if let Some(cell) = current.as_mut() {
                cell.text.push(c);
                cell.text.push(escaped);
            }
            i += 2;
            continue;
        }
        if c == '|' && depth == 0 {
            let header = chars.get(i + 1) == Some(&'|');
            cells.extend(current.take());
            current = Some(Cell {
                header,
                text: String::new(),
            });
            i += if header { 2 } else { 1 };
            continue;
        }
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth = (depth - 1).max(0),
            _ => {}
        }
        if let Some(cell) = current.as_mut() {
            cell.text.push(c);
        }
        i += 1;
    }
    cells.extend(current.filter(|cell| !cell.text.trim().is_empty()));
    cells
}
