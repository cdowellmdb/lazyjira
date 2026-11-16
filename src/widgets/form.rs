use crate::mouse::Target;
use crate::style;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
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

/// Cells of an inline field's label: the longest, "Pinned epics" or "Project key", and a space.
pub(crate) const LABEL_WIDTH: usize = 13;

/// A field's label: bold accent while it has focus, muted otherwise.
pub(crate) fn label_style(focused: bool) -> Style {
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
            Constraint::Length((LABEL_WIDTH as u16).min(area.width)),
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
    let room = Rect::new(x, row.y, row.right().saturating_sub(x), row.height.min(1));
    crate::footer::render_pairs(f, app, room, hints);
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

/// What a menu panel shows, built once a frame: the title, the lines, the line each option is
/// on, the chosen option, where a body with no options is scrolled to, and the hints.
pub struct Menu {
    pub title: Line<'static>,
    pub lines: Vec<Line<'static>>,
    pub choices: Vec<(usize, usize)>,
    pub selected: Option<usize>,
    pub scroll: u16,
    pub hints: &'static [(&'static str, &'static str)],
}

impl Menu {
    /// The body's rows: every line, down to one.
    pub fn shape(&self) -> [Row; 1] {
        [Row::flex(self.lines.len() as u16, 1)]
    }
}

/// Draws a menu in the panel docked in `area`. With a chosen option its lines scroll to keep it
/// in view and each option is clickable; without, the lines wrap and scroll as the menu says.
pub fn render_menu(f: &mut ratatui::Frame, app: &crate::app::App, area: Rect, menu: Menu) {
    let (body, footer) = render_panel_frame(f, app, area, menu.title);
    crate::footer::render_pairs(f, app, footer, menu.hints);
    let Some(selected) = menu.selected else {
        f.render_widget(
            Paragraph::new(menu.lines)
                .wrap(Wrap { trim: false })
                .scroll((menu.scroll, 0)),
            body,
        );
        return;
    };
    let selected_line = menu
        .choices
        .iter()
        .find(|(_, index)| *index == selected)
        .map_or(0, |(line, _)| *line);
    let scroll = selected_line.saturating_sub(body.height.saturating_sub(1) as usize);
    choice_targets(app, body, scroll, &menu.choices);
    f.render_widget(Paragraph::new(menu.lines).scroll((scroll as u16, 0)), body);
}

/// Cells of a close button: `  ×  `, its click cell with the glyph in the middle.
const CLOSE_WIDTH: u16 = 5;

/// A muted `×` centered in a click cell that ends one cell before the right edge of `row`'s
/// first line and does `target` when clicked.
pub fn close_button(f: &mut ratatui::Frame, app: &crate::app::App, row: Rect, target: Target) {
    if row.width < 2 * CLOSE_WIDTH || row.is_empty() {
        return;
    }
    let rect = Rect::new(row.right() - CLOSE_WIDTH - 1, row.y, CLOSE_WIDTH, 1);
    f.render_widget(
        Paragraph::new(Span::styled("  ×  ", style::secondary())),
        rect,
    );
    app.mouse_targets.borrow_mut().push((rect, target));
}

/// Draws a sheet over the whole frame, which it clears: `title` in bold on the first row with
/// a `×` that closes it, a blank row, then the body. Returns the body, two cells in from
/// each side, and makes it selectable.
pub fn render_sheet(f: &mut ratatui::Frame, app: &crate::app::App, title: &str) -> Rect {
    let area = f.area();
    f.render_widget(Clear, area);
    let [head, _, rest] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(self::title(title, "")), head);
    close_button(f, app, head, Target::Key(KeyCode::Esc));
    let inner = Rect::new(
        rest.x.saturating_add(2),
        rest.y,
        rest.width.saturating_sub(4),
        rest.height,
    );
    crate::mouse::selectable(app, inner);
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
/// its top, `title` under it and a `×` that cancels, and makes what is under the title
/// selectable. Returns the body and the last row, for the buttons or hints, a cell in from each
/// side.
pub fn render_panel_frame(
    f: &mut ratatui::Frame,
    app: &crate::app::App,
    area: Rect,
    title: Line<'static>,
) -> (Rect, Rect) {
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
        rest.x.saturating_add(PANEL_MARGIN / 2),
        rest.y,
        rest.width.saturating_sub(PANEL_MARGIN),
        rest.height,
    );
    crate::mouse::selectable(app, inner);
    let [body, footer] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(u16::from(!inner.is_empty())),
    ])
    .areas(inner);
    (body, footer)
}

