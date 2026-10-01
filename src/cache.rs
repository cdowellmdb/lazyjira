use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The statuses the app knows about: move shortcut keys, default colors, and the fallback
/// for names the `[statuses]` config doesn't list. Tickets keep Jira's own status name
/// (`Ticket::status`); `Status::from_str` only interprets it, so Done, Closed and Resolved
/// all read as `Status::Closed`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

/// How the `[statuses]` config places each status name: the display order of status groups
/// and epic children, and whether a status is done. Names match case-insensitively.
///
/// Order: active statuses in config order, then statuses the config doesn't list (in
/// first-seen order), then done statuses. A name in both lists counts as done.
///
/// A name the config doesn't list takes the place of a listed name `Status::from_str` reads
/// the same way, so with the default config "Open" sits with To Do and "Resolved" with Done.
/// Failing that, a name that reads as `Status::Closed` is done and goes last, and anything
/// else is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRules {
    /// Each configured name, by `name_key`.
    listed: HashMap<String, Placement>,
    /// For each built-in status, the first configured name that reads as it.
    built_in: HashMap<Status, Placement>,
    /// Where names the config doesn't list go: after the active ones, before the done ones.
    unlisted: usize,
    /// Where an unlisted done name goes: after every configured one.
    last: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placement {
    rank: usize,
    done: bool,
}

impl StatusRules {
    pub fn new(active: &[String], done: &[String]) -> Self {
        let mut listed = HashMap::new();
        for name in Self::names(active) {
            let rank = listed.len();
            listed
                .entry(name_key(name))
                .or_insert(Placement { rank, done: false });
        }
        let unlisted = listed.len();
        let mut next = unlisted + 1;
        for name in Self::names(done) {
            let key = name_key(name);
            if !listed.get(&key).is_some_and(|p: &Placement| p.done) {
                listed.insert(
                    key,
                    Placement {
                        rank: next,
                        done: true,
                    },
                );
                next += 1;
            }
        }

        let mut built_in = HashMap::new();
        for name in Self::names(active).chain(Self::names(done)) {
            let status = Status::from_str(name);
            if !matches!(status, Status::Other(_)) {
                built_in.entry(status).or_insert(listed[&name_key(name)]);
            }
        }
        Self {
            listed,
            built_in,
            unlisted,
            last: next,
        }
    }

    fn names(names: &[String]) -> impl Iterator<Item = &str> {
        names
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
    }

    fn place(&self, name: &str) -> Placement {
        if let Some(placement) = self.listed.get(&name_key(name)) {
            return *placement;
        }
        let status = Status::from_str(name.trim());
        if let Some(placement) = self.built_in.get(&status) {
            return *placement;
        }
        Placement {
            rank: if status == Status::Closed {
                self.last
            } else {
                self.unlisted
            },
            done: status == Status::Closed,
        }
    }

    /// Position of the status named `name` in display order; lower comes first.
    pub fn rank(&self, name: &str) -> usize {
        self.place(name).rank
    }

    /// Whether the status named `name` is done (hidden by `d`, counted as epic progress).
    pub fn is_done(&self, name: &str) -> bool {
        self.place(name).done
    }

    /// Groups tickets by status name in display order. Tickets keep their order within a
    /// group, and unlisted statuses keep their first-seen order.
    pub fn group<'a>(
        &self,
        tickets: impl IntoIterator<Item = &'a Ticket>,
    ) -> Vec<(String, Vec<&'a Ticket>)> {
        let mut groups: Vec<(String, Vec<&Ticket>)> = Vec::new();
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

impl Default for StatusRules {
    fn default() -> Self {
        let statuses = crate::config::StatusConfig::default();
        Self::new(&statuses.active, &statuses.done)
    }
}

/// Jira and the config may capitalize a status name differently.
fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
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
    /// Jira's name for the status, e.g. "Resolved". `StatusRules` decides its order and
    /// whether it's done. Stored as `status_name`: caches from before it, whose `status` held
    /// a collapsed `Status`, then fail to load and are fetched again.
    #[serde(rename = "status_name")]
    pub status: String,
    pub assignee: Option<String>,
    pub assignee_email: Option<String>,
    #[serde(default)]
    pub reporter: Option<String>,
    pub description: Option<String>,
    pub labels: Vec<String>,
    pub epic_key: Option<String>,
    pub epic_name: Option<String>,
    /// The ticket this sub-task belongs to. Read from Jira's search on every refresh, never
    /// from the detail cache, so a re-parented sub-task doesn't keep its old parent.
    #[serde(default)]
    pub parent_key: Option<String>,
    #[serde(default)]
    pub detail_loaded: bool,
    #[serde(default)]
    pub activity: Vec<ActivityEntry>,
}

