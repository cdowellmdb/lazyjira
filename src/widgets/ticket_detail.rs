use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

use crate::app::{App, DetailMode};
use crate::cache::{ActivityKind, Epic, Status, StatusRules, Ticket};
use crate::move_picker::MovePicker;
use crate::views::common::{panel, status_color};
use crate::widgets::activity::format_timestamp;
use crate::widgets::markup;

/// Width of the field names ("Assignee", ...) in the header.
const FIELD_WIDTH: usize = 10;

const VIEW_HINTS: &[(&str, &str)] = &[
    ("↑↓", "scroll"),
    ("←→", "prev/next"),
    ("m", "move"),
    ("C", "comment"),
    ("a", "assign"),
    ("e", "edit"),
    ("h", "activity"),
    ("o", "browser"),
    ("z", "zoom"),
    ("Esc", "close"),
];

const EPIC_HINTS: &[(&str, &str)] = &[
    ("↑↓", "scroll"),
    ("←→", "prev/next"),
    ("o", "browser"),
    ("z", "zoom"),
    ("Esc", "close"),
];

fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let mut result: String = s.chars().take(max.saturating_sub(3)).collect();
        result.push_str("...");
        result
    } else {
        s.to_string()
    }
}

fn progress_bar(done: usize, total: usize, width: usize) -> String {
    if total == 0 || width == 0 {
        return format!("[{}]", "-".repeat(width));
    }

    let filled = ((done * width) + (total / 2)) / total;
    format!(
        "[{}{}]",
        "#".repeat(filled.min(width)),
        "-".repeat(width.saturating_sub(filled.min(width)))
    )
}

/// Draws the overlay's frame for `key` and returns the area inside it.
fn render_frame(f: &mut ratatui::Frame, app: &App, key: &str) -> (Rect, Rect) {
    let area = if app.detail_fullscreen {
        f.area()
    } else {
        centered_rect(80, 85, f.area())
    };
    f.render_widget(Clear, area);

    let mut block = panel()
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::styled(
                " [×] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", key),
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    if let Some((index, count)) = app.detail_position() {
        block = block.title(
            Line::from(Span::styled(format!(" {} of {} ", index, count), muted())).right_aligned(),
        );
    }
    let inner = block.inner(area);
    app.text_selection.borrow_mut().area = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        area.height.saturating_sub(1),
    );
    f.render_widget(block, area);
    if area.width > 2 && area.height > 0 {
        app.mouse_targets.borrow_mut().push((
            Rect::new(area.x + 1, area.y, 5.min(area.width - 2), 1),
            crate::mouse::Target::CloseDetail,
        ));
    }
    (area, inner)
}

pub fn render(f: &mut ratatui::Frame, app: &App) {
    if let Some(ticket_key) = app.detail_ticket_key.as_ref() {
        if let Some(ticket) = app.find_ticket(ticket_key) {
            let (area, inner) = render_frame(f, app, ticket_key);
            match &app.detail_mode {
                DetailMode::View => render_view(f, area, inner, app, ticket),
                DetailMode::MoveLoading { .. } => render_with_footer(
                    f,
                    inner,
                    vec![Line::from(Span::styled(
                        format!("Loading {}'s transitions from Jira…", ticket_key),
                        Style::default().fg(Color::Yellow),
                    ))],
                    &[("Esc", "cancel")],
                    app,
                ),
                DetailMode::MovePicker(picker) => {
                    render_move_picker(f, inner, ticket, picker, app.status_rules(), app)
                }
                DetailMode::ResolutionPicker { picker, selected } => {
                    render_resolution_picker(f, inner, picker, *selected, app)
                }
                DetailMode::History { scroll } => {
                    crate::widgets::activity::render(f, inner, &ticket.activity, *scroll);
                    crate::widgets::form::buttons(
                        f,
                        app,
                        Rect::new(
                            inner.x,
                            inner.bottom().saturating_sub(1),
                            inner.width,
                            u16::from(inner.height > 0),
                        ),
                        &[("Back", crossterm::event::KeyCode::Esc)],
                    );
                }
            }
            return;
        }
    }

    let epic_key = match app.detail_epic_key.as_ref() {
        Some(k) => k,
        None => return,
    };
    let epic = match app.cache.epics.iter().find(|e| &e.key == epic_key) {
        Some(e) => e,
        None => return,
    };
    let (area, inner) = render_frame(f, app, &epic.key);
    render_epic_view(f, area, inner, app, epic);
}

