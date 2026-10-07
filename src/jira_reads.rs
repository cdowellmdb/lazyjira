//! What a refresh reads from Jira: the one search behind My Work, Team and Unassigned, saved
//! filters, and epics with their children and sub-tasks. The queries are `jql`'s and running
//! them is `jira_search`'s; what a read returns replaces what's shown through `App`'s
//! `replace_cache` and `replace_epics`.

use std::collections::HashMap;

use anyhow::Result;

use crate::cache::{
    name_from_email, normalize_email, roster_member, Cache, Epic, TeamMember, Ticket,
    UNASSIGNED_TEAM_EMAIL, UNASSIGNED_TEAM_NAME,
};
use crate::config::AppConfig;
use crate::jira_client::fetch_my_email;
use crate::jira_search::{search_all, LIST_FIELDS};
use crate::jql::{
    epic_children_jqls, epics_jql, lists_jql, scoped_jql, subtasks_jqls, TicketFetchScope,
};
use crate::local_cache::{
    load_epics_cache, load_my_email, remember_my_email, save_epics_cache, DetailCache,
};
use crate::subtasks;

/// The sub-tasks among `tickets` and under them. Jira doesn't link a sub-task to its parent's
/// epic, so the epic children's search can't find them.
async fn load_subtasks<'a>(tickets: impl Iterator<Item = &'a Ticket>) -> Result<Vec<Ticket>> {
    let keys: Vec<String> = tickets.map(|ticket| ticket.key.clone()).collect();
    search_all(subtasks_jqls(&keys), LIST_FIELDS).await
}

/// Splits one search's tickets into My Work and Team, each ordered by key. Every ticket stays
/// visible: one assigned to someone `roster_member` can't find adds that person to `members`
/// (under their email, or their display name when Jira hides it), and tickets with no assignee
/// become the Unassigned member's. Emails are compared exactly (see `normalize_email`).
fn bucket_tickets(
    mut found: Vec<Ticket>,
    members: &mut Vec<TeamMember>,
    my_email: &str,
) -> (Vec<Ticket>, Vec<Ticket>) {
    found.sort_by(|a, b| key_order(&a.key, &b.key));
    found.dedup_by(|a, b| a.key == b.key);
    let (mut mine, mut team) = (Vec::new(), Vec::new());
    let mut has_unassigned = false;
    for mut ticket in found {
        match ticket.assignee.clone() {
            None => {
                has_unassigned = true;
                ticket.assignee = Some(UNASSIGNED_TEAM_NAME.to_string());
                ticket.assignee_email = Some(UNASSIGNED_TEAM_EMAIL.to_string());
            }
            Some(name) => {
                let email = match roster_member(members, &ticket) {
                    Some(member) => member.email.clone(),
                    None => {
                        let email = ticket.assignee_email.clone().unwrap_or(name.clone());
                        members.push(TeamMember {
                            name,
                            email: email.clone(),
                        });
                        email
                    }
                };
                ticket.assignee_email = Some(email.clone());
                if email == my_email {
                    mine.push(ticket.clone());
                }
            }
        }
        team.push(ticket);
    }
    if has_unassigned {
        members.push(TeamMember {
            name: UNASSIGNED_TEAM_NAME.to_string(),
            email: UNASSIGNED_TEAM_EMAIL.to_string(),
        });
    }
    (mine, team)
}

/// Orders ticket keys as Jira does: by project, then by number (`AMP-9` before `AMP-10`).
fn key_order(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |key: &str| {
        let (project, number) = key.rsplit_once('-').unwrap_or((key, ""));
        (project.to_string(), number.parse::<u64>().ok())
    };
    split(a).cmp(&split(b)).then_with(|| a.cmp(b))
}

/// What an epic row needs from the epic list; the epic's children come from their own search.
const EPIC_FIELDS: &[&str] = &["summary"];

