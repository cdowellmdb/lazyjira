use crate::app::{App, TicketSyncStage};
use crate::background::FollowUp;
use crate::cache::Cache;
use crate::lists::{attach_epics_to_tickets, reconcile_epic_child_statuses};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheRefreshPhase {
    ActiveOnly,
    Full,
    Manual,
}

impl CacheRefreshPhase {
    /// The startup stage this read belongs to; a manual read belongs to none.
    fn stage(self) -> Option<TicketSyncStage> {
        match self {
            CacheRefreshPhase::ActiveOnly => Some(TicketSyncStage::ActiveOnly),
            CacheRefreshPhase::Full => Some(TicketSyncStage::Full),
            CacheRefreshPhase::Manual => None,
        }
    }
}

impl App {
    /// Marks a manual refresh as started, unless a refresh is already running, and returns
    /// whether it did. The caller sends the read.
    pub fn begin_manual_refresh(&mut self) -> bool {
        if self.loading {
            return false;
        }
        self.loading = true;
        self.ticket_sync_stage = None;
        true
    }

    /// Marks a new cache refresh as the one whose answer counts, and returns its request id.
    pub fn begin_cache_refresh(&mut self) -> u64 {
        self.cache_refresh_request = self.next_request_id();
        self.cache_refresh_request
    }

    /// Applies a cache refresh's answer (`request` is the id it was sent with) and returns the
    /// work it leads to. An answer to a superseded request, or for a stage that has passed,
    /// changes nothing.
    pub fn apply_cache_refresh(
        &mut self,
        phase: CacheRefreshPhase,
        request: u64,
        requested_at: u64,
        result: Result<Cache, String>,
    ) -> FollowUp {
        if request != self.cache_refresh_request
            || phase
                .stage()
                .is_some_and(|stage| self.ticket_sync_stage != Some(stage))
        {
            return FollowUp::default();
        }
        match (phase, result) {
            (CacheRefreshPhase::ActiveOnly, Ok(cache)) => {
                self.ticket_sync_stage = Some(TicketSyncStage::Full);
                self.show_refreshed_lists(cache, requested_at, false);
                self.flash = Some("Active tickets refreshed. Syncing recently done...".to_string());
                FollowUp {
                    next_phase: Some(CacheRefreshPhase::Full),
                    prefetch_details: true,
                    ..FollowUp::default()
                }
            }
            (CacheRefreshPhase::ActiveOnly, Err(e)) => {
                self.ticket_sync_stage = Some(TicketSyncStage::Full);
                self.flash = Some(format!(
                    "Active refresh failed ({}). Trying full refresh...",
                    e
                ));
                FollowUp {
                    next_phase: Some(CacheRefreshPhase::Full),
                    ..FollowUp::default()
                }
            }
            (CacheRefreshPhase::Full, Ok(cache)) => {
                self.ticket_sync_stage = None;
                self.show_refreshed_lists(cache, requested_at, true);
                FollowUp {
                    prefetch_details: true,
                    save_snapshot: Some(phase),
                    ..FollowUp::default()
                }
            }
            (CacheRefreshPhase::Full, Err(e)) => {
                self.ticket_sync_stage = None;
                self.flash = Some(format!("Full refresh failed: {}", e));
                FollowUp::default()
            }
            (CacheRefreshPhase::Manual, Ok(cache)) => {
                self.loading = false;
                self.ticket_sync_stage = None;
                self.show_refreshed_lists(cache, requested_at, true);
                FollowUp {
                    prefetch_details: true,
                    save_snapshot: Some(phase),
                    ..FollowUp::default()
                }
            }
            (CacheRefreshPhase::Manual, Err(e)) => {
                self.loading = false;
                self.flash = Some(format!("Refresh failed: {}", e));
                FollowUp::default()
            }
        }
    }

    /// Shows a list read's tickets with the epics already shown, which may be newer than the read:
    /// a list read doesn't read epics.
    fn show_refreshed_lists(&mut self, mut cache: Cache, requested_at: u64, full_scope: bool) {
        cache.epics = std::mem::take(&mut self.cache.epics);
        attach_epics_to_tickets(&mut cache.my_tickets, &mut cache.team_tickets, &cache.epics);
        reconcile_epic_child_statuses(&mut cache.epics, &cache.my_tickets, &cache.team_tickets);
        if full_scope {
            self.replace_cache_full_scope(cache, requested_at);
        } else {
            self.replace_cache(cache, requested_at);
        }
        self.cache_stale_age_secs = None;
        self.clamp_selection();
    }