/// Key hints, wrapped to `width` without splitting a hint.
fn hint_lines(hints: &[(&str, &str)], width: u16) -> Vec<Line<'static>> {
    let mut content = Vec::new();
    for (i, (key, label)) in hints.iter().enumerate() {
        if i > 0 {
            content.push(Span::raw("  "));
        }
        content.push(Span::styled(
            key.replace(' ', "\u{a0}"),
            Style::default().fg(Color::Cyan),
        ));
        content.push(Span::styled(
            format!("\u{a0}{}", label.replace(' ', "\u{a0}")),
            muted(),
        ));
    }
    markup::wrap(vec![], vec![], content, width as usize)
}

/// Draws `hints` at the bottom of `area`, after a blank line, and returns the
/// area above them.
pub(crate) fn render_footer(
    f: &mut ratatui::Frame,
    area: Rect,
    hints: &[(&str, &str)],
    app: &App,
) -> Rect {
    let footer = hint_lines(hints, area.width);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(footer.len() as u16 + 1),
        ])
        .split(area);
    let footer_area = Rect {
        y: chunks[1].y + 1,
        height: chunks[1].height.saturating_sub(1),
        ..chunks[1]
    };
    for (row, line) in footer.iter().take(footer_area.height as usize).enumerate() {
        let mut x = footer_area.x;
        for span in &line.spans {
            let width = span.width() as u16;
            if span.style.fg == Some(Color::Cyan) {
                let key = match span.content.as_ref() {
                    "Esc" => Some(crossterm::event::KeyCode::Esc),
                    "Enter" => Some(crossterm::event::KeyCode::Enter),
                    value if value.chars().count() == 1 => {
                        value.chars().next().map(crossterm::event::KeyCode::Char)
                    }
                    _ => None,
                };
                if let Some(key) = key {
                    app.mouse_targets.borrow_mut().push((
                        Rect::new(
                            x,
                            footer_area.y + row as u16,
                            width.min(footer_area.right().saturating_sub(x)),
                            1,
                        ),
                        crate::mouse::Target::Key(key),
                    ));
                }
                if span.content == "←→" {
                    app.mouse_targets.borrow_mut().push((
                        Rect::new(x, footer_area.y + row as u16, 1, 1),
                        crate::mouse::Target::Key(crossterm::event::KeyCode::Left),
                    ));
                    app.mouse_targets.borrow_mut().push((
                        Rect::new(x + 1, footer_area.y + row as u16, 1, 1),
                        crate::mouse::Target::Key(crossterm::event::KeyCode::Right),
                    ));
                }
            }
            x = x.saturating_add(width);
        }
    }
    f.render_widget(Paragraph::new(footer), footer_area);
    chunks[0]
}

/// Draws `lines` in `body` at the overlay's scroll position, with a scrollbar
/// on the overlay's right border when they don't fit, and records the scroll
/// limits for the scroll keys.
fn render_scrollable(
    f: &mut ratatui::Frame,
    app: &App,
    frame: Rect,
    body: Rect,
    lines: Vec<Line<'static>>,
) {
    let max = lines
        .len()
        .saturating_sub(body.height as usize)
        .min(u16::MAX as usize) as u16;
    app.detail_scroll_max.set(max);
    app.detail_page_height.set(body.height.max(1));
    let scroll = app.detail_scroll.min(max);

    f.render_widget(
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        body,
    );

    if max > 0 {
        let mut state = ScrollbarState::new(max as usize).position(scroll as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .symbols(symbols::scrollbar::VERTICAL)
                .begin_symbol(None)
                .end_symbol(None)
                .track_style(muted())
                .thumb_style(Style::default().fg(Color::Reset)),
            frame.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut state,
        );
    }
}

fn rule(width: usize) -> Line<'static> {
    Line::from(Span::styled("─".repeat(width), muted()))
}

/// A rule with `title` set into it: `── Comments (2) ─────`.
fn section(title: &str, width: usize) -> Line<'static> {
    let lead = "── ";
    let used = lead.chars().count() + title.chars().count() + 1;
    Line::from(vec![
        Span::styled(lead, muted()),
        Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", "─".repeat(width.saturating_sub(used))),
            muted(),
        ),
    ])
}

