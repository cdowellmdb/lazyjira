//! Bulk move and assignment progression, planned work, and result acceptance.
//! The workflow changes App state and returns work; it never calls Jira itself.

use crate::app::App;
use crate::bulk_plan::{self, BulkPlan, FetchedTransitions};
use crossterm::event::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkAction {
    Move,
    Assign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BulkTarget {
    Move {
        /// Name of the status to move to.
        destination: String,
        /// Name of the chosen resolution, if any.
        resolution: Option<String>,
    },
    Assign {
        member_email: String,
        member_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkSummary {
    pub action: BulkAction,
    pub target: BulkTarget,
    pub total: usize,
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub successful_keys: Vec<String>,
    pub failed_details: Vec<(String, String)>,
    /// Tickets that weren't sent, and why.
    pub skipped: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum BulkState {
    ActionPicker {
        targets: Vec<String>,
        selected: usize,
    },
    /// Waiting for Jira to list each target's transitions. Only the answer to `request` is used.
    MoveLoading { targets: Vec<String>, request: u64 },
    /// Choosing a destination from `bulk_plan::destinations(&fetched)`.
    MoveStatusPicker {
        targets: Vec<String>,
        fetched: crate::bulk_plan::FetchedTransitions,
        selected: usize,
    },
    /// Choosing one resolution from `plan.resolution_choices()` for the whole move.
    MoveResolutionPicker {
        targets: Vec<String>,
        destination: String,
        plan: crate::bulk_plan::BulkPlan,
        selected: usize,
    },
    AssignPicker {
        targets: Vec<String>,
        selected: usize,
        search: String,
    },
    Confirm {
        targets: Vec<String>,
        target: BulkTarget,
        plan: crate::bulk_plan::BulkPlan,
    },
    Running {
        request: u64,
        targets: Vec<String>,
        target: BulkTarget,
    },
    Result {
        summary: BulkSummary,
        /// Lines scrolled past, so every failure can be read.
        scroll: u16,
    },
}

/// Work requested by the bulk workflow; the caller executes it and returns the result.
#[derive(Debug)]
pub enum BulkCall {
    FetchTransitions {
        targets: Vec<String>,
        request: u64,
    },
    Execute {
        request: u64,
        target: BulkTarget,
        plan: BulkPlan,
    },
}

/// Apply successful ticket changes even after dismissal, but show results only for the
/// operation still displayed. Esc dismisses a modal without cancelling its Jira work.
pub fn complete(app: &mut App, request: u64, summary: BulkSummary) {
    apply_bulk_successes(app, &summary);
    let action = match summary.action {
        BulkAction::Move => "move",
        BulkAction::Assign => "assign",
    };
    app.flash = Some(format!(
        "Bulk {} complete: {} succeeded, {} failed, {} skipped",
        action,
        summary.succeeded,
        summary.failed,
        summary.skipped.len()
    ));
    if matches!(app.bulk_state, Some(BulkState::Running { request: waiting, .. }) if waiting == request)
    {
        app.bulk_state = Some(BulkState::Result { summary, scroll: 0 });
    }
}

pub fn summarize(
    action: BulkAction,
    target: BulkTarget,
    results: Vec<(String, std::result::Result<(), String>)>,
    skipped: Vec<(String, String)>,
) -> BulkSummary {
    let attempted = results.len();
    let mut successful_keys = Vec::new();
    let mut failed_details = Vec::new();
    for (key, result) in results {
        match result {
            Ok(()) => successful_keys.push(key),
            Err(err) => failed_details.push((key, err)),
        }
    }
    BulkSummary {
        action,
        target,
        total: attempted + skipped.len(),
        attempted,
        succeeded: successful_keys.len(),
        failed: failed_details.len(),
        successful_keys,
        failed_details,
        skipped,
    }
}

fn apply_bulk_successes(app: &mut App, summary: &BulkSummary) {
    match &summary.target {
        BulkTarget::Move { destination, .. } => {
            for key in &summary.successful_keys {
                app.record_move(key, destination);
            }
        }
        BulkTarget::Assign {
            member_email,
            member_name,
        } => {
            for key in &summary.successful_keys {
                app.update_ticket_assignee(key, member_name, member_email);
            }
        }
    }
    app.clamp_selection();
}

/// The list row after `key`: j/Down moves down, k/Up moves up, other keys leave it.
fn list_step(selected: usize, key: KeyCode, rows: usize) -> usize {
    match key {
        KeyCode::Char('j') | KeyCode::Down => (selected + 1).min(rows.saturating_sub(1)),
        KeyCode::Char('k') | KeyCode::Up => selected.saturating_sub(1),
        _ => selected,
    }
}

pub fn open(app: &mut App) {
    let mut targets = app.selected_visible_ticket_keys_in_order();
    if targets.is_empty() {
        if let Some(key) = app.selected_ticket_key() {
            targets.push(key);
        }
    }
    if targets.is_empty() {
        app.flash = Some("No tickets selected".to_string());
        return;
    }
    app.bulk_state = Some(BulkState::ActionPicker {
        targets,
        selected: 0,
    });
}

/// Offers the bulk move's destinations, unless the user has since closed the bulk modal or
/// started another bulk action.
pub fn receive_transitions(app: &mut App, request: u64, fetched: FetchedTransitions) {
    if let Some(BulkState::MoveLoading {
        targets,
        request: waiting,
    }) = &app.bulk_state
    {
        if *waiting == request {
            app.bulk_state = Some(BulkState::MoveStatusPicker {
                targets: targets.clone(),
                fetched,
                selected: 0,
            });
        }
    }
}

pub fn handle_key(app: &mut App, key: KeyCode) -> Option<BulkCall> {
    let state = app.bulk_state.clone()?;
    match state {
        BulkState::ActionPicker { targets, selected } => match key {
            KeyCode::Esc => app.bulk_state = None,
            KeyCode::Char('j') | KeyCode::Down => {
                let new_sel = (selected + 1).min(1);
                app.bulk_state = Some(BulkState::ActionPicker {
                    targets,
                    selected: new_sel,
                });
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.bulk_state = Some(BulkState::ActionPicker {
                    targets,
                    selected: selected.saturating_sub(1),
                });
            }
            KeyCode::Enter if selected == 0 => {
                let request = app.next_request_id();
                app.bulk_state = Some(BulkState::MoveLoading {
                    targets: targets.clone(),
                    request,
                });
                return Some(BulkCall::FetchTransitions { targets, request });
            }
            KeyCode::Enter => {
                app.bulk_state = Some(BulkState::AssignPicker {
                    targets,
                    selected: 0,
                    search: String::new(),
                });
            }
            _ => {}
        },
        BulkState::MoveLoading { .. } => {
            if key == KeyCode::Esc {
                app.bulk_state = None;
            }
        }
        BulkState::MoveStatusPicker {
            targets,
            fetched,
            selected,
        } => {
            let destinations = bulk_plan::destinations(&fetched);
            match key {
                KeyCode::Esc => app.bulk_state = None,
                KeyCode::Enter => {
                    let (destination, _) = destinations.into_iter().nth(selected)?;
                    let plan = bulk_plan::plan_move(app, &fetched, &destination);
                    app.bulk_state =
                        Some(if plan.resolution_choices().iter().any(Option::is_some) {
                            BulkState::MoveResolutionPicker {
                                targets,
                                destination,
                                plan,
                                selected: 0,
                            }
                        } else {
                            BulkState::Confirm {
                                targets,
                                target: BulkTarget::Move {
                                    destination,
                                    resolution: None,
                                },
                                plan: plan.with_resolution(None),
                            }
                        });
                }
                _ => {
                    app.bulk_state = Some(BulkState::MoveStatusPicker {
                        targets,
                        fetched,
                        selected: list_step(selected, key, destinations.len()),
                    });
                }
            }
        }
        BulkState::MoveResolutionPicker {
            targets,
            destination,
            plan,
            selected,
        } => {
            let choices = plan.resolution_choices();
            match key {
                KeyCode::Esc => app.bulk_state = None,
                KeyCode::Enter => {
                    let choice = choices.get(selected)?;
                    let target = BulkTarget::Move {
                        destination,
                        resolution: choice.as_ref().map(|r| r.name.clone()),
                    };
                    app.bulk_state = Some(BulkState::Confirm {
                        targets,
                        target,
                        plan: plan.with_resolution(choice.as_ref()),
                    });
                }
                _ => {
                    app.bulk_state = Some(BulkState::MoveResolutionPicker {
                        targets,
                        destination,
                        plan,
                        selected: list_step(selected, key, choices.len()),
                    });
                }
            }
        }
        BulkState::AssignPicker {
            targets,
            mut selected,
            mut search,
        } => {
            let options = app
                .cache
                .team_members
                .iter()
                .map(|member| format!("{} ({})", member.name, member.email))
                .collect::<Vec<_>>();
            match key {
                KeyCode::Esc => app.bulk_state = None,
                KeyCode::Enter => {
                    if !crate::widgets::form::matching(&options, &search).contains(&selected) {
                        app.flash = Some("No matching assignee".into());
                        return None;
                    }
                    let member = app.cache.team_members.get(selected)?;
                    let target = BulkTarget::Assign {
                        member_email: member.email.clone(),
                        member_name: member.name.clone(),
                    };
                    let plan = bulk_plan::plan_assign(app, &targets, &member.email);
                    app.bulk_state = Some(BulkState::Confirm {
                        targets,
                        target,
                        plan,
                    });
                }
                _ => {
                    crate::widgets::form::choose(&options, &mut selected, &mut search, key);
                    app.bulk_state = Some(BulkState::AssignPicker {
                        targets,
                        selected,
                        search,
                    });
                }
            }
        }
        BulkState::Confirm {
            targets,
            target,
            plan,
        } => match key {
            KeyCode::Esc => app.bulk_state = None,
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.flash = Some(format!(
                    "Running bulk action on {} tickets...",
                    targets.len()
                ));
                let request = app.next_request_id();
                app.bulk_state = Some(BulkState::Running {
                    request,
                    targets,
                    target: target.clone(),
                });
                return Some(BulkCall::Execute {
                    request,
                    target,
                    plan,
                });
            }
            _ => {}
        },
        BulkState::Running { .. } => {
            if matches!(key, KeyCode::Esc) {
                app.bulk_state = None;
            }
        }
        BulkState::Result { summary, scroll } => match key {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => app.bulk_state = None,
            KeyCode::Char('j') | KeyCode::Down => {
                app.bulk_state = Some(BulkState::Result {
                    summary,
                    scroll: scroll.saturating_add(1),
                });
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.bulk_state = Some(BulkState::Result {
                    summary,
                    scroll: scroll.saturating_sub(1),
                });
            }
            _ => {}
        },
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::Ticket;
    fn ticket(key: &str, summary: &str, status: &str) -> Ticket {
        Ticket {
            summary: summary.into(),
            ..Ticket::for_test(key, status)
        }
    }
    #[test]
    fn summarize_bulk_results_all_success() {
        let target = BulkTarget::Move {
            destination: "In Progress".to_string(),
            resolution: None,
        };
        let summary = summarize(
            BulkAction::Move,
            target.clone(),
            vec![
                ("DEMO-1".to_string(), Ok(())),
                ("DEMO-2".to_string(), Ok(())),
            ],
            Vec::new(),
        );
        assert_eq!(summary.target, target);
        assert_eq!(summary.succeeded, 2);
        assert_eq!(summary.failed, 0);
        assert!(summary.skipped.is_empty());
    }

    #[test]
    fn summarize_bulk_results_partial_failure_and_skips() {
        let summary = summarize(
            BulkAction::Assign,
            BulkTarget::Assign {
                member_email: "dev@example.com".to_string(),
                member_name: "Dev".to_string(),
            },
            vec![
                ("DEMO-1".to_string(), Ok(())),
                ("DEMO-2".to_string(), Err("boom".to_string())),
            ],
            vec![("DEMO-3".to_string(), "already assigned".to_string())],
        );
        assert_eq!(summary.total, 3);
        assert_eq!(summary.attempted, 2);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failed_details.len(), 1);
        assert_eq!(
            summary.skipped,
            [("DEMO-3".to_string(), "already assigned".to_string())]
        );
    }

    #[test]
    fn summarize_bulk_results_empty_attempts() {
        let summary = summarize(
            BulkAction::Move,
            BulkTarget::Move {
                destination: "Closed".to_string(),
                resolution: None,
            },
            vec![],
            vec![
                ("DEMO-1".to_string(), "already Closed".to_string()),
                (
                    "DEMO-2".to_string(),
                    "no transition to Closed from Open".to_string(),
                ),
            ],
        );
        assert_eq!(summary.total, 2);
        assert_eq!(summary.attempted, 0);
        assert_eq!(summary.succeeded, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped.len(), 2);
    }

    #[test]
    fn bulk_transitions_for_an_old_request_are_dropped() {
        let mut app = App::new();
        let targets = vec!["DEMO-1".to_string()];
        let fetched = || vec![("DEMO-1".to_string(), Ok(Vec::new()))];
        app.bulk_state = Some(BulkState::MoveLoading {
            targets: targets.clone(),
            request: 2,
        });

        receive_transitions(&mut app, 1, fetched());
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::MoveLoading { request: 2, .. })
        ));

        receive_transitions(&mut app, 2, fetched());
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::MoveStatusPicker { .. })
        ));

        // Closed the modal while Jira was answering.
        app.bulk_state = None;
        receive_transitions(&mut app, 2, fetched());
        assert!(app.bulk_state.is_none());
    }

    // No Tokio runtime: nothing here may reach Jira.
    #[test]
    fn bulk_move_asks_once_for_a_resolution_then_confirms_the_plan() {
        use crate::transitions::tests::{transition, with_resolution};

        let mut app = App::new();
        app.loading = false;
        app.cache.my_tickets = vec![
            ticket("DEMO-1", "A", "In Progress"),
            ticket("DEMO-2", "B", "In Progress"),
        ];
        let done = transition("805", "Done", "Done");
        app.select_all_visible_tickets();
        open(&mut app);
        let Some(BulkCall::FetchTransitions { targets, request }) =
            handle_key(&mut app, KeyCode::Enter)
        else {
            panic!("opening a bulk move should request transitions");
        };
        assert_eq!(targets, ["DEMO-1", "DEMO-2"]);
        receive_transitions(
            &mut app,
            request,
            vec![
                (
                    "DEMO-1".to_string(),
                    Ok(vec![with_resolution(
                        done.clone(),
                        true,
                        &[("101", "Fixed")],
                    )]),
                ),
                (
                    "DEMO-2".to_string(),
                    Ok(vec![
                        transition("803", "Stop Progress", "Open"),
                        with_resolution(done, true, &[("102", "Won't Fix")]),
                    ]),
                ),
            ],
        );

        // "Done" (2 tickets) sorts before "Open" (1 ticket).
        handle_key(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.bulk_state,
            Some(BulkState::MoveResolutionPicker { ref destination, .. }) if destination == "Done"
        ));
        handle_key(&mut app, KeyCode::Enter);

        let Some(BulkState::Confirm { target, plan, .. }) = &app.bulk_state else {
            panic!("expected the confirm step");
        };
        assert_eq!(
            target,
            &BulkTarget::Move {
                destination: "Done".to_string(),
                resolution: Some("Fixed".to_string()),
            }
        );
        assert_eq!(plan.jobs.len(), 1);
        assert_eq!(plan.jobs[0].0, "DEMO-1");
        assert_eq!(plan.skipped[0].0, "DEMO-2");

        let Some(BulkCall::Execute {
            request,
            target,
            plan,
        }) = handle_key(&mut app, KeyCode::Enter)
        else {
            panic!("confirmation should return the work to execute");
        };
        let summary = summarize(
            BulkAction::Move,
            target,
            vec![("DEMO-1".into(), Ok(()))],
            plan.skipped,
        );
        complete(&mut app, request, summary);
        assert_eq!(app.find_ticket("DEMO-1").unwrap().status, "Done");
        assert_eq!(app.find_ticket("DEMO-2").unwrap().status, "In Progress");
        assert!(matches!(app.bulk_state, Some(BulkState::Result { .. })));
    }

    #[test]
    fn dismissed_work_updates_tickets_without_replacing_a_newer_workflow() {
        fn start_assignment(app: &mut App) -> (u64, BulkTarget) {
            open(app);
            assert!(handle_key(app, KeyCode::Down).is_none());
            assert!(handle_key(app, KeyCode::Enter).is_none());
            assert!(handle_key(app, KeyCode::Enter).is_none());
            let Some(BulkCall::Execute {
                request,
                target,
                plan,
            }) = handle_key(app, KeyCode::Enter)
            else {
                panic!("expected assignment work");
            };
            assert_eq!(plan.jobs.len(), 1);
            (request, target)
        }

        for reopen in [false, true] {
            let mut app = App::new();
            app.cache.my_tickets = vec![Ticket::for_test("DEMO-1", "In Progress")];
            app.cache.team_members = vec![crate::cache::TeamMember {
                name: "Alex".into(),
                email: "alex@example.com".into(),
            }];
            app.selected_index = 1;
            let (old_request, target) = start_assignment(&mut app);
            assert!(handle_key(&mut app, KeyCode::Esc).is_none());
            let newer = reopen.then(|| start_assignment(&mut app));

            complete(
                &mut app,
                old_request,
                summarize(
                    BulkAction::Assign,
                    target,
                    vec![("DEMO-1".into(), Ok(()))],
                    vec![],
                ),
            );
            assert_eq!(
                app.find_ticket("DEMO-1").unwrap().assignee.as_deref(),
                Some("Alex")
            );
            if let Some((new_request, target)) = newer {
                assert_ne!(new_request, old_request);
                assert!(
                    matches!(app.bulk_state, Some(BulkState::Running { request, .. }) if request == new_request)
                );
                complete(
                    &mut app,
                    new_request,
                    summarize(
                        BulkAction::Assign,
                        target,
                        vec![("DEMO-1".into(), Err("denied".into()))],
                        vec![],
                    ),
                );
                let Some(BulkState::Result { summary, .. }) = app.bulk_state else {
                    panic!("current completion should show its result");
                };
                assert_eq!(summary.failed_details, [("DEMO-1".into(), "denied".into())]);
            } else {
                assert!(app.bulk_state.is_none());
            }
        }
    }
}
