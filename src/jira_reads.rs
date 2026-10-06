//! What lazyjira reads from Jira: the one search behind My Work, Team and Unassigned, saved
//! filters, epics with their children and sub-tasks, and ticket details. Reads go through
//! `jira_rest`'s search, except one ticket's fresh detail, which is read as the issue itself.
//! What a read returns replaces what's shown through `App`'s `replace_cache`, `replace_epics`
//! and `enrich_ticket`.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::bounded::for_each_bounded;
use crate::cache::{
    name_from_email, normalize_email, Cache, Epic, TeamMember, Ticket, UNASSIGNED_TEAM_EMAIL,
    UNASSIGNED_TEAM_NAME,
};
use crate::config::AppConfig;
use crate::jira_client::fetch_my_email;
use crate::jira_rest::describe;
use crate::jql::{self, is_key, key_list, KEYS_PER_SEARCH};
use crate::local_cache::{
    load_epics_cache, load_my_email, remember_my_email, save_epics_cache, DetailCache,
};
use crate::subtasks;

#[derive(Debug, Clone, Copy)]
enum TicketFetchScope {
    ActiveOnly,
    ActiveAndRecentDone,
}

/// How many of a read's searches run at once: a project with 400 epics needs about fifty,
/// which one after another is a minute, and a few at a time is easy on Jira.
const SEARCHES_AT_ONCE: usize = 4;

/// Runs `search` on each of `jqls`, `SEARCHES_AT_ONCE` at a time, and gives each answer to `done`
/// with the position of its query in `jqls`, as it arrives rather than in order. A search that
/// panics answers with an error.
async fn search_each<S, F>(
    jqls: Vec<String>,
    search: S,
    mut done: impl FnMut(usize, Result<Vec<Ticket>>),
) where
    S: Fn(String) -> F,
    F: Future<Output = Result<Vec<Ticket>>> + Send + 'static,
{
    for_each_bounded(SEARCHES_AT_ONCE, jqls, search, |at, joined| {
        done(
            at,
            joined.unwrap_or_else(|e| Err(anyhow!("A search task failed: {e}"))),
        )
    })
    .await
}

/// The tickets every one of `jqls` finds, in the order of the queries. A search that fails stops
/// the queued ones from being sent, and the first failure in query order fails them all (a query
/// sent after the failure is later in that order than the one that failed).
async fn search_in_order<S, F>(jqls: Vec<String>, search: S) -> Result<Vec<Ticket>>
where
    S: Fn(String) -> F,
    F: Future<Output = Result<Vec<Ticket>>> + Send + 'static,
{
    let failed = Arc::new(AtomicBool::new(false));
    let stop_after_failure = |jql: String| {
        let (failed, search) = (failed.clone(), search(jql));
        async move {
            if failed.load(Ordering::SeqCst) {
                anyhow::bail!("not sent, as an earlier search failed");
            }
            let found = search.await;
            if found.is_err() {
                failed.store(true, Ordering::SeqCst);
            }
            found
        }
    };
    let mut answers = Vec::new();
    search_each(jqls, stop_after_failure, |at, found| {
        answers.push((at, found))
    })
    .await;
    answers.sort_by_key(|(at, _)| *at);
    let found = answers
        .into_iter()
        .map(|(_, found)| found)
        .collect::<Result<Vec<_>>>()?;
    Ok(found.concat())
}

/// Every ticket `jqls` find (see `search_in_order`), asking Jira for `fields`.
async fn search_all(jqls: Vec<String>, fields: &'static [&'static str]) -> Result<Vec<Ticket>> {
    search_in_order(jqls, |jql| async move {
        crate::jira_rest::search(&jql, fields).await
    })
    .await
}

/// The searches that find the sub-tasks among `keys` and under them, `KEYS_PER_SEARCH` keys to a
/// search, each key once. A chunk with no usable key sends no search.
fn subtasks_jqls(keys: &[String]) -> Vec<String> {
    let mut keys = keys.to_vec();
    keys.sort();
    keys.dedup();
    keys.chunks(KEYS_PER_SEARCH)
        .filter_map(key_list)
        .map(|list| {
            format!("(key in ({list}) OR parent in ({list})) AND issuetype in subTaskIssueTypes()")
        })
        .collect()
}

