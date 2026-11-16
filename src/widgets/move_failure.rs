use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use super::form::{self, Row};
use crate::app::App;
use crate::style;

/// The panel's body: the message, a blank line and Jira's error.
fn rows(app: &App) -> [Row; 1] {
    let error = app
        .moves
        .failures()
        .first()
        .map_or(0, |failure| failure.error.lines().count());
    [Row::flex(2 + error as u16, 1)]
}

pub fn need(app: &App) -> form::Need {
    form::need(&rows(app))
}

/// Shows the oldest move Jira rejected, with Jira's full error.
pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let failures = app.moves.failures();
    let Some(failure) = failures.first() else {
        return;
    };
    let more = match failures.len() - 1 {
        0 => String::new(),
        more => format!("({more} more)"),
    };
    let (body, footer) = form::render_panel_frame(f, app, area, form::title("Move failed", &more));
    let [body] = form::split(rows(app), body);

    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "Jira did not move {} to {}. Its status is unchanged.",
                failure.key, failure.target
            ),
            style::error().patch(style::heading()),
        )),
        Line::from(""),
    ];
    lines.extend(Text::raw(failure.error.as_str()).lines);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    form::button_row(
        f,
        app,
        footer,
        &[("Dismiss", KeyCode::Enter), ("Browser", KeyCode::Char('o'))],
        &[("Esc", "dismiss")],
    );
}
