//! The status area under the content. A key press or a list read leaves one line there: a
//! message, the search, or the footer's hints. While a detail is open it holds the detail's
//! hints instead, wrapped onto as many rows as the width needs so that every key stays a click
//! target at every width, and a message takes a row of its own above them.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::config::AppConfig;
use crate::footer::{self, Pairs};
use crate::style;
use crate::widgets;

pub struct Status {
    /// A message over the hints: a flash, or the moves waiting on Jira.
    message: Option<Line<'static>>,
    /// The detail's hints, a row of them per entry. Empty when the area is one `line`.
    hints: Vec<Pairs<'static>>,
    line: Line<'static>,
}

/// What the area under the content shows in `width` columns. A flash error leads, then a panel
/// (its own hints are in it) or the detail (its hints), then the search, the moves waiting on
/// Jira and the footer's hints. A flash shows over the detail's hints, which it must not hide,
/// and so do the moves waiting on Jira.
pub fn status(app: &App, config: &AppConfig, width: u16) -> Status {
    // One line: the terminal drops a line break, which would run Jira's messages together.
    let flash = app.flash.as_ref().map(|flash| {
        Line::from(Span::styled(
            flash.lines().collect::<Vec<_>>().join("; "),
            style::error(),
        ))
    });
    let moving = || {
        app.moves
            .pending_message()
            .map(|pending| Line::from(Span::styled(pending, style::warning())))
    };
    let line = |line| Status {
        message: None,
        hints: Vec::new(),
        line,
    };
    if widgets::dock::open(app).is_some() {
        line(flash.unwrap_or_default())
    } else if app.is_detail_open() {
        Status {
            message: flash.or_else(moving),
            hints: footer::wrap_pairs(widgets::ticket_detail::hints(app), width),
            line: Line::default(),
        }
    } else if let Some(flash) = flash {
        line(flash)
    } else if let Some(search) = &app.search {
        line(Line::from(Span::styled(
            format!("/{search}"),
            style::warning(),
        )))
    } else if let Some(moving) = moving() {
        line(moving)
    } else {
        line(footer::line(app, &config.preferences.theme, width))
    }
}

impl Status {
    /// The rows the area needs.
    pub fn height(&self) -> u16 {
        u16::from(self.message.is_some()) + self.hints.len().max(1) as u16
    }

    pub fn render(self, f: &mut ratatui::Frame, app: &App, area: Rect) {
        if self.hints.is_empty() {
            f.render_widget(Paragraph::new(self.line), area);
            return;
        }
        let mut rows = area.rows();
        if let Some(message) = self.message {
            if let Some(row) = rows.next() {
                f.render_widget(Paragraph::new(message), row);
            }
        }
        for (pairs, row) in self.hints.into_iter().zip(rows) {
            footer::render_pairs(f, app, row, pairs);
        }
    }
}
