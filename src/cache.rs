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

/// The keys `Status::from_move_shortcut` reads, as the move pickers' hints list them.
pub const MOVE_SHORTCUTS: &str = "p/w/n/t/v/b/c";

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

    /// The move pickers' "[p] " before a destination called `name`, or as many spaces when no
    /// shortcut leads there.
    pub fn move_shortcut_prefix(name: &str) -> String {
        match Status::from_str(name) {
            Status::Other(_) => "    ".to_string(),
            status => format!("[{}] ", status.move_shortcut()),
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

    /// Groups tickets by status name in display order. Done groups are in `order_done`'s order;
    /// other groups keep their tickets' order, and unlisted
    /// statuses keep their first-seen order.
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
        for (status, tickets) in &mut groups {
            if self.is_done(status) {
                self.order_done(tickets);
            }
        }
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

    /// Orders done tickets for display: most recently updated first, those without an `updated`
    /// last; ties keep their order. Every list of done tickets goes through it.
    pub fn order_done(&self, done: &mut [&Ticket]) {
        done.sort_by_cached_key(|ticket| std::cmp::Reverse(ticket.updated_secs()));
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

/// A single entry in a ticket's activity: a comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub timestamp: String,
    pub author: String,
    pub author_email: Option<String>,
    pub kind: ActivityKind,
}

/// What an activity entry is. Comments are all Jira's search and issue reads give (neither asks
/// for the changelog). It stays an enum so caches written so far still load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActivityKind {
    Comment { body: String },
}

/// The roster member `ticket` is assigned to: the one with the assignee's email, else the one
/// with their display name, for when Jira hides the email (or the roster has another address).
/// The list search only finds tickets assigned to a roster email, so an email the roster lacks
/// is another address of someone on it, and the name is how to tell who. A detail read can show
/// anyone, so a stranger who shares a display name with a roster member is taken for them until
/// the next list read; that is accepted over a duplicate row for the same person.
pub fn roster_member<'a>(members: &'a [TeamMember], ticket: &Ticket) -> Option<&'a TeamMember> {
    let by_email = ticket
        .assignee_email
        .as_deref()
        .and_then(|email| members.iter().find(|member| member.email == email));
    by_email.or_else(|| {
        let name = ticket.assignee.as_deref()?.to_lowercase();
        members
            .iter()
            .find(|member| member.name.to_lowercase() == name)
    })
}

/// The Team row that holds tickets nobody has taken, and the email that stands for it.
pub const UNASSIGNED_TEAM_NAME: &str = "Unassigned";
pub const UNASSIGNED_TEAM_EMAIL: &str = "__unassigned__";

/// What a ticket's status reads as when Jira's answer has none. It isn't a real status, so it
/// shows plainly instead of passing as To Do, and a detail read never copies it over a real one.
pub const UNKNOWN_STATUS: &str = "Unknown";

/// A single Jira ticket.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Jira's `updated` timestamp as Jira sends it (`2026-09-30T10:23:20.000+0000`). Filled by
    /// the list search; caches from before it load without one. The lists show its age and
    /// order done groups by it.
    #[serde(default)]
    pub updated: Option<String>,
    #[serde(default)]
    pub detail_loaded: bool,
    #[serde(default)]
    pub activity: Vec<ActivityEntry>,
}

impl Ticket {
    /// `updated` in Unix seconds, or `None` when it's missing or not Jira's
    /// `YYYY-MM-DDTHH:MM:SS.fff±HHMM` (or `±HH:MM`).
    pub fn updated_secs(&self) -> Option<i64> {
        let s = self.updated.as_deref()?;
        let num = |range: std::ops::Range<usize>| s.get(range)?.parse::<i64>().ok();
        let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
        let secs = num(11..13)? * 3600 + num(14..16)? * 60 + num(17..19)?;
        let sign_at = 19 + s.get(19..)?.find(['+', '-'])?;
        let offset = s[sign_at + 1..].replace(':', "");
        let offset = offset.get(0..2)?.parse::<i64>().ok()? * 3600
            + offset.get(2..4)?.parse::<i64>().ok()? * 60;
        let offset = if s[sign_at..].starts_with('-') {
            -offset
        } else {
            offset
        };
        // Days since 1970-01-01 in the proleptic Gregorian calendar (Howard Hinnant's
        // days_from_civil), counting years from March so the leap day ends the year.
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
        let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
        Some(days * 86_400 + secs - offset)
    }
}

