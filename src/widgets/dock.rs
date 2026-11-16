//! The one panel that forms, pickers and notices share. It docks at the bottom of the content
//! area, under what is already there, which shrinks to make room. `open` says which panel is
//! showing, in the order the keys reach them (`handle_key`), `height` how much of the area it
//! takes, and `render` draws it.

use ratatui::layout::Rect;

use super::{
    assign, bulk_actions, bulk_upload, comment, create_ticket, edit_fields, filter_edit,
    move_failure, move_status,
};
use crate::app::App;

/// The rows a panel's rule and title take.
pub const CHROME: u16 = 2;
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

/// The rows `panel` takes of `content`: what it needs for its title and body, but no more than
/// `MAX_PERCENT` of the area.
pub fn height(app: &App, panel: Panel, content: Rect) -> u16 {
    let rows = match panel {
        Panel::MoveFailure => move_failure::rows(app),
        Panel::FilterEdit => filter_edit::rows(app),
        Panel::BulkUpload => bulk_upload::rows(app),
        Panel::Create => create_ticket::rows(app),
        Panel::Comment => comment::rows(app),
        Panel::Assign => assign::rows(app),
        Panel::Edit => edit_fields::rows(app),
        Panel::Bulk => bulk_actions::rows(app),
        Panel::Move => move_status::rows(app),
    };
    let most = u32::from(content.height) * u32::from(MAX_PERCENT) / 100;
    (CHROME + rows).min(most as u16)
}

pub fn render(f: &mut ratatui::Frame, app: &App, panel: Panel, area: Rect) {
    match panel {
        Panel::MoveFailure => move_failure::render(f, app, area),
        Panel::FilterEdit => filter_edit::render(f, app, area),
        Panel::BulkUpload => bulk_upload::render(f, app, area),
        Panel::Create => create_ticket::render(f, app, area),
        Panel::Comment => comment::render(f, app, area),
        Panel::Assign => assign::render(f, app, area),
        Panel::Edit => edit_fields::render(f, app, area),
        Panel::Bulk => bulk_actions::render(f, app, area),
        Panel::Move => move_status::render(f, app, area),
    }
}