/// A header row: the field name, then `value` wrapped under itself.
fn push_field(lines: &mut Vec<Line<'static>>, name: &str, value: Vec<Span<'static>>, width: usize) {
    lines.extend(markup::wrap(
        vec![Span::styled(format!("{:<FIELD_WIDTH$}", name), muted())],
        vec![Span::raw(" ".repeat(FIELD_WIDTH))],
        value,
        width,
    ));
}

fn title_lines(summary: &str, width: usize) -> Vec<Line<'static>> {
    markup::wrap(
        vec![],
        vec![],
        vec![Span::styled(
            summary.to_string(),
            Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        )],
        width,
    )
}

fn indented(line: Line<'static>, indent: &str) -> Line<'static> {
    let mut spans = vec![Span::raw(indent.to_string())];
    spans.extend(line.spans);
    Line::from(spans)
}

fn header_lines(ticket: &Ticket, rules: &StatusRules, width: usize) -> Vec<Line<'static>> {
    let mut lines = title_lines(&ticket.summary, width);
    lines.push(Line::from(""));

    let color = status_color(&ticket.status, rules);
    push_field(
        &mut lines,
        "Status",
        vec![Span::styled(
            format!("● {}", ticket.status),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )],
        width,
    );

    let assignee = match &ticket.assignee {
        Some(name) => Span::styled(name.clone(), Style::default().fg(Color::Reset)),
        None => Span::styled("Unassigned", muted().add_modifier(Modifier::ITALIC)),
    };
    push_field(&mut lines, "Assignee", vec![assignee], width);

    if let Some(reporter) = &ticket.reporter {
        push_field(
            &mut lines,
            "Reporter",
            vec![Span::styled(
                reporter.clone(),
                Style::default().fg(Color::Reset),
            )],
            width,
        );
    }

    if let Some(epic_key) = &ticket.epic_key {
        let mut value = vec![Span::styled(
            epic_key.clone(),
            Style::default().fg(Color::Magenta),
        )];
        if let Some(name) = &ticket.epic_name {
            value.push(Span::styled(
                format!(" · {}", name),
                Style::default().fg(Color::Reset),
            ));
        }
        push_field(&mut lines, "Epic", value, width);
    }

    if !ticket.labels.is_empty() {
        let mut chips = Vec::new();
        for (i, label) in ticket.labels.iter().enumerate() {
            if i > 0 {
                chips.push(Span::raw(" "));
            }
            chips.push(Span::styled(
                format!("\u{a0}{}\u{a0}", label),
                Style::default().fg(Color::Yellow).bg(Color::DarkGray),
            ));
        }
        push_field(&mut lines, "Labels", chips, width);
    }

    lines
}

fn description_lines(app: &App, ticket: &Ticket, width: usize) -> Vec<Line<'static>> {
    if ticket.detail_loaded {
        return match ticket.description.as_deref() {
            Some(desc) if !desc.trim().is_empty() => markup::render(desc, width as u16),
            _ => vec![Line::from(Span::styled(
                "No description.",
                muted().add_modifier(Modifier::ITALIC),
            ))],
        };
    }
    match app.detail_fetch_error(&ticket.key) {
        Some(error) => {
            let mut lines = markup::wrap(
                vec![],
                vec![],
                vec![Span::styled(
                    format!("Couldn't load details: {}", error.trim()),
                    Style::default().fg(Color::Red),
                )],
                width,
            );
            lines.push(Line::from(Span::styled(
                "Close and reopen the ticket to try again.",
                muted(),
            )));
            lines
        }
        None => vec![Line::from(Span::styled(
            "Loading details…",
            Style::default().fg(Color::Yellow),
        ))],
    }
}