/// The epics, each with the found tickets that name it as their `epic_key`, sorted by key and
/// carrying the epic's name. `listed` is the epic search's answer in any order, and a page
/// boundary can repeat an epic or a child, so each is kept once. A ticket whose epic isn't listed
/// (it's in another project) belongs to no epic here.
fn group_by_epic(mut listed: Vec<Ticket>, mut found: Vec<Ticket>) -> Vec<Epic> {
    listed.sort_by(|a, b| key_order(&a.key, &b.key));
    listed.dedup_by(|a, b| a.key == b.key);
    let mut epics: Vec<Epic> = listed
        .into_iter()
        .map(|epic| Epic {
            key: epic.key,
            summary: epic.summary,
            children: Vec::new(),
        })
        .collect();
    let index: HashMap<String, usize> = epics
        .iter()
        .enumerate()
        .map(|(at, epic)| (epic.key.clone(), at))
        .collect();

    found.sort_by(|a, b| key_order(&a.key, &b.key));
    found.dedup_by(|a, b| a.key == b.key);
    for mut child in found {
        let at = child.epic_key.as_ref().and_then(|key| index.get(key));
        if let Some(&at) = at {
            child.epic_name = Some(epics[at].summary.clone());
            epics[at].children.push(child);
        }
    }
    epics
}

/// Fetch all epics and their children: one search for the epics, one per `KEYS_PER_SEARCH`
/// epics for their children, and one per `KEYS_PER_SEARCH` children for their sub-tasks (see
/// `load_subtasks`). Any failed search fails the refresh, so the last epics stay on screen
/// instead of epics with fewer sub-tasks, and a different progress, replacing them.
async fn fetch_epics(config: &AppConfig) -> Result<Vec<Epic>> {
    use crate::jira_rest::{epic_link_field, search};

    let project = &config.jira.project;
    let listed = search(&epics_jql(project), EPIC_FIELDS).await?;
    let keys: Vec<String> = listed.iter().map(|epic| epic.key.clone()).collect();
    let found = search_all(
        epic_children_jqls(project, &keys, epic_link_field().is_some()),
        LIST_FIELDS,
    )
    .await?;

    let mut epics = group_by_epic(listed, found);
    let subtasks = load_subtasks(epics.iter().flat_map(|epic| &epic.children)).await?;
    subtasks::add_to_epics(&mut epics, &subtasks);
    Ok(epics)
}

fn reconcile_epic_child_statuses(
    epics: &mut [Epic],
    my_tickets: &[Ticket],
    team_tickets: &[Ticket],
) {
    let mut latest_by_key: HashMap<&str, &Ticket> = HashMap::new();

    for ticket in team_tickets.iter().chain(my_tickets) {
        latest_by_key.insert(ticket.key.as_str(), ticket);
    }

    for epic in epics {
        for child in &mut epic.children {
            if let Some(latest) = latest_by_key.get(child.key.as_str()) {
                child.status = latest.status.clone();
            }
        }
    }
}

pub fn attach_epics_to_tickets(
    my_tickets: &mut [Ticket],
    team_tickets: &mut [Ticket],
    epics: &[Epic],
) {
    let epic_by_ticket: HashMap<String, (String, String)> = epics
        .iter()
        .flat_map(|epic| {
            epic.children
                .iter()
                .map(|child| (child.key.clone(), (epic.key.clone(), epic.summary.clone())))
        })
        .collect();
    let name_by_epic: HashMap<&str, &str> = epics
        .iter()
        .map(|epic| (epic.key.as_str(), epic.summary.as_str()))
        .collect();

    let attach_epic = |ticket: &mut Ticket| {
        if let Some(epic_key) = ticket.epic_key.as_deref() {
            // The epic the search read is fresher than the epics cache's children.
            ticket.epic_name = name_by_epic.get(epic_key).map(|name| name.to_string());
        } else if let Some((epic_key, epic_name)) = epic_by_ticket.get(&ticket.key) {
            ticket.epic_key = Some(epic_key.clone());
            ticket.epic_name = Some(epic_name.clone());
        }
    };

    for ticket in my_tickets {
        attach_epic(ticket);
    }
    for ticket in team_tickets {
        attach_epic(ticket);
    }
}

/// Refresh the full epic relationship graph and write it to local cache.
pub async fn refresh_epics_cache(config: &AppConfig) -> Result<Vec<Epic>> {
    let epics = fetch_epics(config).await?;
    save_epics_cache(&config.jira.project, &epics)?;
    Ok(epics)
}

/// Asks `jira me` for the current user's email, and remembers it for later refreshes.
async fn refresh_my_email(project: &str) -> Result<String> {
    let email = normalize_email(&fetch_my_email().await?);
    remember_my_email(project, &email);
    Ok(email)
}

/// The remembered email, so a refresh doesn't wait on `jira me`; asks Jira only the first time.
async fn my_email(project: &str) -> Result<String> {
    match load_my_email(project) {
        Some(email) => Ok(email),
        None => refresh_my_email(project).await,
    }
}

