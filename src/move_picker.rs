//! The single-ticket move picker: the transitions Jira offers for the ticket in the detail view,
//! the p/w/n/t/v/b/c shortcuts, and the resolution prompt.
//!
//! These functions only change app state and return the Jira call to make. The caller starts
//! it, so the key handling can be tested without reaching Jira.

use crossterm::event::KeyCode;

use crate::app::{App, DetailMode};
use crate::cache::Status;
use crate::transitions::{resolution_choices, Resolution, Transition};

/// Jira work the picker asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum JiraCall {
    /// List `key`'s transitions, then hand the answer to [`receive`] with the same `request`.
    FetchTransitions { key: String, request: u64 },
    /// Send transition `id`. The move is already registered with `MoveTracker::start`.
    Transition {
        key: String,
        id: String,
        resolution_id: Option<String>,
    },
}

/// A ticket's transitions, and the one the user is choosing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePicker {
    pub transitions: Vec<Transition>,
    /// Set when a shortcut matched several transitions, to list only those.
    pub only_to: Option<Status>,
    pub selected: usize,
    /// Whether the selected row waits for Enter or y.
    pub confirming: bool,
}

impl MovePicker {
    fn new(transitions: Vec<Transition>) -> Self {
        Self {
            transitions,
            only_to: None,
            selected: 0,
            confirming: false,
        }
    }

    /// The transitions listed, in Jira's order.
    pub fn rows(&self) -> Vec<&Transition> {
        self.transitions
            .iter()
            .filter(|t| {
                self.only_to
                    .as_ref()
                    .is_none_or(|status| t.leads_to(status))
            })
            .collect()
    }

    pub fn selected_transition(&self) -> Option<&Transition> {
        self.rows().get(self.selected).copied()
    }

    /// The resolutions the selected transition accepts. `None` is "No resolution".
    pub fn resolution_choices(&self) -> Vec<Option<Resolution>> {
        self.selected_transition()
            .map_or_else(Vec::new, |t| resolution_choices(&t.resolution))
    }
}

/// Opens the picker for the ticket in the detail view: shows a loading state while Jira lists
/// the ticket's transitions.
pub fn open(app: &mut App) -> Option<JiraCall> {
    let key = app
        .detail_ticket_key
        .clone()
        .filter(|key| app.find_ticket(key).is_some())?;
    let request = app.next_request_id();
    app.detail_mode = DetailMode::MoveLoading { request };
    Some(JiraCall::FetchTransitions { key, request })
}

/// Shows Jira's transitions for `key`, unless the user has since closed the picker, opened it
/// again, or moved on to another ticket.
pub fn receive(app: &mut App, key: &str, request: u64, result: Result<Vec<Transition>, String>) {
    let waiting = app.detail_ticket_key.as_deref() == Some(key)
        && matches!(app.detail_mode, DetailMode::MoveLoading { request: r } if r == request);
    if !waiting {
        return;
    }
    match result {
        Ok(transitions) => app.detail_mode = DetailMode::MovePicker(MovePicker::new(transitions)),
        Err(error) => {
            app.detail_mode = DetailMode::View;
            // The status bar is one line, and Jira's errors can have several.
            let error = error.lines().collect::<Vec<_>>().join(" ");
            app.flash = Some(format!("Couldn't load {}'s transitions: {}", key, error));
        }
    }
}

/// Handles a key while the picker, its loading state or the resolution prompt is showing.
pub fn handle_key(app: &mut App, key: KeyCode) -> Option<JiraCall> {
    match app.detail_mode.clone() {
        DetailMode::MoveLoading { .. } => {
            if key == KeyCode::Esc {
                app.detail_mode = DetailMode::View;
            }
            None
        }
        DetailMode::MovePicker(picker) => picker_key(app, picker, key),
        DetailMode::ResolutionPicker { picker, selected } => {
            resolution_key(app, picker, selected, key)
        }
        DetailMode::View | DetailMode::History { .. } => None,
    }
}

