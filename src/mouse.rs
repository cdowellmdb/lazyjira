use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::{
    buffer::Buffer,
    style::{Color, Style},
    widgets::Paragraph,
};
use tokio::sync::mpsc::UnboundedSender;
use tui_textarea::CursorMove;

use crate::{
    app::{App, BulkUploadState, DetailMode, FilterFocus, Tab},
    bulk_actions::BulkState,
    config::AppConfig,
    BackgroundMessage,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Tab(Tab),
    Row(usize),
    Mark(usize),
    Fold(usize),
    Filter(usize),
    Field(usize),
    Text {
        field: usize,
        top: usize,
        left: usize,
    },
    Choose {
        field: usize,
        index: usize,
    },
    CloseDetail,
    Copy,
    Key(KeyCode),
}

#[derive(Default)]
pub struct TextSelection {
    pub area: Rect,
    pub editor_selected: bool,
    // ponytail: selects visible rendered rows; use source ranges if offscreen selection is needed.
    screen: Buffer,
    range: Option<(Position, Position)>,
    pressed: Option<Press>,
}

#[derive(Clone, Copy)]
struct Press {
    start: Position,
    target: Option<(Rect, Target)>,
    dragged: bool,
}

impl TextSelection {
    fn point(&self, point: Position) -> Position {
        let area = self.screen.area;
        let y = point.y.clamp(area.y, area.bottom().saturating_sub(1));
        let column = point.x.clamp(area.x, area.right());
        let mut x = area.x;
        while x < column {
            let width =
                unicode_width::UnicodeWidthStr::width(self.screen[(x, y)].symbol()).max(1) as u16;
            if x + width > column {
                break;
            }
            x += width;
        }
        Position::new(x, y)
    }

    fn rows(&self) -> Vec<Rect> {
        let Some((mut start, mut end)) = self.range else {
            return Vec::new();
        };
        if (start.y, start.x) > (end.y, end.x) {
            std::mem::swap(&mut start, &mut end);
        }
        (start.y..=end.y)
            .map(|y| {
                let left = if y == start.y {
                    start.x
                } else {
                    self.screen.area.x
                };
                let right = if y == end.y {
                    end.x
                } else {
                    self.screen.area.right()
                };
                Rect::new(left, y, right.saturating_sub(left), 1)
            })
            .collect()
    }

