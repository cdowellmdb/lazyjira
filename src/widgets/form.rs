use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear};

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

pub fn render_modal_frame(
    f: &mut ratatui::Frame,
    title: &str,
    percent_x: u16,
    percent_y: u16,
) -> Rect {
    let area = centered_rect(percent_x, percent_y, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title));
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

/// Render a single-line text input with label.
pub fn render_text_input(lines: &mut Vec<Line>, label: &str, value: &str, focused: bool) {
    let cursor = if focused { "_" } else { "" };
    lines.push(Line::from(vec![
        Span::styled(
            format!("{}: ", label),
            Style::default().fg(if focused {
                Color::Cyan
            } else {
                Color::DarkGray
            }),
        ),
        Span::styled(
            value.to_string(),
            Style::default().fg(Color::White).add_modifier(if focused {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ),
        Span::styled(cursor, Style::default().fg(Color::DarkGray)),
    ]));
}

/// Render a picker list with selected highlight.
pub fn render_picker(
    lines: &mut Vec<Line>,
    label: &str,
    options: &[String],
    selected: usize,
    focused: bool,
) {
    lines.push(Line::from(Span::styled(
        format!("{}:", label),
        Style::default().fg(if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        }),
    )));
    for (i, option) in options.iter().enumerate() {
        let prefix = if i == selected { "> " } else { "  " };
        let style = if i == selected && focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(Span::styled(
            format!("  {}{}", prefix, option),
            style,
        )));
    }
}