/// The ticket's comments, oldest first.
fn comment_lines(ticket: &Ticket, width: usize) -> Vec<Line<'static>> {
    // Activity is newest first.
    let comments: Vec<_> = ticket
        .activity
        .iter()
        .rev()
        .map(|entry| {
            let ActivityKind::Comment { body } = &entry.kind;
            (entry, body)
        })
        .collect();

    let title = match comments.len() {
        0 => "Comments".to_string(),
        n => format!("Comments ({})", n),
    };
    let mut lines = vec![Line::from(""), section(&title, width)];
    if comments.is_empty() {
        lines.push(Line::from(Span::styled(
            "No comments yet. Press C to add one.",
            muted().add_modifier(Modifier::ITALIC),
        )));
        return lines;
    }

    let indent = "  ";
    for (entry, body) in comments {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                entry.author.clone(),
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {}", format_timestamp(&entry.timestamp)), muted()),
        ]));
        let body_width = width.saturating_sub(indent.len()) as u16;
        lines.extend(
            markup::render(body, body_width)
                .into_iter()
                .map(|line| indented(line, indent)),
        );
    }
    lines
}

fn render_view(f: &mut ratatui::Frame, frame: Rect, area: Rect, app: &App, ticket: &Ticket) {
    let body = render_footer(f, area, VIEW_HINTS, app);
    let width = body.width as usize;

    let mut lines = header_lines(ticket, app.status_rules(), width);
    lines.push(Line::from(""));
    lines.push(rule(width));
    lines.push(Line::from(""));
    lines.extend(description_lines(app, ticket, width));
    if ticket.detail_loaded {
        lines.extend(comment_lines(ticket, width));
    }

    render_scrollable(f, app, frame, body, lines);
}

fn render_epic_view(f: &mut ratatui::Frame, frame: Rect, area: Rect, app: &App, epic: &Epic) {
    let body = render_footer(f, area, EPIC_HINTS, app);
    let width = body.width as usize;
    let rules = app.status_rules();

    let mut lines = title_lines(&epic.summary, width);
    lines.push(Line::from(""));

    let total = epic.total();
    let done = epic.done_count(rules);
    push_field(
        &mut lines,
        "Progress",
        vec![Span::styled(
            format!(
                "{}\u{a0}\u{a0}{}\u{a0}/\u{a0}{}\u{a0}({:.1}%)",
                progress_bar(done, total, 24),
                done,
                total,
                epic.progress_pct(rules)
            ),
            Style::default().fg(Color::Green),
        )],
        width,
    );

    let mut counts = Vec::new();
    for (status, tickets) in rules.group(&epic.children) {
        if !counts.is_empty() {
            counts.push(Span::raw("  "));
        }
        counts.push(Span::styled(
            format!("{}:\u{a0}{}", status.as_str(), tickets.len()).replace(' ', "\u{a0}"),
            Style::default().fg(status_color(status.as_str(), rules)),
        ));
    }
    if counts.is_empty() {
        counts.push(Span::styled("No related tickets", muted()));
    }
    push_field(&mut lines, "Status", counts, width);

    lines.push(Line::from(""));
    lines.push(section(&format!("Related tickets ({})", total), width));
    lines.push(Line::from(""));

    let mut children: Vec<_> = epic.children.iter().collect();
    rules.sort_tickets(&mut children);
    if children.is_empty() {
        lines.push(Line::from(Span::styled("(no related tickets)", muted())));
    }
    let summary_width = width.saturating_sub(12 + 15 + 2).max(10);
    for ticket in children {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<12}", ticket.key),
                Style::default().fg(Color::Reset),
            ),
            Span::styled(
                format!("{:<15}", truncate(&ticket.status, 14)),
                Style::default().fg(status_color(&ticket.status, rules)),
            ),
            Span::raw("  "),
            Span::styled(
                truncate(&ticket.summary, summary_width),
                Style::default().fg(Color::Reset),
            ),
        ]));
    }

    render_scrollable(f, app, frame, body, lines);
}

/// Renders `lines` above the key `hints`.
fn render_with_footer(
    f: &mut ratatui::Frame,
    area: Rect,
    lines: Vec<Line>,
    hints: &[(&str, &str)],
    app: &App,
) {
    let body = render_footer(f, area, hints, app);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
}

fn heading(text: String) -> Line<'static> {
    Line::from(Span::styled(
        text,
        Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD),
    ))
}

pub(crate) fn option_style(color: Color, selected: bool) -> Style {
    let style = Style::default().fg(color);
    if selected {
        style.add_modifier(Modifier::BOLD).bg(Color::DarkGray)
    } else {
        style
    }
}

