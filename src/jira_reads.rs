//! What a refresh reads from Jira: the one search behind My Work, Team and Unassigned, saved
//! filters, and epics with their children and sub-tasks. The queries are `jql`'s, running them is
//! `jira_search`'s and shaping the tickets found is `lists`'s; what a read returns replaces
//! what's shown through `App`'s `replace_cache` and `replace_epics`.

use std::future::Future;

use anyhow::Result;

use crate::cache::{name_from_email, normalize_email, Cache, Epic, TeamMember, Ticket};
use crate::config::AppConfig;
use crate::jira_client::fetch_my_email;
use crate::jira_search::{search_in_order, LIST_FIELDS};
use crate::jql::{
    children_by_epic_link_jql, children_by_parent_jqls, epics_jql, lists_jql, scoped_jql,
    subtasks_jql, TicketFetchScope,
};
use crate::lists::{
    attach_epics_to_tickets, bucket_tickets, group_by_epic, reconcile_epic_child_statuses,
};
use crate::local_cache::{
    load_epics_cache, load_my_email, remember_my_email, save_epics_cache, DetailCache,
};
use crate::subtasks;

/// What an epic row needs from the epic list; the epic's children come from their own searches.
const EPIC_FIELDS: &[&str] = &["summary"];

/// All the project's epics with their children, read through `search` (a JQL query and the
/// fields to ask for). Three reads go out at once: the epics, followed by the tickets naming one
/// of them as `parent` (`children_by_parent_jqls`); every ticket with an Epic Link, when
/// `has_epic_link` (jira-cli's config knows the field); and every sub-task, which Jira doesn't
/// link to an epic and `subtasks::add_to_epics` places under their parents. The last two cover
/// the whole project, so they wait for nobody's keys. Any failed search fails the read, so the
/// last epics stay on screen instead of epics with fewer children, and a different progress,
/// replacing them.
async fn read_epics<S, F>(project: &str, has_epic_link: bool, search: S) -> Result<Vec<Epic>>
where
    S: Fn(String, &'static [&'static str]) -> F,
    F: Future<Output = Result<Vec<Ticket>>> + Send + 'static,
{
    let listed_with_parent_children = async {
        let listed = search(epics_jql(project), EPIC_FIELDS).await?;
        let keys: Vec<String> = listed.iter().map(|epic| epic.key.clone()).collect();
        let by_parent = search_in_order(children_by_parent_jqls(project, &keys), |jql| {
            search(jql, LIST_FIELDS)
        })
        .await?;
        Ok((listed, by_parent))
    };
    let epic_link_children = async {
        if has_epic_link {
            search(children_by_epic_link_jql(project), LIST_FIELDS).await
        } else {
            Ok(Vec::new())
        }
    };
    let subtasks = search(subtasks_jql(project), LIST_FIELDS);
    let ((listed, mut children), by_epic_link, subtasks) =
        tokio::try_join!(listed_with_parent_children, epic_link_children, subtasks)?;

    children.extend(by_epic_link);
    let mut epics = group_by_epic(listed, children);
    subtasks::add_to_epics(&mut epics, &subtasks);
    Ok(epics)
}

