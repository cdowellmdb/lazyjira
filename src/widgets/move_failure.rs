use std::collections::VecDeque;

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::UnicodeWidthChar;

use super::form::{self, Row};
use crate::app::App;
use crate::moves::MoveFailure;
use crate::style;

/// The message, a blank line and Jira's error, which is not cut or reflowed.
fn lines(failure: &MoveFailure) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "Jira did not move {} to {}. Its status is unchanged.",
                failure.key, failure.target
            ),
            style::error().patch(style::heading()),
        )),
        Line::from(""),
    ];
    lines.extend(Text::raw(failure.error.clone()).lines);
    lines
}

/// The rows `line` takes in `width` columns when a `Paragraph` wraps it without trimming
/// (`Wrap { trim: false }`): a port of ratatui's `WordWrapper`, counting rows instead of keeping
/// them. A test compares it with what the `Paragraph` draws. Ratatui breaks the text into
/// graphemes and this counts `char`s, so a multi-character grapheme (an emoji with a joiner)
/// can make it differ by a row.
fn wrapped_rows(line: &str, width: u16) -> usize {
    let max = usize::from(width);
    if max == 0 {
        return 0;
    }
    let mut rows = 0;
    // The line being built, as the widths it holds, and the word and whitespace after it.
    let (mut line_len, mut line_width) = (0, 0);
    let (mut word_len, mut word_width) = (0, 0);
    let mut spaces: VecDeque<usize> = VecDeque::new();
    let mut space_width = 0;
    let mut after_word = false;
    for c in line.chars() {
        let is_space = c.is_whitespace() && c != '\u{00a0}';
        let w = c.width().unwrap_or(0);
        if w > max {
            continue;
        }
        let word_ended = after_word && is_space;
        let overflows = line_len == 0 && word_width + space_width + w > max;
        if word_ended || overflows {
            line_len += spaces.len() + word_len;
            line_width += space_width + word_width;
            spaces.clear();
            (space_width, word_len, word_width) = (0, 0, 0);
        }
        let full = line_width >= max;
        let word_overflows = w > 0 && line_width + space_width + word_width >= max;
        if full || word_overflows {
            let mut room = max.saturating_sub(line_width);
            rows += 1;
            (line_len, line_width) = (0, 0);
            while let Some(&space) = spaces.front() {
                if space > room {
                    break;
                }
                space_width -= space;
                room -= space;
                spaces.pop_front();
            }
            if is_space && spaces.is_empty() {
                continue;
            }
        }
        if is_space {
            space_width += w;
            spaces.push_back(w);
        } else {
            word_len += 1;
            word_width += w;
        }
        after_word = !is_space;
    }
    // A line of only whitespace takes the empty row ratatui pushes for it, and its whitespace.
    if line_len == 0 && word_len == 0 && !spaces.is_empty() {
        rows += 1;
    }
    if line_len + spaces.len() + word_len > 0 {
        rows += 1;
    }
    rows.max(1)
}

/// The rows `lines` take in `width` columns.
fn wrapped_height(lines: &[Line], width: u16) -> usize {
    lines
        .iter()
        .map(|line| wrapped_rows(&line.to_string(), width))
        .sum()
}

/// The panel's body: the message, a blank line and Jira's error, as many rows as they wrap to
/// in `width` columns.
fn rows(app: &App, width: u16) -> [Row; 1] {
    let wrapped = app
        .moves
        .failures()
        .first()
        .map_or(0, |failure| wrapped_height(&lines(failure), width));
    [Row::flex(wrapped.min(usize::from(u16::MAX)) as u16, 1)]
}

/// What the panel needs in a content area `width` columns wide.
pub fn need(app: &App, width: u16) -> form::Need {
    form::need(&rows(app, width.saturating_sub(form::PANEL_MARGIN)))
}

