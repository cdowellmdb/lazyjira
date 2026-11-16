use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use super::form;
use crate::app::App;
use crate::style;

/// The rows the panel's body needs: the message, a blank line, Jira's error and the buttons.
pub fn rows(app: &App) -> u16 {
    let error = app
        .moves
        .failures()
        .first()
        .map_or(0, |failure| failure.error.lines().count());
    2 + error as u16 + 1
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
    let inner = form::render_modal_frame(f, app, area, form::title("Move failed", &more));
    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);

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
