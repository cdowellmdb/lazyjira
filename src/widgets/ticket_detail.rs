use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{App, DetailMode};
use crate::cache::Status;

fn status_color(status: &Status) -> Color {
    match status {
        Status::NeedsTriage => Color::White,
        Status::ReadyForWork => Color::Blue,
        Status::InProgress => Color::Yellow,
        Status::ToDo => Color::White,
        Status::InReview => Color::Cyan,
        Status::Blocked => Color::Red,
        Status::Done => Color::Green,
        Status::Other(_) => Color::Magenta,
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

fn parse_heading(line: &str) -> Option<(u8, &str)> {
    let trimmed = line.trim_start();

    for level in 1..=6u8 {
        let jira_prefix = format!("h{}. ", level);
        if let Some(rest) = trimmed.strip_prefix(&jira_prefix) {
            return Some((level, rest.trim()));
        }
    }

    for level in (1..=6u8).rev() {
        let md_prefix = "#".repeat(level as usize) + " ";
        if let Some(rest) = trimmed.strip_prefix(&md_prefix) {
            return Some((level, rest.trim()));
        }
    }

    for level in 1..=6u8 {
        let open = format!("<h{}>", level);
        let close = format!("</h{}>", level);
        if trimmed.starts_with(&open) && trimmed.ends_with(&close) {
            let content = trimmed
                .trim_start_matches(&open)
                .trim_end_matches(&close)
                .trim();
            return Some((level, content));
        }
    }

    None
}

fn parse_bullet(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let ws = line.len().saturating_sub(trimmed.len());

    let stars = trimmed.chars().take_while(|c| *c == '*').count();
    if stars > 0
        && trimmed
            .chars()
            .nth(stars)
            .map(|c| c == ' ')
            .unwrap_or(false)
    {
        let text = trimmed[stars + 1..].trim().to_string();
        return Some((ws + (stars.saturating_sub(1) * 2), text));
    }

    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes > 0
        && trimmed
            .chars()
            .nth(hashes)
            .map(|c| c == ' ')
            .unwrap_or(false)
    {
        let text = trimmed[hashes + 1..].trim().to_string();
        return Some((ws + (hashes.saturating_sub(1) * 2), format!("1. {}", text)));
    }

    if let Some(rest) = trimmed.strip_prefix("- ") {
        return Some((ws, rest.trim().to_string()));
    }

    None
}

fn normalize_inline(s: &str) -> String {
    s.replace("{{", "`").replace("}}", "`")
}

fn push_description_lines(lines: &mut Vec<Line>, desc: &str) {
    let mut in_code = false;

    for raw in desc.lines() {
        let trimmed = raw.trim();

        if trimmed == "{code}" || trimmed.starts_with("{code:") || trimmed == "```" {
            in_code = !in_code;
            lines.push(Line::from(Span::styled(
                "---- code ----",
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        if in_code {
            lines.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(Color::White),
            )));
            continue;
        }

        if let Some((level, text)) = parse_heading(raw) {
            lines.push(Line::from(""));
            let marker = match level {
                1 => "H1",
                2 => "H2",
                3 => "H3",
                _ => "H",
            };
            let heading_text = format!("{} {}", marker, normalize_inline(text));
            lines.push(Line::from(Span::styled(
                heading_text,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        if let Some((indent, item)) = parse_bullet(raw) {
            let prefix = " ".repeat(indent);
            lines.push(Line::from(Span::styled(
                format!("{}• {}", prefix, normalize_inline(&item)),
                Style::default().fg(Color::Gray),
            )));
            continue;
        }

        lines.push(Line::from(Span::styled(
            normalize_inline(raw),
            Style::default().fg(Color::Gray),
        )));
    }
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
        DetailMode::View => render_view(f, inner, ticket, app.detail_scroll),
        DetailMode::MovePicker {
            selected,
            confirm_target,
        } => render_move_picker(f, inner, ticket, *selected, confirm_target.as_ref()),
        DetailMode::History { scroll } => {
            crate::widgets::activity::render(f, inner, &ticket.activity, *scroll);
        }
    }
}

fn render_view(f: &mut ratatui::Frame, area: Rect, ticket: &crate::cache::Ticket, scroll: u16) {
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
    let assignee_str = ticket.assignee.as_deref().unwrap_or("Unassigned");
    lines.push(Line::from(vec![
        Span::raw("Status: "),
        Span::styled(
            ticket.status.as_str(),
            Style::default().fg(status_color(&ticket.status)),
        ),
        Span::raw("    Assignee: "),
        Span::styled(assignee_str, Style::default().fg(Color::White)),
    ]));

    // Line 4: Reporter (if loaded)
    if let Some(ref reporter) = ticket.reporter {
        lines.push(Line::from(vec![
            Span::raw("Reporter: "),
            Span::styled(reporter.as_str(), Style::default().fg(Color::White)),
        ]));
    }

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

    // Labels
    if !ticket.labels.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("Labels: "),
            Span::styled(ticket.labels.join(", "), Style::default().fg(Color::Yellow)),
        ]));
        lines.push(Line::from(""));
    }

    // Line 6+: Description
    let desc = ticket.description.as_deref().unwrap_or("(no description)");
    push_description_lines(&mut lines, desc);

    let body = Paragraph::new(lines)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(body, body_area);

    // Footer
    let footer = Paragraph::new(Line::from(Span::styled(
        "[↑/↓] scroll  [Esc] close  [o] browser  [m] move  [C] comment  [a] assign  [e] edit  [h] history",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, footer_area);
}

fn render_move_picker(
    f: &mut ratatui::Frame,
    area: Rect,
    ticket: &crate::cache::Ticket,
    selected: usize,
    confirm_target: Option<&Status>,
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
            style = style.add_modifier(Modifier::BOLD).bg(Color::DarkGray);
        }
        lines.push(Line::from(Span::styled(
            format!("{}[{}] {}", prefix, status.move_shortcut(), status.as_str()),
            style,
        )));
    }

    if let Some(status) = confirm_target {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "Confirm move to {}? Press Enter or y. Esc cancels.",
                status.as_str()
            ),
            Style::default().fg(Color::Yellow),
        )));
    }

    let body = Paragraph::new(lines);
    f.render_widget(body, body_area);

    // Footer
    let footer = Paragraph::new(Line::from(Span::styled(
        "[j/k/↑/↓] choose   [p/w/n/t/v/b/d] confirm   [Shift+key] move now   [Esc] cancel",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, footer_area);
}
