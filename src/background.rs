//! What the answer to background work does to `App`, and the work it leads to.
//!
//! `main.rs` spawns the work and sends each answer back as a `BackgroundMessage`. `apply` changes
//! `App` for it and returns the `FollowUp` for `main.rs` to start, so nothing here spawns a task,
//! reads Jira or touches a file.

use crate::app::{App, BulkUploadPreview, BulkUploadState, BulkUploadSummary, FilterFocus};
use crate::bulk_actions::{self, BulkSummary};
use crate::bulk_plan::FetchedTransitions;
use crate::cache::{Cache, Epic, Ticket};
use crate::cache_refresh::CacheRefreshPhase;
use crate::move_picker;
use crate::transitions::Transition;

/// Results of background work. `requested_at` is `app.moves.now()` when a Jira read was
/// requested, so a read that predates a confirmed move can't undo it.
pub enum BackgroundMessage {
    EpicsRefreshed {
        request: u64,
        requested_at: u64,
        result: Result<Vec<Epic>, String>,
    },
    CacheRefreshed {
        request: u64,
        phase: CacheRefreshPhase,
        requested_at: u64,
        result: Result<Cache, String>,
    },
    TicketDetailFetched {
        key: String,
        requested_at: u64,
        result: Result<Ticket, String>,
    },
    /// Jira's transitions for the move picker. `request` is the picker's `MoveLoading` request.
    TransitionsFetched {
        key: String,
        request: u64,
        result: Result<Vec<Transition>, String>,
    },
    /// Every selected ticket's transitions for a bulk move, for the bulk `MoveLoading` request.
    BulkTransitionsFetched {
        request: u64,
        fetched: FetchedTransitions,
    },
    TicketMoved {
        key: String,
        result: Result<(), String>,
    },
    TicketCreated(Result<String, String>),
    CommentAdded(Result<String, String>),
    TicketAssigned {
        key: String,
        result: Result<(), String>,
    },
    TicketEdited {
        key: String,
        result: Result<(), String>,
    },
    BulkCompleted {
        request: u64,
        summary: BulkSummary,
    },
    BulkUploadPreviewReady(Result<BulkUploadPreview, String>),
    BulkUploadCompleted(BulkUploadSummary),
    /// A saved filter's tickets. `request` is the run's `App::filter_request`.
    FilterResults {
        request: u64,
        requested_at: u64,
        result: Result<Vec<Ticket>, String>,
    },
}

/// The work `main.rs` starts after a background message is applied, in this order.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FollowUp {
    /// Prefetch the details the new lists lack.
    pub prefetch_details: bool,
    /// Save the cache as the snapshot, then report it with `App::snapshot_saved` for this phase.
    pub save_snapshot: Option<CacheRefreshPhase>,
    /// The cache refresh to send next.
    pub next_phase: Option<CacheRefreshPhase>,
    /// Fetch this ticket's detail; `apply` has already marked the fetch as begun.
    pub fetch_detail: Option<String>,
}

impl FollowUp {
    /// A manual refresh, unless a refresh is already running.
    fn manual_refresh(app: &mut App) -> Self {
        FollowUp {
            next_phase: app
                .begin_manual_refresh()
                .then_some(CacheRefreshPhase::Manual),
            ..FollowUp::default()
        }
    }
}