/// The sub-tasks among `tickets` and under them. Jira doesn't link a sub-task to its parent's
/// epic, so the epic children's search can't find them.
async fn load_subtasks<'a>(tickets: impl Iterator<Item = &'a Ticket>) -> Result<Vec<Ticket>> {
    let keys: Vec<String> = tickets.map(|ticket| ticket.key.clone()).collect();
    search_all(subtasks_jqls(&keys), LIST_FIELDS).await
}

/// What a detail adds to a list row's fields: what the detail overlay shows. `enrich_ticket`
/// copies the rest over the row.
const DETAIL_ONLY_FIELDS: &[&str] = &["reporter", "description", "comment"];

/// Reads one ticket's detail fresh from Jira: when it's opened, and after a move. It reads the
/// issue itself rather than searching, so it can't miss a change made a moment ago.
pub async fn fetch_ticket_detail(key: &str) -> Result<Ticket> {
    Ok(as_detail(
        crate::jira_rest::issue(key, &detail_fields()).await?,
    ))
}

/// What a detail read asks for: the list fields and the detail-only ones.
fn detail_fields() -> Vec<&'static str> {
    [LIST_FIELDS, DETAIL_ONLY_FIELDS].concat()
}

/// Reads the details of `keys` over Jira's REST search, a chunk of tickets per request. See
/// `read_details`.
pub async fn fetch_ticket_details(
    keys: &[String],
    deliver: impl FnMut(String, Result<Ticket, String>),
) {
    let fields = detail_fields();
    read_details(
        keys,
        |jql| {
            let fields = fields.clone();
            async move { crate::jira_rest::search(&jql, &fields).await }
        },
        deliver,
    )
    .await
}

/// A ticket read with the detail fields, which only a detail read gets, so it counts as loaded.
fn as_detail(mut ticket: Ticket) -> Ticket {
    ticket.detail_loaded = true;
    ticket
}

/// Reads the details of `keys`, `KEYS_PER_SEARCH` at a time (usually one page of results), with
/// `search`, which runs a JQL query, a few chunks at once (`SEARCHES_AT_ONCE`). Each key's
/// outcome goes to `deliver` as its chunk is read. A chunk that fails fails only its keys, so the
/// rest are still read and the failed ones are asked for again later. A key that isn't shaped
/// like a ticket key is refused without being searched, and takes no place in a chunk.
async fn read_details<S, F>(
    keys: &[String],
    search: S,
    mut deliver: impl FnMut(String, Result<Ticket, String>),
) where
    S: Fn(String) -> F,
    F: Future<Output = Result<Vec<Ticket>>> + Send + 'static,
{
    let (valid, malformed): (Vec<String>, Vec<String>) =
        keys.iter().cloned().partition(|key| is_key(key));
    for key in malformed {
        let error = format!("{key:?} isn't a ticket key");
        deliver(key, Err(error));
    }

    let chunks: Vec<&[String]> = valid.chunks(KEYS_PER_SEARCH).collect();
    let jqls = chunks
        .iter()
        .map(|chunk| format!("key in ({})", chunk.join(",")))
        .collect();
    search_each(jqls, search, |at, found| match found {
        Ok(tickets) => {
            let mut found: HashMap<_, _> = tickets
                .into_iter()
                .map(|ticket| (ticket.key.clone(), ticket))
                .collect();
            for key in chunks[at] {
                let result = found
                    .remove(key)
                    .map(as_detail)
                    .ok_or_else(|| "Jira didn't return this ticket".to_string());
                deliver(key.clone(), result);
            }
        }
        Err(e) => {
            let error = describe(&e);
            for key in chunks[at] {
                deliver(key.clone(), Err(error.clone()));
            }
        }
    })
    .await
}

/// What a list row needs. `key` always comes back, and the Epic Link field is added by the
/// search. `issuetype` tells a sub-task's parent from an epic.
const LIST_FIELDS: &[&str] = &[
    "summary",
    "status",
    "assignee",
    "labels",
    "parent",
    "issuetype",
    "updated",
];

