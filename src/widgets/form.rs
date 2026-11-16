use crate::mouse::Target;
use crate::style;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use tui_textarea::{CursorMove, TextArea};
use unicode_width::UnicodeWidthStr;

pub fn editor(value: &str) -> TextArea<'static> {
    let mut editor = TextArea::new(value.split('\n').map(str::to_string).collect());
    editor.move_cursor(CursorMove::Bottom);
    editor.move_cursor(CursorMove::End);
    editor.set_cursor_line_style(Style::default());
    editor.set_cursor_style(style::cursor());
    editor.set_selection_style(style::text_selection());
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

/// Cells of an inline field's label: the longest, "Pinned epics", and a space.
const LABEL_WIDTH: u16 = 13;

/// A field's label: bold accent while it has focus, muted otherwise.
fn label_style(focused: bool) -> Style {
    if focused {
        style::accent().patch(style::heading())
    } else {
        style::secondary()
    }
}

/// A text field in `area`: its `label` (the words after ` · ` say how to fill it), then the
/// editor. One row holds the label and the value side by side. More rows put the label on a
/// muted rule over the value, which takes the rest. No box.
pub fn render_editor(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    label: &str,
    editor: &TextArea<'_>,
    focused: bool,
    field: usize,
) {
    if area.is_empty() {
        return;
    }
    let (name, detail) = label.split_once(" · ").unwrap_or((label, ""));
    let value = if area.height == 1 {
        let [head, value] = Layout::horizontal([
            Constraint::Length(LABEL_WIDTH.min(area.width)),
            Constraint::Min(0),
        ])
        .areas(area);
        f.render_widget(
            Paragraph::new(Span::styled(name.to_string(), label_style(focused))),
            head,
        );
        value
    } else {
        let [head, value] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        let said = if detail.is_empty() {
            name.to_string()
        } else {
            format!("{name}  {detail}")
        };
        let rule = (head.width as usize).saturating_sub(said.width() + 1);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(name.to_string(), label_style(focused)),
                Span::styled(
                    format!("{} {}", &said[name.len()..], "─".repeat(rule)),
                    style::secondary(),
                ),
            ])),
            head,
        );
        value
    };
    app.mouse_targets
        .borrow_mut()
        .push((area, Target::Field(field)));
    if focused {
        app.text_selection.borrow_mut().editor_selected = editor
            .selection_range()
            .is_some_and(|(start, end)| start != end);
        f.render_widget(editor, value);
        let (row, column) = editor.cursor();
        let cursor = style::cursor().bg.unwrap_or_default();
        for y in value.y..value.bottom() {
            for x in value.x..value.right() {
                if f.buffer_mut()[(x, y)].bg == cursor {
                    let width: usize = editor.lines()[row]
                        .chars()
                        .take(column)
                        .fold(0, |width, c| width + char_width(editor, c, width));
                    app.mouse_targets.borrow_mut().push((
                        value,
                        Target::Text {
                            field,
                            top: row.saturating_sub((y - value.y) as usize),
                            left: width.saturating_sub((x - value.x) as usize),
                        },
                    ));
                    return;
                }
            }
        }
    } else {
        if area.height == 1 && editor.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(detail.to_string(), style::secondary())),
                value,
            );
        } else {
            f.render_widget(Paragraph::new(text(editor)), value);
        }
        app.mouse_targets.borrow_mut().push((
            value,
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

/// A list of `options` in `area`: its label (with what has been typed to filter), then one
/// option per row, the chosen one marked `›`. No box.
pub fn render_choices(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    field: (usize, &str),
    options: &[String],
    selected: usize,
    query: &str,
) {
    if area.is_empty() {
        return;
    }
    let [head, inner] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let mut label = vec![Span::styled(
        field.1.to_string(),
        label_style(app.focused_field() == Some(field.0)),
    )];
    if !query.is_empty() {
        label.push(Span::raw(format!(": {query}")));
    }
    f.render_widget(Paragraph::new(Line::from(label)), head);
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
            style::warning().patch(style::heading())
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

/// Draws `buttons` along `area`'s first row and returns the column after the last, with its gap.
pub fn buttons(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    buttons: &[(&str, KeyCode)],
) -> u16 {
    // A filled block, with its shortcut letter (in either case) underlined when the label has it.
    let style = style::button();
    let mut x = area.x;
    for (label, key) in buttons {
        let width = (label.width() as u16 + 2).min(area.right().saturating_sub(x));
        if width == 0 || area.height == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        let shortcut = match key {
            KeyCode::Char(c) => label
                .char_indices()
                .find(|(_, l)| l.eq_ignore_ascii_case(c)),
            _ => None,
        };
        let line = match shortcut {
            Some((i, letter)) => {
                let end = i + letter.len_utf8();
                Line::from(vec![
                    Span::styled(format!(" {}", &label[..i]), style),
                    Span::styled(&label[i..end], style::button_letter()),
                    Span::styled(format!("{} ", &label[end..]), style),
                ])
            }
            None => Line::styled(format!(" {label} "), style),
        };
        f.render_widget(Paragraph::new(line), rect);
        app.mouse_targets
            .borrow_mut()
            .push((rect, Target::Key(*key)));
        x = x.saturating_add(width + 2);
    }
    x
}

/// The last row of a form: its `buttons`, then `hints` in the room they leave.
pub fn button_row(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    row: Rect,
    buttons_: &[(&str, KeyCode)],
    hints: &[(&'static str, &'static str)],
) {
    let x = buttons(f, app, row, buttons_);
    let room = Rect::new(
        x,
        row.y,
        row.right().saturating_sub(x),
        u16::from(!row.is_empty()),
    );
    if !room.is_empty() {
        let line = crate::footer::pairs_line(hints, room.width);
        crate::footer::register_keys(app, &line, room);
        f.render_widget(Paragraph::new(line), room);
    }
}

/// The bottom row of `area` as a hint line, and the rows above it.
pub fn hint_row(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    hints: &[(&'static str, &'static str)],
) -> Rect {
    let [body, row] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(u16::from(!area.is_empty())),
    ])
    .areas(area);
    let line = crate::footer::pairs_line(hints, row.width);
    crate::footer::register_keys(app, &line, row);
    f.render_widget(Paragraph::new(line), row);
    body
}

/// Registers a click target over each option's line in `body`, which shows its lines from line
/// `scroll`. `choices` are (line, option index).
pub fn choice_targets(
    app: &crate::app::App,
    body: Rect,
    scroll: usize,
    choices: &[(usize, usize)],
) {
    for &(line, index) in choices {
        if let Some(y) = line
            .checked_sub(scroll)
            .filter(|y| *y < body.height as usize)
        {
            app.mouse_targets.borrow_mut().push((
                Rect::new(body.x, body.y + y as u16, body.width, 1),
                Target::Choose { field: 0, index },
            ));
        }
    }
}

/// A menu: `lines` over a hint line, scrolled to keep the chosen option (`choices.1`) in view,
/// each option clickable.
pub fn menu(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    lines: Vec<Line<'static>>,
    choices: (&[(usize, usize)], usize),
    hints: &[(&'static str, &'static str)],
) {
    let body = hint_row(f, app, area, hints);
    let selected_line = choices
        .0
        .iter()
        .find(|(_, index)| *index == choices.1)
        .map_or(0, |(line, _)| *line);
    let scroll = selected_line.saturating_sub(body.height.saturating_sub(1) as usize);
    choice_targets(app, body, scroll, choices.0);
    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), body);
}