/// Keeps the remembered email current for the next refresh, in the background; a failure keeps
/// the old one. With none remembered the first refresh asks `jira me` itself, so asking here too
/// would run it twice at once.
pub fn keep_my_email_current(project: &str) {
    if load_my_email(project).is_some() {
        let project = project.to_string();
        tokio::spawn(async move {
            let _ = refresh_my_email(&project).await;
        });
    }
}

async fn fetch_with_scope(
    config: &AppConfig,
    scope: TicketFetchScope,
    details: &DetailCache,
) -> Result<Cache> {
    crate::jira_rest::ensure_ready()?;
    let mut team_members = config.team_members();

    let project = &config.jira.project;
    let my_email = my_email(project).await?;
    if !team_members.iter().any(|member| member.email == my_email) {
        team_members.push(TeamMember {
            name: name_from_email(&my_email),
            email: my_email.clone(),
        });
    }
    let mut epics = load_epics_cache(project);

    // A failed search returns here, so the caller keeps showing the last snapshot.
    let emails: Vec<&str> = team_members.iter().map(|m| m.email.as_str()).collect();
    let found = crate::jira_rest::search(&lists_jql(config, &emails, scope), LIST_FIELDS).await?;
    let (mut my_tickets, mut team_tickets) = bucket_tickets(found, &mut team_members, &my_email);

    attach_epics_to_tickets(&mut my_tickets, &mut team_tickets, &epics);
    reconcile_epic_child_statuses(&mut epics, &my_tickets, &team_tickets);

    let mut cache = Cache {
        my_tickets,
        team_tickets,
        epics,
        team_members,
    };
    details.hydrate(&mut cache);
    Ok(cache)
}

/// Fetch active (non-Done) tickets first for fast startup accuracy. The result already holds
/// the details in `details`.
pub async fn fetch_active_only(config: &AppConfig, details: &DetailCache) -> Result<Cache> {
    fetch_with_scope(config, TicketFetchScope::ActiveOnly, details).await
}

/// Fetch active + recently done tickets for a complete cache refresh, with the details in
/// `details` already filled in.
pub async fn fetch_all(config: &AppConfig, details: &DetailCache) -> Result<Cache> {
    fetch_with_scope(config, TicketFetchScope::ActiveAndRecentDone, details).await
}

