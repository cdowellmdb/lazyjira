use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use super::form;

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let state = match &app.comment_state {
        Some(s) => s,
        None => return,
    };

    let title = format!("Comment on {}", state.ticket_key);
    let inner = form::render_modal_frame(f, &title, 50, 30);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    form::render_text_input(
        &mut lines,
        "Comment",
        &state.body,
        true,
    );

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Footer hints
    lines.push(Line::from(Span::styled(
        "[Enter] submit  [Esc] cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let body = Paragraph::new(lines);
    f.render_widget(body, inner);
}
