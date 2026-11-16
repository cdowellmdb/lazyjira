use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::style;
use crate::widgets::{form, markup};

// An empty key marks a section heading.
const MAIN: &[(&str, &str)] = &[
    ("", "Navigation"),
    ("Tab", "Next tab"),
    ("Shift+Tab", "Previous tab / filter sidebar"),
    ("j/k · ↑/↓", "Move selection"),
    ("Enter", "Open ticket or epic"),
    ("z / Z", "Fold parent or group / all groups"),
    ("?", "Keyboard shortcuts"),
    ("q", "Quit lazyjira"),
    ("", "Selection & bulk actions"),
    ("Space", "Mark ticket or group"),
    ("A / u", "Select all / clear selection"),
    ("Drag", "Select text with the mouse"),
    ("Ctrl+C", "Copy selected text"),
    ("B", "Move or assign selected tickets"),
    ("U", "Upload tickets from CSV"),
    ("", "Search & filtering"),
    ("/", "Search tickets, labels, or people"),
    ("↑/↓ · Ctrl+j/k", "Navigate while searching"),
    ("Esc", "Leave search"),
    ("", "Status filters · My Work & Team"),
    ("d", "Show / hide done tickets"),
    ("f / F", "Next / previous status focus"),
    ("", "Create & refresh"),
    ("c", "Create ticket"),
    ("S", "Settings: teammates, pinned epics, Done visibility, starting tab, theme, wrapped summaries"),
    ("r", "Refresh tickets"),
];

const DETAIL: &[(&str, &str)] = &[
    ("", "Detail navigation"),
    ("j/k · ↑/↓", "Scroll"),
    ("PgUp/PgDn", "Scroll a page (Space: next)"),
    ("g/G · Home/End", "Jump to top / bottom"),
    ("←/→", "Previous / next ticket"),
    ("z", "Toggle full screen"),
    ("o", "Open in browser"),
    ("Esc", "Close detail / go back"),
    ("", "Ticket actions"),
    ("m", "Move ticket"),
    ("C", "Add comment"),
    ("a", "Assign ticket"),
    ("e", "Edit summary, labels & description"),
    ("h", "Activity: comments, newest first"),
    ("Ctrl+E / F4", "External editor (text fields)"),
    ("Shift+Enter", "Newline (comments & descriptions)"),
    ("", "Move picker"),
    ("j/k · ↑/↓", "Choose transition"),
    (crate::cache::MOVE_SHORTCUTS, "Pick destination by status"),
    ("Shift+key", "Move immediately"),
    ("Enter / y", "Select / confirm move"),
    ("Esc", "Cancel / go back"),
    ("o", "Open ticket after a failed move"),
    ("", "Filters tab"),
    ("j / k", "Navigate filters or results"),
    ("Tab / S-Tab", "Results or next tab / sidebar or prev tab"),
    ("Enter", "Run filter / open result"),
    ("Space · A/u · B", "Mark / select / bulk (results)"),
    ("z / Z", "Fold parent or status / all statuses"),
    ("U", "Upload tickets from CSV"),
    ("n / e / x", "New / edit / delete filter"),
];

/// Every key column of the help, for the footer's test that each hint is documented.
#[cfg(test)]
pub(crate) fn keys() -> impl Iterator<Item = &'static str> {
    MAIN.iter().chain(DETAIL).map(|(keys, _)| *keys)
}

fn rows(bindings: &[(&str, &str)], width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let key_width = if width >= 40 { 17 } else { 0 };
    for &(key, label) in bindings {
        if key.is_empty() {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.extend(markup::wrap(
                vec![],
                vec![],
                vec![Span::styled(label.to_string(), style::heading())],
                width as usize,
            ));
        } else {
            let key = Span::styled(format!("{key:<key_width$}"), style::key_hint());
            if key_width == 0 {
                lines.extend(markup::wrap(
                    vec![],
                    vec![],
                    vec![key.clone()],
                    width as usize,
                ));
            }
            lines.extend(markup::wrap(
                if key_width > 0 { vec![key] } else { vec![] },
                vec![Span::raw(" ".repeat(key_width))],
                vec![Span::styled(label.to_string(), style::primary())],
                width as usize,
            ));
        }
    }
    lines
}

/// The help as a sheet over the whole frame: two columns when the width allows, one below.
pub fn render(f: &mut ratatui::Frame, app: &App) {
    let inner = form::render_sheet(f, app, "Keyboard shortcuts");
    let wide = inner.width >= 110;
    let column_width = if wide {
        (inner.width - 4) / 2
    } else {
        inner.width
    };
    let mut left = rows(MAIN, column_width);
    let right = rows(DETAIL, column_width);
    let columns = if wide {
        vec![left, right]
    } else {
        left.push(Line::default());
        left.extend(right);
        vec![left]
    };
    let count = columns.iter().map(Vec::len).max().unwrap_or(0);
    let [body, hint] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(u16::from(!inner.is_empty())),
    ])
    .areas(inner);
    let max = count.saturating_sub(body.height as usize) as u16;
    let scroll = app.keybindings_scroll.min(max);
    app.keybindings_scroll_max.set(max);
    app.keybindings_page_height.set(body.height.max(1));
    if max > 0 {
        // Where the view is, ending before the `[×]`.
        let head = Rect::new(inner.x, 0, inner.width.saturating_sub(7), 1);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(
                    "{}–{} / {}",
                    scroll + 1,
                    (scroll + body.height).min(count as u16),
                    count
                ),
                style::secondary(),
            )))
            .right_aligned(),
            head,
        );
    }
    let column_areas = Layout::horizontal(if wide {
        vec![
            Constraint::Length(column_width),
            Constraint::Length(4),
            Constraint::Min(0),
        ]
    } else {
        vec![Constraint::Min(0)]
    })
    .split(body);
    for (i, lines) in columns.into_iter().enumerate() {
        f.render_widget(
            Paragraph::new(lines).scroll((scroll, 0)),
            column_areas[i * 2],
        );
    }
    let scroll_hint = match (max, inner.width >= 60) {
        (0, _) => "",
        (_, true) => "↑↓/j/k · PgUp/PgDn ",
        (_, false) => "↑↓ ",
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(scroll_hint, style::key_hint()),
            Span::styled(if max > 0 { "scroll   " } else { "" }, style::secondary()),
            Span::styled("Esc/?", style::key_hint()),
            Span::styled(" close", style::secondary()),
        ])),
        hint,
    );
    app.mouse_targets.borrow_mut().push((
        hint,
        crate::mouse::Target::Key(crossterm::event::KeyCode::Esc),
    ));
}
