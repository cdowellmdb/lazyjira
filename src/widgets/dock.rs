//! The one panel that forms, pickers and notices share. It docks at the bottom of the content
//! area, under what is already there, which shrinks to make room. `open` says which panel is
//! showing, in the order the keys reach them (`handle_key`), `height` how much of the area it
//! takes, and `render` draws it.

use ratatui::layout::Rect;

use super::{
    assign, bulk_actions, bulk_upload, comment, create_ticket, edit_fields, filter_edit, form,
    move_failure, move_status,
};
use crate::app::App;

/// The rows a panel's rule and title take, and the last row, for its buttons or hints.
const CHROME: u16 = form::CHROME;
const FOOTER: u16 = 1;
/// The most of the content area a panel takes, in percent.
const MAX_PERCENT: u16 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    MoveFailure,
    FilterEdit,
    BulkUpload,
    Create,
    Comment,
    Assign,
    Edit,
    Bulk,
    Move,
}

/// The panel that has the keys, if any is open.
pub fn open(app: &App) -> Option<Panel> {
    [
        (!app.moves.failures().is_empty(), Panel::MoveFailure),
        (app.is_filter_edit_open(), Panel::FilterEdit),
        (app.is_bulk_upload_open(), Panel::BulkUpload),
        (app.is_create_ticket_open(), Panel::Create),
        (app.is_comment_open(), Panel::Comment),
        (app.is_assign_open(), Panel::Assign),
        (app.is_edit_open(), Panel::Edit),
        (app.is_bulk_open(), Panel::Bulk),
        (move_status::is_open(app), Panel::Move),
    ]
    .into_iter()
    .find_map(|(open, panel)| open.then_some(panel))
}

/// A panel ready to draw: which it is, how tall, and the body of a menu, built once.
pub struct Docked {
    panel: Panel,
    menu: Option<form::Menu>,
    pub height: u16,
}

/// The panel that has the keys, sized for `content`: the rows its rule, title, body and last row
/// take, but no more than `MAX_PERCENT` of the area, and no fewer than every row of its body
/// needs to show (`form::Row`), whatever the cap says.
pub fn plan(app: &App, content: Rect) -> Option<Docked> {
    let panel = open(app)?;
    let menu = match panel {
        Panel::Bulk => bulk_actions::menu(app),
        Panel::Move => move_status::menu(app),
        _ => None,
    };
    let need = match (&menu, panel) {
        (Some(menu), _) => form::need(&menu.shape()),
        (None, Panel::MoveFailure) => move_failure::need(app, content.width),
        (None, Panel::FilterEdit) => filter_edit::need(app),
        (None, Panel::BulkUpload) => bulk_upload::need(app),
        (None, Panel::Create) => create_ticket::need(app),
        (None, Panel::Comment) => comment::need(app),
        (None, Panel::Assign) => assign::need(app),
        (None, Panel::Edit) => edit_fields::need(app),
        (None, Panel::Bulk | Panel::Move) => form::need(&[]),
    };
    let frame = CHROME + FOOTER;
    let cap = (u32::from(content.height) * u32::from(MAX_PERCENT) / 100) as u16;
    let height = (frame + need.want)
        .min(cap)
        .max(frame + need.min)
        .min(content.height);
    Some(Docked {
        panel,
        menu,
        height,
    })
}

pub fn render(f: &mut ratatui::Frame, app: &App, docked: Docked, area: Rect) {
    if let Some(menu) = docked.menu {
        return form::render_menu(f, app, area, menu);
    }
    match docked.panel {
        Panel::MoveFailure => move_failure::render(f, app, area),
        Panel::FilterEdit => filter_edit::render(f, app, area),
        Panel::BulkUpload => bulk_upload::render(f, app, area),
        Panel::Create => create_ticket::render(f, app, area),
        Panel::Comment => comment::render(f, app, area),
        Panel::Assign => assign::render(f, app, area),
        Panel::Edit => edit_fields::render(f, app, area),
        Panel::Bulk | Panel::Move => {}
    }
}