/// Run an arbitrary JQL query and return matching tickets.
pub async fn fetch_jql_query(config: &AppConfig, jql: &str) -> Result<Vec<Ticket>> {
    crate::jira_rest::search(&scoped_jql(&config.jira.project, jql), LIST_FIELDS).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{JiraConfig, StatusConfig};
    use std::collections::BTreeMap;

    fn test_ticket(key: &str, status: &str) -> Ticket {
        Ticket {
            summary: format!("Summary for {}", key),
            ..Ticket::for_test(key, status)
        }
    }

    fn test_config() -> AppConfig {
        AppConfig {
            jira: JiraConfig {
                project: "AMP".into(),
                team_name: "Code Generation".into(),
                done_window_days: 14,
                epics_i_care_about: vec![],
            },
            team: BTreeMap::new(),
            statuses: StatusConfig {
                active: vec!["In Progress".into(), "To Do".into()],
                done: vec!["Done".into(), "Closed".into()],
            },
            filters: vec![],
            preferences: Default::default(),
            themes: Default::default(),
        }
    }

    fn assigned(key: &str, status: &str, name: &str, email: &str) -> Ticket {
        Ticket {
            assignee: Some(name.to_string()),
            assignee_email: Some(email.to_string()),
            ..test_ticket(key, status)
        }
    }

    fn roster() -> Vec<TeamMember> {
        vec![
            TeamMember {
                name: "Alex Rivera".into(),
                email: "alex.rivera@example.com".into(),
            },
            TeamMember {
                name: "Sam Chen".into(),
                email: "sam.chen@example.com".into(),
            },
        ]
    }

    fn ticket_keys(tickets: &[Ticket]) -> Vec<&str> {
        tickets.iter().map(|t| t.key.as_str()).collect()
    }

    #[test]
    fn one_search_fills_my_work_and_team_by_the_roster_email() {
        let found = vec![
            assigned("AMP-3", "To Do", "Sam", "sam.chen@example.com"),
            assigned("AMP-2", "Done", "Alex", "alex.rivera@example.com"),
            assigned("AMP-1", "To Do", "Alex", "alex.rivera@example.com"),
            test_ticket("AMP-4", "To Do"),
        ];
        let mut members = roster();
        let (mine, team) = bucket_tickets(found, &mut members, "alex.rivera@example.com");
        assert_eq!(ticket_keys(&mine), ["AMP-1", "AMP-2"]);
        assert_eq!(ticket_keys(&team), ["AMP-1", "AMP-2", "AMP-3", "AMP-4"]);
    }

    #[test]
    fn an_email_spelled_differently_by_jira_and_the_roster_still_groups() {
        let mut config = test_config();
        config
            .team
            .insert("Sam Chen".to_string(), "Sam.Chen@Example.com".to_string());
        let mut members = config.team_members();
        let body = r#"{"total": 1, "issues": [{"key": "AMP-1", "fields": {
            "summary": "One", "status": {"name": "To Do"},
            "assignee": {"displayName": "Sam C.", "emailAddress": "SAM.CHEN@example.com"}}}]}"#;
        let found = crate::jira_issue::parse_search_page(body, None)
            .unwrap()
            .tickets;

        let (_, team) = bucket_tickets(found, &mut members, "me@example.com");

        assert_eq!(members.len(), 1, "Sam is already on the roster");
        assert_eq!(
            team[0].assignee_email.as_deref(),
            Some("sam.chen@example.com")
        );
        // The name is Jira's own: a display name that differs from the roster's still groups.
        assert_eq!(team[0].assignee.as_deref(), Some("Sam C."));
    }

    #[test]
    fn an_assignee_whose_email_jira_hides_is_found_on_the_roster_by_display_name() {
        let mut hidden = test_ticket("AMP-1", "To Do");
        hidden.assignee = Some("alex rivera".to_string());
        let mut members = roster();

        let (mine, team) = bucket_tickets(vec![hidden], &mut members, "alex.rivera@example.com");

        assert_eq!(members.len(), 2, "no one new joins the roster");
        assert_eq!(ticket_keys(&mine), ["AMP-1"]);
        assert_eq!(
            team[0].assignee_email.as_deref(),
            Some("alex.rivera@example.com")
        );
    }

    #[test]
    fn display_names_match_whatever_the_case_even_beyond_ascii() {
        let mut members = vec![TeamMember {
            name: "José Álvarez".into(),
            email: "jose@example.com".into(),
        }];
        let mut hidden = test_ticket("AMP-1", "To Do");
        hidden.assignee = Some("JOSÉ ÁLVAREZ".to_string());

        let (_, team) = bucket_tickets(vec![hidden], &mut members, "me@example.com");

        assert_eq!(members.len(), 1, "José is already on the roster");
        assert_eq!(team[0].assignee_email.as_deref(), Some("jose@example.com"));
    }

    #[test]
    fn a_ticket_nobody_on_the_roster_matches_stays_visible_under_its_assignee() {
        // An email the roster doesn't have: the assignee joins Team with that email.
        let stranger = assigned("AMP-5", "To Do", "Pat Doe", "pat@example.com");
        // No email at all: the display name stands in for it, which jira-cli can assign to.
        let mut hidden = test_ticket("AMP-6", "To Do");
        hidden.assignee = Some("Kim Lo".to_string());
        let mut hidden_again = test_ticket("AMP-7", "To Do");
        hidden_again.assignee = Some("Kim Lo".to_string());
        let mut members = roster();

        let (mine, team) = bucket_tickets(
            vec![stranger, hidden, hidden_again],
            &mut members,
            "alex.rivera@example.com",
        );

        assert!(mine.is_empty());
        assert_eq!(ticket_keys(&team), ["AMP-5", "AMP-6", "AMP-7"]);
        let joined: Vec<_> = members[2..]
            .iter()
            .map(|m| (m.name.as_str(), m.email.as_str()))
            .collect();
        assert_eq!(
            joined,
            [("Pat Doe", "pat@example.com"), ("Kim Lo", "Kim Lo")],
            "one member each, however many tickets"
        );
        let grouped: Vec<_> = team
            .iter()
            .map(|t| t.assignee_email.as_deref().unwrap())
            .collect();
        assert_eq!(grouped, ["pat@example.com", "Kim Lo", "Kim Lo"]);
    }

    #[test]
    fn tickets_without_an_assignee_go_to_the_unassigned_row() {
        let mut members = roster();
        let (mine, team) = bucket_tickets(
            vec![test_ticket("AMP-9", "To Do"), test_ticket("AMP-8", "To Do")],
            &mut members,
            "alex.rivera@example.com",
        );
        assert!(mine.is_empty());
        assert_eq!(team[0].assignee.as_deref(), Some("Unassigned"));
        assert_eq!(team[0].assignee_email.as_deref(), Some("__unassigned__"));
        // The Unassigned member is added once, after the roster.
        let emails: Vec<_> = members.iter().map(|m| m.email.as_str()).collect();
        assert_eq!(
            emails,
            [
                "alex.rivera@example.com",
                "sam.chen@example.com",
                "__unassigned__"
            ]
        );
    }

    #[test]
    fn keys_are_ordered_by_project_then_number() {
        let mut found: Vec<Ticket> = ["AMP-10", "AMP-2", "AMP-9", "ABC-100", "AMP-1"]
            .iter()
            .map(|key| test_ticket(key, "To Do"))
            .collect();
        found.sort_by(|a, b| key_order(&a.key, &b.key));
        assert_eq!(
            ticket_keys(&found),
            ["ABC-100", "AMP-1", "AMP-2", "AMP-9", "AMP-10"]
        );
    }

    /// What the children searches answer, as Jira shapes it (the Epic Link field is a plain
    /// string): three children of AMP-100, one found twice (two searches can both match it), one
    /// of AMP-200 through `parent`, one through Epic Link, and one of an epic outside the list.
    const CHILDREN_PAGE: &str = r#"{"total": 7, "issues": [
        {"key": "AMP-3", "fields": {"summary": "Three", "status": {"name": "Resolved"},
          "issuetype": {"name": "Task", "subtask": false}, "customfield_10857": "AMP-100"}},
        {"key": "AMP-1", "fields": {"summary": "One", "status": {"name": "Done"},
          "issuetype": {"name": "Task", "subtask": false}, "customfield_10857": "AMP-100"}},
        {"key": "AMP-2", "fields": {"summary": "Two", "status": {"name": "In Progress"},
          "issuetype": {"name": "Task", "subtask": false}, "customfield_10857": "AMP-100"}},
        {"key": "AMP-2", "fields": {"summary": "Two", "status": {"name": "In Progress"},
          "issuetype": {"name": "Task", "subtask": false}, "customfield_10857": "AMP-100"}},
        {"key": "AMP-4", "fields": {"summary": "Four", "status": {"name": "To Do"},
          "issuetype": {"name": "Story", "subtask": false}, "parent": {"key": "AMP-200"}}},
        {"key": "AMP-5", "fields": {"summary": "Five", "status": {"name": "Done"},
          "issuetype": {"name": "Task", "subtask": false}, "customfield_10857": "AMP-200"}},
        {"key": "AMP-6", "fields": {"summary": "Six", "status": {"name": "Done"},
          "issuetype": {"name": "Task", "subtask": false}, "customfield_10857": "AMP-999"}}]}"#;

    fn children_page() -> Vec<Ticket> {
        crate::jira_issue::parse_search_page(CHILDREN_PAGE, Some("customfield_10857"))
            .unwrap()
            .tickets
    }

    fn epic_tickets(rows: &[(&str, &str)]) -> Vec<Ticket> {
        rows.iter()
            .map(|(key, summary)| Ticket {
                summary: summary.to_string(),
                ..test_ticket(key, "In Progress")
            })
            .collect()
    }

    fn subtask(key: &str, parent: &str, status: &str) -> Ticket {
        Ticket {
            parent_key: Some(parent.to_string()),
            ..test_ticket(key, status)
        }
    }

    #[test]
    fn children_are_grouped_under_the_epic_they_name_with_one_copy_each() {
        let found = children_page();
        // The epic search can answer out of order, and a page boundary can repeat an epic.
        let listed = epic_tickets(&[
            ("AMP-300", "Quiet"),
            ("AMP-200", "Search"),
            ("AMP-100", "Checkout"),
            ("AMP-200", "Search"),
        ]);

        let epics = group_by_epic(listed, found);

        let keys = |epic: &Epic| {
            epic.children
                .iter()
                .map(|t| t.key.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            epics.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            ["AMP-100", "AMP-200", "AMP-300"]
        );
        assert_eq!(epics[0].summary, "Checkout");
        assert_eq!(keys(&epics[0]), ["AMP-1", "AMP-2", "AMP-3"]);
        // A non-sub-task's parent is its epic when no Epic Link says otherwise.
        assert_eq!(keys(&epics[1]), ["AMP-4", "AMP-5"]);
        // AMP-6 names an epic that isn't listed, so it belongs to no epic here.
        assert!(epics[2].children.is_empty());
        for child in &epics[0].children {
            assert_eq!(child.epic_key.as_deref(), Some("AMP-100"));
            assert_eq!(child.epic_name.as_deref(), Some("Checkout"));
        }
    }

    #[test]
    fn epic_progress_counts_the_children_and_their_sub_tasks() {
        let found = children_page();
        let listed = epic_tickets(&[
            ("AMP-100", "Checkout"),
            ("AMP-200", "Search"),
            ("AMP-300", "Quiet"),
        ]);
        let mut epics = group_by_epic(listed, found);
        // AMP-1 gained a done sub-task, AMP-4 an open one: Jira doesn't link them to the epic.
        subtasks::add_to_epics(
            &mut epics,
            &[
                subtask("AMP-7", "AMP-1", "Done"),
                subtask("AMP-8", "AMP-4", "To Do"),
            ],
        );

        let rules = crate::cache::StatusRules::new(
            &["In Progress".to_string(), "To Do".to_string()],
            &["Done".to_string(), "Closed".to_string()],
        );
        // Worked by hand: AMP-100 is AMP-1, AMP-2, AMP-3 and AMP-7, of which AMP-1, AMP-3
        // (Resolved follows Done) and AMP-7 are done; AMP-200 is AMP-4, AMP-5 and AMP-8, of
        // which only AMP-5 is done.
        assert_eq!(
            epics
                .iter()
                .map(|e| (e.total(), e.done_count(&rules)))
                .collect::<Vec<_>>(),
            [(4, 3), (3, 1), (0, 0)]
        );
        assert_eq!(epics[0].progress_pct(&rules), 75.0);
        assert_eq!(epics[2].progress_pct(&rules), 0.0);
        assert_eq!(epics[1].children[2].parent_key.as_deref(), Some("AMP-4"));
    }

    #[test]
    fn reconcile_epic_child_statuses_uses_latest_ticket_status() {
        let mut epics = vec![Epic {
            key: "AMP-100".to_string(),
            summary: "Epic".to_string(),
            children: vec![test_ticket("AMP-1", "To Do")],
        }];
        let mut resolved = test_ticket("AMP-1", "To Do");
        resolved.status = "Resolved".to_string();
        let my_tickets = vec![resolved];
        let team_tickets = vec![test_ticket("AMP-2", "Needs Triage")];

        reconcile_epic_child_statuses(&mut epics, &my_tickets, &team_tickets);

        assert_eq!(epics[0].children[0].status, "Resolved");
    }

    #[test]
    fn reconcile_epic_child_statuses_leaves_unknown_children_unchanged() {
        let mut epics = vec![Epic {
            key: "AMP-100".to_string(),
            summary: "Epic".to_string(),
            children: vec![test_ticket("AMP-1", "To Do")],
        }];

        reconcile_epic_child_statuses(&mut epics, &[], &[]);

        assert_eq!(epics[0].children[0].status, "To Do");
    }

    #[test]
    fn an_epic_key_from_the_search_gets_its_name_from_the_epics() {
        let epics = vec![Epic {
            key: "AMP-100".to_string(),
            summary: "Checkout".to_string(),
            children: vec![test_ticket("AMP-2", "To Do")],
        }];
        let mut searched = test_ticket("AMP-1", "To Do");
        searched.epic_key = Some("AMP-100".to_string());
        // Not a child in the epics cache yet, but the search says which epic it's in.
        let mut mine = vec![searched, test_ticket("AMP-2", "To Do")];
        attach_epics_to_tickets(&mut mine, &mut [], &epics);
        assert_eq!(mine[0].epic_name.as_deref(), Some("Checkout"));
        assert_eq!(mine[1].epic_key.as_deref(), Some("AMP-100"));
        assert_eq!(mine[1].epic_name.as_deref(), Some("Checkout"));

        // The search is fresher than the epics cache, so its epic wins over the cache's.
        let mut moved = test_ticket("AMP-2", "To Do");
        moved.epic_key = Some("AMP-200".to_string());
        let mut mine = vec![moved];
        attach_epics_to_tickets(&mut mine, &mut [], &epics);
        assert_eq!(mine[0].epic_key.as_deref(), Some("AMP-200"));
        assert_eq!(mine[0].epic_name, None);
    }
}