fn picker_key(app: &mut App, mut picker: MovePicker, key: KeyCode) -> Option<JiraCall> {
    match key {
        KeyCode::Esc => {
            app.detail_mode = DetailMode::View;
            return None;
        }
        KeyCode::Enter | KeyCode::Char('y' | 'Y') if picker.confirming => {
            return choose(app, picker);
        }
        KeyCode::Enter => picker.confirming = picker.selected_transition().is_some(),
        KeyCode::Char('j') | KeyCode::Down => {
            picker.selected = (picker.selected + 1).min(picker.rows().len().saturating_sub(1));
            picker.confirming = false;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            picker.selected = picker.selected.saturating_sub(1);
            picker.confirming = false;
        }
        KeyCode::Char(c) => {
            if let Some(status) = Status::from_move_shortcut(c) {
                return shortcut(app, picker, status, c.is_ascii_uppercase());
            }
        }
        _ => {}
    }
    app.detail_mode = DetailMode::MovePicker(picker);
    None
}

/// A status shortcut matches every transition whose destination maps onto `status`. One match
/// is selected (and sent right away when `now`); several are listed so the user picks one.
fn shortcut(app: &mut App, mut picker: MovePicker, status: Status, now: bool) -> Option<JiraCall> {
    let matching: Vec<usize> = (0..picker.transitions.len())
        .filter(|&i| picker.transitions[i].leads_to(&status))
        .collect();
    match matching.as_slice() {
        [] => {
            app.flash = Some(format!(
                "No transition to {} from {}",
                status.as_str(),
                detail_status_name(app)
            ));
        }
        [only] => {
            picker.only_to = None;
            picker.selected = *only;
            if now {
                return choose(app, picker);
            }
            picker.confirming = true;
        }
        _ => {
            app.flash = Some(format!(
                "Several transitions lead to {}. Pick one.",
                status.as_str()
            ));
            picker.only_to = Some(status);
            picker.selected = 0;
            picker.confirming = false;
        }
    }
    app.detail_mode = DetailMode::MovePicker(picker);
    None
}

/// The user chose the selected transition. Ask for a resolution when its resolution field has
/// values to choose from, else send it.
fn choose(app: &mut App, picker: MovePicker) -> Option<JiraCall> {
    if picker.resolution_choices().iter().any(Option::is_some) {
        app.detail_mode = DetailMode::ResolutionPicker {
            picker,
            selected: 0,
        };
        return None;
    }
    send(app, picker, None)
}

fn resolution_key(
    app: &mut App,
    picker: MovePicker,
    selected: usize,
    key: KeyCode,
) -> Option<JiraCall> {
    let choices = picker.resolution_choices();
    let selected = match key {
        KeyCode::Esc => {
            app.detail_mode = DetailMode::MovePicker(MovePicker {
                confirming: false,
                ..picker
            });
            return None;
        }
        KeyCode::Enter => return send(app, picker, choices.get(selected)?.as_ref()),
        KeyCode::Char('j') | KeyCode::Down => (selected + 1).min(choices.len().saturating_sub(1)),
        KeyCode::Char('k') | KeyCode::Up => selected.saturating_sub(1),
        _ => selected,
    };
    app.detail_mode = DetailMode::ResolutionPicker { picker, selected };
    None
}

/// Registers a move through the selected transition and returns the call that sends it. Refuses
/// when the transition needs a resolution it can't get, or when the ticket already has a move
/// running.
fn send(app: &mut App, picker: MovePicker, choice: Option<&Resolution>) -> Option<JiraCall> {
    let transition = picker.selected_transition()?.clone();
    let resolution_id = match transition.resolution_to_send(choice) {
        Ok(resolution_id) => resolution_id,
        Err(reason) => {
            app.flash = Some(format!(
                "Can't move: {}. Move it in Jira instead (Esc, then o).",
                reason
            ));
            app.detail_mode = DetailMode::MovePicker(MovePicker {
                confirming: false,
                ..picker
            });
            return None;
        }
    };
    let key = app.detail_ticket_key.clone()?;
    app.detail_mode = DetailMode::View;
    if !app.moves.start(&key, &transition.to_name) {
        app.flash = Some(format!(
            "{} is already being moved. Wait for Jira to answer.",
            key
        ));
        return None;
    }
    Some(JiraCall::Transition {
        key,
        id: transition.id.clone(),
        resolution_id,
    })
}