/// The rows of a panel's chrome: its rule and its title.
pub const CHROME: u16 = 2;

/// The cells a panel's body is narrower than its area by: one in from each side.
pub const PANEL_MARGIN: u16 = 2;

/// One row of a panel's body: the rows it wants, and the fewest it keeps when the panel is
/// short. A field's rows are its label and its value, so a field's minimum is one row, the
/// label and value side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    want: u16,
    min: u16,
}

impl Row {
    /// A row that keeps its height until nothing else can give.
    pub const fn fixed(rows: u16) -> Row {
        Row {
            want: rows,
            min: rows,
        }
    }

    /// A row that wants `want` rows and gives up to `want - min` of them when the panel is short.
    pub const fn flex(want: u16, min: u16) -> Row {
        Row { want, min }
    }
}

/// How tall a panel's body wants to be, and the least it needs for every row to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Need {
    pub want: u16,
    pub min: u16,
}

/// The rows `rows` want and need in all. A panel declares its `rows` once, and sizes itself
/// (`need`) and lays itself out (`split`) from them, so the two cannot drift apart.
pub fn need(rows: &[Row]) -> Need {
    Need {
        want: rows.iter().map(|row| row.want).sum(),
        min: rows.iter().map(|row| row.min).sum(),
    }
}

/// Splits `area` into `rows`, top to bottom. Short of room, the last rows give up theirs first,
/// each down to its minimum, then the last row to have any left gives up the rest. Room left over
/// goes to the last row that gives.
pub fn split<const N: usize>(rows: [Row; N], area: Rect) -> [Rect; N] {
    let mut heights = rows.map(|row| row.want);
    let mut deficit = heights.iter().sum::<u16>().saturating_sub(area.height);
    for (height, row) in heights.iter_mut().zip(rows).rev() {
        let give = deficit.min(*height - row.min);
        *height -= give;
        deficit -= give;
    }
    for height in heights.iter_mut().rev() {
        let give = deficit.min(*height);
        *height -= give;
        deficit -= give;
    }
    let spare = area.height.saturating_sub(heights.iter().sum::<u16>());
    if let Some(at) = rows.iter().rposition(|row| row.want > row.min) {
        heights[at] += spare;
    }
    let mut y = area.y;
    heights.map(|height| {
        let rect = Rect::new(area.x, y, area.width, height);
        y += height;
        rect
    })
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

    /// The heights `split` gives `rows` in `height` rows.
    fn heights<const N: usize>(rows: [Row; N], height: u16) -> [u16; N] {
        split(rows, Rect::new(0, 5, 10, height)).map(|rect| rect.height)
    }

    #[test]
    fn rows_are_split_top_down_and_the_last_rows_give_up_theirs_first_down_to_their_minimums() {
        // The create form's body: pickers (4, at least 2), summary, labels, description
        // (1 + 3 lines, at least 1).
        let create = [
            Row::flex(4, 2),
            Row::fixed(1),
            Row::fixed(1),
            Row::flex(4, 1),
        ];
        assert_eq!(need(&create), Need { want: 10, min: 5 });
        assert_eq!(heights(create, 10), [4, 1, 1, 4]);
        assert_eq!(heights(create, 9), [4, 1, 1, 3]);
        assert_eq!(heights(create, 7), [4, 1, 1, 1]);
        assert_eq!(heights(create, 6), [3, 1, 1, 1]);
        assert_eq!(heights(create, 5), [2, 1, 1, 1]);
        // Below the minimums the last rows go first, down to nothing.
        assert_eq!(heights(create, 4), [2, 1, 1, 0]);
        assert_eq!(heights(create, 3), [2, 1, 0, 0]);
        assert_eq!(heights(create, 0), [0, 0, 0, 0]);
        // Room left over goes to the last row that gives.
        assert_eq!(heights(create, 12), [4, 1, 1, 6]);
        let rects = split(create, Rect::new(2, 5, 10, 6));
        assert_eq!(
            rects,
            [
                Rect::new(2, 5, 10, 3),
                Rect::new(2, 8, 10, 1),
                Rect::new(2, 9, 10, 1),
                Rect::new(2, 10, 10, 1),
            ]
        );
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
