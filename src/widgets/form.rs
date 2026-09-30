use crate::mouse::Target;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph};
use tui_textarea::{CursorMove, TextArea};

use crate::views::common::panel;

pub fn editor(value: &str) -> TextArea<'static> {
    let mut editor = TextArea::new(value.split('\n').map(str::to_string).collect());
    editor.move_cursor(CursorMove::Bottom);
    editor.move_cursor(CursorMove::End);
    editor.set_cursor_line_style(Style::default());
    editor.set_cursor_style(Style::default().fg(Color::Black).bg(Color::Cyan));
    editor.set_selection_style(Style::default().fg(Color::Black).bg(Color::LightBlue));
    editor
}

pub fn text(editor: &TextArea<'_>) -> String {
    editor.lines().join("\n")
}

pub fn char_width(editor: &TextArea<'_>, c: char, offset: usize) -> usize {
    if c == '\t' && editor.tab_length() > 0 {
        let tab = editor.tab_length() as usize;
        tab - offset % tab
    } else {
        unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
    }
}

pub fn input(editor: &mut TextArea<'_>, key: KeyCode, modifiers: KeyModifiers, multiline: bool) {
    if key == KeyCode::Enter
        || (modifiers.contains(KeyModifiers::CONTROL) && matches!(key, KeyCode::Char('j' | 'm')))
    {
        if multiline {
            editor.insert_newline();
        }
    } else {
        editor.input(KeyEvent::new(key, modifiers));
    }
}

pub fn paste(editor: &mut TextArea<'_>, value: &str, multiline: bool) {
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    editor.insert_str(if multiline {
        value
    } else {
        value.replace('\n', " ")
    });
}

pub fn render_editor(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    label: &str,
    editor: &TextArea<'_>,
    focused: bool,
    field: usize,
) {
    let block = panel()
        .title(format!(" {label} "))
        .border_style(Style::default().fg(if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        }));
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.mouse_targets
        .borrow_mut()
        .push((area, Target::Field(field)));
    if focused {
        app.text_selection.borrow_mut().editor_selected = editor
            .selection_range()
            .is_some_and(|(start, end)| start != end);
        f.render_widget(editor, inner);
        let (row, column) = editor.cursor();
        for y in inner.y..inner.bottom() {
            for x in inner.x..inner.right() {
                if f.buffer_mut()[(x, y)].bg == Color::Cyan {
                    let width: usize = editor.lines()[row]
                        .chars()
                        .take(column)
                        .fold(0, |width, c| width + char_width(editor, c, width));
                    app.mouse_targets.borrow_mut().push((
                        inner,
                        Target::Text {
                            field,
                            top: row.saturating_sub((y - inner.y) as usize),
                            left: width.saturating_sub((x - inner.x) as usize),
                        },
                    ));
                    return;
                }
            }
        }
    } else {
        f.render_widget(Paragraph::new(text(editor)), inner);
        app.mouse_targets.borrow_mut().push((
            inner,
            Target::Text {
                field,
                top: 0,
                left: 0,
            },
        ));
    }
}

pub fn matching(options: &[String], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    options
        .iter()
        .enumerate()
        .filter(|(_, value)| value.to_lowercase().contains(&query))
        .map(|(i, _)| i)
        .collect()
}

pub fn choose(options: &[String], selected: &mut usize, query: &mut String, key: KeyCode) {
    match key {
        KeyCode::Char(c) => {
            query.push(c);
        }
        KeyCode::Backspace => {
            query.pop();
        }
        _ => {}
    }
    let matches = matching(options, query);
    let current = matches.iter().position(|i| i == selected).unwrap_or(0);
    let index = match key {
        KeyCode::Down => (current + 1).min(matches.len().saturating_sub(1)),
        KeyCode::Up => current.saturating_sub(1),
        _ => current,
    };
    if let Some(index) = matches.get(index) {
        *selected = *index;
    }
}

pub fn render_choices(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    field: (usize, &str),
    options: &[String],
    selected: usize,
    query: &str,
) {
    let block = panel()
        .title(format!(
            " {}{} ",
            field.1,
            if query.is_empty() {
                String::new()
            } else {
                format!(": {query}")
            }
        ))
        .border_style(
            Style::default().fg(if app.focused_field() == Some(field.0) {
                Color::Cyan
            } else {
                Color::DarkGray
            }),
        );
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.mouse_targets
        .borrow_mut()
        .push((area, Target::Field(field.0)));
    let matches = matching(options, query);
    let current = matches.iter().position(|i| *i == selected).unwrap_or(0);
    let start = current.saturating_sub(inner.height.saturating_sub(1) as usize);
    let mut lines = Vec::new();
    for (row, index) in matches
        .iter()
        .skip(start)
        .take(inner.height as usize)
        .enumerate()
    {
        let style = if *index == selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::styled(
            format!(
                "{}{}",
                if *index == selected { "› " } else { "  " },
                options[*index]
            ),
            style,
        ));
        app.mouse_targets.borrow_mut().push((
            Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
            Target::Choose {
                field: field.0,
                index: *index,
            },
        ));
    }
    if lines.is_empty() {
        lines.push(Line::from("No matches"));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

pub fn buttons(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    buttons: &[(&str, KeyCode)],
) {
    let mut x = area.x;
    for (label, key) in buttons {
        let label = format!("[{label}]");
        let width = (label.len() as u16).min(area.right().saturating_sub(x));
        if width == 0 || area.height == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        f.render_widget(
            Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
            rect,
        );
        app.mouse_targets
            .borrow_mut()
            .push((rect, Target::Key(*key)));
        x = x.saturating_add(width + 2);
    }
}

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
    app: &crate::app::App,
    title: &str,
    percent_x: u16,
    percent_y: u16,
) -> Rect {
    let area = centered_rect(percent_x, percent_y, f.area());
    f.render_widget(Clear, area);
    let block = panel().title(format!(" {} ", title));
    let inner = block.inner(area);
    app.text_selection.borrow_mut().area = inner;
    f.render_widget(block, area);
    inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_and_paste_work_in_the_middle_without_submitting() {
        let mut field = editor("h🙂o");
        field.move_cursor(CursorMove::Back);
        paste(&mut field, "i\r\nthere", true);
        assert_eq!(text(&field), "h🙂i\nthereo");
        input(&mut field, KeyCode::Home, KeyModifiers::NONE, true);
        input(&mut field, KeyCode::Delete, KeyModifiers::NONE, true);
        assert_eq!(text(&field), "h🙂i\nhereo");
        let mut summary = editor("Fix ");
        paste(&mut summary, "login\r\nflow", false);
        assert_eq!(text(&summary), "Fix login flow");
        let choices = vec![
            "Alex (alex@example.com)".into(),
            "Priya (priya@example.com)".into(),
        ];
        let mut selected = 0;
        let mut query = "PRI".into();
        choose(&choices, &mut selected, &mut query, KeyCode::Null);
        assert_eq!(selected, 1);
        assert_eq!(matching(&choices, &query), [1]);
    }
}