    /// Reports saving the snapshot that a `phase` read's `save_snapshot` asked for.
    pub fn snapshot_saved(&mut self, phase: CacheRefreshPhase, saved: Result<(), String>) {
        self.flash = Some(match (phase, saved) {
            (CacheRefreshPhase::Manual, Ok(())) => "Refreshed!".to_string(),
            (CacheRefreshPhase::Manual, Err(e)) => format!("Refreshed (cache save failed: {})", e),
            (_, Ok(())) => "Ticket cache is up to date".to_string(),
            (_, Err(e)) => format!("Cache snapshot write failed: {}", e),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::CacheRefreshPhase;
    use crate::app::{App, Tab, TicketSyncStage};
    use crate::background::FollowUp;
    use crate::cache::{Cache, Epic, Ticket};

    fn my_work_cache(tickets: &[(&str, &str)]) -> Cache {
        let mut cache = Cache::empty();
        cache.my_tickets = tickets
            .iter()
            .map(|(key, status)| Ticket::for_test(key, status))
            .collect();
        cache
    }

    fn my_work_keys(app: &App) -> Vec<&str> {
        app.cache
            .my_tickets
            .iter()
            .map(|t| t.key.as_str())
            .collect()
    }

    /// The app showing DEMO-1 from a snapshot 180 seconds old, with a refresh of `phase` sent.
    fn app_refreshing(phase: CacheRefreshPhase) -> (App, u64) {
        let mut app = App::new();
        app.replace_cache(my_work_cache(&[("DEMO-1", "In Progress")]), app.moves.now());
        app.loading = phase == CacheRefreshPhase::Manual;
        app.cache_stale_age_secs = Some(180);
        app.ticket_sync_stage = phase.stage();
        let request = app.begin_cache_refresh();
        (app, request)
    }

    #[test]
    fn a_superseded_refresh_or_one_for_a_stage_that_has_passed_changes_nothing() {
        use CacheRefreshPhase::{ActiveOnly, Full, Manual};
        for (phase, stage, superseded) in [
            (ActiveOnly, Some(TicketSyncStage::ActiveOnly), true),
            (Full, Some(TicketSyncStage::Full), true),
            (Manual, None, true),
            (ActiveOnly, Some(TicketSyncStage::Full), false),
            (ActiveOnly, None, false),
            (Full, Some(TicketSyncStage::ActiveOnly), false),
            (Full, None, false),
        ] {
            for result in [
                Ok(my_work_cache(&[("DEMO-2", "To Do")])),
                Err("boom".into()),
            ] {
                let (mut app, request) = app_refreshing(phase);
                app.ticket_sync_stage = stage;
                if superseded {
                    app.begin_cache_refresh();
                }
                let loading = app.loading;

                let follow_up = app.apply_cache_refresh(phase, request, app.moves.now(), result);

                let case = format!("{phase:?} {stage:?} superseded={superseded}");
                assert_eq!(follow_up, FollowUp::default(), "{case}");
                assert_eq!(my_work_keys(&app), ["DEMO-1"], "{case}");
                assert_eq!(app.loading, loading, "{case}");
                assert_eq!(app.flash, None, "{case}");
                assert_eq!(app.cache_stale_age_secs, Some(180), "{case}");
                assert_eq!(app.ticket_sync_stage, stage, "{case}");
            }
        }
    }

    #[test]
    fn an_active_only_read_is_applied_and_the_full_read_follows() {
        let (mut app, request) = app_refreshing(CacheRefreshPhase::ActiveOnly);

        let follow_up = app.apply_cache_refresh(
            CacheRefreshPhase::ActiveOnly,
            request,
            app.moves.now(),
            Ok(my_work_cache(&[("DEMO-2", "To Do")])),
        );

        assert_eq!(
            follow_up,
            FollowUp {
                next_phase: Some(CacheRefreshPhase::Full),
                prefetch_details: true,
                ..FollowUp::default()
            }
        );
        assert_eq!(my_work_keys(&app), ["DEMO-2"]);
        assert_eq!(app.cache_stale_age_secs, None);
        assert_eq!(app.ticket_sync_stage, Some(TicketSyncStage::Full));
        assert_eq!(
            app.flash.as_deref(),
            Some("Active tickets refreshed. Syncing recently done...")
        );
    }

    #[test]
    fn a_failed_active_only_read_keeps_the_snapshot_and_tries_the_full_read() {
        let (mut app, request) = app_refreshing(CacheRefreshPhase::ActiveOnly);

        let follow_up = app.apply_cache_refresh(
            CacheRefreshPhase::ActiveOnly,
            request,
            app.moves.now(),
            Err("boom".into()),
        );

        assert_eq!(
            follow_up,
            FollowUp {
                next_phase: Some(CacheRefreshPhase::Full),
                ..FollowUp::default()
            }
        );
        assert_eq!(my_work_keys(&app), ["DEMO-1"]);
        assert_eq!(app.cache_stale_age_secs, Some(180));
        assert_eq!(app.ticket_sync_stage, Some(TicketSyncStage::Full));
        assert_eq!(
            app.flash.as_deref(),
            Some("Active refresh failed (boom). Trying full refresh...")
        );
    }

    #[test]
    fn a_full_read_is_applied_and_saved_as_the_snapshot() {
        let (mut app, request) = app_refreshing(CacheRefreshPhase::Full);

        let follow_up = app.apply_cache_refresh(
            CacheRefreshPhase::Full,
            request,
            app.moves.now(),
            Ok(my_work_cache(&[
                ("DEMO-1", "In Progress"),
                ("DEMO-2", "Closed"),
            ])),
        );

        assert_eq!(
            follow_up,
            FollowUp {
                prefetch_details: true,
                save_snapshot: Some(CacheRefreshPhase::Full),
                ..FollowUp::default()
            }
        );
        assert_eq!(my_work_keys(&app), ["DEMO-1", "DEMO-2"]);
        assert!(app.is_collapsed(Tab::MyWork, "Closed"));
        assert_eq!(app.cache_stale_age_secs, None);
        assert_eq!(app.ticket_sync_stage, None);
    }

    #[test]
    fn a_failed_full_read_ends_the_sync_and_keeps_the_lists() {
        let (mut app, request) = app_refreshing(CacheRefreshPhase::Full);

        let follow_up = app.apply_cache_refresh(
            CacheRefreshPhase::Full,
            request,
            app.moves.now(),
            Err("boom".into()),
        );

        assert_eq!(follow_up, FollowUp::default());
        assert_eq!(my_work_keys(&app), ["DEMO-1"]);
        assert_eq!(app.ticket_sync_stage, None);
        assert_eq!(app.flash.as_deref(), Some("Full refresh failed: boom"));
    }

    #[test]
    fn a_list_read_keeps_the_epics_shown_and_puts_its_tickets_in_them() {
        let (mut app, request) = app_refreshing(CacheRefreshPhase::Manual);
        // An epics read lands while the list read is out.
        app.replace_epics(
            vec![Epic::for_test("DEMO-100", &[("To Do", 1)])],
            app.moves.now(),
        );

        app.apply_cache_refresh(
            CacheRefreshPhase::Manual,
            request,
            app.moves.now(),
            Ok(my_work_cache(&[("DEMO-0", "In Progress")])),
        );

        let epic = &app.cache.epics[0];
        assert_eq!(epic.key, "DEMO-100", "the epics read isn't undone");
        assert_eq!(epic.children[0].status, "In Progress");
        assert_eq!(
            app.cache.my_tickets[0].epic_key.as_deref(),
            Some("DEMO-100")
        );
    }

    #[test]
    fn a_manual_read_is_applied_and_saved() {
        let (mut app, request) = app_refreshing(CacheRefreshPhase::Manual);

        let follow_up = app.apply_cache_refresh(
            CacheRefreshPhase::Manual,
            request,
            app.moves.now(),
            Ok(my_work_cache(&[("DEMO-2", "To Do")])),
        );

        assert_eq!(
            follow_up,
            FollowUp {
                prefetch_details: true,
                save_snapshot: Some(CacheRefreshPhase::Manual),
                ..FollowUp::default()
            }
        );
        assert!(!app.loading);
        assert_eq!(my_work_keys(&app), ["DEMO-2"]);
        assert_eq!(app.cache_stale_age_secs, None);
        assert_eq!(app.ticket_sync_stage, None);
    }

    #[test]
    fn a_failed_manual_read_stops_loading_and_keeps_the_lists() {
        let (mut app, request) = app_refreshing(CacheRefreshPhase::Manual);

        let follow_up = app.apply_cache_refresh(
            CacheRefreshPhase::Manual,
            request,
            app.moves.now(),
            Err("boom".into()),
        );

        assert_eq!(follow_up, FollowUp::default());
        assert!(!app.loading);
        assert_eq!(my_work_keys(&app), ["DEMO-1"]);
        assert_eq!(app.cache_stale_age_secs, Some(180));
        assert_eq!(app.flash.as_deref(), Some("Refresh failed: boom"));
    }

    #[test]
    fn a_saved_snapshot_is_reported_by_the_phase_that_asked_for_it() {
        for (phase, saved, flash) in [
            (
                CacheRefreshPhase::Full,
                Ok(()),
                "Ticket cache is up to date",
            ),
            (
                CacheRefreshPhase::Full,
                Err("disk full".to_string()),
                "Cache snapshot write failed: disk full",
            ),
            (CacheRefreshPhase::Manual, Ok(()), "Refreshed!"),
            (
                CacheRefreshPhase::Manual,
                Err("disk full".to_string()),
                "Refreshed (cache save failed: disk full)",
            ),
        ] {
            let mut app = App::new();

            app.snapshot_saved(phase, saved);

            assert_eq!(app.flash.as_deref(), Some(flash), "{phase:?}");
        }
    }

    /// A manual read after the full read leaves a done group that first appears open
    /// (a ticket resolved mid-session), and keeps folded the ones it already had.
    fn assert_a_new_done_group_stays_open(app: &mut App) {
        let request = app.begin_cache_refresh();
        app.apply_cache_refresh(
            CacheRefreshPhase::Manual,
            request,
            app.moves.now(),
            Ok(my_work_cache(&[
                ("DEMO-1", "Resolved"),
                ("DEMO-2", "Closed"),
            ])),
        );
        assert!(!app.is_collapsed(Tab::MyWork, "Resolved"));
        assert!(app.is_collapsed(Tab::MyWork, "Closed"));
    }

    #[test]
    fn after_startup_from_a_snapshot_a_new_done_group_stays_open() {
        let (mut app, request) = app_refreshing(CacheRefreshPhase::ActiveOnly);
        app.epics_refreshing = true;
        app.apply_cache_refresh(
            CacheRefreshPhase::ActiveOnly,
            request,
            app.moves.now(),
            Ok(my_work_cache(&[("DEMO-1", "In Progress")])),
        );
        let request = app.begin_cache_refresh();
        app.apply_cache_refresh(
            CacheRefreshPhase::Full,
            request,
            app.moves.now(),
            Ok(my_work_cache(&[
                ("DEMO-1", "In Progress"),
                ("DEMO-2", "Closed"),
            ])),
        );

        assert_a_new_done_group_stays_open(&mut app);
    }

    #[test]
    fn on_a_first_run_whose_full_read_fails_a_manual_read_folds_its_done_groups_once() {
        // No snapshot: startup applies the active-only read itself, then sends the full read.
        let mut app = App::new();
        app.replace_cache(my_work_cache(&[("DEMO-1", "In Progress")]), app.moves.now());
        app.loading = false;
        app.ticket_sync_stage = Some(TicketSyncStage::Full);
        app.epics_refreshing = true;
        let request = app.begin_cache_refresh();
        app.apply_cache_refresh(
            CacheRefreshPhase::Full,
            request,
            app.moves.now(),
            Err("boom".into()),
        );

        let request = app.begin_cache_refresh();
        app.apply_cache_refresh(
            CacheRefreshPhase::Manual,
            request,
            app.moves.now(),
            Ok(my_work_cache(&[
                ("DEMO-1", "In Progress"),
                ("DEMO-2", "Closed"),
            ])),
        );
        assert!(app.is_collapsed(Tab::MyWork, "Closed"));

        assert_a_new_done_group_stays_open(&mut app);
    }
}
