use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use super::form;

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let state = match &app.assign_state {
        Some(s) => s,
        None => return,
    };

    let title = format!("Assign {}", state.ticket_key);
    let inner = form::render_modal_frame(f, &title, 40, 50);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for (i, member) in app.cache.team_members.iter().enumerate() {
        let prefix = if i == state.selected { "> " } else { "  " };
        let style = if i == state.selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(Span::styled(
            format!("  {}{} ({})", prefix, member.name, member.email),
            style,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Footer hints
    lines.push(Line::from(Span::styled(
        "[j/k] navigate  [Enter] assign  [Esc] cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let body = Paragraph::new(lines);
    f.render_widget(body, inner);
}