/// Shows the oldest move Jira rejected, with Jira's full error. When the error is taller than
/// the panel, `j/k` scroll it.
pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let failures = app.moves.failures();
    let Some(failure) = failures.first() else {
        return;
    };
    let more = match failures.len() - 1 {
        0 => String::new(),
        more => format!("({more} more)"),
    };
    let (body, footer) = form::render_panel_frame(f, app, area, form::title("Move failed", &more));
    let [text] = form::split(rows(app, body.width), body);
    let lines = lines(failure);
    let max = wrapped_height(&lines, text.width).saturating_sub(usize::from(text.height));
    app.moves
        .set_failure_scroll_max(max.min(usize::from(u16::MAX)) as u16);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.moves.failure_scroll(), 0)),
        text,
    );
    let hints: &[(&str, &str)] = if max > 0 {
        &[("j/k", "scroll"), ("Esc", "dismiss")]
    } else {
        &[("Esc", "dismiss")]
    };
    form::button_row(
        f,
        app,
        footer,
        &[("Dismiss", KeyCode::Enter), ("Browser", KeyCode::Char('o'))],
        hints,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::widgets::Widget;

    /// The rows a `Paragraph` draws `text` over in `width` columns, which is how many it takes
    /// when `text` ends in something visible.
    fn drawn_rows(text: &str, width: u16) -> usize {
        let area = Rect::new(0, 0, width, 200);
        let mut buffer = Buffer::empty(area);
        Paragraph::new(Text::raw(text))
            .wrap(Wrap { trim: false })
            .render(area, &mut buffer);
        (0..area.height)
            .rev()
            .find(|&y| (0..width).any(|x| buffer[(x, y)].symbol() != " "))
            .map_or(0, |y| usize::from(y) + 1)
    }

    #[test]
    fn the_row_count_is_what_a_paragraph_draws_at_exact_boundaries() {
        // (text, width, rows): a word that fits exactly, one cell more, and a long word.
        for (text, width, rows) in [
            ("abcde", 5, 1),
            ("abcdef", 5, 2),
            ("abcdefghijk", 5, 3),
            ("ab cd", 5, 1),
            ("ab cde", 5, 2),
            ("ab  cd", 5, 2),
            ("日本語", 6, 1),
            ("日本語x", 6, 2),
            ("one\ntwo\n\nx", 5, 4),
            ("  indented", 20, 1),
            ("a                      b", 10, 3),
        ] {
            assert_eq!(wrapped_rows_of(text, width), rows, "{text:?} in {width}");
            assert_eq!(drawn_rows(text, width), rows, "{text:?} drawn in {width}");
        }
    }

    fn wrapped_rows_of(text: &str, width: u16) -> usize {
        Text::raw(text)
            .lines
            .iter()
            .map(|line| wrapped_rows(&line.to_string(), width))
            .sum()
    }

    #[test]
    fn the_row_count_matches_what_a_paragraph_draws_for_many_texts_and_widths() {
        let pieces = [
            "a",
            "bb",
            "ccc",
            "dddddddddddddddddddddddd",
            " ",
            "  ",
            "   ",
            "日本",
            "é",
            "\n",
            "x-y",
            "\u{00a0}",
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move |bound: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize % bound
        };
        let mut counted = 0;
        for _ in 0..3000 {
            let mut text: String = (0..1 + next(14))
                .map(|_| pieces[next(pieces.len())])
                .collect();
            text.push('Z');
            let width = 4 + next(14) as u16;
            // Ratatui 0.29 panics drawing whitespace wider than the area, which a Jira error
            // never has at these widths; leave those texts out.
            let Ok(drawn) = std::panic::catch_unwind(|| drawn_rows(&text, width)) else {
                continue;
            };
            counted += 1;
            assert_eq!(
                wrapped_rows_of(&text, width),
                drawn,
                "{text:?} in {width} columns"
            );
        }
        assert!(counted > 2000, "{counted} texts compared");
    }
}