fn detail_status_name(app: &App) -> String {
    app.detail_ticket_key
        .as_deref()
        .and_then(|key| app.find_ticket(key))
        .map(|ticket| ticket.status_name().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::Ticket;
    use crate::transitions::parse_transitions;
    use crate::transitions::tests::{resolution, transition, with_resolution, BACKLOG_EPIC};

    const KEY: &str = "DEMO-2478";

    fn app_with_detail(status: &str) -> App {
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![Ticket::for_test(KEY, status)];
        app.open_detail(KEY.to_string());
        app
    }

    /// The app with the picker open on `transitions`, as if Jira had just answered.
    fn picker_app(status: &str, transitions: Vec<Transition>) -> App {
        let mut app = app_with_detail(status);
        let Some(JiraCall::FetchTransitions { key, request }) = open(&mut app) else {
            panic!("m should ask Jira for transitions");
        };
        assert!(matches!(app.detail_mode, DetailMode::MoveLoading { .. }));
        receive(&mut app, &key, request, Ok(transitions));
        app
    }

    fn picker(app: &App) -> &MovePicker {
        match &app.detail_mode {
            DetailMode::MovePicker(picker) => picker,
            other => panic!("expected the move picker, got {:?}", other),
        }
    }

    fn row_names(app: &App) -> Vec<&str> {
        picker(app).rows().iter().map(|t| t.name.as_str()).collect()
    }

    #[test]
    fn backlog_epic_c_matches_nothing_and_p_finds_resume_progress() {
        let mut app = picker_app("Backlog", parse_transitions(BACKLOG_EPIC).unwrap());
        assert_eq!(
            row_names(&app),
            ["Cancelled", "Resume Progress", "Ready", "Waiting for Input"]
        );

        assert_eq!(handle_key(&mut app, KeyCode::Char('c')), None);
        assert_eq!(
            app.flash.as_deref(),
            Some("No transition to Closed from Backlog")
        );
        assert!(!picker(&app).confirming);

        assert_eq!(handle_key(&mut app, KeyCode::Char('p')), None);
        let chosen = picker(&app).selected_transition().unwrap();
        assert_eq!(
            (chosen.id.as_str(), chosen.name.as_str()),
            ("824", "Resume Progress")
        );
        assert!(picker(&app).confirming);

        // Enter confirms and sends transition 824 by id, with no resolution.
        assert_eq!(
            handle_key(&mut app, KeyCode::Enter),
            Some(JiraCall::Transition {
                key: KEY.to_string(),
                id: "824".to_string(),
                resolution_id: None,
            })
        );
        assert!(matches!(app.detail_mode, DetailMode::View));
        assert_eq!(
            app.moves.pending_message().as_deref(),
            Some("Moving DEMO-2478 to In Progress…")
        );
    }

    #[test]
    fn uppercase_shortcut_with_one_match_sends_right_away() {
        let mut app = picker_app("Backlog", parse_transitions(BACKLOG_EPIC).unwrap());
        assert_eq!(
            handle_key(&mut app, KeyCode::Char('T')),
            Some(JiraCall::Transition {
                key: KEY.to_string(),
                id: "87".to_string(),
                resolution_id: None,
            })
        );
    }

    #[test]
    fn shortcut_with_several_matches_lists_only_those_and_sends_nothing() {
        let story = vec![
            transition("823", "In Progress", "In Progress"),
            transition("870", "Closed", "Closed"),
            transition("871", "Resolved", "Resolved"),
        ];
        let mut app = picker_app("In Progress", story);

        assert_eq!(handle_key(&mut app, KeyCode::Char('C')), None);
        assert_eq!(row_names(&app), ["Closed", "Resolved"]);
        assert!(app.moves.pending_message().is_none());

        handle_key(&mut app, KeyCode::Down);
        handle_key(&mut app, KeyCode::Enter);
        assert_eq!(
            handle_key(&mut app, KeyCode::Char('y')),
            Some(JiraCall::Transition {
                key: KEY.to_string(),
                id: "871".to_string(),
                resolution_id: None,
            })
        );
    }

    #[test]
    fn identical_duplicates_count_as_one_match_with_the_lowest_id() {
        let body = r#"{"transitions": [
          {"id": "836", "name": "Ready for Work", "to": {"name": "Ready for Work"}, "fields": {}},
          {"id": "87", "name": "Ready for Work", "to": {"name": "Ready for Work"}, "fields": {}}
        ]}"#;
        let mut app = picker_app("Backlog", parse_transitions(body).unwrap());
        assert_eq!(
            handle_key(&mut app, KeyCode::Char('W')),
            Some(JiraCall::Transition {
                key: KEY.to_string(),
                id: "87".to_string(),
                resolution_id: None,
            })
        );
    }

    #[test]
    fn transitions_for_an_old_request_or_another_ticket_are_dropped() {
        let mut app = app_with_detail("Backlog");
        let Some(JiraCall::FetchTransitions { request, .. }) = open(&mut app) else {
            panic!("m should ask Jira for transitions");
        };
        let transitions = || Ok(parse_transitions(BACKLOG_EPIC).unwrap());

        // The user closed the picker and opened it again: the first answer is stale.
        handle_key(&mut app, KeyCode::Esc);
        let Some(JiraCall::FetchTransitions { request: again, .. }) = open(&mut app) else {
            panic!("m should ask Jira for transitions");
        };
        receive(&mut app, KEY, request, transitions());
        assert_eq!(app.detail_mode, DetailMode::MoveLoading { request: again });

        // The user moved on to another ticket.
        app.open_detail("DEMO-1".to_string());
        receive(&mut app, KEY, again, transitions());
        assert_eq!(app.detail_mode, DetailMode::View);
    }

    #[test]
    fn failed_fetch_returns_to_the_detail_view_with_the_error() {
        let mut app = app_with_detail("Backlog");
        let Some(JiraCall::FetchTransitions { request, .. }) = open(&mut app) else {
            panic!("m should ask Jira for transitions");
        };
        receive(
            &mut app,
            KEY,
            request,
            Err("Jira answered 404 Not Found.\nIssue does not exist.".into()),
        );
        assert_eq!(app.detail_mode, DetailMode::View);
        assert_eq!(
            app.flash.as_deref(),
            Some("Couldn't load DEMO-2478's transitions: Jira answered 404 Not Found. Issue does not exist.")
        );
    }

    #[test]
    fn optional_resolution_offers_no_resolution_first() {
        let done = with_resolution(
            transition("805", "Done", "Done"),
            false,
            &[("101", "Fixed"), ("102", "Won't Fix")],
        );
        let mut app = picker_app("In Progress", vec![done]);

        assert_eq!(handle_key(&mut app, KeyCode::Char('C')), None);
        let DetailMode::ResolutionPicker { picker, .. } = &app.detail_mode else {
            panic!("expected the resolution picker");
        };
        assert_eq!(
            picker.resolution_choices(),
            [
                None,
                Some(resolution("101", "Fixed")),
                Some(resolution("102", "Won't Fix"))
            ]
        );

        assert_eq!(
            handle_key(&mut app, KeyCode::Enter),
            Some(JiraCall::Transition {
                key: KEY.to_string(),
                id: "805".to_string(),
                resolution_id: None,
            })
        );
    }

    #[test]
    fn required_resolution_sends_the_chosen_value() {
        let done = with_resolution(
            transition("805", "Done", "Done"),
            true,
            &[("101", "Fixed"), ("102", "Won't Fix")],
        );
        let mut app = picker_app("In Progress", vec![done]);
        handle_key(&mut app, KeyCode::Char('C'));
        handle_key(&mut app, KeyCode::Down);
        assert_eq!(
            handle_key(&mut app, KeyCode::Enter),
            Some(JiraCall::Transition {
                key: KEY.to_string(),
                id: "805".to_string(),
                resolution_id: Some("102".to_string()),
            })
        );
    }

    #[test]
    fn required_resolution_without_values_explains_instead_of_trapping() {
        let done = with_resolution(transition("805", "Done", "Done"), true, &[]);
        let mut app = picker_app("In Progress", vec![done]);

        assert_eq!(handle_key(&mut app, KeyCode::Char('C')), None);
        assert!(matches!(app.detail_mode, DetailMode::MovePicker(_)));
        assert_eq!(
            app.flash.as_deref(),
            Some(
                "Can't move: \"Done\" requires a resolution, but Jira offers none to choose \
                 from. Move it in Jira instead (Esc, then o)."
            )
        );
        assert!(app.moves.pending_message().is_none());
    }

    #[test]
    fn second_move_is_blocked_while_first_is_running() {
        let mut app = picker_app("Backlog", parse_transitions(BACKLOG_EPIC).unwrap());
        assert!(app.moves.start(KEY, "Cancelled"));

        assert_eq!(handle_key(&mut app, KeyCode::Char('P')), None);
        assert!(matches!(app.detail_mode, DetailMode::View));
        assert_eq!(
            app.flash.as_deref(),
            Some("DEMO-2478 is already being moved. Wait for Jira to answer.")
        );
        assert_eq!(
            app.moves.pending_message().as_deref(),
            Some("Moving DEMO-2478 to Cancelled…")
        );
    }
}
