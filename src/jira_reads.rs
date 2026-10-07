//! What a refresh reads from Jira: the one search behind My Work, Team and Unassigned, saved
//! filters, and epics with their children and sub-tasks. The queries are `jql`'s, running them is
//! `jira_search`'s and shaping the tickets found is `lists`'s; what a read returns replaces
//! what's shown through `App`'s `replace_cache` and `replace_epics`.

use anyhow::Result;

use crate::cache::{name_from_email, normalize_email, Cache, Epic, TeamMember, Ticket};
use crate::config::AppConfig;
use crate::jira_client::fetch_my_email;
use crate::jira_search::{search_all, LIST_FIELDS};
use crate::jql::{
    epic_children_jqls, epics_jql, lists_jql, scoped_jql, subtasks_jqls, TicketFetchScope,
};
use crate::lists::{
    attach_epics_to_tickets, bucket_tickets, group_by_epic, reconcile_epic_child_statuses,
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

/// What an epic row needs from the epic list; the epic's children come from their own search.
const EPIC_FIELDS: &[&str] = &["summary"];

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
