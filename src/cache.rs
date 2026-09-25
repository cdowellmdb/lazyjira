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

    /// All statuses in display order.
    pub fn all() -> &'static [Status] {
        &[
            Status::InProgress,
            Status::ReadyForWork,
            Status::NeedsTriage,
            Status::ToDo,
            Status::InReview,
            Status::Blocked,
            Status::Closed,
        ]
    }
}

/// Groups tickets by status in display order: canonical statuses in `Status::all()`
/// order, then workflow-specific `Status::Other` statuses in first-seen order.
pub fn group_by_status<'a>(
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
    // The sort is stable, so `Other` statuses keep their first-seen order.
    groups.sort_by_key(|(status, _)| {
        Status::all()
            .iter()
            .position(|canonical| canonical == status)
            .unwrap_or(usize::MAX)
    });
    groups
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
