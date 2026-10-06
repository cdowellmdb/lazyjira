//! Jira's issue JSON as a [`Ticket`]. A search result and a single issue share one shape
//! (`{key, fields: {..}}`, holding only the fields asked for), so one pure parser reads both.
//! Neither read asks for the changelog, so a ticket's activity is its comments.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::cache::{normalize_email, ActivityEntry, ActivityKind, Ticket, UNKNOWN_STATUS};

/// One page of a search answer.
#[derive(Debug)]
pub struct SearchPage {
    /// The page's tickets; an issue with no key is skipped.
    pub tickets: Vec<Ticket>,
    /// How many issues Jira sent, skipped ones included: where the next page starts.
    pub sent: usize,
    /// How many issues match the whole search.
    pub total: usize,
}

/// Reads one page of a search answer. An answer with no `total` or `issues` is an error: a
/// missing total would end the search after this page and pass the rest off as complete.
pub fn parse_search_page(body: &str, epic_link_field: Option<&str>) -> Result<SearchPage> {
    let json: Value = serde_json::from_str(body).context("Jira's search answer isn't JSON")?;
    let issues = json["issues"]
        .as_array()
        .context("Jira's search answer has no issues")?;
    let total = json["total"]
        .as_u64()
        .context("Jira's search answer has no total")?;
    Ok(SearchPage {
        tickets: issues
            .iter()
            .filter_map(|issue| ticket_from_issue(issue, epic_link_field))
            .collect(),
        sent: issues.len(),
        total: total as usize,
    })
}