/// Fetch all epics and their children from Jira (see `read_epics`).
async fn fetch_epics(config: &AppConfig) -> Result<Vec<Epic>> {
    let has_epic_link = crate::jira_rest::epic_link_field().is_some();
    read_epics(
        &config.jira.project,
        has_epic_link,
        |jql, fields| async move { crate::jira_rest::search(&jql, fields).await },
    )
    .await
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
    use std::sync::Mutex;
    use std::time::Duration;

    const EPICS: &str = "project = \"DEMO\" AND issuetype = Epic ORDER BY key";
    const BY_PARENT: &str = "project = \"DEMO\" AND parent in (DEMO-1,DEMO-2) ORDER BY key";
    const BY_EPIC_LINK: &str = "project = \"DEMO\" AND \"Epic Link\" is not EMPTY ORDER BY key";
    const SUBTASKS: &str = "project = \"DEMO\" AND issuetype in subTaskIssueTypes() ORDER BY key";

    fn child(key: &str, epic: &str) -> Ticket {
        Ticket {
            epic_key: Some(epic.to_string()),
            ..Ticket::for_test(key, "To Do")
        }
    }

    fn subtask(key: &str, parent: &str) -> Ticket {
        Ticket {
            parent_key: Some(parent.to_string()),
            ..Ticket::for_test(key, "To Do")
        }
    }

    /// What a made-up Jira answers each search with: two epics, a child of each (one by Epic
    /// Link and `parent` both, one by `parent`), a child of another project's epic, and two
    /// sub-tasks, one of them under a ticket that has no epic.
    fn answer(jql: &str) -> Result<Vec<Ticket>> {
        Ok(match jql {
            EPICS => vec![
                Ticket::for_test("DEMO-1", "To Do"),
                Ticket::for_test("DEMO-2", "To Do"),
            ],
            BY_EPIC_LINK => vec![child("DEMO-10", "DEMO-1"), child("DEMO-30", "OTHER-1")],
            // DEMO-10 names its epic both ways, and is its child once.
            BY_PARENT => vec![child("DEMO-20", "DEMO-2"), child("DEMO-10", "DEMO-1")],
            SUBTASKS => vec![subtask("DEMO-11", "DEMO-10"), subtask("DEMO-41", "DEMO-40")],
            other => anyhow::bail!("unexpected search: {other}"),
        })
    }

    /// Reads the epics from a Jira that takes 10 ms to answer each search with `answer`,
    /// returning what was read, the searches sent and how long the read took.
    async fn read(
        has_epic_link: bool,
        answer: impl Fn(&str) -> Result<Vec<Ticket>>,
    ) -> (
        Result<Vec<Epic>>,
        Vec<(String, &'static [&'static str])>,
        Duration,
    ) {
        let sent = Mutex::new(Vec::new());
        let started = tokio::time::Instant::now();
        let epics = read_epics("DEMO", has_epic_link, |jql, fields| {
            let found = answer(&jql);
            sent.lock().unwrap().push((jql, fields));
            async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                found
            }
        })
        .await;
        let mut sent = sent.into_inner().unwrap();
        sent.sort();
        (epics, sent, started.elapsed())
    }

    fn keys(epics: &[Epic]) -> Vec<(&str, Vec<&str>)> {
        epics
            .iter()
            .map(|epic| {
                let children = epic.children.iter().map(|child| child.key.as_str());
                (epic.key.as_str(), children.collect())
            })
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn epics_gather_their_children_by_epic_link_and_parent_and_those_childrens_sub_tasks() {
        let (epics, sent, took) = read(true, answer).await;

        assert_eq!(
            keys(&epics.unwrap()),
            [
                ("DEMO-1", vec!["DEMO-10", "DEMO-11"]),
                ("DEMO-2", vec!["DEMO-20"])
            ],
            "another project's epic's child and a sub-task outside every epic belong to none"
        );
        assert_eq!(
            sent,
            [
                (BY_EPIC_LINK.to_string(), LIST_FIELDS),
                (EPICS.to_string(), EPIC_FIELDS),
                (SUBTASKS.to_string(), LIST_FIELDS),
                (BY_PARENT.to_string(), LIST_FIELDS),
            ]
        );
        // The Epic Link children and the sub-tasks are read while the epics and then their
        // `parent` children are: two searches' time, not four.
        assert_eq!(took, Duration::from_millis(20));
    }

    #[tokio::test(start_paused = true)]
    async fn without_an_epic_link_field_children_are_found_by_parent_alone() {
        let (epics, sent, _) = read(false, answer).await;

        assert_eq!(
            keys(&epics.unwrap()),
            [
                ("DEMO-1", vec!["DEMO-10", "DEMO-11"]),
                ("DEMO-2", vec!["DEMO-20"])
            ]
        );
        let sent: Vec<&str> = sent.iter().map(|(jql, _)| jql.as_str()).collect();
        assert_eq!(sent, [EPICS, SUBTASKS, BY_PARENT]);
    }

    #[tokio::test(start_paused = true)]
    async fn any_failed_search_fails_the_whole_epics_read() {
        for failing in [EPICS, BY_PARENT, BY_EPIC_LINK, SUBTASKS] {
            let (epics, _, _) = read(true, |jql| {
                if jql == failing {
                    anyhow::bail!("Jira answered 503.")
                }
                answer(jql)
            })
            .await;
            assert_eq!(
                epics.unwrap_err().to_string(),
                "Jira answered 503.",
                "{failing}"
            );
        }
    }
}
