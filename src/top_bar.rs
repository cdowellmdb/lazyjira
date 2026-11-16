//! The top bar: `lazyjira`, the tabs and the Settings button on one line, a muted rule under it.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Tab};
use crate::mouse::Target;
use crate::style;

/// Rows the bar takes: the line and its rule.
pub const HEIGHT: u16 = 2;
/// Blank cells between the name and the first tab, and between tabs.
const TAB_GAP: u16 = 3;
/// A tab's click target is its title and the cell either side.
const TAB_PAD: u16 = 1;
const SETTINGS_LABEL: &str = "Settings";
/// A button's label has this many cells either side.
const BUTTON_PAD: u16 = 1;
/// The Settings button is its label with its padding, and ends two cells from the edge.
const SETTINGS_WIDTH: u16 = SETTINGS_LABEL.len() as u16 + 2 * BUTTON_PAD;
const SETTINGS_EDGE: u16 = 2;

pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let [line, rule] = Layout::vertical([Constraint::Length(1); 2]).areas(area);
    let mut spans = vec![Span::raw(" "), Span::styled("lazyjira", style::secondary())];
    let mut x = area.x + 1 + "lazyjira".width() as u16;
    for tab in Tab::all() {
        let title = tab.title();
        let width = title.width() as u16;
        x += TAB_GAP;
        let rect = Rect::new(
            x - TAB_PAD,
            area.y,
            (width + 2 * TAB_PAD).min(area.right().saturating_sub(x - TAB_PAD)),
            1,
        );
        app.mouse_targets
            .borrow_mut()
            .push((rect, Target::Tab(*tab)));
        spans.push(Span::raw(" ".repeat(TAB_GAP as usize)));
        spans.push(Span::styled(
            title,
            if *tab == app.active_tab {
                style::accent().patch(style::heading())
            } else {
                style::secondary()
            },
        ));
        x += width;
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(style::primary()),
        line,
    );
    if area.right().saturating_sub(x) >= SETTINGS_WIDTH + SETTINGS_EDGE {
        crate::widgets::form::buttons(
            f,
            app,
            Rect::new(
                area.right() - SETTINGS_WIDTH - SETTINGS_EDGE,
                area.y,
                SETTINGS_WIDTH,
                1,
            ),
            &[(SETTINGS_LABEL, crossterm::event::KeyCode::Char('S'))],
        );
    }
    f.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(style::secondary()),
        rule,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};
    use ratatui::Terminal;

    fn draw(app: &App, width: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, 3)).unwrap();
        terminal
            .draw(|f| render(f, app, Rect::new(0, 0, width, HEIGHT)))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn one_line_holds_the_name_the_tabs_and_a_right_aligned_settings_over_a_rule() {
        let app = App::new();
        let buffer = draw(&app, 160);
        let line = row(&buffer, 0);
        assert!(
            line.starts_with(" lazyjira   My Work   Team   Epics   Unassigned   Filters   "),
            "{line}"
        );
        assert_eq!(
            line.trim_end().len(),
            157,
            "Settings ends two cells from the edge"
        );
        assert_eq!(line.find("Settings"), Some(149));
        assert_eq!(row(&buffer, 1), "─".repeat(160));
        assert!(buffer.content.iter().all(|cell| cell.symbol() != "│"));
    }

    #[test]
    fn the_active_tab_is_bold_cyan_and_the_rest_are_muted() {
        let mut app = App::new();
        app.active_tab = Tab::Epics;
        let buffer = draw(&app, 160);
        for (x, active) in [
            (12, false),
            (22, false),
            (29, true),
            (37, false),
            (50, false),
        ] {
            let cell = &buffer[(x, 0)];
            assert_eq!(
                (cell.fg, cell.modifier.contains(Modifier::BOLD)),
                if active {
                    (Color::Cyan, true)
                } else {
                    (Color::DarkGray, false)
                },
                "x = {x}"
            );
        }
        assert_eq!(buffer[(1, 0)].fg, Color::DarkGray);
        assert_eq!(buffer[(0, 1)].fg, Color::DarkGray);
    }

    #[test]
    fn each_tab_and_the_settings_button_has_a_target_over_its_own_cells() {
        let app = App::new();
        draw(&app, 160);
        assert_eq!(
            *app.mouse_targets.borrow(),
            vec![
                (Rect::new(11, 0, 9, 1), Target::Tab(Tab::MyWork)),
                (Rect::new(21, 0, 6, 1), Target::Tab(Tab::Team)),
                (Rect::new(28, 0, 7, 1), Target::Tab(Tab::Epics)),
                (Rect::new(36, 0, 12, 1), Target::Tab(Tab::Unassigned)),
                (Rect::new(49, 0, 9, 1), Target::Tab(Tab::Filters)),
                (
                    Rect::new(148, 0, 10, 1),
                    Target::Key(crossterm::event::KeyCode::Char('S'))
                ),
            ]
        );
    }

    #[test]
    fn a_terminal_too_narrow_for_settings_leaves_it_out_and_clips_the_last_tab() {
        let app = App::new();
        // Filters ends at column 57, so Settings needs 12 more cells.
        draw(&app, 69);
        assert_eq!(app.mouse_targets.borrow().len(), 6);
        app.mouse_targets.borrow_mut().clear();
        draw(&app, 68);
        assert_eq!(app.mouse_targets.borrow().len(), 5);
        app.mouse_targets.borrow_mut().clear();
        draw(&app, 52);
        let targets = app.mouse_targets.borrow();
        assert_eq!(
            targets.last().copied(),
            Some((Rect::new(49, 0, 3, 1), Target::Tab(Tab::Filters)))
        );
    }

    #[test]
    fn at_80_columns_everything_fits_and_settings_ends_two_cells_from_the_edge() {
        let app = App::new();
        let buffer = draw(&app, 80);
        let line = row(&buffer, 0);
        assert_eq!(
            line.trim_end(),
            format!(
                " lazyjira   My Work   Team   Epics   Unassigned   Filters{}Settings",
                " ".repeat(12)
            )
        );
        assert_eq!(line.find("Settings"), Some(69));
        assert_eq!(line.trim_end().len(), 77);
        assert_eq!(row(&buffer, 1), "─".repeat(80));
        assert_eq!(app.mouse_targets.borrow().len(), 6);
    }

    #[test]
    fn a_one_row_bar_draws_no_rule() {
        let app = App::new();
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|f| render(f, &app, Rect::new(0, 0, 80, 1)))
            .unwrap();
        let line = row(terminal.backend().buffer(), 0);
        assert!(!line.contains('─'), "{line}");
        assert!(line.starts_with(" lazyjira   My Work"));
    }
}