#[cfg(test)]
impl Ticket {
    /// A bare ticket in the status Jira calls `status`.
    pub fn for_test(key: &str, status: &str) -> Self {
        Ticket {
            key: key.to_string(),
            summary: key.to_string(),
            status: status.to_string(),
            ..Ticket::default()
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

/// The form every email takes inside the app. Jira, `jira me` and the roster can spell one
/// address in different cases, and Team groups by exact email, so each email is normalized as it
/// comes in (from Jira's issues, the roster and `jira me`) and everything after compares exactly.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// A display name made from an email's local part: `alex.rivera@…` is `Alex Rivera`.
pub fn name_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    local
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = String::new();
                    out.push(first.to_ascii_uppercase());
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    use super::{normalize_email, Status, StatusRules, Ticket, MOVE_SHORTCUTS};

    #[test]
    fn the_move_shortcut_hint_lists_exactly_the_shortcut_keys() {
        let listed: Vec<char> = MOVE_SHORTCUTS.split('/').flat_map(str::chars).collect();
        let readable: Vec<char> = ('a'..='z')
            .filter(|&c| Status::from_move_shortcut(c).is_some())
            .collect();
        let mut sorted = listed.clone();
        sorted.sort();
        assert_eq!(sorted, readable);
        for c in listed {
            assert_eq!(Status::from_move_shortcut(c).unwrap().move_shortcut(), c);
        }
    }

    #[test]
    fn a_name_is_made_from_the_local_part_of_an_email() {
        assert_eq!(
            super::name_from_email("alex.rivera@example.com"),
            "Alex Rivera"
        );
        assert_eq!(super::name_from_email("sam@example.com"), "Sam");
        assert_eq!(super::name_from_email("j..doe@example.com"), "J Doe");
    }

    #[test]
    fn emails_are_compared_without_case_or_padding() {
        assert_eq!(
            normalize_email(" Sam.Chen@Example.COM "),
            "sam.chen@example.com"
        );
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn group_names(rules: &StatusRules, statuses: &[&str]) -> Vec<String> {
        let tickets: Vec<Ticket> = statuses
            .iter()
            .enumerate()
            .map(|(i, status)| Ticket::for_test(&format!("DEMO-{}", i), status))
            .collect();
        rules
            .group(&tickets)
            .into_iter()
            .map(|(status, _)| status)
            .collect()
    }

    fn demo_rules() -> StatusRules {
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
        let rules = demo_rules();
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
        let rules = demo_rules();
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
            Ticket::for_test("DEMO-3", "Closed"),
            Ticket::for_test("DEMO-2", "In Progress"),
            Ticket::for_test("DEMO-9", "On Deck"),
            Ticket::for_test("DEMO-1", "In Progress"),
        ];
        let mut refs: Vec<&Ticket> = tickets.iter().collect();
        rules.sort_tickets(&mut refs);
        let keys: Vec<&str> = refs.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, ["DEMO-1", "DEMO-2", "DEMO-9", "DEMO-3"]);
    }

    #[test]
    fn done_tickets_list_newest_first_with_missing_updates_last_and_ties_in_order() {
        let at = |key: &str, updated: Option<&str>| {
            let mut ticket = Ticket::for_test(key, "Closed");
            ticket.updated = updated.map(|time| format!("2026-09-{time}.000+0000"));
            ticket
        };
        let tickets = [
            at("DEMO-1", None),
            at("DEMO-2", Some("01T00:00:00")),
            at("DEMO-3", Some("30T10:00:00")),
            at("DEMO-4", Some("01T00:00:00")),
            at("DEMO-5", None),
        ];
        let mut refs: Vec<&Ticket> = tickets.iter().collect();
        StatusRules::default().order_done(&mut refs);
        let keys: Vec<&str> = refs.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, ["DEMO-3", "DEMO-2", "DEMO-4", "DEMO-1", "DEMO-5"]);
    }

    #[test]
    fn updated_secs_reads_jiras_timestamp_in_its_offset() {
        let at = |updated: Option<&str>| {
            let mut ticket = Ticket::for_test("DEMO-1", "Closed");
            ticket.updated = updated.map(String::from);
            ticket.updated_secs()
        };
        assert_eq!(at(Some("1970-01-01T00:00:00.000+0000")), Some(0));
        assert_eq!(
            at(Some("2026-09-30T10:23:20.000+0000")),
            Some(1_790_763_800)
        );
        // The same instant written two hours east, and with a colon in the offset.
        assert_eq!(
            at(Some("2026-09-30T12:23:20.000+0200")),
            Some(1_790_763_800)
        );
        assert_eq!(
            at(Some("2026-09-30T05:23:20.000-05:00")),
            Some(1_790_763_800)
        );
        // A leap day, one second before midnight.
        assert_eq!(
            at(Some("2024-02-29T23:59:59.000+0000")),
            Some(1_709_251_199)
        );
        assert_eq!(at(None), None);
        assert_eq!(at(Some("yesterday")), None);
    }
}