    fn text(&self) -> Option<String> {
        self.range.filter(|(start, end)| start != end)?;
        Some(
            self.rows()
                .into_iter()
                .map(|row| {
                    let mut text = String::new();
                    let mut x = row.x;
                    while x < row.right() {
                        let symbol = self.screen[(x, row.y)].symbol();
                        text.push_str(symbol);
                        x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
                    }
                    if row.right() == self.screen.area.right() {
                        text.trim_end().to_string()
                    } else {
                        text
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

pub fn render_selection(f: &mut ratatui::Frame, app: &App) {
    let mut selection = app.text_selection.borrow_mut();
    let area = selection.area.intersection(f.area());
    let mut screen = Buffer::empty(area);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            screen[(x, y)] = f.buffer_mut()[(x, y)].clone();
        }
    }
    // A selection must keep referring to the text the user actually highlighted.
    let pressed_rows = selection
        .pressed
        .and_then(|press| press.target)
        .filter(|(_, target)| !matches!(target, Target::Text { .. } | Target::Copy))
        .map(|(area, _)| area.intersection(screen.area));
    if selection.screen.area != screen.area
        || selection.rows().iter().any(|row| {
            (row.x..row.right())
                .any(|x| selection.screen[(x, row.y)].symbol() != screen[(x, row.y)].symbol())
        })
        || pressed_rows.is_some_and(|area| {
            (area.y..area.bottom()).any(|y| {
                (area.x..area.right())
                    .any(|x| selection.screen[(x, y)].symbol() != screen[(x, y)].symbol())
            })
        })
    {
        selection.range = None;
        selection.pressed = None;
    }
    selection.screen = screen;
    for row in selection.rows() {
        f.buffer_mut()
            .set_style(row, Style::default().fg(Color::Black).bg(Color::LightBlue));
    }
    if selection.range.is_some_and(|(start, end)| start != end) || selection.editor_selected {
        let footer = Rect::new(
            f.area().x,
            f.area().bottom().saturating_sub(1),
            f.area().width,
            u16::from(f.area().height > 0),
        );
        f.render_widget(ratatui::widgets::Clear, footer);
        f.render_widget(
            Paragraph::new(format!(
                "[Copy]  {}",
                app.flash.as_deref().unwrap_or("Ctrl+C copy · Esc clear")
            ))
            .style(Style::default().fg(Color::Cyan)),
            footer,
        );
        app.mouse_targets.borrow_mut().push((
            Rect {
                width: footer.width.min(6),
                ..footer
            },
            Target::Copy,
        ));
    }
}

pub fn begin_layer(app: &App) {
    app.mouse_targets.borrow_mut().clear();
    app.text_selection.borrow_mut().editor_selected = false;
}

pub fn selected_text(app: &mut App) -> Option<String> {
    if let Some((editor, _)) = app.current_editor() {
        if let Some((start, end)) = editor.selection_range().filter(|(start, end)| start != end) {
            return Some(
                (start.0..=end.0)
                    .map(|row| {
                        let left = if row == start.0 { start.1 } else { 0 };
                        let right = if row == end.0 {
                            end.1
                        } else {
                            editor.lines()[row].chars().count()
                        };
                        editor.lines()[row]
                            .chars()
                            .skip(left)
                            .take(right - left)
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }
    app.text_selection.borrow().text()
}

pub fn clear(app: &mut App) {
    clear_screen(app);
    if let Some((editor, _)) = app.current_editor() {
        editor.cancel_selection();
    }
}

pub fn clear_screen(app: &App) {
    let mut selection = app.text_selection.borrow_mut();
    selection.range = None;
    selection.pressed = None;
}

pub fn copy_selected(app: &mut App) {
    let Some(text) = selected_text(app) else {
        return;
    };
    if let Some((editor, _)) = app.current_editor() {
        editor.set_yank_text(text.clone());
    }
    let result = copy_to_clipboard(&text);
    app.flash = Some(match result {
        Ok(()) => "Copied selection".into(),
        Err(error) => format!("Couldn't copy: {error:#}"),
    });
}

fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let written = child.stdin.take().unwrap().write_all(text.as_bytes());
    let output = child.wait_with_output()?;
    written?;
    anyhow::ensure!(
        output.status.success(),
        "pbcopy exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn target_at(app: &App, point: Position) -> Option<(Rect, Target)> {
    app.mouse_targets
        .borrow()
        .iter()
        .rev()
        .find(|(area, _)| area.contains(point))
        .copied()
}

fn position_editor(app: &mut App, area: Rect, target: Target, point: Position) {
    let Target::Text { field, top, left } = target else {
        return;
    };
    app.focus_field(field);
    if let Some((editor, _)) = app.current_editor() {
        let row = (top
            + point
                .y
                .clamp(area.y, area.bottom().saturating_sub(1))
                .saturating_sub(area.y) as usize)
            .min(editor.lines().len() - 1);
        let width = left + point.x.clamp(area.x, area.right()).saturating_sub(area.x) as usize;
        let mut used = 0;
        let column = editor.lines()[row]
            .chars()
            .take_while(|c| {
                used += crate::widgets::form::char_width(editor, *c, used);
                used <= width
            })
            .count();
        editor.move_cursor(CursorMove::Jump(
            row.min(u16::MAX as usize) as u16,
            column.min(u16::MAX as usize) as u16,
        ));
    }
}

fn drag(app: &mut App, point: Position) {
    let Some(mut press) = app.text_selection.borrow().pressed else {
        return;
    };
    if point == press.start && !press.dragged {
        return;
    }
    press.dragged = true;
    app.text_selection.borrow_mut().pressed = Some(press);
    if let Some((_, Target::Text { field, .. })) = press.target {
        let target = app
            .mouse_targets
            .borrow()
            .iter()
            .find(|(_, target)| matches!(target, Target::Text {field: f, ..} if *f == field))
            .copied();
        if let Some((area, target)) = target {
            position_editor(app, area, target, point);
        }
    } else {
        let mut selection = app.text_selection.borrow_mut();
        if selection.screen.area.contains(press.start) {
            selection.range = Some((selection.point(press.start), selection.point(point)));
        }
    }
}

pub fn register_rows(
    app: &App,
    area: Rect,
    rows: &[(usize, usize, bool)],
    scroll: u16,
    lines: &[ratatui::text::Line<'_>],
) {
    let inner = crate::views::common::panel().inner(area);
    for &(line, index, header) in rows {
        if let Some(y) = line
            .checked_sub(scroll as usize)
            .filter(|y| *y < inner.height as usize)
        {
            let rect = Rect::new(inner.x, inner.y + y as u16, inner.width, 1);
            let mut targets = app.mouse_targets.borrow_mut();
            targets.push((rect, Target::Row(index)));
            let text = lines[line]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            let prefix = text.find('[').unwrap_or(0) as u16;
            targets.push((
                Rect::new(
                    rect.x + prefix,
                    rect.y,
                    rect.width.saturating_sub(prefix).min(3),
                    1,
                ),
                Target::Mark(index),
            ));
            if header {
                targets.push((
                    Rect::new(rect.x + 4, rect.y, rect.width.saturating_sub(4).min(1), 1),
                    Target::Fold(index),
                ));
            } else if let Some(arrow) = text.split(" │ ").next().and_then(|key_cell| {
                key_cell
                    .find(['▼', '▶'])
                    .map(|at| key_cell[..at].chars().count())
            }) {
                // A parent's fold arrow sits in its key cell.
                targets.push((
                    Rect::new(rect.x + arrow as u16, rect.y, 1, 1),
                    Target::Fold(index),
                ));
            }
        }
    }
}

pub async fn handle(
    app: &mut App,
    event: MouseEvent,
    tx: &UnboundedSender<BackgroundMessage>,
    config: &mut AppConfig,
) {
    match event.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            clear_screen(app);
            let hovered = app
                .mouse_targets
                .borrow()
                .iter()
                .rev()
                .find(|(area, _)| area.contains(Position::new(event.column, event.row)))
                .map(|(_, target)| *target);
            if let Some(
                Target::Field(field) | Target::Text { field, .. } | Target::Choose { field, .. },
            ) = hovered
            {
                app.focus_field(field);
            }
            if let Some((editor, _)) = app.current_editor() {
                editor.input(event);
                return;
            }
            let key = if event.kind == MouseEventKind::ScrollUp {
                KeyCode::Up
            } else {
                KeyCode::Down
            };
            for _ in 0..3 {
                crate::handle_key(app, KeyEvent::new(key, KeyModifiers::NONE), tx, config).await;
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let point = Position::new(event.column, event.row);
            let target = target_at(app, point);
            if !matches!(target, Some((_, Target::Copy))) {
                clear(app);
                if let Some((area, target @ Target::Text { .. })) = target {
                    position_editor(app, area, target, point);
                    if let Some((editor, _)) = app.current_editor() {
                        editor.start_selection();
                    }
                }
            }
            app.text_selection.borrow_mut().pressed = Some(Press {
                start: point,
                target,
                dragged: false,
            });
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            drag(app, Position::new(event.column, event.row));
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let point = Position::new(event.column, event.row);
            drag(app, point);
            let press = app.text_selection.borrow_mut().pressed.take();
            let Some(press) = press.filter(|press| !press.dragged) else {
                return;
            };
            let target = press.target;
            if target != target_at(app, point) {
                return;
            }
            let Some((_, target)) = target else {
                return;
            };
            if matches!(target, Target::Row(_) | Target::Mark(_) | Target::Fold(_))
                && app.active_tab == Tab::Filters
            {
                app.filter_focus = FilterFocus::Results;
            }
            match target {
                Target::Tab(tab) => app.switch_tab(tab),
                Target::Row(index) => {
                    if app.selected_index == index {
                        crate::handle_key(
                            app,
                            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                            tx,
                            config,
                        )
                        .await;
                    } else {
                        app.selected_index = index;
                    }
                }
                Target::Mark(index) => {
                    app.selected_index = index;
                    app.toggle_selection_at_cursor();
                }
                Target::Fold(index) => {
                    app.selected_index = index;
                    if let Some(group) = app.selected_header_group_id() {
                        app.toggle_group_collapse(&group);
                    } else if let Some(parent) = app.selected_fold_parent() {
                        app.toggle_parent_fold(&parent);
                    }
                }
                Target::Filter(index) => {
                    let run =
                        app.filter_focus == FilterFocus::Sidebar && app.filter_sidebar_idx == index;
                    app.filter_focus = FilterFocus::Sidebar;
                    app.filter_sidebar_idx = index;
                    if run {
                        crate::handle_key(
                            app,
                            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                            tx,
                            config,
                        )
                        .await;
                    }
                }
                Target::Field(field) => app.focus_field(field),
                Target::Text { .. } => {
                    if let Some((editor, _)) = app.current_editor() {
                        editor.cancel_selection();
                    }
                }
                Target::Choose { field, index } => {
                    // Like a list row: clicking the menu option that's already chosen takes it.
                    if menu_choice(app) == Some(index) {
                        crate::handle_key(
                            app,
                            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                            tx,
                            config,
                        )
                        .await;
                    } else {
                        choose(app, field, index);
                    }
                }
                Target::CloseDetail => app.close_detail(),
                Target::Copy => copy_selected(app),
                Target::Key(KeyCode::Char('S')) => {
                    app.settings = Some(crate::settings::Settings::new(config))
                }
                Target::Key(key) => {
                    crate::handle_key(app, KeyEvent::new(key, KeyModifiers::NONE), tx, config)
                        .await;
                }
            }
        }
        _ => {}
    }
}

/// The chosen option of the menu on top (bulk actions, or the detail's move and resolution
/// pickers), where Enter takes it. Forms' pickers aren't menus: Enter there submits the form.
fn menu_choice(app: &App) -> Option<usize> {
    if app.settings.is_some() || app.create_ticket.is_some() || app.assign_state.is_some() {
        return None;
    }
    match (&app.bulk_state, &app.detail_mode) {
        (
            Some(
                BulkState::ActionPicker { selected, .. }
                | BulkState::MoveStatusPicker { selected, .. }
                | BulkState::MoveResolutionPicker { selected, .. }
                | BulkState::AssignPicker { selected, .. },
            ),
            _,
        ) => Some(*selected),
        (Some(_), _) => None,
        (None, DetailMode::MovePicker(picker)) if app.is_detail_open() => Some(picker.selected),
        (None, DetailMode::ResolutionPicker { selected, .. }) if app.is_detail_open() => {
            Some(*selected)
        }
        _ => None,
    }
}

fn choose(app: &mut App, field: usize, index: usize) {
    app.focus_field(field);
    if let Some(state) = &mut app.settings {
        if field == 2 {
            state.start_tab = index.min(4);
        }
        if field == 3 {
            state.show_done = index != 0;
        }
        if field == 4 {
            state.theme = index.min(state.themes.len() - 1);
        }
    } else if let Some(state) = &mut app.create_ticket {
        match field {
            0 => state.issue_type_idx = index,
            2 => state.assignee_idx = index,
            3 => state.epic_idx = index,
            _ => {}
        }
    } else if let Some(state) = &mut app.assign_state {
        state.selected = index;
    } else if let Some(state) = &mut app.bulk_state {
        match state {
            BulkState::ActionPicker { selected, .. }
            | BulkState::MoveStatusPicker { selected, .. }
            | BulkState::MoveResolutionPicker { selected, .. }
            | BulkState::AssignPicker { selected, .. } => *selected = index,
            _ => {}
        }
    } else if let Some(BulkUploadState::Preview { selected, .. }) = &mut app.bulk_upload_state {
        *selected = index;
    } else {
        match &mut app.detail_mode {
            DetailMode::MovePicker(picker) => {
                picker.selected = index;
                picker.confirming = false;
            }
            DetailMode::ResolutionPicker { selected, .. } => *selected = index,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache::{TeamMember, Ticket},
        widgets::form,
    };
    use ratatui::{backend::TestBackend, Terminal};

    fn draw(app: &App, config: &AppConfig) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| crate::ui(f, app, config)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn locate(app: &App, text: &str) -> Position {
        let selection = app.text_selection.borrow();
        let area = selection.screen.area;
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if text.chars().enumerate().all(|(i, c)| {
                    x + (i as u16) < area.right()
                        && selection.screen[(x + i as u16, y)].symbol() == c.to_string()
                }) {
                    return Position::new(x, y);
                }
            }
        }
        panic!("{text:?} not drawn");
    }

    async fn send(
        app: &mut App,
        config: &mut AppConfig,
        tx: &UnboundedSender<BackgroundMessage>,
        kind: MouseEventKind,
        point: Position,
    ) -> Buffer {
        handle(
            app,
            MouseEvent {
                kind,
                column: point.x,
                row: point.y,
                modifiers: KeyModifiers::NONE,
            },
            tx,
            config,
        )
        .await;
        draw(app, config)
    }

    async fn drag_between(
        app: &mut App,
        config: &mut AppConfig,
        tx: &UnboundedSender<BackgroundMessage>,
        start: Position,
        end: Position,
    ) -> Buffer {
        let mut buffer = Buffer::default();
        for (kind, point) in [
            (MouseEventKind::Down(MouseButton::Left), start),
            (MouseEventKind::Drag(MouseButton::Left), end),
            (MouseEventKind::Up(MouseButton::Left), end),
        ] {
            buffer = send(app, config, tx, kind, point).await;
        }
        buffer
    }

    #[tokio::test]
    async fn dragging_selects_visible_text_without_opening_rows_or_clicking_through_modals() {
        let mut config: AppConfig =
            toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new();
        app.loading = false;
        let mut ticket = Ticket::for_test("DEMO-1", "In Progress");
        ticket.detail_loaded = true;
        ticket.description = Some("alpha 🙂 cafe\u{301}\n\nsecond line".into());
        app.cache.my_tickets = vec![ticket];
        draw(&app, &config);
        let start = locate(&app, "DEMO-1");
        let (_, Target::Row(index)) = target_at(&app, start).unwrap() else {
            panic!("ticket key must be in a row")
        };
        app.selected_index = index;
        draw(&app, &config);
        let end = Position::new(start.x + 6, start.y);
        let buffer = drag_between(&mut app, &mut config, &tx, start, end).await;
        assert_eq!(buffer[(start.x, start.y)].bg, Color::LightBlue);
        assert_eq!(selected_text(&mut app).as_deref(), Some("DEMO-1"));
        assert!(!app.is_detail_open());
        assert_eq!(app.selected_ticket_count(), 0);
        assert!(app
            .mouse_targets
            .borrow()
            .iter()
            .any(|(_, target)| *target == Target::Copy));
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        crate::handle_key(&mut app, esc, &tx, &mut config).await;
        assert_eq!(selected_text(&mut app), None);
        draw(&app, &config);
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            send(&mut app, &mut config, &tx, kind, start).await;
        }
        assert!(app.is_detail_open());

        let alpha = locate(&app, "alpha");
        // Reverse dragging includes the whole wide glyph, without its continuation cell.
        let start = Position::new(alpha.x + 8, alpha.y);
        let end = Position::new(alpha.x + 6, alpha.y);
        drag_between(&mut app, &mut config, &tx, start, end).await;
        assert_eq!(selected_text(&mut app).as_deref(), Some("🙂"));
        // Covered list checkboxes never become active while selecting popup text.
        assert_eq!(app.selected_ticket_count(), 0);
        let second = locate(&app, "second");
        let end = Position::new(second.x + 6, second.y);
        drag_between(&mut app, &mut config, &tx, alpha, end).await;
        let text = selected_text(&mut app).unwrap();
        assert!(text.starts_with("alpha 🙂 cafe\u{301}\n\n"), "{text:?}");
        assert!(text.ends_with("second"));
        assert!(!text.contains('│'));
        app.cache.my_tickets[0].description = Some("changed description".into());
        draw(&app, &config);
        assert_eq!(selected_text(&mut app), None);
        let point = locate(&app, "changed");
        let end = Position::new(point.x + 7, point.y);
        drag_between(&mut app, &mut config, &tx, point, end).await;
        Terminal::new(TestBackend::new(8, 4))
            .unwrap()
            .draw(|f| crate::ui(f, &app, &config))
            .unwrap();
        assert_eq!(selected_text(&mut app), None);
    }

    #[tokio::test]
    async fn editor_drag_selection_copies_and_replaces_unicode_and_multiline_text() {
        let mut config: AppConfig =
            toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new();
        app.comment_state = Some(crate::app::CommentState {
            ticket_key: "DEMO-1".into(),
            body: form::editor("h🙂o\nsecond"),
        });
        draw(&app, &config);
        let area = app
            .mouse_targets
            .borrow()
            .iter()
            .find(|(_, target)| matches!(target, Target::Text { .. }))
            .unwrap()
            .0;
        let start = Position::new(area.x + 1, area.y);
        let end = Position::new(area.x + 3, area.y);
        let buffer = drag_between(&mut app, &mut config, &tx, start, end).await;
        assert_eq!(buffer[(start.x, start.y)].bg, Color::LightBlue);
        assert_eq!(selected_text(&mut app).as_deref(), Some("🙂"));
        crate::handle_paste(&mut app, "X");
        assert_eq!(
            form::text(&app.comment_state.as_ref().unwrap().body),
            "hXo\nsecond"
        );
        draw(&app, &config);
        let start = Position::new(area.x + 3, area.y + 1);
        let end = Position::new(area.x + 1, area.y);
        drag_between(&mut app, &mut config, &tx, start, end).await;
        assert_eq!(selected_text(&mut app).as_deref(), Some("Xo\nsec"));
        let key = KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::NONE);
        crate::handle_key(&mut app, key, &tx, &mut config).await;
        assert_eq!(
            form::text(&app.comment_state.as_ref().unwrap().body),
            "hZond"
        );
        draw(&app, &config);
        let next = Position::new(end.x + 1, end.y);
        drag_between(&mut app, &mut config, &tx, end, next).await;
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        crate::handle_key(&mut app, esc, &tx, &mut config).await;
        assert!(app.comment_state.is_some());
        assert_eq!(selected_text(&mut app), None);
        let copy = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        crate::handle_key(&mut app, copy, &tx, &mut config).await;
        assert_eq!(
            form::text(&app.comment_state.as_ref().unwrap().body),
            "hZond"
        );
        crate::handle_key(&mut app, esc, &tx, &mut config).await;
        assert!(app.comment_state.is_none());
        app.comment_state = Some(crate::app::CommentState {
            ticket_key: "DEMO-1".into(),
            body: form::editor("\twith tab"),
        });
        draw(&app, &config);
        let start = Position::new(area.x + 4, area.y);
        let end = Position::new(area.x + 8, area.y);
        drag_between(&mut app, &mut config, &tx, start, end).await;
        assert_eq!(selected_text(&mut app).as_deref(), Some("with"));
    }

    async fn click(
        app: &mut App,
        config: &mut AppConfig,
        tx: &UnboundedSender<BackgroundMessage>,
        matches: impl Fn(Target) -> bool,
    ) {
        let rect = app
            .mouse_targets
            .borrow()
            .iter()
            .find(|(_, target)| matches(*target))
            .unwrap()
            .0;
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            handle(
                app,
                MouseEvent {
                    kind,
                    column: rect.x,
                    row: rect.y,
                    modifiers: KeyModifiers::NONE,
                },
                tx,
                config,
            )
            .await;
        }
    }

    #[tokio::test]
    async fn bulk_actions_take_clicks_on_options_hints_and_close() {
        let mut config: AppConfig =
            toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![Ticket::for_test("DEMO-1", "To Do")];
        app.selected_index = 1;
        crate::bulk_actions::open(&mut app);
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Choose { index: 1, .. })
        })
        .await;
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::ActionPicker { selected: 1, .. })
        ));
        // Clicking the chosen option again takes it, like a list row.
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Choose { index: 1, .. })
        })
        .await;
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::AssignPicker { .. })
        ));
        // Back to the first step to try the Enter hint.
        app.bulk_state = Some(BulkState::ActionPicker {
            targets: vec!["DEMO-1".into()],
            selected: 1,
        });
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Key(KeyCode::Enter))
        })
        .await;
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::AssignPicker { .. })
        ));

        // The [×] in the title closes it, like the ticket detail's.
        draw(&app, &config);
        let close = app
            .mouse_targets
            .borrow()
            .iter()
            .find(|(_, t)| matches!(t, Target::Key(KeyCode::Esc)))
            .unwrap()
            .0;
        let buffer = draw(&app, &config);
        assert_eq!(buffer[(close.x + 1, close.y)].symbol(), "[");
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Key(KeyCode::Esc))
        })
        .await;
        assert!(app.bulk_state.is_none());
    }

    #[tokio::test]
    async fn clicking_a_parents_arrow_folds_its_sub_tasks() {
        let mut config: AppConfig =
            toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Epics;
        let mut child = Ticket::for_test("DEMO-2", "To Do");
        child.parent_key = Some("DEMO-1".into());
        app.cache.epics = vec![crate::cache::Epic {
            key: "EPIC-9".into(),
            summary: "Epic".into(),
            children: vec![Ticket::for_test("DEMO-1", "To Do"), child],
        }];
        // The epic header is row 0, then the parent, then its sub-task.
        assert_eq!(app.item_count(), 3);
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| matches!(t, Target::Fold(1))).await;
        assert!(app.is_parent_folded("DEMO-1"));
        assert_eq!(app.item_count(), 2);
        assert!(!app.is_collapsed(Tab::Epics, "EPIC-9"));

        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| matches!(t, Target::Fold(1))).await;
        assert!(!app.is_parent_folded("DEMO-1"));
        assert_eq!(app.item_count(), 3);
    }

    #[tokio::test]
    async fn mouse_uses_rendered_rows_and_keeps_modal_actions_separate() {
        let mut config: AppConfig =
            toml::from_str("[jira]\nproject = 'DEMO'\nteam_name = 'Demo'\n").unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new();
        app.loading = true; // Refreshing does not hide the existing rows.
        app.cache.my_tickets = (1..30)
            .map(|i| {
                let mut ticket = Ticket::for_test(&format!("DEMO-{i}"), "In Progress");
                ticket.detail_loaded = true;
                ticket
            })
            .collect();
        app.cache.team_members.push(TeamMember {
            name: "Alex".into(),
            email: "alex@example.com".into(),
        });
        app.selected_index = 20;
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Tab(Tab::Team))
        })
        .await;
        assert_eq!(app.active_tab, Tab::Team);
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Tab(Tab::MyWork))
        })
        .await;
        assert_eq!(app.selected_index, 20);
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Mark(20))
        })
        .await;
        assert_eq!(app.selected_ticket_count(), 1);
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| matches!(t, Target::Row(20))).await;
        assert!(app.is_detail_open());
        draw(&app, &config);
        assert!(!app
            .mouse_targets
            .borrow()
            .iter()
            .any(|(_, t)| matches!(t, Target::Tab(_) | Target::Row(_))));
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Key(KeyCode::Esc))
        })
        .await;
        assert!(!app.is_detail_open());

        app.comment_state = Some(crate::app::CommentState {
            ticket_key: "DEMO-1".into(),
            body: form::editor("h🙂o"),
        });
        draw(&app, &config);
        let area = app
            .mouse_targets
            .borrow()
            .iter()
            .find(|(_, t)| matches!(t, Target::Text { .. }))
            .unwrap()
            .0;
        handle(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x + 3,
                row: area.y,
                modifiers: KeyModifiers::NONE,
            },
            &tx,
            &mut config,
        )
        .await;
        crate::handle_comment_keys(&mut app, KeyCode::Char('i'), KeyModifiers::NONE, &tx);
        crate::handle_paste(&mut app, "\nthere");
        assert_eq!(
            form::text(&app.comment_state.as_ref().unwrap().body),
            "h🙂i\nthereo"
        );
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Key(KeyCode::F(4)))
        })
        .await;
        assert!(app.external_editor_requested);
        app.comment_state = None;
        app.external_editor_requested = false;

        app.bulk_state = Some(BulkState::ActionPicker {
            targets: vec!["DEMO-1".into()],
            selected: 0,
        });
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Choose { index: 1, .. })
        })
        .await;
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::ActionPicker { selected: 1, .. })
        ));
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Key(KeyCode::Enter))
        })
        .await;
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::AssignPicker { .. })
        ));
        draw(&app, &config);
        // The assignee is already chosen, so clicking it takes it.
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Choose { index: 0, .. })
        })
        .await;
        assert!(matches!(app.bulk_state, Some(BulkState::Confirm { .. })));
        assert!(rx.try_recv().is_err()); // No Jira write before confirmation.

        app.bulk_state = None;
        app.switch_tab(Tab::Filters);
        app.cache = crate::cache::Cache::empty();
        app.filter_results = vec![Ticket::for_test("DEMO-1", "In Progress")];
        app.mark_cache_changed();
        app.filter_focus = FilterFocus::Sidebar;
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| matches!(t, Target::Mark(1))).await;
        assert_eq!(app.filter_focus, FilterFocus::Results);
        assert!(app.is_ticket_selected("DEMO-1"));

        app.switch_tab(Tab::MyWork);
        app.search = Some("search stays here".into());
        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Key(KeyCode::Char('S')))
        })
        .await;
        assert!(app.settings.is_some());
        assert_eq!(app.search.as_deref(), Some("search stays here"));

        draw(&app, &config);
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Choose { field: 4, index: 2 })
        })
        .await;
        assert_eq!(app.focused_field(), Some(4));
        assert_eq!(app.settings.as_ref().map(|state| state.theme), Some(2));
    }
}