/// The one search behind My Work, Team and Unassigned: the active tickets of everyone in
/// `assignee_emails`, plus those done inside the window for the full scope, and the active
/// tickets nobody has taken that are the team's by Assigned Teams. A REST search needs the
/// project and an order spelled out. The order keeps pages stable while tickets change
/// underneath them.
fn lists_jql(config: &AppConfig, assignee_emails: &[&str], scope: TicketFetchScope) -> String {
    let assignees = assignee_emails
        .iter()
        .map(|email| jql::quote(email))
        .collect::<Vec<_>>()
        .join(", ");
    let active = config.active_status_clause();
    let statuses = match scope {
        TicketFetchScope::ActiveOnly => format!("status in {active}"),
        TicketFetchScope::ActiveAndRecentDone => format!(
            "(status in {active} OR (status in {} AND updated >= {}))",
            config.done_status_clause(),
            config.done_window()
        ),
    };
    format!(
        "project = {} AND ((assignee in ({assignees}) AND {statuses}) \
         OR (assignee is EMPTY AND \"Assigned Teams\" = {} AND status in {active})) \
         ORDER BY key",
        jql::quote(&config.jira.project),
        jql::quote(&config.jira.team_name)
    )
}

/// The roster member `ticket` is assigned to: the one with the assignee's email, else the one
/// with their display name, for when Jira hides the email (or the roster has another address).
/// The list search only finds tickets assigned to a roster email, so an email the roster lacks
/// is another address of someone on it, and the name is how to tell who.
fn roster_member<'a>(members: &'a [TeamMember], ticket: &Ticket) -> Option<&'a TeamMember> {
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

/// The searches that find the children of `epic_keys`, `KEYS_PER_SEARCH` epics to a search. A
/// child names its epic through the Epic Link field (company-managed projects, when jira-cli's
/// config knows the field) or `parent` (team-managed). A chunk with no usable key sends no
/// search. The order keeps pages stable while tickets change underneath them.
fn epic_children_jqls(project: &str, epic_keys: &[String], has_epic_link: bool) -> Vec<String> {
    epic_keys
        .chunks(KEYS_PER_SEARCH)
        .filter_map(key_list)
        .map(|list| {
            let link = if has_epic_link {
                format!("\"Epic Link\" in ({list}) OR ")
            } else {
                String::new()
            };
            format!(
                "project = {} AND ({link}parent in ({list})) ORDER BY key",
                jql::quote(project)
            )
        })
        .collect()
}

/// A saved filter's `jql` limited to `project`, with an order. The filter's own ORDER BY is
/// kept (after the project, outside the parentheses); without one, newest first.
fn scoped_jql(project: &str, jql: &str) -> String {
    // ponytail: the last "order by" is taken as the clause, so one inside a quoted string
    // fails the query; parse JQL properly if filters ever need it.
    let split = jql.to_ascii_lowercase().rfind("order by");
    let (condition, order) = match split {
        Some(at) => (jql[..at].trim(), jql[at..].trim()),
        None => (jql.trim(), "ORDER BY created DESC"),
    };
    let project = jql::quote(project);
    if condition.is_empty() {
        format!("project = {project} {order}")
    } else {
        format!("project = {project} AND ({condition}) {order}")
    }
}

