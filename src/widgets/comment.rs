use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::form;
use crate::app::App;

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let state = match &app.comment_state {
        Some(s) => s,
        None => return,
    };

    let title = format!("Comment on {}", state.ticket_key);
    let inner = form::render_modal_frame(f, &title, 50, 30);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let label = Paragraph::new(Line::from(Span::styled(
        "Comment:",
        Style::default().fg(Color::Cyan),
    )));
    f.render_widget(label, sections[0]);

    let mut body_text = state.body.clone();
    body_text.push('_');
    let body = Paragraph::new(body_text)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    f.render_widget(body, sections[1]);

    let footer = Paragraph::new(Line::from(Span::styled(
        "[Shift+Enter] newline  [Enter] submit  [Esc] cancel",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(footer, sections[2]);
}