/// Cells of a close button: ` [×] `.
const CLOSE_WIDTH: u16 = 5;

/// A muted `[×]` ending one cell before the right edge of `row`'s first line, which does
/// `target` when clicked.
pub fn close_button(f: &mut ratatui::Frame, app: &crate::app::App, row: Rect, target: Target) {
    if row.width < 2 * CLOSE_WIDTH || row.is_empty() {
        return;
    }
    let rect = Rect::new(row.right() - CLOSE_WIDTH - 1, row.y, CLOSE_WIDTH, 1);
    f.render_widget(
        Paragraph::new(Span::styled(" [×] ", style::secondary())),
        rect,
    );
    app.mouse_targets.borrow_mut().push((rect, target));
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

pub fn render_popup_frame(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    title: &str,
    percent_x: u16,
    percent_y: u16,
) -> Rect {
    let area = centered_rect(percent_x, percent_y, f.area());
    f.render_widget(Clear, area);
    let block = crate::views::common::panel().title(format!(" {} ", title));
    let inner = block.inner(area);
    crate::mouse::selectable(app, inner);
    f.render_widget(block, area);
    inner
}

/// A panel's title line: `name` in bold, then `detail` muted.
pub fn title(name: impl Into<String>, detail: &str) -> Line<'static> {
    let mut spans = vec![Span::raw(" "), Span::styled(name.into(), style::heading())];
    if !detail.is_empty() {
        spans.push(Span::styled(format!(" {detail}"), style::secondary()));
    }
    Line::from(spans)
}

/// Draws the frame of a panel docked in `area`: it clears the area, draws a muted rule across
/// its top, `title` under it and a `[×]` that cancels, and makes what is under the title
/// selectable. Returns that area, a cell in from each side.
pub fn render_modal_frame(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    title: Line<'static>,
) -> Rect {
    f.render_widget(Clear, area);
    let [rule, head, rest] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    f.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(style::secondary()),
        rule,
    );
    f.render_widget(Paragraph::new(title), head);
    close_button(f, app, head, Target::Key(KeyCode::Esc));
    let inner = Rect::new(
        rest.x.saturating_add(1),
        rest.y,
        rest.width.saturating_sub(2),
        rest.height,
    );
    crate::mouse::selectable(app, inner);
    inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    #[test]
    fn buttons_are_filled_blocks_sized_by_columns_with_their_letter_underlined() {
        let app = crate::app::App::new();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(30, 1)).unwrap();
        terminal
            .draw(|f| {
                let end = buttons(
                    f,
                    &app,
                    f.area(),
                    &[("設定", KeyCode::Enter), ("Reload", KeyCode::Char('r'))],
                );
                assert_eq!(end, 18, "after the last button and its gap");
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        // "設定" is 4 columns wide: its button spans 6, then a 2-column gap.
        assert_eq!(buffer[(5, 0)].bg, Color::Gray);
        assert_eq!(buffer[(6, 0)].bg, Color::Reset);
        assert_eq!(buffer[(8, 0)].bg, Color::Gray);
        assert_eq!(buffer[(9, 0)].symbol(), "R");
        assert_eq!(buffer[(9, 0)].fg, Color::Black);
        assert!(buffer[(9, 0)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buffer[(10, 0)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buffer[(1, 0)].modifier.contains(Modifier::UNDERLINED));
        let targets: Vec<Rect> = app.mouse_targets.borrow().iter().map(|(r, _)| *r).collect();
        assert_eq!(targets, [Rect::new(0, 0, 6, 1), Rect::new(8, 0, 8, 1)]);
    }

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
