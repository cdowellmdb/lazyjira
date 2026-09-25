//! Ticket moves: which are waiting on Jira, which Jira confirmed, and which it rejected.
//!
//! Every Jira read (detail fetch, refresh, filter query) is stamped with
//! [`MoveTracker::now`] when it is requested. A read stamped before a ticket's latest
//! confirmed move is stale for that ticket, so it must not overwrite the moved status.

use std::collections::{BTreeMap, HashMap};

use crate::app::App;

/// A move Jira rejected. It stays on screen until the user dismisses it.
#[derive(Debug, PartialEq, Eq)]
pub struct MoveFailure {
    pub key: String,
    /// Name of the status the move was meant to reach.
    pub target: String,
    pub error: String,
}

#[derive(Debug, Default)]
pub struct MoveTracker {
    /// Logical clock, advanced each time Jira confirms a move.
    clock: u64,
    /// Target status name of each move still waiting on Jira.
    in_flight: BTreeMap<String, String>,
    /// Clock value and status name of each ticket's latest confirmed move.
    confirmed: HashMap<String, (u64, String)>,
    /// Rejected moves, oldest first.
    failures: Vec<MoveFailure>,
}

impl MoveTracker {
    /// Stamp for a Jira read requested now.
    pub fn now(&self) -> u64 {
        self.clock
    }

    /// Marks `key` as moving to `target`. Returns false if `key` already has a move running.
    pub fn start(&mut self, key: &str, target: &str) -> bool {
        if self.in_flight.contains_key(key) {
            return false;
        }
        self.in_flight.insert(key.to_string(), target.to_string());
        true
    }

    /// Records that Jira now has `key` in `status`. Reads requested earlier are stale for `key`.
    fn confirm(&mut self, key: &str, status: &str) {
        self.clock += 1;
        self.confirmed
            .insert(key.to_string(), (self.clock, status.to_string()));
    }

    /// Whether a read of `key` requested at `requested_at` predates its latest confirmed move.
    pub fn is_stale(&self, key: &str, requested_at: u64) -> bool {
        self.confirmed
            .get(key)
            .is_some_and(|(confirmed_at, _)| *confirmed_at > requested_at)
    }

    /// Status-bar text for the moves still waiting on Jira.
    pub fn pending_message(&self) -> Option<String> {
        if self.in_flight.is_empty() {
            return None;
        }
        let moves: Vec<String> = self
            .in_flight
            .iter()
            .map(|(key, target)| format!("{} to {}", key, target))
            .collect();
        Some(format!("Moving {}…", moves.join(", ")))
    }

    pub fn failures(&self) -> &[MoveFailure] {
        &self.failures
    }

    pub fn dismiss_failure(&mut self) {
        if !self.failures.is_empty() {
            self.failures.remove(0);
        }
    }
}

impl App {
    /// Applies Jira's answer to the move started on `key`. On success the ticket shows its new
    /// status everywhere. On failure its status is left alone and the error is kept for display.
    pub fn finish_move(&mut self, key: &str, result: Result<(), String>) {
        let Some(target) = self.moves.in_flight.remove(key) else {
            return;
        };
        match result {
            Ok(()) => {
                self.flash = Some(format!("Moved {} to {}", key, target));
                self.record_move(key, &target);
                self.clamp_selection();
            }
            Err(error) => self.moves.failures.push(MoveFailure {
                key: key.to_string(),
                target,
                error,
            }),
        }
    }

    /// Jira confirmed `key` is now in the status named `status`: show it everywhere and ignore
    /// older reads of it.
    pub fn record_move(&mut self, key: &str, status: &str) {
        self.moves.confirm(key, status);
        self.update_ticket_status(key, status);
    }

