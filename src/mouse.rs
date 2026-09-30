use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use tokio::sync::mpsc::UnboundedSender;
use tui_textarea::CursorMove;

use crate::{
    app::{App, BulkUploadState, DetailMode, FilterFocus, Tab},
    bulk_actions::BulkState,
    config::AppConfig,
    BackgroundMessage,
};

#[derive(Clone, Copy, Debug)]
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
    Key(KeyCode),
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
            let prefix = lines[line]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .find('[')
                .unwrap_or(0) as u16;
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
            let target = app
                .mouse_targets
                .borrow()
                .iter()
                .rev()
                .find(|(area, _)| area.contains(Position::new(event.column, event.row)))
                .copied();
            let Some((area, target)) = target else {
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
                Target::Text { field, top, left } => {
                    app.focus_field(field);
                    if let Some((editor, _)) = app.current_editor() {
                        let row =
                            (top + (event.row - area.y) as usize).min(editor.lines().len() - 1);
                        let width = left + (event.column - area.x) as usize;
                        let mut used = 0;
                        let column = editor.lines()[row]
                            .chars()
                            .take_while(|c| {
                                used += unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                                used <= width
                            })
                            .count();
                        editor.move_cursor(CursorMove::Jump(
                            row.min(u16::MAX as usize) as u16,
                            column.min(u16::MAX as usize) as u16,
                        ));
                    }
                }
                Target::Choose { field, index } => choose(app, field, index),
                Target::CloseDetail => app.close_detail(),
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

fn choose(app: &mut App, field: usize, index: usize) {
    app.focus_field(field);
    if let Some(state) = &mut app.settings {
        if field == 2 {
            state.start_tab = index.min(4);
        }
        if field == 3 {
            state.show_done = index != 0;
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

    fn draw(app: &App, config: &AppConfig) {
        Terminal::new(TestBackend::new(80, 24))
            .unwrap()
            .draw(|f| crate::ui(f, app, config))
            .unwrap();
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
        handle(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            },
            tx,
            config,
        )
        .await;
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
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Choose { index: 0, .. })
        })
        .await;
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::AssignPicker { .. })
        ));
        click(&mut app, &mut config, &tx, |t| {
            matches!(t, Target::Key(KeyCode::Enter))
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
    }
}
