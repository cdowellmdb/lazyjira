use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, FilterFocus};
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let t: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", t)
    } else {
        s.to_string()
    }
}

pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App, config: &crate::config::AppConfig) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(area);

    render_sidebar(f, chunks[0], app, config);
    render_results(f, chunks[1], app);
}

fn render_sidebar(
    f: &mut ratatui::Frame,
    area: Rect,
    app: &App,
    config: &crate::config::AppConfig,
) {
    let sidebar_focused = app.filter_focus == FilterFocus::Sidebar;
    let border_style = if sidebar_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut lines = Vec::new();

    if config.filters.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No saved filters",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Press n to create one",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, filter) in config.filters.iter().enumerate() {
            let is_selected = i == app.filter_sidebar_idx;
            let prefix = if is_selected { "> " } else { "  " };

            let style = if is_selected && sidebar_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray)
            } else if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, filter.name),
                style,
            )));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Saved Filters ")
        .border_style(border_style);
    let widget = Paragraph::new(lines).block(block);
    f.render_widget(widget, area);
}

fn render_results(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let results_focused = app.filter_focus == FilterFocus::Results;
    let border_style = if results_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut lines = Vec::new();

    if app.filter_loading {
        lines.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(Color::Yellow),
        )));
    } else if app.filter_results.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Select a filter and press Enter to run it",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let key_w = 14usize;
        let status_w = 14usize;
        let inner = area.width.saturating_sub(2) as usize;
        let fixed = 2 + key_w + 3 + status_w + 3 + 3;
        let summary_w = inner.saturating_sub(fixed).max(12);

        let heading_style = Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD);

        lines.push(Line::from(vec![
            Span::styled(format!("  {:<key_w$}", "SEL KEY"), heading_style),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<status_w$}", "STATUS"), heading_style),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<summary_w$}", "SUMMARY"), heading_style),
        ]));

        let header_w = 2 + key_w + 3 + status_w + 3 + summary_w;
        lines.push(Line::from(Span::styled(
            "-".repeat(header_w),
            Style::default().fg(Color::DarkGray),
        )));

        for (i, ticket) in app.filter_results.iter().enumerate() {
            let is_selected = i == app.selected_index && results_focused;
            let marker = if app.is_ticket_selected(&ticket.key) {
                "[x]"
            } else {
                "[ ]"
            };

            let base = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let status_style = if is_selected {
                Style::default()
                    .fg(status_color(&ticket.status))
                    .bg(Color::DarkGray)
            } else {
                Style::default().fg(status_color(&ticket.status))
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<key_w$}", format!("{} {}", marker, ticket.key)),
                    base,
                ),
                Span::styled(" | ", base),
                Span::styled(
                    format!("{:<status_w$}", truncate(ticket.status.as_str(), status_w)),
                    status_style,
                ),
                Span::styled(" | ", base),
                Span::styled(
                    format!("{:<summary_w$}", truncate(&ticket.summary, summary_w)),
                    base,
                ),
            ]));
        }
    }

    // Scroll to keep selected row visible
    let visible = area.height.saturating_sub(2) as usize;
    let scroll_y = if results_focused && app.selected_index + 2 >= visible {
        ((app.selected_index + 2) - visible + 1) as u16
    } else {
        0
    };

    let title = if app.filter_results.is_empty() {
        " Results ".to_string()
    } else {
        format!(" Results ({}) ", app.filter_results.len())
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style);
    let widget = Paragraph::new(lines).block(block).scroll((scroll_y, 0));
    f.render_widget(widget, area);
}
