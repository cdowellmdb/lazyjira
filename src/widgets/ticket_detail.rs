use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{App, DetailMode};
use crate::cache::Status;

fn status_color(status: &Status) -> Color {
    match status {
        Status::InProgress => Color::Yellow,
        Status::ToDo => Color::White,
        Status::InReview => Color::Cyan,
        Status::Blocked => Color::Red,
        Status::Done => Color::Green,
    }
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

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let ticket_key = match app.detail_ticket_key.as_ref() {
        Some(k) => k,
        None => return,
    };

    let ticket = match app.find_ticket(ticket_key) {
        Some(t) => t,
        None => return,
    };

    let area = centered_rect(60, 60, f.area());

    // Clear the area behind the popup
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", ticket_key));

    let inner = block.inner(area);
    f.render_widget(block, area);

    match &app.detail_mode {
        DetailMode::View => render_view(f, inner, ticket),
        DetailMode::MovePicker { selected } => render_move_picker(f, inner, ticket, *selected),
    }
}

fn render_view(f: &mut ratatui::Frame, area: Rect, ticket: &crate::cache::Ticket) {
    // Split into body and footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let body_area = chunks[0];
    let footer_area = chunks[1];

    // Build body lines
    let mut lines: Vec<Line> = Vec::new();

    // Line 1: Summary (bold, white)
    lines.push(Line::from(Span::styled(
        ticket.summary.clone(),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));

    // Line 2: empty
    lines.push(Line::from(""));

    // Line 3: Status + Assignee
    let assignee_str = ticket
        .assignee
        .as_deref()
        .unwrap_or("Unassigned");
    lines.push(Line::from(vec![
        Span::raw("Status: "),
        Span::styled(
            ticket.status.as_str(),
            Style::default().fg(status_color(&ticket.status)),
        ),
        Span::raw("    Assignee: "),
        Span::styled(assignee_str, Style::default().fg(Color::White)),
    ]));

    // Line 4: Epic (if present)
    if let (Some(ref epic_key), Some(ref epic_name)) = (&ticket.epic_key, &ticket.epic_name) {
        lines.push(Line::from(vec![
            Span::raw("Epic: "),
            Span::styled(
                format!("{} ({})", epic_key, epic_name),
                Style::default().fg(Color::Magenta),
            ),
        ]));
    } else if let Some(ref epic_key) = ticket.epic_key {
        lines.push(Line::from(vec![
            Span::raw("Epic: "),
            Span::styled(epic_key.clone(), Style::default().fg(Color::Magenta)),
        ]));
    }

    // Line 5: empty
    lines.push(Line::from(""));

    // Line 6+: Description
    let desc = ticket
        .description
        .as_deref()
        .unwrap_or("(no description)");
    for line in desc.lines() {
        lines.push(Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Gray),
        )));
    }

    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(body, body_area);

    // Footer
    let footer = Paragraph::new(Line::from(Span::styled(
        "[Esc] close   [o] open in browser   [m] move",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, footer_area);
}

fn render_move_picker(
    f: &mut ratatui::Frame,
    area: Rect,
    ticket: &crate::cache::Ticket,
    selected: usize,
) {
    // Split into body and footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let body_area = chunks[0];
    let footer_area = chunks[1];

    let mut lines: Vec<Line> = Vec::new();

    // Line 1: "Move to:" (bold)
    lines.push(Line::from(Span::styled(
        "Move to:",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));

    // Line 2: empty
    lines.push(Line::from(""));

    // List of status options
    let options = ticket.status.others();
    for (i, status) in options.iter().enumerate() {
        let prefix = if i == selected { "> " } else { "  " };
        let mut style = Style::default().fg(status_color(status));
        if i == selected {
            style = style
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray);
        }
        lines.push(Line::from(Span::styled(
            format!("{}{}", prefix, status.as_str()),
            style,
        )));
    }

    let body = Paragraph::new(lines);
    f.render_widget(body, body_area);

    // Footer
    let footer = Paragraph::new(Line::from(Span::styled(
        "[Enter] confirm   [Esc] cancel",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, footer_area);
}