/// Applies `message` to `app` and returns the work it leads to.
pub fn apply(app: &mut App, message: BackgroundMessage) -> FollowUp {
    match message {
        BackgroundMessage::EpicsRefreshed {
            request,
            requested_at,
            result,
        } => {
            // The answer to a superseded epics refresh changes nothing.
            if request == app.epic_refresh_request {
                app.epics_refreshing = false;
                match result {
                    Ok(epics) => {
                        app.replace_epics(epics, requested_at);
                        app.flash = Some("Epic relationships refreshed".to_string());
                    }
                    Err(e) => app.flash = Some(format!("Epic refresh failed: {}", e)),
                }
            }
        }
        BackgroundMessage::CacheRefreshed {
            request,
            phase,
            requested_at,
            result,
        } => return app.apply_cache_refresh(phase, request, requested_at, result),
        BackgroundMessage::TicketDetailFetched {
            key,
            requested_at,
            result,
        } => {
            app.end_detail_fetch(&key);
            match result {
                Ok(detail) => {
                    if app.enrich_ticket(&key, requested_at, &detail) {
                        app.details.record(detail);
                    }
                }
                Err(e) => app.fail_detail_fetch(&key, e),
            }
        }
        BackgroundMessage::TransitionsFetched {
            key,
            request,
            result,
        } => move_picker::receive(app, &key, request, result),
        BackgroundMessage::BulkTransitionsFetched { request, fetched } => {
            bulk_actions::receive_transitions(app, request, fetched)
        }
        BackgroundMessage::TicketMoved { key, result } => {
            let succeeded = result.is_ok();
            app.finish_move(&key, result);
            if succeeded {
                app.begin_detail_fetch(&key);
                return FollowUp {
                    fetch_detail: Some(key),
                    ..FollowUp::default()
                };
            }
        }
        BackgroundMessage::TicketCreated(result) => match result {
            Ok(key) => {
                app.flash = Some(format!("Created {}", key));
                app.delight.created(1);
                // Pick up the new ticket.
                return FollowUp::manual_refresh(app);
            }
            Err(e) => app.flash = Some(format!("Create failed: {}", e)),
        },
        BackgroundMessage::CommentAdded(result) => match result {
            Ok(key) => {
                app.flash = Some(format!("Comment added to {}", key));
                app.delight.commented();
            }
            Err(e) => app.flash = Some(format!("Comment failed: {}", e)),
        },
        BackgroundMessage::TicketAssigned { key, result } => {
            app.flash = Some(match result {
                Ok(()) => format!("Assigned {}", key),
                Err(e) => format!("Assign failed for {}: {}", key, e),
            });
        }
        BackgroundMessage::TicketEdited { key, result } => {
            app.flash = Some(match result {
                Ok(()) => format!("Updated {}", key),
                Err(e) => format!("Edit failed for {}: {}", key, e),
            });
        }
        BackgroundMessage::BulkCompleted { request, summary } => {
            bulk_actions::complete(app, request, summary);
        }
        BackgroundMessage::BulkUploadPreviewReady(result) => match result {
            Ok(preview) => {
                app.flash = Some(format!(
                    "Preview ready: {} rows ({} invalid)",
                    preview.total_rows, preview.invalid_rows
                ));
                app.bulk_upload_state = Some(BulkUploadState::Preview {
                    preview,
                    selected: 0,
                });
            }
            Err(e) => {
                if let Some(BulkUploadState::PathInput { loading, .. }) =
                    app.bulk_upload_state.as_mut()
                {
                    *loading = false;
                }
                app.flash = Some(format!("CSV preview failed: {}", e));
            }
        },
        BackgroundMessage::BulkUploadCompleted(summary) => {
            app.delight.created(summary.succeeded);
            app.flash = Some(format!(
                "Bulk upload complete: {} succeeded, {} failed",
                summary.succeeded, summary.failed
            ));
            if matches!(app.bulk_upload_state, Some(BulkUploadState::Running { .. })) {
                app.bulk_upload_state = Some(BulkUploadState::Result { summary });
            }
            return FollowUp::manual_refresh(app);
        }
        BackgroundMessage::FilterResults {
            request,
            requested_at,
            result,
        } => {
            // The answer to a filter run that a newer run replaced changes nothing.
            if request != app.filter_request {
                return FollowUp::default();
            }
            app.filter_loading = false;
            match result {
                Ok(tickets) => {
                    let count = tickets.len();
                    app.show_filter_results(tickets, requested_at);
                    app.prune_selection_to_visible();
                    app.filter_focus = FilterFocus::Results;
                    app.selected_index = 0;
                    app.flash = Some(format!("Filter returned {} tickets", count));
                }
                Err(e) => {
                    app.clear_filter_results();
                    app.prune_selection_to_visible();
                    app.flash = Some(format!("Filter query failed: {}", e));
                }
            }
        }
    }
    FollowUp::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{DetailMode, Tab, TicketSyncStage};
    use crate::bulk_actions::{BulkAction, BulkState, BulkTarget};
    use crate::move_picker::JiraCall;

    fn app_with_my_ticket(key: &str, status: &str) -> App {
        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![Ticket::for_test(key, status)];
        app
    }

    fn upload_summary(succeeded: usize, failed: usize) -> BulkUploadSummary {
        BulkUploadSummary {
            source_path: "/tmp/demo.csv".to_string(),
            total_rows: succeeded + failed,
            attempted: succeeded + failed,
            succeeded,
            failed,
            created_keys: vec![],
            failed_details: vec![],
        }
    }

    fn upload_preview(total_rows: usize, invalid_rows: usize) -> BulkUploadPreview {
        BulkUploadPreview {
            source_path: "/tmp/demo.csv".to_string(),
            rows: vec![],
            total_rows,
            valid_rows: total_rows - invalid_rows,
            invalid_rows,
            warning_count: 0,
        }
    }

    #[test]
    fn a_superseded_epics_refresh_changes_nothing_and_the_current_one_lands() {
        let mut app = App::new();
        app.epic_refresh_request = 2;
        app.epics_refreshing = true;
        let epics = |request| BackgroundMessage::EpicsRefreshed {
            request,
            requested_at: 0,
            result: Ok(vec![Epic::for_test("DEMO-100", &[])]),
        };

        assert_eq!(apply(&mut app, epics(1)), FollowUp::default());
        assert!(app.epics_refreshing);
        assert!(app.cache.epics.is_empty());
        assert_eq!(app.flash, None);

        assert_eq!(apply(&mut app, epics(2)), FollowUp::default());
        assert!(!app.epics_refreshing);
        assert_eq!(app.cache.epics.len(), 1);
        assert_eq!(app.flash.as_deref(), Some("Epic relationships refreshed"));

        // A failed refresh keeps the epics it couldn't replace.
        app.epics_refreshing = true;
        let failed = BackgroundMessage::EpicsRefreshed {
            request: 2,
            requested_at: 0,
            result: Err("boom".to_string()),
        };
        assert_eq!(apply(&mut app, failed), FollowUp::default());
        assert!(!app.epics_refreshing);
        assert_eq!(app.cache.epics.len(), 1);
        assert_eq!(app.flash.as_deref(), Some("Epic refresh failed: boom"));
    }

    #[test]
    fn a_cache_refresh_returns_what_applying_it_leads_to() {
        let mut app = App::new();
        app.ticket_sync_stage = Some(TicketSyncStage::Full);
        let request = app.begin_cache_refresh();
        let mut cache = Cache::empty();
        cache.my_tickets = vec![Ticket::for_test("DEMO-1", "To Do")];

        let refreshed = BackgroundMessage::CacheRefreshed {
            request,
            phase: CacheRefreshPhase::Full,
            requested_at: app.moves.now(),
            result: Ok(cache),
        };
        let follow_up = apply(&mut app, refreshed);

        assert_eq!(
            follow_up,
            FollowUp {
                prefetch_details: true,
                save_snapshot: Some(CacheRefreshPhase::Full),
                ..FollowUp::default()
            }
        );
        assert_eq!(app.cache.my_tickets[0].key, "DEMO-1");
    }

    #[test]
    fn a_fetched_detail_enriches_the_ticket_and_a_failed_fetch_keeps_its_error() {
        let mut app = app_with_my_ticket("DEMO-1", "To Do");
        app.begin_detail_fetch("DEMO-1");
        let mut detail = Ticket::for_test("DEMO-1", "In Progress");
        detail.description = Some("The body".to_string());
        let fetched = BackgroundMessage::TicketDetailFetched {
            key: "DEMO-1".to_string(),
            requested_at: app.moves.now(),
            result: Ok(detail),
        };

        assert_eq!(apply(&mut app, fetched), FollowUp::default());
        let ticket = &app.cache.my_tickets[0];
        assert_eq!(ticket.status, "In Progress");
        assert_eq!(ticket.description.as_deref(), Some("The body"));
        // The detail is kept for the next list read to hydrate from.
        let mut read = Cache::empty();
        read.my_tickets = vec![Ticket::for_test("DEMO-1", "To Do")];
        app.details.hydrate(&mut read);
        assert_eq!(read.my_tickets[0].description.as_deref(), Some("The body"));
        // The fetch has ended, so another can begin.
        assert!(app.begin_detail_fetch("DEMO-1"));

        let failed = BackgroundMessage::TicketDetailFetched {
            key: "DEMO-1".to_string(),
            requested_at: app.moves.now(),
            result: Err("boom".to_string()),
        };
        assert_eq!(apply(&mut app, failed), FollowUp::default());
        assert_eq!(app.detail_fetch_error("DEMO-1"), Some("boom"));
        assert!(app.begin_detail_fetch("DEMO-1"));
    }

    #[test]
    fn a_confirmed_move_shows_the_status_and_asks_for_the_tickets_detail() {
        let mut app = app_with_my_ticket("DEMO-1", "To Do");
        assert!(app.moves.start("DEMO-1", "In Progress"));
        let moved = BackgroundMessage::TicketMoved {
            key: "DEMO-1".to_string(),
            result: Ok(()),
        };

        assert_eq!(
            apply(&mut app, moved),
            FollowUp {
                fetch_detail: Some("DEMO-1".to_string()),
                ..FollowUp::default()
            }
        );
        assert_eq!(app.cache.my_tickets[0].status, "In Progress");
        // The fetch main.rs is asked to send is already marked as begun.
        assert!(!app.begin_detail_fetch("DEMO-1"));
    }

    #[test]
    fn a_rejected_move_keeps_the_status_and_asks_for_nothing() {
        let mut app = app_with_my_ticket("DEMO-1", "To Do");
        assert!(app.moves.start("DEMO-1", "In Progress"));
        let rejected = BackgroundMessage::TicketMoved {
            key: "DEMO-1".to_string(),
            result: Err("Resolution is required.".to_string()),
        };

        assert_eq!(apply(&mut app, rejected), FollowUp::default());
        assert_eq!(app.cache.my_tickets[0].status, "To Do");
        assert_eq!(app.moves.failures().len(), 1);
        assert!(app.begin_detail_fetch("DEMO-1"));
    }

    #[test]
    fn a_created_ticket_or_finished_bulk_upload_starts_one_manual_refresh() {
        let created = || BackgroundMessage::TicketCreated(Ok("DEMO-9".to_string()));
        let uploaded = || BackgroundMessage::BulkUploadCompleted(upload_summary(2, 1));
        type Message = fn() -> BackgroundMessage;
        let cases: [(Message, &str); 2] = [
            (created, "Created DEMO-9"),
            (uploaded, "Bulk upload complete: 2 succeeded, 1 failed"),
        ];
        for (message, flash) in cases {
            // With no refresh running, a manual one starts and the startup sync gives way.
            let mut app = App::new();
            app.loading = false;
            app.ticket_sync_stage = Some(TicketSyncStage::Full);
            assert_eq!(
                apply(&mut app, message()),
                FollowUp {
                    next_phase: Some(CacheRefreshPhase::Manual),
                    ..FollowUp::default()
                },
                "{flash}"
            );
            assert!(app.loading, "{flash}");
            assert!(app.ticket_sync_stage.is_none(), "{flash}");
            assert_eq!(app.flash.as_deref(), Some(flash));

            // With one already running, the answer starts no second refresh.
            assert_eq!(apply(&mut app, message()), FollowUp::default(), "{flash}");
            assert!(app.loading, "{flash}");
        }
    }

    #[test]
    fn a_finished_bulk_upload_shows_its_result_only_while_the_upload_is_on_screen() {
        let mut app = App::new();
        app.bulk_upload_state = Some(BulkUploadState::Running {
            preview: upload_preview(3, 0),
        });
        apply(
            &mut app,
            BackgroundMessage::BulkUploadCompleted(upload_summary(2, 1)),
        );
        match &app.bulk_upload_state {
            Some(BulkUploadState::Result { summary }) => assert_eq!(*summary, upload_summary(2, 1)),
            other => panic!("expected the result, got {other:?}"),
        }

        // A dismissed upload stays dismissed.
        app.bulk_upload_state = None;
        apply(
            &mut app,
            BackgroundMessage::BulkUploadCompleted(upload_summary(2, 1)),
        );
        assert!(app.bulk_upload_state.is_none());

        // So does the preview of another upload opened since.
        app.bulk_upload_state = Some(BulkUploadState::Preview {
            preview: upload_preview(3, 0),
            selected: 0,
        });
        apply(
            &mut app,
            BackgroundMessage::BulkUploadCompleted(upload_summary(2, 1)),
        );
        assert!(matches!(
            app.bulk_upload_state,
            Some(BulkUploadState::Preview { .. })
        ));
    }

    #[test]
    fn a_csv_preview_opens_or_says_why_it_failed() {
        let path_input = || {
            Some(BulkUploadState::PathInput {
                path: crate::widgets::form::editor("demo.csv").into(),
                loading: true,
            })
        };
        let mut app = App::new();
        app.bulk_upload_state = path_input();
        apply(
            &mut app,
            BackgroundMessage::BulkUploadPreviewReady(Ok(upload_preview(3, 1))),
        );
        assert!(matches!(
            app.bulk_upload_state,
            Some(BulkUploadState::Preview { selected: 0, .. })
        ));
        assert_eq!(
            app.flash.as_deref(),
            Some("Preview ready: 3 rows (1 invalid)")
        );

        // A failed preview leaves the path on screen, no longer loading.
        app.bulk_upload_state = path_input();
        apply(
            &mut app,
            BackgroundMessage::BulkUploadPreviewReady(Err("no such file".to_string())),
        );
        assert!(matches!(
            app.bulk_upload_state,
            Some(BulkUploadState::PathInput { loading: false, .. })
        ));
        assert_eq!(
            app.flash.as_deref(),
            Some("CSV preview failed: no such file")
        );
    }

    #[test]
    fn a_write_flashes_its_outcome_and_leads_to_nothing() {
        let key = || "DEMO-1".to_string();
        fn boom<T>() -> Result<T, String> {
            Err("boom".to_string())
        }
        let cases = [
            (
                BackgroundMessage::CommentAdded(Ok(key())),
                "Comment added to DEMO-1",
            ),
            (
                BackgroundMessage::CommentAdded(boom()),
                "Comment failed: boom",
            ),
            (
                BackgroundMessage::TicketAssigned {
                    key: key(),
                    result: Ok(()),
                },
                "Assigned DEMO-1",
            ),
            (
                BackgroundMessage::TicketAssigned {
                    key: key(),
                    result: boom(),
                },
                "Assign failed for DEMO-1: boom",
            ),
            (
                BackgroundMessage::TicketEdited {
                    key: key(),
                    result: Ok(()),
                },
                "Updated DEMO-1",
            ),
            (
                BackgroundMessage::TicketEdited {
                    key: key(),
                    result: boom(),
                },
                "Edit failed for DEMO-1: boom",
            ),
            (
                BackgroundMessage::TicketCreated(boom()),
                "Create failed: boom",
            ),
        ];
        for (message, flash) in cases {
            let mut app = App::new();
            app.loading = false;
            assert_eq!(apply(&mut app, message), FollowUp::default(), "{flash}");
            assert_eq!(app.flash.as_deref(), Some(flash));
            assert!(!app.loading, "{flash}");
        }
    }

    #[test]
    fn filter_results_land_in_the_results_pane_and_a_failed_query_empties_it() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        app.filter_loading = true;
        app.selected_index = 3;
        // A ticket selected in the results on screen, which the new results don't have.
        app.show_filter_results(vec![Ticket::for_test("DEMO-9", "To Do")], 0);
        app.select_all_visible_tickets();
        assert_eq!(app.selected_ticket_count(), 1);
        let results = BackgroundMessage::FilterResults {
            request: app.filter_request,
            requested_at: app.moves.now(),
            result: Ok(vec![
                Ticket::for_test("DEMO-1", "Done"),
                Ticket::for_test("DEMO-2", "In Progress"),
            ]),
        };

        assert_eq!(apply(&mut app, results), FollowUp::default());
        assert!(!app.filter_loading);
        assert_eq!(app.filter_results.len(), 2);
        assert_eq!(app.filter_focus, FilterFocus::Results);
        assert_eq!(app.selected_index, 0);
        assert!(app.is_collapsed(Tab::Filters, "Done"));
        assert_eq!(app.selected_ticket_count(), 0);
        assert_eq!(app.flash.as_deref(), Some("Filter returned 2 tickets"));

        app.select_all_visible_tickets();
        assert_eq!(app.selected_ticket_count(), 1);
        app.filter_loading = true;
        let failed = BackgroundMessage::FilterResults {
            request: app.filter_request,
            requested_at: app.moves.now(),
            result: Err("bad JQL".to_string()),
        };
        assert_eq!(apply(&mut app, failed), FollowUp::default());
        assert!(!app.filter_loading);
        assert!(app.filter_results.is_empty());
        assert!(!app.is_collapsed(Tab::Filters, "Done"));
        assert_eq!(app.selected_ticket_count(), 0);
        assert_eq!(app.flash.as_deref(), Some("Filter query failed: bad JQL"));
    }

    #[test]
    fn an_answer_requested_before_a_confirmed_move_does_not_undo_it() {
        let mut app = app_with_my_ticket("DEMO-1", "To Do");
        let before_the_move = app.moves.now();
        assert!(app.moves.start("DEMO-1", "In Progress"));
        let moved = BackgroundMessage::TicketMoved {
            key: "DEMO-1".to_string(),
            result: Ok(()),
        };
        apply(&mut app, moved);
        let as_jira_had_it = || Ticket::for_test("DEMO-1", "To Do");

        let detail = BackgroundMessage::TicketDetailFetched {
            key: "DEMO-1".to_string(),
            requested_at: before_the_move,
            result: Ok(as_jira_had_it()),
        };
        apply(&mut app, detail);
        assert_eq!(app.cache.my_tickets[0].status, "In Progress");

        let mut lists = Cache::empty();
        lists.my_tickets = vec![Ticket {
            summary: "From the list read".to_string(),
            ..as_jira_had_it()
        }];
        let refreshed = BackgroundMessage::CacheRefreshed {
            request: app.begin_cache_refresh(),
            phase: CacheRefreshPhase::Manual,
            requested_at: before_the_move,
            result: Ok(lists),
        };
        apply(&mut app, refreshed);
        assert_eq!(app.cache.my_tickets[0].summary, "From the list read");
        assert_eq!(app.cache.my_tickets[0].status, "In Progress");

        let epics = BackgroundMessage::EpicsRefreshed {
            request: app.epic_refresh_request,
            requested_at: before_the_move,
            result: Ok(vec![Epic {
                children: vec![as_jira_had_it()],
                ..Epic::for_test("DEMO-100", &[])
            }]),
        };
        apply(&mut app, epics);
        assert_eq!(app.cache.epics[0].children[0].status, "In Progress");

        let results = BackgroundMessage::FilterResults {
            request: app.filter_request,
            requested_at: before_the_move,
            result: Ok(vec![as_jira_had_it()]),
        };
        apply(&mut app, results);
        assert_eq!(app.filter_results[0].status, "In Progress");
    }

    #[test]
    fn the_quit_receipt_counts_what_was_created_and_commented() {
        let mut app = App::new();
        app.delight.enabled = true;
        apply(
            &mut app,
            BackgroundMessage::TicketCreated(Ok("DEMO-9".to_string())),
        );
        apply(
            &mut app,
            BackgroundMessage::CommentAdded(Ok("DEMO-9".to_string())),
        );
        apply(
            &mut app,
            BackgroundMessage::BulkUploadCompleted(upload_summary(2, 1)),
        );

        let receipt = app.delight.receipt().expect("a receipt");
        assert!(receipt.contains("💬 1"), "{receipt}");
        assert!(receipt.contains("✚ 3 created"), "{receipt}");
    }

    #[test]
    fn the_move_pickers_and_the_bulk_result_get_their_answers() {
        // The single-ticket move picker opens on the transitions it asked for.
        let mut app = app_with_my_ticket("DEMO-1", "To Do");
        app.open_detail("DEMO-1".to_string());
        let Some(JiraCall::FetchTransitions { key, request }) = move_picker::open(&mut app) else {
            panic!("the picker should ask Jira for transitions");
        };
        let transitions = BackgroundMessage::TransitionsFetched {
            key,
            request,
            result: Ok(vec![]),
        };
        assert_eq!(apply(&mut app, transitions), FollowUp::default());
        assert!(matches!(app.detail_mode, DetailMode::MovePicker(_)));

        // The bulk move's destination picker opens on the transitions it asked for.
        app.bulk_state = Some(BulkState::MoveLoading {
            targets: vec!["DEMO-1".to_string()],
            request: 7,
        });
        let fetched = BackgroundMessage::BulkTransitionsFetched {
            request: 7,
            fetched: vec![],
        };
        assert_eq!(apply(&mut app, fetched), FollowUp::default());
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::MoveStatusPicker { .. })
        ));

        // A finished bulk action says how it went.
        let summary = BulkSummary {
            action: BulkAction::Assign,
            target: BulkTarget::Assign {
                member_email: "ann@example.com".to_string(),
                member_name: "Ann".to_string(),
            },
            total: 1,
            attempted: 1,
            succeeded: 0,
            failed: 1,
            successful_keys: vec![],
            failed_details: vec![("DEMO-1".to_string(), "boom".to_string())],
            skipped: vec![],
        };
        let completed = BackgroundMessage::BulkCompleted {
            request: 8,
            summary,
        };
        assert_eq!(apply(&mut app, completed), FollowUp::default());
        assert_eq!(
            app.flash.as_deref(),
            Some("Bulk assign complete: 0 succeeded, 1 failed, 0 skipped")
        );
    }

    #[test]
    fn only_the_latest_filter_runs_answer_is_shown() {
        let mut app = App::new();
        app.loading = false;
        app.active_tab = Tab::Filters;
        app.filter_request = 2;
        app.filter_loading = true;
        let answer = |request, result| BackgroundMessage::FilterResults {
            request,
            requested_at: 0,
            result,
        };
        let found = |key: &str| Ok(vec![Ticket::for_test(key, "To Do")]);

        // The first run's answer arrives while the second is still out: nothing changes.
        let stale = answer(1, found("DEMO-1"));
        assert_eq!(apply(&mut app, stale), FollowUp::default());
        assert!(app.filter_loading);
        assert!(app.filter_results.is_empty());
        assert_eq!(app.filter_focus, FilterFocus::Sidebar);
        assert_eq!(app.flash, None);

        apply(&mut app, answer(2, found("DEMO-2")));
        assert!(!app.filter_loading);
        assert_eq!(app.filter_results[0].key, "DEMO-2");

        // Arriving last, the first run's answer or failure still doesn't replace the second's,
        // or move the cursor off its ticket.
        app.selected_index = 1;
        for stale in [
            answer(1, found("DEMO-1")),
            answer(1, Err("boom".to_string())),
        ] {
            apply(&mut app, stale);
            assert_eq!(app.selected_index, 1);
            assert_eq!(app.filter_results.len(), 1);
            assert_eq!(app.filter_results[0].key, "DEMO-2");
            assert_eq!(app.flash.as_deref(), Some("Filter returned 1 tickets"));
        }
    }
}