/// All the project's epics, in an order that keeps pages stable while epics change underneath
/// them.
fn epics_jql(project: &str) -> String {
    format!(
        "project = {} AND issuetype = Epic ORDER BY key",
        jql::quote(project)
    )
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
    crate::jira_rest::ready()?;
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

    #[test]
    fn the_epic_list_is_every_epic_in_the_project() {
        assert_eq!(
            epics_jql("AMP"),
            "project = \"AMP\" AND issuetype = Epic ORDER BY key"
        );
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

    #[test]
    fn the_list_search_covers_the_roster_and_the_teams_unassigned_work() {
        let config = test_config();
        let emails = ["alex@example.com", "sam@example.com"];
        assert_eq!(
            lists_jql(&config, &emails, TicketFetchScope::ActiveOnly),
            "project = \"AMP\" AND (\
             (assignee in (\"alex@example.com\", \"sam@example.com\") \
              AND status in (\"In Progress\", \"To Do\")) \
             OR (assignee is EMPTY AND \"Assigned Teams\" = \"Code Generation\" \
              AND status in (\"In Progress\", \"To Do\"))) ORDER BY key"
        );
    }

    #[test]
    fn the_full_list_search_adds_recently_done_tickets_inside_the_window() {
        let mut config = test_config();
        config.jira.done_window_days = 7;
        let jql = lists_jql(
            &config,
            &["alex@example.com"],
            TicketFetchScope::ActiveAndRecentDone,
        );
        assert_eq!(
            jql,
            "project = \"AMP\" AND (\
             (assignee in (\"alex@example.com\") AND (status in (\"In Progress\", \"To Do\") \
              OR (status in (\"Done\", \"Closed\") AND updated >= -7d))) \
             OR (assignee is EMPTY AND \"Assigned Teams\" = \"Code Generation\" \
              AND status in (\"In Progress\", \"To Do\"))) ORDER BY key"
        );
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

    fn epic_keys(count: usize) -> Vec<String> {
        (1..=count).map(|n| format!("AMP-{n}")).collect()
    }

    #[test]
    fn epic_children_are_searched_fifty_epics_at_a_time() {
        // Exactly 50 epics: one search holding all of them.
        let one = epic_children_jqls("AMP", &epic_keys(50), true);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0], {
            let list = epic_keys(50).join(",");
            format!(
                "project = \"AMP\" AND (\"Epic Link\" in ({list}) OR parent in ({list})) \
                 ORDER BY key"
            )
        });
        // 51 epics: a second search that holds only the 51st.
        let two = epic_children_jqls("AMP", &epic_keys(51), true);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0], one[0]);
        assert_eq!(
            two[1],
            "project = \"AMP\" AND (\"Epic Link\" in (AMP-51) OR parent in (AMP-51)) \
             ORDER BY key"
        );
        assert!(epic_children_jqls("AMP", &[], true).is_empty());
    }

    #[test]
    fn without_an_epic_link_field_only_the_parent_link_is_searched() {
        assert_eq!(
            epic_children_jqls("AMP", &epic_keys(2), false),
            ["project = \"AMP\" AND (parent in (AMP-1,AMP-2)) ORDER BY key"]
        );
    }

    #[test]
    fn epic_keys_that_are_not_shaped_like_keys_stay_out_of_the_query() {
        let keys = vec!["AMP-1".to_string(), "x\") OR 1=1".to_string()];
        assert_eq!(
            epic_children_jqls("AMP", &keys, false),
            ["project = \"AMP\" AND (parent in (AMP-1)) ORDER BY key"]
        );
        // A chunk with nothing left to ask about sends no search.
        assert!(epic_children_jqls("AMP", &["no good".to_string()], false).is_empty());
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
    fn a_saved_filter_is_limited_to_the_project_and_keeps_its_own_order() {
        assert_eq!(
            scoped_jql("AMP", "type = Bug AND assignee = currentUser()"),
            "project = \"AMP\" AND (type = Bug AND assignee = currentUser()) \
             ORDER BY created DESC"
        );
        // The filter's own ordering wins, and stays outside the parentheses.
        assert_eq!(
            scoped_jql("AMP", "status = Blocked order by updated ASC"),
            "project = \"AMP\" AND (status = Blocked) order by updated ASC"
        );
        assert_eq!(
            scoped_jql("AMP", "ORDER BY priority DESC"),
            "project = \"AMP\" ORDER BY priority DESC"
        );
        assert_eq!(
            scoped_jql("AMP", "  "),
            "project = \"AMP\" ORDER BY created DESC"
        );
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

    fn keys(range: std::ops::RangeInclusive<u32>) -> Vec<String> {
        range.map(|n| format!("DEMO-{n}")).collect()
    }

    /// Runs `read_details` with a Jira that answers each search with `answer(jql)`, returning
    /// the searches made and what each key got.
    async fn read(
        keys: &[String],
        answer: impl Fn(&str) -> Result<Vec<Ticket>>,
    ) -> (Vec<String>, Vec<(String, Result<Ticket, String>)>) {
        let searches = std::cell::RefCell::new(Vec::new());
        let mut delivered = Vec::new();
        read_details(
            keys,
            |jql| {
                let result = answer(&jql);
                searches.borrow_mut().push(jql);
                async move { result }
            },
            |key, result| delivered.push((key, result)),
        )
        .await;
        (searches.into_inner(), delivered)
    }

    #[tokio::test]
    async fn details_are_read_with_one_key_search_and_marked_loaded() {
        let (searches, delivered) = read(&keys(1..=2), |_| {
            Ok(vec![
                test_ticket("DEMO-2", "Done"),
                test_ticket("DEMO-1", "To Do"),
            ])
        })
        .await;

        assert_eq!(searches, ["key in (DEMO-1,DEMO-2)"]);
        // Each key gets its own result, in the order asked, whatever order Jira answered in.
        let got: Vec<_> = delivered
            .iter()
            .map(|(key, result)| (key.as_str(), result.as_ref().unwrap().detail_loaded))
            .collect();
        assert_eq!(got, [("DEMO-1", true), ("DEMO-2", true)]);
        assert_eq!(delivered[1].1.as_ref().unwrap().status, "Done");
    }

    /// A Jira that has every ticket asked for.
    fn has_all(jql: &str) -> Result<Vec<Ticket>> {
        let list = jql
            .strip_prefix("key in (")
            .unwrap()
            .strip_suffix(')')
            .unwrap();
        Ok(list
            .split(',')
            .map(|key| test_ticket(key, "To Do"))
            .collect())
    }

    #[tokio::test]
    async fn details_are_searched_fifty_keys_at_a_time() {
        // Exactly 50 keys fit one search.
        let (searches, delivered) = read(&keys(1..=50), has_all).await;
        assert_eq!(searches, [format!("key in ({})", keys(1..=50).join(","))]);
        assert_eq!(delivered.len(), 50);

        // The 51st key starts a second search that holds only it.
        let (searches, delivered) = read(&keys(1..=51), has_all).await;
        assert_eq!(
            searches,
            [
                format!("key in ({})", keys(1..=50).join(",")),
                "key in (DEMO-51)".to_string()
            ]
        );
        assert_eq!(delivered.len(), 51);
        assert!(delivered.iter().all(|(_, result)| result.is_ok()));

        // Nothing to read, nothing to search.
        let (searches, delivered) = read(&[], has_all).await;
        assert!(searches.is_empty() && delivered.is_empty());
    }

    #[tokio::test]
    async fn a_malformed_key_takes_no_place_in_a_chunk() {
        // 50 usable keys and a malformed one: still one search of exactly the 50.
        let mut asked = keys(1..=50);
        asked.insert(10, "no good".to_string());
        let (searches, delivered) = read(&asked, has_all).await;
        assert_eq!(searches, [format!("key in ({})", keys(1..=50).join(","))]);
        assert_eq!(delivered.len(), 51);

        // The 51st usable key starts the second search, alone.
        asked.push("DEMO-51".to_string());
        let (searches, _) = read(&asked, has_all).await;
        assert_eq!(
            searches,
            [
                format!("key in ({})", keys(1..=50).join(",")),
                "key in (DEMO-51)".to_string()
            ]
        );
    }

    /// A search for jql "n" that answers one ticket, T-n, after a while: the earlier the query
    /// the longer it takes, so answers arrive out of order. `running` and `most` count how many
    /// searches are at it at once, and the most there were.
    fn slow_search(
        running: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
        most: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
        jql: String,
    ) -> impl Future<Output = Result<Vec<Ticket>>> + Send + 'static {
        use std::sync::atomic::Ordering::SeqCst;
        let (running, most) = (running.clone(), most.clone());
        async move {
            let now = running.fetch_add(1, SeqCst) + 1;
            most.fetch_max(now, SeqCst);
            let n: u64 = jql.parse().unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(4 * (12 - n.min(11)))).await;
            running.fetch_sub(1, SeqCst);
            if jql == "7" {
                anyhow::bail!("Jira answered 503.");
            }
            Ok(vec![test_ticket(&format!("T-{jql}"), "To Do")])
        }
    }

    /// Runs `count` queries ("0", "1", ...) through `search_in_order`, skipping the one that
    /// fails unless `count` is past it, returning what it found and how many ran at once.
    async fn run_queries(count: usize) -> (Result<Vec<Ticket>>, usize) {
        let running = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let most = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let jqls = (0..count).map(|n| n.to_string()).collect();
        let found = search_in_order(jqls, |jql| slow_search(&running, &most, jql)).await;
        (found, most.load(std::sync::atomic::Ordering::SeqCst))
    }

    #[tokio::test]
    async fn searches_run_four_at_a_time_and_answer_in_the_order_asked() {
        // Fewer queries than the limit all run at once; exactly the limit does too.
        assert_eq!(run_queries(3).await.1, 3);
        assert_eq!(run_queries(4).await.1, 4);
        // The fifth waits for one of the four to finish, so never more than four are at it.
        let (found, most) = run_queries(5).await;
        assert_eq!(most, 4);
        assert_eq!(
            ticket_keys(&found.unwrap()),
            ["T-0", "T-1", "T-2", "T-3", "T-4"],
            "in query order although the earlier ones finish last"
        );
        // Seven queries (0 to 6) are answered in order too, still four at a time.
        let (found, most) = run_queries(7).await;
        assert_eq!(most, 4);
        assert_eq!(
            ticket_keys(&found.unwrap()),
            ["T-0", "T-1", "T-2", "T-3", "T-4", "T-5", "T-6"]
        );
        // No queries, nothing to run.
        assert!(run_queries(0).await.0.unwrap().is_empty());
    }

    #[tokio::test]
    async fn one_failed_search_fails_them_all_and_the_first_in_query_order_is_reported() {
        // Query "7" fails after a while, and nothing is found for the read.
        let (found, _) = run_queries(10).await;
        assert_eq!(found.unwrap_err().to_string(), "Jira answered 503.");
    }

    /// A search for jql "n" that records it was sent, then answers T-n after 20 ms, except "1",
    /// which fails after 5 ms.
    fn recorded_search(
        sent: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        jql: String,
    ) -> impl Future<Output = Result<Vec<Ticket>>> + Send + 'static {
        let sent = sent.clone();
        async move {
            sent.lock().unwrap().push(jql.clone());
            if jql == "1" {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                anyhow::bail!("Jira answered 401.");
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Ok(vec![test_ticket(&format!("T-{jql}"), "To Do")])
        }
    }

    #[tokio::test]
    async fn a_failed_search_stops_the_queued_ones_from_being_sent() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let jqls = (0..10).map(|n| n.to_string()).collect();

        let found = search_in_order(jqls, |jql| recorded_search(&sent, jql)).await;

        assert_eq!(found.unwrap_err().to_string(), "Jira answered 401.");
        // The first four were out together when "1" failed; the other six never were.
        let mut sent = sent.lock().unwrap().clone();
        sent.sort();
        assert_eq!(sent, ["0", "1", "2", "3"]);
    }

    #[tokio::test]
    async fn a_search_that_panics_fails_the_read_instead_of_hanging_it() {
        let jqls = vec!["0".to_string(), "1".to_string()];

        let found = search_in_order(jqls, |jql| async move {
            if jql == "1" {
                panic!("a parser bug");
            }
            Ok(vec![test_ticket("T-0", "To Do")])
        })
        .await;

        let error = format!("{:#}", found.unwrap_err());
        assert!(error.contains("search task"), "{error}");
    }

    #[tokio::test]
    async fn a_failed_chunk_fails_only_its_keys_and_the_next_chunk_is_still_read() {
        let (searches, delivered) = read(&keys(1..=51), |jql| {
            if jql.contains("DEMO-1,") {
                Err(anyhow::anyhow!("Jira answered 503."))
            } else {
                has_all(jql)
            }
        })
        .await;

        assert_eq!(searches.len(), 2);
        for (key, result) in &delivered[..50] {
            assert_eq!(result.as_ref().unwrap_err(), "Jira answered 503.", "{key}");
        }
        assert_eq!(delivered[50].0, "DEMO-51");
        assert!(delivered[50].1.is_ok());
    }

    #[tokio::test]
    async fn a_key_jira_leaves_out_fails_alone_and_a_malformed_key_is_never_searched() {
        let (searches, delivered) = read(
            &["DEMO-1".into(), "DEMO-2".into(), "x\") OR 1=1".into()],
            |_| Ok(vec![test_ticket("DEMO-1", "To Do")]),
        )
        .await;

        assert_eq!(searches, ["key in (DEMO-1,DEMO-2)"]);
        // The malformed key is refused before anything is searched.
        assert_eq!(
            delivered[0].1.as_ref().unwrap_err(),
            "\"x\\\") OR 1=1\" isn't a ticket key"
        );
        assert_eq!(delivered[1].0, "DEMO-1");
        assert!(delivered[1].1.is_ok());
        assert_eq!(delivered[2].0, "DEMO-2");
        assert_eq!(
            delivered[2].1.as_ref().unwrap_err(),
            "Jira didn't return this ticket"
        );

        // A chunk of nothing but malformed keys makes no search at all.
        let (searches, delivered) = read(&["no good".into()], has_all).await;
        assert!(searches.is_empty());
        assert_eq!(
            delivered[0].1.as_ref().unwrap_err(),
            "\"no good\" isn't a ticket key"
        );
    }

    #[test]
    fn sub_tasks_are_searched_fifty_keys_at_a_time() {
        let sub_task_search = |list: String| {
            format!("(key in ({list}) OR parent in ({list})) AND issuetype in subTaskIssueTypes()")
        };
        // Zero-padded, so the order the searches sort the keys into is the order of the numbers.
        let padded = |range: std::ops::RangeInclusive<u32>| -> Vec<String> {
            range.map(|n| format!("AMP-{n:03}")).collect()
        };
        // Exactly 50 keys: one search holding all of them.
        let fifty = subtasks_jqls(&padded(1..=50));
        assert_eq!(fifty, [sub_task_search(padded(1..=50).join(","))]);
        // 51 keys: a second search that holds only the 51st, however the keys arrive: each
        // key is searched once, even when it is given twice and out of order.
        let mut shuffled = padded(1..=51);
        shuffled.reverse();
        shuffled.extend(padded(1..=51));
        assert_eq!(
            subtasks_jqls(&shuffled),
            [
                sub_task_search(padded(1..=50).join(",")),
                sub_task_search("AMP-051".to_string())
            ]
        );
        // Anything that isn't shaped like a key stays out; nothing left, no search.
        let keys = vec!["AMP-1".to_string(), "x\") OR 1=1".to_string()];
        assert_eq!(subtasks_jqls(&keys), [sub_task_search("AMP-1".to_string())]);
        assert!(subtasks_jqls(&["no good".to_string()]).is_empty());
        assert!(subtasks_jqls(&[]).is_empty());
    }

    #[test]
    fn jql_built_from_config_cannot_be_broken_by_a_quote_in_it() {
        let mut config = test_config();
        config.jira.team_name = r#"Team "A""#.into();
        config.statuses.active = vec![r#"On "Hold""#.into()];
        let jql = lists_jql(&config, &["a@example.com"], TicketFetchScope::ActiveOnly);
        assert!(jql.contains(r#""Assigned Teams" = "Team \"A\"""#), "{jql}");
        assert!(jql.contains(r#"status in ("On \"Hold\"")"#), "{jql}");
        assert_eq!(
            epics_jql(r#"A"B"#),
            r#"project = "A\"B" AND issuetype = Epic ORDER BY key"#
        );
        assert_eq!(
            scoped_jql(r#"A"B"#, "type = Bug"),
            r#"project = "A\"B" AND (type = Bug) ORDER BY created DESC"#
        );
        assert_eq!(
            epic_children_jqls(r#"A"B"#, &epic_keys(1), false),
            [r#"project = "A\"B" AND (parent in (AMP-1)) ORDER BY key"#]
        );
    }
}
