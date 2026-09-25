//! What a bulk action will send for each selected ticket, and why the others are skipped.
//! It is all decided before anything is sent to Jira.

use std::collections::{BTreeMap, BTreeSet};

use crate::app::App;
use crate::cache::Ticket;
use crate::transitions::{resolution_choices, Resolution, Transition};

/// Each selected ticket's transitions, or why Jira couldn't list them, in selection order.
pub type FetchedTransitions = Vec<(String, Result<Vec<Transition>, String>)>;

/// The Jira call a bulk action makes for one ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BulkJob {
    Move {
        transition: Transition,
        resolution_id: Option<String>,
    },
    Assign {
        email: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BulkPlan {
    pub jobs: Vec<(String, BulkJob)>,
    /// Tickets that won't be sent, with the reason shown in the summary.
    pub skipped: Vec<(String, String)>,
}

/// Plans assigning each of `targets` to `email`.
pub fn plan_assign(app: &App, targets: &[String], email: &str) -> BulkPlan {
    plan(app, targets.iter().map(|key| (key, ())), |ticket, ()| {
        if ticket.assignee_email.as_deref() == Some(email) {
            return Err("already assigned".to_string());
        }
        Ok(BulkJob::Assign {
            email: email.to_string(),
        })
    })
}

/// Plans moving each ticket to the status named `destination` through its transition there.
/// A ticket with no such transition, or with several the user could tell apart, is skipped.
/// The resolution is added afterwards with [`BulkPlan::with_resolution`].
pub fn plan_move(app: &App, fetched: &FetchedTransitions, destination: &str) -> BulkPlan {
    let per_ticket = fetched.iter().map(|(key, result)| (key, result));
    plan(app, per_ticket, |ticket, transitions| {
        // The real status name, so a Resolved ticket can still be moved to Closed.
        if ticket.status_name() == destination {
            return Err(format!("already {}", destination));
        }
        let transitions = transitions
            .as_ref()
            .map_err(|error| format!("couldn't load its transitions: {}", error))?;
        let matching: Vec<&Transition> = transitions
            .iter()
            .filter(|t| t.to_name == destination)
            .collect();
        match matching.as_slice() {
            [] => Err(format!(
                "no transition to {} from {}",
                destination,
                ticket.status_name()
            )),
            [only] => Ok(BulkJob::Move {
                transition: (*only).clone(),
                resolution_id: None,
            }),
            several => Err(format!(
                "ambiguous: {} all lead to {}. Move it on its own",
                several
                    .iter()
                    .map(|t| t.label(transitions))
                    .collect::<Vec<_>>()
                    .join(", "),
                destination
            )),
        }
    })
}

/// Skips tickets that are no longer loaded, and asks `job_for` about the rest.
fn plan<'a, T>(
    app: &App,
    per_ticket: impl IntoIterator<Item = (&'a String, T)>,
    job_for: impl Fn(&Ticket, T) -> Result<BulkJob, String>,
) -> BulkPlan {
    let mut plan = BulkPlan::default();
    for (key, item) in per_ticket {
        let job = match app.find_ticket(key) {
            Some(ticket) => job_for(ticket, item),
            None => Err("no longer loaded".to_string()),
        };
        match job {
            Ok(job) => plan.jobs.push((key.clone(), job)),
            Err(reason) => plan.skipped.push((key.clone(), reason)),
        }
    }
    plan
}

impl BulkPlan {
    /// The choices for the one resolution prompt, covering every planned move whose transition
    /// has a resolution field.
    pub fn resolution_choices(&self) -> Vec<Option<Resolution>> {
        resolution_choices(self.jobs.iter().filter_map(|(_, job)| match job {
            BulkJob::Move { transition, .. } => transition.resolution.as_ref(),
            BulkJob::Assign { .. } => None,
        }))
    }

    /// Applies the chosen resolution (`None` is "No resolution") to each planned move: it is
    /// sent where the transition allows it, left out where the field is optional, and the ticket
    /// is skipped as incompatible where the field requires something else.
    pub fn with_resolution(self, choice: Option<&Resolution>) -> Self {
        let mut plan = BulkPlan {
            jobs: Vec::new(),
            skipped: self.skipped,
        };
        for (key, job) in self.jobs {
            match job {
                BulkJob::Move { transition, .. } => match transition.resolution_to_send(choice) {
                    Ok(resolution_id) => plan.jobs.push((
                        key,
                        BulkJob::Move {
                            transition,
                            resolution_id,
                        },
                    )),
                    Err(reason) => plan
                        .skipped
                        .push((key, format!("incompatible: {}", reason))),
                },
                assign @ BulkJob::Assign { .. } => plan.jobs.push((key, assign)),
            }
        }
        plan
    }
}

/// The statuses a bulk move can offer, with how many tickets have a transition to each,
/// most common first.
pub fn destinations(fetched: &FetchedTransitions) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for transitions in fetched
        .iter()
        .filter_map(|(_, result)| result.as_ref().ok())
    {
        let reachable: BTreeSet<&str> = transitions.iter().map(|t| t.to_name.as_str()).collect();
        for name in reachable {
            *counts.entry(name).or_default() += 1;
        }
    }
    let mut counts: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(name, count)| (name.to_string(), count))
        .collect();
    // Stable, so equal counts stay alphabetical.
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transitions::tests::{resolution, transition, with_resolution};

    fn app_with(tickets: &[(&str, &str)]) -> App {
        let mut app = App::new();
        app.cache.my_tickets = tickets
            .iter()
            .map(|(k, s)| Ticket::for_test(k, s))
            .collect();
        app
    }

    fn sent(plan: &BulkPlan) -> Vec<(&str, &str, Option<&str>)> {
        plan.jobs
            .iter()
            .map(|(key, job)| match job {
                BulkJob::Move {
                    transition,
                    resolution_id,
                } => (
                    key.as_str(),
                    transition.id.as_str(),
                    resolution_id.as_deref(),
                ),
                BulkJob::Assign { email } => (key.as_str(), email.as_str(), None),
            })
            .collect()
    }

    fn skipped(plan: &BulkPlan) -> Vec<(&str, &str)> {
        plan.skipped
            .iter()
            .map(|(key, reason)| (key.as_str(), reason.as_str()))
            .collect()
    }

    #[test]
    fn bulk_move_from_resolved_to_closed_is_not_skipped() {
        let app = app_with(&[("DEMO-1", "Resolved"), ("DEMO-2", "Closed")]);
        let fetched = vec![
            (
                "DEMO-1".to_string(),
                Ok(vec![transition("870", "Closed", "Closed")]),
            ),
            (
                "DEMO-2".to_string(),
                Ok(vec![transition("871", "Resolved", "Resolved")]),
            ),
        ];
        let plan = plan_move(&app, &fetched, "Closed");
        assert_eq!(sent(&plan), [("DEMO-1", "870", None)]);
        assert_eq!(skipped(&plan), [("DEMO-2", "already Closed")]);
    }

    #[test]
    fn bulk_move_skips_ambiguous_missing_and_unloaded_tickets() {
        let app = app_with(&[
            ("DEMO-1", "In Progress"),
            ("DEMO-2", "In Progress"),
            ("DEMO-3", "Backlog"),
            ("DEMO-4", "In Progress"),
        ]);
        let fetched = vec![
            (
                "DEMO-1".to_string(),
                Ok(vec![transition(
                    "861",
                    "Waiting for Approval",
                    "Waiting for Approval",
                )]),
            ),
            (
                "DEMO-2".to_string(),
                Ok(vec![
                    transition("861", "Waiting for Approval", "Waiting for Approval"),
                    transition("862", "Request LGTM", "Waiting for Approval"),
                ]),
            ),
            (
                "DEMO-3".to_string(),
                Ok(vec![transition("812", "Cancelled", "Cancelled")]),
            ),
            (
                "DEMO-4".to_string(),
                Err("Jira answered 404 Not Found.".to_string()),
            ),
        ];
        let plan = plan_move(&app, &fetched, "Waiting for Approval");
        assert_eq!(sent(&plan), [("DEMO-1", "861", None)]);
        assert_eq!(
            skipped(&plan),
            [
                (
                    "DEMO-2",
                    "ambiguous: Waiting for Approval, Request LGTM → Waiting for Approval all \
                     lead to Waiting for Approval. Move it on its own"
                ),
                (
                    "DEMO-3",
                    "no transition to Waiting for Approval from Backlog"
                ),
                (
                    "DEMO-4",
                    "couldn't load its transitions: Jira answered 404 Not Found."
                ),
            ]
        );
    }

    #[test]
    fn bulk_resolution_applies_where_allowed_and_skips_where_required_otherwise() {
        let app = app_with(&[
            ("DEMO-1", "In Progress"),
            ("DEMO-2", "In Progress"),
            ("DEMO-3", "In Progress"),
            ("DEMO-4", "In Progress"),
        ]);
        let done = transition("805", "Done", "Done");
        let fetched = vec![
            // Required, and "Fixed" is allowed.
            (
                "DEMO-1".to_string(),
                Ok(vec![with_resolution(
                    done.clone(),
                    true,
                    &[("101", "Fixed")],
                )]),
            ),
            // Required, but only "Won't Fix" is allowed.
            (
                "DEMO-2".to_string(),
                Ok(vec![with_resolution(
                    done.clone(),
                    true,
                    &[("102", "Won't Fix")],
                )]),
            ),
            // Optional, and "Fixed" isn't allowed: sent without a resolution.
            (
                "DEMO-3".to_string(),
                Ok(vec![with_resolution(
                    done.clone(),
                    false,
                    &[("103", "Duplicate")],
                )]),
            ),
            // No resolution field at all.
            ("DEMO-4".to_string(), Ok(vec![done])),
        ];
        let plan = plan_move(&app, &fetched, "Done");
        assert_eq!(
            plan.resolution_choices(),
            [
                None,
                Some(resolution("101", "Fixed")),
                Some(resolution("102", "Won't Fix")),
                Some(resolution("103", "Duplicate")),
            ]
        );

        let fixed = plan
            .clone()
            .with_resolution(Some(&resolution("101", "Fixed")));
        assert_eq!(
            sent(&fixed),
            [
                ("DEMO-1", "805", Some("101")),
                ("DEMO-3", "805", None),
                ("DEMO-4", "805", None)
            ]
        );
        assert_eq!(
            skipped(&fixed),
            [(
                "DEMO-2",
                "incompatible: \"Done\" requires a resolution and doesn't allow Fixed"
            )]
        );

        let none = plan.with_resolution(None);
        assert_eq!(
            sent(&none),
            [("DEMO-3", "805", None), ("DEMO-4", "805", None)]
        );
        assert_eq!(
            skipped(&none)
                .iter()
                .map(|(key, _)| *key)
                .collect::<Vec<_>>(),
            ["DEMO-1", "DEMO-2"]
        );
    }

    #[test]
    fn bulk_assign_skips_tickets_already_assigned() {
        let mut app = app_with(&[("DEMO-1", "In Progress"), ("DEMO-2", "In Progress")]);
        app.cache.my_tickets[1].assignee_email = Some("dev@example.com".to_string());
        let targets = [
            "DEMO-1".to_string(),
            "DEMO-2".to_string(),
            "DEMO-9".to_string(),
        ];
        let plan = plan_assign(&app, &targets, "dev@example.com");
        assert_eq!(sent(&plan), [("DEMO-1", "dev@example.com", None)]);
        assert_eq!(
            skipped(&plan),
            [
                ("DEMO-2", "already assigned"),
                ("DEMO-9", "no longer loaded")
            ]
        );
    }

    #[test]
    fn destinations_count_tickets_not_transitions() {
        let fetched = vec![
            (
                "DEMO-1".to_string(),
                Ok(vec![
                    transition("861", "Waiting for Approval", "Waiting for Approval"),
                    transition("862", "Request LGTM", "Waiting for Approval"),
                    transition("870", "Closed", "Closed"),
                ]),
            ),
            (
                "DEMO-2".to_string(),
                Ok(vec![
                    transition("805", "Done", "Done"),
                    transition("870", "Closed", "Closed"),
                ]),
            ),
            ("DEMO-3".to_string(), Err("boom".to_string())),
        ];
        assert_eq!(
            destinations(&fetched),
            [
                ("Closed".to_string(), 2),
                ("Done".to_string(), 1),
                ("Waiting for Approval".to_string(), 1),
            ]
        );
    }
}
