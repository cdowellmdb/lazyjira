use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use super::form;

pub fn render(f: &mut ratatui::Frame, app: &App) {
    let state = match &app.edit_state {
        Some(s) => s,
        None => return,
    };

    let title = format!("Edit {}", state.ticket_key);
    let inner = form::render_modal_frame(f, &title, 50, 40);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    // Summary text input (field 0)
    form::render_text_input(
        &mut lines,
        "Summary",
        &state.summary,
        state.focused_field == 0,
    );

    lines.push(Line::from(""));

    // Labels text input (field 1)
    form::render_text_input(
        &mut lines,
        "Labels (comma-separated)",
        &state.labels,
        state.focused_field == 1,
    );

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Footer hints
    lines.push(Line::from(Span::styled(
        "[Tab] next field  [Enter] save  [Esc] cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let body = Paragraph::new(lines);
    f.render_widget(body, inner);
}
