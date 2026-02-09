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
        Status::Closed => Color::Green,
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let mut result: String = s.chars().take(max.saturating_sub(3)).collect();
        result.push_str("...");
        result
    } else {
        s.to_string()
    }
}

fn epic_status_rank(status: &Status) -> usize {
    match status {
        Status::InProgress => 0,
        Status::ReadyForWork => 1,
        Status::NeedsTriage => 2,
        Status::ToDo => 3,
        Status::InReview => 4,
        Status::Other(_) => 5,
        Status::Blocked => 6,
        Status::Closed => 7,
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

pub fn render(f: &mut ratatui::Frame, app: &App, resolutions: &[String]) {
    if let Some(ticket_key) = app.detail_ticket_key.as_ref() {
        if let Some(ticket) = app.find_ticket(ticket_key) {
            let area = centered_rect(60, 60, f.area());
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
                DetailMode::ResolutionPicker {
                    target_status,
                    selected,
                } => render_resolution_picker(f, inner, target_status, *selected, resolutions),
                DetailMode::History { scroll } => {
                    crate::widgets::activity::render(f, inner, &ticket.activity, *scroll);
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

    let area = centered_rect(70, 70, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", epic.key));

    let inner = block.inner(area);
    f.render_widget(block, area);
    render_epic_view(f, inner, epic, app.detail_scroll);
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

fn render_epic_view(f: &mut ratatui::Frame, area: Rect, epic: &crate::cache::Epic, scroll: u16) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let body_area = chunks[0];
    let footer_area = chunks[1];

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        epic.summary.clone(),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let total = epic.total();
    let done = epic.done_count();
    let pct = epic.progress_pct();
    lines.push(Line::from(vec![
        Span::raw("Progress: "),
        Span::styled(
            format!(
                "{}  {} / {} ({:.1}%)",
                progress_bar(done, total, 24),
                done,
                total,
                pct
            ),
            Style::default().fg(Color::Green),
        ),
    ]));

    let counts = epic.count_by_status();
    let mut parts = Vec::new();
    for status in Status::all() {
        let count = counts.get(status).copied().unwrap_or(0);
        if count > 0 {
            parts.push(format!("{}: {}", status.as_str(), count));
        }
    }
    if parts.is_empty() {
        parts.push("No related tickets".to_string());
    }
    lines.push(Line::from(vec![
        Span::raw("Status: "),
        Span::styled(parts.join("  "), Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Related Tickets",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let mut children: Vec<_> = epic.children.iter().collect();
    children.sort_by(|a, b| {
        epic_status_rank(&a.status)
            .cmp(&epic_status_rank(&b.status))
            .then_with(|| a.key.cmp(&b.key))
    });
    if children.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no related tickets)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for ticket in children {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<12}", ticket.key),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:<15}", ticket.status.as_str()),
                    Style::default().fg(status_color(&ticket.status)),
                ),
                Span::raw("  "),
                Span::styled(
                    truncate(&ticket.summary, 78),
                    Style::default().fg(Color::Gray),
                ),
            ]));
        }
    }

    let body = Paragraph::new(lines)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(body, body_area);

    let footer = Paragraph::new(Line::from(Span::styled(
        "[↑/↓] scroll  [Esc] close  [o] browser",
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
        "[j/k/↑/↓] choose   [p/w/n/t/v/b/c] confirm   [Shift+key] move now   [Esc] cancel",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, footer_area);
}

fn render_resolution_picker(
    f: &mut ratatui::Frame,
    area: Rect,
    target_status: &Status,
    selected: usize,
    resolutions: &[String],
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let body_area = chunks[0];
    let footer_area = chunks[1];

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        format!("Moving to {} — select resolution:", target_status.as_str()),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for (i, resolution) in resolutions.iter().enumerate() {
        let prefix = if i == selected { "> " } else { "  " };
        let mut style = Style::default().fg(Color::White);
        if i == selected {
            style = style.add_modifier(Modifier::BOLD).bg(Color::DarkGray);
        }
        lines.push(Line::from(Span::styled(
            format!("{}{}", prefix, resolution),
            style,
        )));
    }

    let body = Paragraph::new(lines);
    f.render_widget(body, body_area);

    let footer = Paragraph::new(Line::from(Span::styled(
        "[j/k/↑/↓] choose   [Enter] confirm   [Esc] back",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, footer_area);
}
