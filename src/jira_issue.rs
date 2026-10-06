//! Jira's issue JSON as a [`Ticket`]. A search result and `jira issue view --raw` share one shape
//! (`{key, fields: {..}}`, the search holding only the fields it asked for), so one pure parser
//! reads both.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::cache::{ActivityEntry, ActivityKind, Ticket};

/// One page of a search answer: its tickets (an issue with no key is skipped) and the total
/// number of matches.
pub fn parse_search_page(
    body: &str,
    epic_link_field: Option<&str>,
) -> Result<(Vec<Ticket>, usize)> {
    let json: Value = serde_json::from_str(body).context("Jira's search answer isn't JSON")?;
    let tickets = json["issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|issue| ticket_from_issue(issue, epic_link_field))
        .collect();
    let total = json["total"].as_u64().unwrap_or(0) as usize;
    Ok((tickets, total))
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
        status: text(&fields["status"]["name"]).unwrap_or_else(|| "To Do".to_string()),
        assignee: text(&fields["assignee"]["displayName"]),
        assignee_email: text(&fields["assignee"]["emailAddress"]),
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

/// Field changes (`changelog.histories`, present when the issue was fetched with the changelog)
/// and comments, newest first.
fn activity(issue: &Value) -> Vec<ActivityEntry> {
    let text = |value: &Value| value.as_str().map(str::to_string);
    let author = |entry: &Value| {
        (
            entry["author"]["displayName"]
                .as_str()
                .unwrap_or("Unknown")
                .to_string(),
            text(&entry["author"]["emailAddress"]),
        )
    };
    let mut activity = Vec::new();

    for history in issue["changelog"]["histories"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let (author, author_email) = author(history);
        for item in history["items"].as_array().into_iter().flatten() {
            let field = item["field"].as_str().unwrap_or("");
            let from = item["fromString"].as_str().unwrap_or("").to_string();
            let to = item["toString"].as_str().unwrap_or("").to_string();
            let kind = match field {
                "status" => ActivityKind::StatusChange { from, to },
                "assignee" => ActivityKind::AssigneeChange {
                    from: Some(from).filter(|s| !s.is_empty()),
                    to: Some(to).filter(|s| !s.is_empty()),
                },
                _ => ActivityKind::FieldChange {
                    field: field.to_string(),
                    from,
                    to,
                },
            };
            activity.push(ActivityEntry {
                timestamp: history["created"].as_str().unwrap_or("").to_string(),
                author: author.clone(),
                author_email: author_email.clone(),
                kind,
            });
        }
    }

    for comment in issue["fields"]["comment"]["comments"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let (author, author_email) = author(comment);
        activity.push(ActivityEntry {
            timestamp: comment["created"].as_str().unwrap_or("").to_string(),
            author,
            author_email,
            kind: ActivityKind::Comment {
                body: comment["body"].as_str().unwrap_or("").to_string(),
            },
        });
    }

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
        assert_eq!(
            ticket.assignee_email.as_deref(),
            Some("Sam.Doe@Example.com")
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
    fn reads_description_reporter_and_activity_newest_first() {
        let ticket = parse(json!({"key": "DEMO-14",
            "fields": {
                "summary": "Detail", "status": {"name": "In Progress"},
                "reporter": {"displayName": "Priya Shah"},
                "description": "h2. Context",
                "comment": {"comments": [
                    {"created": "2026-09-22T13:30:00.000+0000",
                     "author": {"displayName": "Priya Shah", "emailAddress": "priya@example.com"},
                     "body": "Looks good."}
                ]}
            },
            "changelog": {"histories": [
                {"created": "2026-09-15T09:12:00.000+0000",
                 "author": {"displayName": "Alex Rivera"},
                 "items": [
                    {"field": "status", "fromString": "To Do", "toString": "In Progress"},
                    {"field": "assignee", "fromString": "", "toString": "Alex Rivera"},
                    {"field": "priority", "fromString": "Low", "toString": "High"}
                 ]}
            ]}
        }));
        assert_eq!(ticket.reporter.as_deref(), Some("Priya Shah"));
        assert_eq!(ticket.description.as_deref(), Some("h2. Context"));
        assert_eq!(ticket.activity.len(), 4);
        match &ticket.activity[0].kind {
            ActivityKind::Comment { body } => assert_eq!(body, "Looks good."),
            other => panic!("newest entry should be the comment, got {other:?}"),
        }
        assert_eq!(
            ticket.activity[0].author_email.as_deref(),
            Some("priya@example.com")
        );
        assert!(matches!(
            &ticket.activity[1].kind,
            ActivityKind::StatusChange { from, to } if from == "To Do" && to == "In Progress"
        ));
        assert!(matches!(
            &ticket.activity[2].kind,
            ActivityKind::AssigneeChange { from: None, to: Some(to) } if to == "Alex Rivera"
        ));
        assert!(matches!(
            &ticket.activity[3].kind,
            ActivityKind::FieldChange { field, .. } if field == "priority"
        ));
        assert_eq!(ticket.activity[1].author, "Alex Rivera");
    }

    #[test]
    fn a_search_page_gives_its_tickets_and_the_total() {
        let body = r#"{"startAt": 0, "maxResults": 100, "total": 250, "issues": [
            {"key": "DEMO-1", "fields": {"summary": "One", "status": {"name": "To Do"}}},
            {"fields": {"summary": "No key"}},
            {"key": "DEMO-2", "fields": {"summary": "Two", "status": {"name": "Done"}}}]}"#;
        let (tickets, total) = parse_search_page(body, EPIC_LINK).unwrap();
        assert_eq!(total, 250);
        let keys: Vec<_> = tickets.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, ["DEMO-1", "DEMO-2"]);
        assert!(parse_search_page("<html>", EPIC_LINK).is_err());
    }
}