pub(crate) fn render_menu(
    f: &mut ratatui::Frame,
    app: &App,
    area: Rect,
    lines: Vec<Line<'static>>,
    choices: (&[(usize, usize)], usize),
    hints: &[(&str, &str)],
) {
    let body = render_footer(f, area, hints, app);
    let selected_line = choices
        .0
        .iter()
        .find(|(_, index)| *index == choices.1)
        .map_or(0, |(line, _)| *line);
    let scroll = selected_line.saturating_sub(body.height.saturating_sub(1) as usize);
    for &(line, index) in choices.0 {
        if let Some(y) = line
            .checked_sub(scroll)
            .filter(|y| *y < body.height as usize)
        {
            app.mouse_targets.borrow_mut().push((
                Rect::new(body.x, body.y + y as u16, body.width, 1),
                crate::mouse::Target::Choose { field: 0, index },
            ));
        }
    }
    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), body);
}

fn render_move_picker(
    f: &mut ratatui::Frame,
    area: Rect,
    ticket: &crate::cache::Ticket,
    picker: &MovePicker,
    rules: &StatusRules,
    app: &App,
) {
    let mut lines = vec![
        heading(match &picker.only_to {
            Some(status) => format!(
                "Transitions to {} from {}:",
                status.as_str(),
                ticket.status.as_str()
            ),
            None => format!("Move from {}:", ticket.status.as_str()),
        }),
        Line::from(""),
    ];

    let mut choices = Vec::new();
    let rows = picker.rows();
    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(
                "Jira offers no transitions from {}.",
                ticket.status.as_str()
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }
    for (i, transition) in rows.iter().enumerate() {
        choices.push((lines.len(), i));
        let shortcut = Status::move_shortcut_prefix(&transition.to_name);
        let prefix = if i == picker.selected { "› " } else { "  " };
        lines.push(Line::from(Span::styled(
            format!(
                "{}{}{}",
                prefix,
                shortcut,
                transition.label(&picker.transitions)
            ),
            option_style(
                status_color(&transition.to_name, rules),
                i == picker.selected,
            ),
        )));
    }

    if let Some(transition) = picker.selected_transition().filter(|_| picker.confirming) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "Move with {}? Press Enter or y. Esc cancels.",
                transition.label(&picker.transitions)
            ),
            Style::default().fg(Color::Yellow),
        )));
    }

    render_menu(
        f,
        app,
        area,
        lines,
        (&choices, picker.selected),
        &[
            ("j/k", "choose"),
            (
                "Enter",
                if picker.confirming {
                    "confirm"
                } else {
                    "select"
                },
            ),
            (crate::cache::MOVE_SHORTCUTS, "pick by status"),
            ("Shift+key", "move now"),
            ("Esc", "cancel"),
        ],
    );
}