    /// Re-applies moves Jira confirmed after a list read was requested, so the read can't undo them.
    pub fn reapply_moves_since(&mut self, requested_at: u64) {
        let newer: Vec<(String, String)> = self
            .moves
            .confirmed
            .iter()
            .filter(|(_, (confirmed_at, _))| *confirmed_at > requested_at)
            .map(|(key, (_, status))| (key.clone(), status.clone()))
            .collect();
        for (key, status) in newer {
            self.update_ticket_status(&key, &status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{Cache, Epic, Status, Ticket};

    const KEY: &str = "DSCI-2478";

    /// A ticket in the status Jira calls `status`.
    fn ticket(key: &str, status: &str) -> Ticket {
        let mut ticket = Ticket {
            key: key.to_string(),
            summary: key.to_string(),
            status: Status::ToDo,
            jira_status: None,
            assignee: None,
            assignee_email: None,
            reporter: None,
            description: None,
            labels: Vec::new(),
            epic_key: None,
            epic_name: None,
            detail_loaded: false,
            url: format!("https://jira.example.com/browse/{}", key),
            activity: Vec::new(),
        };
        ticket.set_status(status);
        ticket
    }

    /// An app where `KEY` is in Backlog in My Work, Team, an epic, and filter results.
    fn app_with_ticket_everywhere() -> App {
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![ticket(KEY, "Backlog")];
        app.cache.team_tickets = vec![ticket(KEY, "Backlog")];
        app.cache.epics = vec![Epic {
            key: "DSCI-1".to_string(),
            summary: "Epic".to_string(),
            children: vec![ticket(KEY, "Backlog")],
        }];
        app.filter_results = vec![ticket(KEY, "Backlog")];
        app.mark_cache_changed();
        app
    }

    fn statuses_everywhere(app: &App) -> Vec<&str> {
        app.cache
            .my_tickets
            .iter()
            .chain(&app.cache.team_tickets)
            .chain(app.cache.epics.iter().flat_map(|e| &e.children))
            .chain(&app.filter_results)
            .filter(|t| t.key == KEY)
            .map(|t| t.status_name())
            .collect()
    }

    #[test]
    fn failed_move_leaves_status_unchanged_everywhere_and_keeps_error() {
        let mut app = app_with_ticket_everywhere();
        let error = "Jira answered 400 Bad Request.\nresolution: Resolution is required.";

        assert!(app.moves.start(KEY, "Done"));
        app.finish_move(KEY, Err(error.to_string()));

        assert_eq!(statuses_everywhere(&app), ["Backlog"; 4]);
        assert_eq!(
            app.moves.failures(),
            [MoveFailure {
                key: KEY.to_string(),
                target: "Done".to_string(),
                error: error.to_string(),
            }]
        );
        assert_eq!(app.moves.pending_message(), None);
    }

    #[test]
    fn successful_move_sets_the_real_status_name_and_the_status() {
        let mut app = app_with_ticket_everywhere();
        assert!(app.moves.start(KEY, "Resolved"));
        app.finish_move(KEY, Ok(()));

        assert_eq!(statuses_everywhere(&app), ["Resolved"; 4]);
        assert_eq!(app.cache.my_tickets[0].status, Status::Closed);
        assert_eq!(app.flash.as_deref(), Some("Moved DSCI-2478 to Resolved"));
    }

    #[test]
    fn successful_move_ignores_detail_requested_before_it_finished() {
        let mut app = app_with_ticket_everywhere();
        let requested_before_move = app.moves.now();
        assert!(app.moves.start(KEY, "In Progress"));
        let requested_during_move = app.moves.now();

        app.finish_move(KEY, Ok(()));
        assert_eq!(statuses_everywhere(&app), ["In Progress"; 4]);

        for requested_at in [requested_before_move, requested_during_move] {
            assert!(!app.enrich_ticket(KEY, requested_at, &ticket(KEY, "Backlog")));
            assert_eq!(statuses_everywhere(&app), ["In Progress"; 4]);
        }

        // A read requested after Jira confirmed the move is trusted.
        let requested_after_move = app.moves.now();
        assert!(app.enrich_ticket(KEY, requested_after_move, &ticket(KEY, "In Review")));
        assert_eq!(statuses_everywhere(&app), ["In Review"; 4]);
    }

    #[test]
    fn refresh_requested_before_move_keeps_moved_status() {
        let mut app = App::new();
        let requested_at = app.moves.now();
        assert!(app.moves.start(KEY, "In Progress"));
        app.finish_move(KEY, Ok(()));

        let stale = Cache {
            my_tickets: vec![ticket(KEY, "Backlog")],
            team_tickets: vec![ticket(KEY, "Backlog")],
            epics: Vec::new(),
            team_members: Vec::new(),
        };
        app.replace_cache(stale.clone(), requested_at);
        assert_eq!(statuses_everywhere(&app), ["In Progress"; 2]);

        // A refresh requested after the move is trusted.
        app.replace_cache(stale, app.moves.now());
        assert_eq!(statuses_everywhere(&app), ["Backlog"; 2]);
    }

    #[test]
    fn filter_only_ticket_gets_moved_and_enriched() {
        let mut app = App::new();
        app.filter_results = vec![ticket(KEY, "Backlog")];

        assert!(app.moves.start(KEY, "In Progress"));
        app.finish_move(KEY, Ok(()));
        assert_eq!(app.filter_results[0].status_name(), "In Progress");

        let mut detail = ticket(KEY, "In Review");
        detail.description = Some("From Jira".to_string());
        assert!(app.enrich_ticket(KEY, app.moves.now(), &detail));
        assert_eq!(app.filter_results[0].status_name(), "In Review");
        assert_eq!(
            app.filter_results[0].description.as_deref(),
            Some("From Jira")
        );
        assert!(app.filter_results[0].detail_loaded);
    }

    #[test]
    fn second_move_on_same_ticket_is_rejected_until_first_finishes() {
        let mut app = App::new();
        assert!(app.moves.start(KEY, "Closed"));
        assert!(!app.moves.start(KEY, "In Progress"));
        assert!(app.moves.start("DSCI-1", "In Progress"));
        assert_eq!(
            app.moves.pending_message().as_deref(),
            Some("Moving DSCI-1 to In Progress, DSCI-2478 to Closed…")
        );

        app.finish_move(KEY, Err("boom".to_string()));
        assert!(app.moves.start(KEY, "In Progress"));
    }

    #[test]
    fn failures_are_dismissed_oldest_first() {
        let mut app = App::new();
        for key in ["DSCI-1", "DSCI-2"] {
            assert!(app.moves.start(key, "Closed"));
            app.finish_move(key, Err(format!("{} failed", key)));
        }

        app.moves.dismiss_failure();
        assert_eq!(app.moves.failures()[0].key, "DSCI-2");
        app.moves.dismiss_failure();
        assert!(app.moves.failures().is_empty());
    }
}