#[cfg(test)]
impl Ticket {
    /// A bare ticket in the status Jira calls `status`.
    pub fn for_test(key: &str, status: &str) -> Self {
        Ticket {
            key: key.to_string(),
            summary: key.to_string(),
            status: status.to_string(),
            assignee: None,
            assignee_email: None,
            reporter: None,
            description: None,
            labels: Vec::new(),
            epic_key: None,
            epic_name: None,
            parent_key: None,
            detail_loaded: false,
            activity: Vec::new(),
        }
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

    pub fn done_count(&self, rules: &StatusRules) -> usize {
        self.children
            .iter()
            .filter(|t| rules.is_done(&t.status))
            .count()
    }

    pub fn blocked_count(&self) -> usize {
        self.children
            .iter()
            .filter(|t| Status::from_str(&t.status) == Status::Blocked)
            .count()
    }

    pub fn progress_pct(&self, rules: &StatusRules) -> f64 {
        if self.total() == 0 {
            return 0.0;
        }
        self.done_count(rules) as f64 / self.total() as f64 * 100.0
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
    use super::{StatusRules, Ticket};

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn group_names(rules: &StatusRules, statuses: &[&str]) -> Vec<String> {
        let tickets: Vec<Ticket> = statuses
            .iter()
            .enumerate()
            .map(|(i, status)| Ticket::for_test(&format!("DSCI-{}", i), status))
            .collect();
        rules
            .group(&tickets)
            .into_iter()
            .map(|(status, _)| status)
            .collect()
    }

    fn dsci() -> StatusRules {
        StatusRules::new(
            &names(&[
                "Backlog",
                "Open",
                "On Deck",
                "In Progress",
                "In Team Review",
            ]),
            &names(&["Done", "Closed", "Cancelled", "Won't Do"]),
        )
    }

    #[test]
    fn default_order_starts_with_in_progress_and_ends_with_done() {
        let rules = StatusRules::default();
        assert_eq!(
            group_names(
                &rules,
                &[
                    "Resolved",
                    "Done",
                    "On Deck",
                    "Blocked",
                    "Open",
                    "In Progress",
                    "Stalled"
                ]
            ),
            // Open takes To Do's place and Resolved Done's, as Status::from_str reads them.
            [
                "In Progress",
                "Open",
                "Blocked",
                "On Deck",
                "Stalled",
                "Resolved",
                "Done"
            ]
        );
        assert!(rules.is_done("Resolved"));
        assert!(rules.is_done("closed"));
        assert!(!rules.is_done("On Deck"));
    }

    #[test]
    fn configured_names_keep_their_own_groups_in_config_order() {
        let rules = dsci();
        assert_eq!(
            group_names(
                &rules,
                &[
                    "Won't Do",
                    "Resolved",
                    "In Team Review",
                    "Stalled",
                    "To Do",
                    "On Deck",
                    "Closed",
                    "Backlog"
                ]
            ),
            // Stalled isn't listed, so it follows the active statuses. To Do isn't either, so
            // it takes Open's place; Resolved takes Done's.
            [
                "Backlog",
                "To Do",
                "On Deck",
                "In Team Review",
                "Stalled",
                "Resolved",
                "Closed",
                "Won't Do"
            ]
        );
    }

    #[test]
    fn done_comes_from_config_with_closed_like_names_as_the_fallback() {
        let rules = dsci();
        for done in ["Cancelled", "won't do", "Done", "Resolved"] {
            assert!(rules.is_done(done), "{done} should be done");
        }
        for active in ["Backlog", "Stalled", "In Progress", "Denied"] {
            assert!(!rules.is_done(active), "{active} should be active");
        }

        // Config wins over the built-in reading, and a name in both lists is done.
        let rules = StatusRules::new(&names(&["Done", "Denied"]), &names(&["Denied"]));
        assert!(!rules.is_done("Done"));
        assert!(rules.is_done("Denied"));
        assert!(rules.rank("Denied") > rules.rank("Stalled"));

        // Nothing configured reads as Closed, so Resolved is done and goes last.
        let rules = StatusRules::new(&names(&["Backlog"]), &names(&["Denied"]));
        assert!(rules.is_done("Resolved"));
        assert!(rules.rank("Resolved") > rules.rank("Denied"));
    }

    #[test]
    fn sort_tickets_orders_by_status_then_key() {
        let rules = StatusRules::default();
        let tickets = [
            Ticket::for_test("DSCI-3", "Closed"),
            Ticket::for_test("DSCI-2", "In Progress"),
            Ticket::for_test("DSCI-9", "On Deck"),
            Ticket::for_test("DSCI-1", "In Progress"),
        ];
        let mut refs: Vec<&Ticket> = tickets.iter().collect();
        rules.sort_tickets(&mut refs);
        let keys: Vec<&str> = refs.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, ["DSCI-1", "DSCI-2", "DSCI-9", "DSCI-3"]);
    }
}
