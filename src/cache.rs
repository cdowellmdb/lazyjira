use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a Jira ticket status.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    NeedsTriage,
    ReadyForWork,
    ToDo,
    InProgress,
    InReview,
    Blocked,
    Closed,
    Other(String),
}

impl Status {
    pub fn as_str(&self) -> &str {
        match self {
            Status::NeedsTriage => "Needs Triage",
            Status::ReadyForWork => "Ready for Work",
            Status::ToDo => "To Do",
            Status::InProgress => "In Progress",
            Status::InReview => "In Review",
            Status::Blocked => "Blocked",
            Status::Closed => "Closed",
            Status::Other(s) => s,
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "needs triage" => Status::NeedsTriage,
            "ready for work" => Status::ReadyForWork,
            "to do" | "todo" | "open" | "new" => Status::ToDo,
            "in progress" | "in development" => Status::InProgress,
            "in review" | "review" => Status::InReview,
            "blocked" => Status::Blocked,
            "done" | "closed" | "resolved" => Status::Closed,
            _ => Status::Other(s.to_string()),
        }
    }

    pub fn move_shortcut(&self) -> char {
        match self {
            Status::InProgress => 'p',
            Status::ReadyForWork => 'w',
            Status::NeedsTriage => 'n',
            Status::ToDo => 't',
            Status::InReview => 'v',
            Status::Blocked => 'b',
            Status::Closed => 'c',
            Status::Other(_) => '?',
        }
    }

    pub fn from_move_shortcut(c: char) -> Option<Self> {
        match c.to_ascii_lowercase() {
            'p' => Some(Status::InProgress),
            'w' => Some(Status::ReadyForWork),
            'n' => Some(Status::NeedsTriage),
            't' => Some(Status::ToDo),
            'v' => Some(Status::InReview),
            'b' => Some(Status::Blocked),
            'c' => Some(Status::Closed),
            _ => None,
        }
    }
}

/// The order status groups are shown in, taken from the `[statuses]` config: active
/// statuses in config order, then statuses the config doesn't list (in first-seen order),
/// then done statuses. `Status::Closed` always ranks with the done statuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusOrder {
    /// Rank of each listed status, keyed by `order_key`.
    ranks: HashMap<String, usize>,
    /// Rank of statuses the config doesn't list: after the active ones, before the done ones.
    unlisted: usize,
}

impl StatusOrder {
    /// Orders statuses by the configured `active` and `done` lists. Names are matched
    /// through `Status::from_str`, so "Open" places To Do and "Resolved" places Closed.
    pub fn new(active: &[String], done: &[String]) -> Self {
        let mut ranks = HashMap::new();
        for status in Self::parse(active) {
            // Closed is done wherever the config lists it.
            if status != Status::Closed {
                let rank = ranks.len();
                ranks.entry(order_key(&status)).or_insert(rank);
            }
        }
        let unlisted = ranks.len();
        for status in Self::parse(done).chain([Status::Closed]) {
            let rank = ranks.len() + 1;
            ranks.entry(order_key(&status)).or_insert(rank);
        }
        Self { ranks, unlisted }
    }

    fn parse(names: &[String]) -> impl Iterator<Item = Status> + '_ {
        names
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .map(Status::from_str)
    }

    /// Position of `status` in display order; lower comes first.
    pub fn rank(&self, status: &Status) -> usize {
        self.ranks
            .get(&order_key(status))
            .copied()
            .unwrap_or(self.unlisted)
    }

    /// Groups tickets by status in display order. Tickets keep their order within a group,
    /// and unlisted statuses keep their first-seen order.
    pub fn group<'a>(
        &self,
        tickets: impl IntoIterator<Item = &'a Ticket>,
    ) -> Vec<(Status, Vec<&'a Ticket>)> {
        let mut groups: Vec<(Status, Vec<&Ticket>)> = Vec::new();
        for ticket in tickets {
            match groups
                .iter_mut()
                .find(|(status, _)| *status == ticket.status)
            {
                Some((_, group)) => group.push(ticket),
                None => groups.push((ticket.status.clone(), vec![ticket])),
            }
        }
        // The sort is stable, so unlisted statuses keep their first-seen order.
        groups.sort_by_key(|(status, _)| self.rank(status));
        groups
    }

    /// Sorts tickets by status in display order, then by key.
    pub fn sort_tickets(&self, tickets: &mut [&Ticket]) {
        tickets.sort_by(|a, b| {
            self.rank(&a.status)
                .cmp(&self.rank(&b.status))
                .then_with(|| a.key.cmp(&b.key))
        });
    }
}

impl Default for StatusOrder {
    fn default() -> Self {
        let statuses = crate::config::StatusConfig::default();
        Self::new(&statuses.active, &statuses.done)
    }
}

/// Statuses match case-insensitively: Jira and the config may capitalize a name differently.
fn order_key(status: &Status) -> String {
    status.as_str().to_lowercase()
}

/// A single entry in a ticket's activity history (changelog or comment).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub timestamp: String,
    pub author: String,
    pub author_email: Option<String>,
    pub kind: ActivityKind,
}

/// The type of activity: status change, comment, assignee change, or generic field change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActivityKind {
    StatusChange {
        from: String,
        to: String,
    },
    Comment {
        body: String,
    },
    AssigneeChange {
        from: Option<String>,
        to: Option<String>,
    },
    FieldChange {
        field: String,
        from: String,
        to: String,
    },
}