fn render_resolution_picker(
    f: &mut ratatui::Frame,
    area: Rect,
    picker: &MovePicker,
    selected: usize,
    app: &App,
) {
    let transition = picker
        .selected_transition()
        .map(|t| t.label(&picker.transitions))
        .unwrap_or_default();
    let mut lines = vec![
        heading(format!("{} — select a resolution:", transition)),
        Line::from(""),
    ];
    let mut choices = Vec::new();
    for (i, choice) in picker.resolution_choices().iter().enumerate() {
        choices.push((lines.len(), i));
        let prefix = if i == selected { "› " } else { "  " };
        let name = choice.as_ref().map_or("No resolution", |r| r.name.as_str());
        lines.push(Line::from(Span::styled(
            format!("{}{}", prefix, name),
            option_style(Color::Reset, i == selected),
        )));
    }
    render_menu(
        f,
        app,
        area,
        lines,
        (&choices, selected),
        &[("j/k", "choose"), ("Enter", "move"), ("Esc", "back")],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::ActivityEntry;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn ticket() -> Ticket {
        Ticket {
            key: "DEMO-3228".to_string(),
            summary: "PAY: Payments Platform Revamp".to_string(),
            status: "In Progress".to_string(),
            assignee: Some("Alex Rivera".to_string()),
            reporter: Some("Alex Rivera".to_string()),
            description: Some(
                "*Stakeholders:* Alex Rivera (owner)\n\nh2. Goals\n# Replay the changes\n# Grade the result"
                    .to_string(),
            ),
            labels: vec!["DEMO".to_string(), "data-science".to_string()],
            detail_loaded: true,
            activity: vec![
                comment("2026-09-02T10:00:00.000+0000", "Sam Lee", "Second *reply*"),
                comment("2026-09-01T09:30:00.000+0000", "Alex Rivera", "First"),
            ],
            ..Ticket::default()
        }
    }

    fn comment(timestamp: &str, author: &str, body: &str) -> ActivityEntry {
        ActivityEntry {
            timestamp: timestamp.to_string(),
            author: author.to_string(),
            author_email: None,
            kind: ActivityKind::Comment {
                body: body.to_string(),
            },
        }
    }

    fn app_showing(ticket: Ticket) -> App {
        let mut app = App::new();
        app.loading = false;
        app.open_detail(ticket.key.clone());
        app.cache.my_tickets = vec![ticket];
        app
    }

    fn draw(app: &App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().replace('\u{a0}', " "))
                    .collect()
            })
            .collect()
    }

    fn row(lines: &[String], text: &str) -> usize {
        lines
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("{:?} not drawn:\n{}", text, lines.join("\n")))
    }

    #[tokio::test]
    async fn top_left_close_button_closes_the_popup() {
        let mut config = toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        for (width, height, fullscreen) in [(55, 50, false), (40, 18, false), (80, 24, true)] {
            for mode in [
                DetailMode::View,
                DetailMode::History { scroll: 0 },
                DetailMode::MoveLoading { request: 1 },
            ] {
                let mut app = app_showing(ticket());
                app.detail_fullscreen = fullscreen;
                app.detail_mode = mode;
                let lines = draw(&app, width, height);
                let y = row(&lines, "[×]");
                let x = lines[y].split_once("[×]").unwrap().0.chars().count();
                assert!(x < width as usize / 4 && y < height as usize / 4);
                assert!(!lines.iter().any(|line| line.contains(" Close ")));
                for kind in [
                    crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                    crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
                ] {
                    crate::mouse::handle(
                        &mut app,
                        crossterm::event::MouseEvent {
                            kind,
                            column: x as u16 + 1,
                            row: y as u16,
                            modifiers: crossterm::event::KeyModifiers::NONE,
                        },
                        &tx,
                        &mut config,
                    )
                    .await;
                }
                assert!(!app.is_detail_open());
            }
        }
    }

    #[test]
    fn header_fields_and_comments_oldest_first() {
        let lines = draw(&app_showing(ticket()), 70, 34);
        assert!(lines[row(&lines, "Status")].contains("● In Progress"));
        assert!(lines[row(&lines, "Labels")].contains(" DEMO   data-science "));
        assert!(lines[row(&lines, "── Comments (2) ─")].contains('─'));
        assert!(row(&lines, "Alex Rivera  2026-09-01 09:30") < row(&lines, "Sam Lee"));
        assert!(lines[row(&lines, "Second reply")].contains("   Second reply"));
    }

    #[test]
    fn long_detail_scrolls_only_to_its_last_line() {
        let mut t = ticket();
        t.description = Some((1..=60).map(|n| format!("line {}\n", n)).collect());
        let mut app = app_showing(t);
        draw(&app, 70, 30);
        assert!(app.detail_scroll_max.get() > 0);

        app.detail_scroll = u16::MAX;
        let lines = draw(&app, 70, 30);
        let last = row(&lines, "Second reply");
        assert_eq!(
            row(&lines, "↑↓ scroll"),
            last + 2,
            "last line sits above the footer"
        );
        // The scrollbar replaces the right border, thumb at the bottom.
        assert!(lines[last].trim_end().ends_with('█'));
    }

    #[test]
    fn unloaded_detail_says_loading_or_why_it_failed() {
        let mut t = ticket();
        t.detail_loaded = false;
        let mut app = app_showing(t);
        let lines = draw(&app, 70, 34);
        row(&lines, "Loading details…");
        assert!(!lines.iter().any(|l| l.contains("Comments")));

        app.fail_detail_fetch("DEMO-3228", "jira: timed out".to_string());
        let lines = draw(&app, 70, 34);
        row(&lines, "Couldn't load details: jira: timed out");
    }
}