/// The ticket in a Jira issue, `None` when it has no key. Only fields the issue holds are read:
/// a list search leaves description, reporter and activity empty and `detail_loaded` false, and
/// the caller that asked for them marks it loaded.
///
/// `epic_link_field` is the id of the Epic Link custom field (jira-cli's `epic.link`). Without
/// it, or when the ticket has none, a non-sub-task's `parent` is read as its epic.
pub fn ticket_from_issue(issue: &Value, epic_link_field: Option<&str>) -> Option<Ticket> {
    let fields = &issue["fields"];
    let text = |value: &Value| value.as_str().map(str::to_string);
    // A sub-task's parent is its parent ticket, not an epic.
    let parent = text(&fields["parent"]["key"]);
    let (parent_key, epic_parent) = if fields["issuetype"]["subtask"].as_bool() == Some(true) {
        (parent, None)
    } else {
        (None, parent)
    };
    // The epic link is a string, or a list of one on some instances.
    let epic_key = epic_link_field
        .map(|id| &fields[id])
        .and_then(|link| text(link.get(0).unwrap_or(link)))
        .or(epic_parent);
    Some(Ticket {
        key: text(&issue["key"])?,
        summary: text(&fields["summary"]).unwrap_or_default(),
        status: text(&fields["status"]["name"]).unwrap_or_else(|| UNKNOWN_STATUS.to_string()),
        assignee: text(&fields["assignee"]["displayName"]),
        assignee_email: text(&fields["assignee"]["emailAddress"])
            .as_deref()
            .map(normalize_email),
        reporter: text(&fields["reporter"]["displayName"]),
        description: text(&fields["description"]),
        labels: fields["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(text)
            .collect(),
        epic_key,
        epic_name: None,
        parent_key,
        updated: text(&fields["updated"]),
        detail_loaded: false,
        activity: activity(issue),
    })
}

/// A ticket's comments, newest first.
fn activity(issue: &Value) -> Vec<ActivityEntry> {
    let mut activity: Vec<ActivityEntry> = issue["fields"]["comment"]["comments"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|comment| ActivityEntry {
            timestamp: comment["created"].as_str().unwrap_or("").to_string(),
            author: comment["author"]["displayName"]
                .as_str()
                .unwrap_or("Unknown")
                .to_string(),
            author_email: comment["author"]["emailAddress"]
                .as_str()
                .map(str::to_string),
            kind: ActivityKind::Comment {
                body: comment["body"].as_str().unwrap_or("").to_string(),
            },
        })
        .collect();
    activity.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    activity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::ActivityKind;
    use serde_json::json;

    const EPIC_LINK: Option<&str> = Some("customfield_10857");

    fn parse(issue: serde_json::Value) -> Ticket {
        ticket_from_issue(&issue, EPIC_LINK).expect("issue should parse")
    }

    #[test]
    fn reads_what_a_list_search_returns() {
        let ticket = parse(json!({"key": "DEMO-7", "fields": {
            "summary": "Ship it",
            "status": {"name": "Resolved"},
            "assignee": {"displayName": "Sam Doe", "emailAddress": "Sam.Doe@Example.com"},
            "labels": ["checkout", "perf"],
            "updated": "2026-09-30T10:23:20.000+0000",
            "issuetype": {"name": "Story", "subtask": false},
            "parent": null
        }}));
        assert_eq!(ticket.key, "DEMO-7");
        assert_eq!(ticket.summary, "Ship it");
        assert_eq!(ticket.status, "Resolved");
        assert_eq!(ticket.assignee.as_deref(), Some("Sam Doe"));
        // Jira's spelling is normalized on the way in, so it compares exactly with the roster's.
        assert_eq!(
            ticket.assignee_email.as_deref(),
            Some("sam.doe@example.com")
        );
        assert_eq!(ticket.labels, ["checkout", "perf"]);
        assert_eq!(
            ticket.updated.as_deref(),
            Some("2026-09-30T10:23:20.000+0000")
        );
        assert_eq!(ticket.parent_key, None);
        assert_eq!(ticket.epic_key, None);
        // The list search doesn't ask for the detail-only fields, so it never marks them loaded.
        assert!(!ticket.detail_loaded);
        assert_eq!(ticket.description, None);
        assert!(ticket.activity.is_empty());
    }

    #[test]
    fn an_unassigned_ticket_has_no_assignee_or_email() {
        let ticket = parse(json!({"key": "DEMO-8", "fields": {
            "summary": "Nobody yet", "status": {"name": "To Do"}, "assignee": null,
            "labels": []
        }}));
        assert_eq!(ticket.assignee, None);
        assert_eq!(ticket.assignee_email, None);
        assert_eq!(ticket.updated, None);
    }

    #[test]
    fn an_assignee_without_a_visible_email_keeps_the_name_only() {
        let ticket = parse(json!({"key": "DEMO-9", "fields": {
            "summary": "Hidden email", "status": {"name": "To Do"},
            "assignee": {"displayName": "Sam Doe"}
        }}));
        assert_eq!(ticket.assignee.as_deref(), Some("Sam Doe"));
        assert_eq!(ticket.assignee_email, None);
    }

    #[test]
    fn a_sub_tasks_parent_is_its_parent_ticket_never_an_epic() {
        let ticket = parse(json!({"key": "DEMO-11", "fields": {
            "summary": "Step one", "status": {"name": "To Do"},
            "issuetype": {"name": "Sub-task", "subtask": true},
            "parent": {"key": "DEMO-10", "fields": {"issuetype": {"name": "Story"}}}
        }}));
        assert_eq!(ticket.parent_key.as_deref(), Some("DEMO-10"));
        assert_eq!(ticket.epic_key, None);
    }

    #[test]
    fn another_ticket_types_parent_is_its_epic() {
        let ticket = parse(json!({"key": "DEMO-12", "fields": {
            "summary": "Child", "status": {"name": "To Do"},
            "issuetype": {"name": "Story", "subtask": false},
            "parent": {"key": "DEMO-100"}
        }}));
        assert_eq!(ticket.epic_key.as_deref(), Some("DEMO-100"));
        assert_eq!(ticket.parent_key, None);
    }

    #[test]
    fn the_epic_link_field_named_by_jira_cli_gives_the_epic() {
        // Company-managed Jira keeps the epic in a custom field: a plain string here, and an
        // array on some instances. It wins over `parent`.
        for link in [json!("DEMO-200"), json!(["DEMO-200"])] {
            let ticket = parse(json!({"key": "DEMO-13", "fields": {
                "summary": "Child", "status": {"name": "To Do"},
                "customfield_10857": link, "parent": {"key": "DEMO-100"}
            }}));
            assert_eq!(ticket.epic_key.as_deref(), Some("DEMO-200"));
        }
        // With no field configured, other custom fields aren't guessed at.
        let issue = json!({"key": "DEMO-13", "fields": {
            "summary": "Child", "status": {"name": "To Do"}, "customfield_12551": ["DEMO-300"]
        }});
        assert_eq!(ticket_from_issue(&issue, None).unwrap().epic_key, None);
    }

    #[test]
    fn reads_description_reporter_and_comments_newest_first() {
        let ticket = parse(json!({"key": "DEMO-14",
            "fields": {
                "summary": "Detail", "status": {"name": "In Progress"},
                "reporter": {"displayName": "Priya Shah"},
                "description": "h2. Context",
                "comment": {"comments": [
                    {"created": "2026-09-15T09:12:00.000+0000",
                     "author": {"displayName": "Alex Rivera"},
                     "body": "Started."},
                    {"created": "2026-09-22T13:30:00.000+0000",
                     "author": {"displayName": "Priya Shah", "emailAddress": "priya@example.com"},
                     "body": "Looks good."}
                ]}
            }
        }));
        assert_eq!(ticket.reporter.as_deref(), Some("Priya Shah"));
        assert_eq!(ticket.description.as_deref(), Some("h2. Context"));
        assert_eq!(ticket.activity.len(), 2);
        let ActivityKind::Comment { body } = &ticket.activity[0].kind;
        assert_eq!(body, "Looks good.");
        assert_eq!(
            ticket.activity[0].author_email.as_deref(),
            Some("priya@example.com")
        );
        assert_eq!(ticket.activity[1].author, "Alex Rivera");
        assert_eq!(ticket.activity[1].author_email, None);
    }

    #[test]
    fn a_detail_search_page_gives_each_ticket_its_description_comments_and_parent() {
        // The shape of a `key in (...)` search asking for the detail fields: comments come in
        // the order Jira keeps them (oldest first) and each issue holds only its own.
        let body = r#"{"total": 2, "issues": [
            {"key": "DEMO-20", "fields": {
              "summary": "Story", "status": {"name": "In Progress"},
              "issuetype": {"name": "Story", "subtask": false},
              "reporter": {"displayName": "Priya Shah"},
              "description": "h2. Why\nBecause.",
              "labels": ["checkout"],
              "customfield_10857": "DEMO-1",
              "comment": {"total": 3, "comments": [
                {"created": "2026-09-01T09:00:00.000+0000", "author": {"displayName": "Ann"}, "body": "First"},
                {"created": "2026-09-02T09:00:00.000+0000", "author": {"displayName": "Ben"}, "body": "Second"},
                {"created": "2026-09-03T09:00:00.000+0000", "author": {"displayName": "Cy"}, "body": "Third"}]}}},
            {"key": "DEMO-21", "fields": {
              "summary": "Step", "status": {"name": "To Do"},
              "issuetype": {"name": "Sub-task", "subtask": true},
              "parent": {"key": "DEMO-20", "fields": {"issuetype": {"name": "Story"}}},
              "description": null, "comment": {"total": 0, "comments": []}}}]}"#;
        let tickets = parse_search_page(body, EPIC_LINK).unwrap().tickets;

        let story = &tickets[0];
        assert_eq!(story.description.as_deref(), Some("h2. Why\nBecause."));
        assert_eq!(story.reporter.as_deref(), Some("Priya Shah"));
        assert_eq!(story.labels, ["checkout"]);
        assert_eq!(story.epic_key.as_deref(), Some("DEMO-1"));
        // Activity is newest first, which is how the overlay finds the comments to show them
        // oldest first.
        let bodies: Vec<_> = story
            .activity
            .iter()
            .map(|entry| match &entry.kind {
                ActivityKind::Comment { body } => body.as_str(),
            })
            .collect();
        assert_eq!(bodies, ["Third", "Second", "First"]);

        let step = &tickets[1];
        assert_eq!(step.parent_key.as_deref(), Some("DEMO-20"));
        assert_eq!(step.epic_key, None);
        assert_eq!(step.description, None);
        assert!(step.activity.is_empty());
    }

    #[test]
    fn a_search_page_gives_its_tickets_how_many_issues_it_held_and_the_total() {
        let body = r#"{"startAt": 0, "maxResults": 100, "total": 250, "issues": [
            {"key": "DEMO-1", "fields": {"summary": "One", "status": {"name": "To Do"}}},
            {"fields": {"summary": "No key"}},
            {"key": "DEMO-2", "fields": {"summary": "Two", "status": {"name": "Done"}}}]}"#;
        let page = parse_search_page(body, EPIC_LINK).unwrap();
        assert_eq!(page.total, 250);
        // The next page starts after what Jira sent, not after what could be read.
        assert_eq!(page.sent, 3);
        let keys: Vec<_> = page.tickets.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, ["DEMO-1", "DEMO-2"]);
        assert!(parse_search_page("<html>", EPIC_LINK).is_err());
    }

    #[test]
    fn an_answer_without_a_total_or_issues_is_an_error_not_a_short_list() {
        // Without a total a search would stop after its first page, and read as complete.
        let no_total = r#"{"issues": [{"key": "DEMO-1", "fields": {}}]}"#;
        let error = format!("{:#}", parse_search_page(no_total, EPIC_LINK).unwrap_err());
        assert!(error.contains("no total"), "{error}");
        let no_issues = r#"{"total": 3}"#;
        let error = format!("{:#}", parse_search_page(no_issues, EPIC_LINK).unwrap_err());
        assert!(error.contains("no issues"), "{error}");
    }

    #[test]
    fn a_ticket_with_no_status_in_the_answer_reads_as_unknown_not_to_do() {
        let ticket = parse(json!({"key": "DEMO-15", "fields": {"summary": "No status"}}));
        assert_eq!(ticket.status, "Unknown");
    }
}