/// A single Jira ticket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    pub key: String,
    pub summary: String,
    pub status: Status,
    /// Jira's own name for the status, which `status` may collapse (Resolved becomes
    /// `Status::Closed`). `None` in caches written before this field existed. Read it through
    /// `status_name()`, and change both fields with `set_status()`.
    #[serde(default)]
    pub jira_status: Option<String>,
    pub assignee: Option<String>,
    pub assignee_email: Option<String>,
    #[serde(default)]
    pub reporter: Option<String>,
    pub description: Option<String>,
    pub labels: Vec<String>,
    pub epic_key: Option<String>,
    pub epic_name: Option<String>,
    #[serde(default)]
    pub detail_loaded: bool,
    pub url: String,
    #[serde(default)]
    pub activity: Vec<ActivityEntry>,
}

impl Ticket {
    /// Jira's name for the ticket's status, e.g. "Resolved" rather than "Closed".
    pub fn status_name(&self) -> &str {
        self.jira_status
            .as_deref()
            .unwrap_or_else(|| self.status.as_str())
    }

    /// Sets the status from Jira's name for it.
    pub fn set_status(&mut self, name: &str) {
        self.status = Status::from_str(name);
        self.jira_status = Some(name.to_string());
    }
}

#[cfg(test)]
impl Ticket {
    /// A bare ticket in the status Jira calls `status`.
    pub fn for_test(key: &str, status: &str) -> Self {
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
}

/// An epic with aggregated child ticket info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Epic {
    pub key: String,
    pub summary: String,
    pub children: Vec<Ticket>,
}

impl Epic {
    pub fn total(&self) -> usize {
        self.children.len()
    }

    pub fn done_count(&self) -> usize {
        self.children
            .iter()
            .filter(|t| t.status == Status::Closed)
            .count()
    }

    pub fn count_by_status(&self) -> HashMap<&Status, usize> {
        let mut counts = HashMap::new();
        for ticket in &self.children {
            *counts.entry(&ticket.status).or_insert(0) += 1;
        }
        counts
    }

    pub fn progress_pct(&self) -> f64 {
        if self.total() == 0 {
            return 0.0;
        }
        self.done_count() as f64 / self.total() as f64 * 100.0
    }
}

/// Team member info loaded from team.yml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub name: String,
    pub email: String,
}

/// The full in-memory cache, populated on startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cache {
    pub my_tickets: Vec<Ticket>,
    pub team_tickets: Vec<Ticket>,
    pub epics: Vec<Epic>,
    pub team_members: Vec<TeamMember>,
}

impl Cache {
    pub fn empty() -> Self {
        Self {
            my_tickets: Vec::new(),
            team_tickets: Vec::new(),
            epics: Vec::new(),
            team_members: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Status, StatusOrder, Ticket};

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn group_names(order: &StatusOrder, statuses: &[&str]) -> Vec<String> {
        let tickets: Vec<Ticket> = statuses
            .iter()
            .enumerate()
            .map(|(i, status)| Ticket::for_test(&format!("DSCI-{}", i), status))
            .collect();
        order
            .group(&tickets)
            .iter()
            .map(|(status, _)| status.as_str().to_string())
            .collect()
    }

    #[test]
    fn default_order_starts_with_in_progress_and_ends_with_closed() {
        let order = StatusOrder::default();
        assert_eq!(
            group_names(
                &order,
                &[
                    "Done",
                    "On Deck",
                    "Blocked",
                    "To Do",
                    "In Progress",
                    "Stalled"
                ]
            ),
            [
                "In Progress",
                "To Do",
                "Blocked",
                "On Deck",
                "Stalled",
                "Closed"
            ]
        );
    }

    #[test]
    fn configured_order_places_workflow_statuses_and_matches_names_loosely() {
        let order = StatusOrder::new(
            &names(&[
                "Backlog",
                "Open",
                "On Deck",
                "In Progress",
                "In Team Review",
            ]),
            &names(&["Done", "Closed", "Cancelled"]),
        );
        assert_eq!(
            group_names(
                &order,
                &[
                    "Resolved",
                    "in team review",
                    "Stalled",
                    "To Do",
                    "On Deck",
                    "Cancelled",
                    "BACKLOG"
                ]
            ),
            // "Open" places To Do and "Done" places Closed. Stalled isn't listed, so it
            // goes after the active statuses and before the done ones.
            [
                "BACKLOG",
                "To Do",
                "On Deck",
                "in team review",
                "Stalled",
                "Closed",
                "Cancelled"
            ]
        );
    }

    #[test]
    fn closed_ranks_last_even_when_the_config_omits_it_or_lists_it_as_active() {
        let order = StatusOrder::new(&names(&["Done", "In Progress", ""]), &[]);
        assert!(order.rank(&Status::Closed) > order.rank(&Status::Other("Stalled".into())));
        assert!(order.rank(&Status::InProgress) < order.rank(&Status::Other("Stalled".into())));
    }

    #[test]
    fn sort_tickets_orders_by_status_then_key() {
        let order = StatusOrder::default();
        let tickets = [
            Ticket::for_test("DSCI-3", "Closed"),
            Ticket::for_test("DSCI-2", "In Progress"),
            Ticket::for_test("DSCI-9", "On Deck"),
            Ticket::for_test("DSCI-1", "In Progress"),
        ];
        let mut refs: Vec<&Ticket> = tickets.iter().collect();
        order.sort_tickets(&mut refs);
        let keys: Vec<&str> = refs.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, ["DSCI-1", "DSCI-2", "DSCI-9", "DSCI-3"]);
    }
}
