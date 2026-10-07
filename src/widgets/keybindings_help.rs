use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Padding, Paragraph};

use crate::app::App;
use crate::views::common::panel;
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
    ("S", "Team, epic & startup preferences"),
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
    ("p/w/n/t/v/b/c", "Pick destination by status"),
    ("Shift+key", "Move immediately"),
    ("Enter / y", "Select / confirm move"),
    ("Esc", "Cancel / go back"),
    ("o", "Open ticket after a failed move"),
    ("", "Filters tab"),
    ("j / k", "Navigate filters or results"),
    ("Tab / S-Tab", "Results or next tab / sidebar"),
    ("Enter", "Run filter / open result"),
    ("Space · A/u · B", "Mark / select / bulk (results)"),
    ("z / Z", "Fold parent or status / all statuses"),
    ("U", "Upload tickets from CSV"),
    ("n / e / x", "New / edit / delete filter"),
];

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
                vec![Span::styled(
                    label.to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )],
                width as usize,
            ));
        } else {
            let key = Span::styled(
                format!("{key:<key_width$}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
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
                vec![Span::styled(
                    label.to_string(),
                    Style::default().fg(Color::Reset),
                )],
                width as usize,
            ));
        }
    }
    lines
}

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let available = form::centered_rect(92, 90, f.area());
    let width = available.width.min(128);
    let inner_width = width.saturating_sub(6);
    let wide = inner_width >= 110;
    let column_width = if wide {
        (inner_width - 4) / 2
    } else {
        inner_width
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
    let height = available.height.min(count as u16 + 6);
    let area = Rect {
        x: f.area().x + (f.area().width - width) / 2,
        y: f.area().y + (f.area().height - height) / 2,
        width,
        height,
    };
    f.render_widget(Clear, area);
    let mut block = panel()
        .padding(Padding::new(2, 2, 1, 0))
        .title(Span::styled(
            " Keyboard shortcuts ",
            Style::default().add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    app.text_selection.borrow_mut().area = inner;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(inner);
    let max = count.saturating_sub(chunks[0].height as usize) as u16;
    let scroll = app.keybindings_scroll.min(max);
    app.keybindings_scroll_max.set(max);
    app.keybindings_page_height.set(chunks[0].height.max(1));
    if max > 0 {
        block = block.title(
            Line::from(Span::styled(
                format!(
                    " {}–{} / {} ",
                    scroll + 1,
                    (scroll + chunks[0].height).min(count as u16),
                    count
                ),
                Style::default().fg(Color::DarkGray),
            ))
            .right_aligned(),
        );
    }
    f.render_widget(block, area);
    let column_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if wide {
            vec![
                Constraint::Length(column_width),
                Constraint::Length(4),
                Constraint::Min(0),
            ]
        } else {
            vec![Constraint::Min(0)]
        })
        .split(chunks[0]);
    for (i, lines) in columns.into_iter().enumerate() {
        f.render_widget(
            Paragraph::new(lines).scroll((scroll, 0)),
            column_areas[i * 2],
        );
    }
    let mut footer = vec![Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(Color::DarkGray),
    ))];
    footer.extend(markup::wrap(
        vec![],
        vec![],
        vec![
            Span::styled(
                if max == 0 {
                    ""
                } else if inner.width >= 60 {
                    "↑↓/j/k · PgUp/PgDn "
                } else {
                    "↑↓ "
                },
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                if max > 0 { "scroll   " } else { "" },
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("Esc/?", Style::default().fg(Color::Cyan)),
            Span::styled(" close", Style::default().fg(Color::DarkGray)),
        ],
        inner.width as usize,
    ));
    f.render_widget(Paragraph::new(footer), chunks[1]);
    app.mouse_targets.borrow_mut().push((
        chunks[1],
        crate::mouse::Target::Key(crossterm::event::KeyCode::Esc),
    ));
}
