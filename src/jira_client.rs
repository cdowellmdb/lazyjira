use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::cache::{Cache, Epic, TeamMember, Ticket};
use crate::config::AppConfig;
use crate::jira_issue::ticket_from_issue;
use crate::jira_rest::Subtask;
use crate::subtasks;

/// The ticket's page in the Jira web UI.
pub fn browse_url(key: &str) -> Result<String> {
    Ok(format!(
        "{}/browse/{}",
        crate::jira_rest::server_url()?,
        key
    ))
}
const UNASSIGNED_TEAM_NAME: &str = "Unassigned";
const UNASSIGNED_TEAM_EMAIL: &str = "__unassigned__";
const CACHE_DIR_NAME: &str = "lazyjira";

#[derive(Debug, Clone, Copy)]
enum TicketFetchScope {
    ActiveOnly,
    ActiveAndRecentDone,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CacheSnapshot {
    saved_at_unix_secs: u64,
    cache: Cache,
}

#[derive(Debug, Clone)]
pub struct StartupCacheSnapshot {
    pub cache: Cache,
    pub age_secs: u64,
}

/// Run a CLI command and return stdout as a String.
async fn run_cmd(program: &str, args: &[&str]) -> Result<String> {
    // Tests must never reach the real Jira through jira-cli, as `jira_rest` refuses to as well.
    if cfg!(test) {
        anyhow::bail!("tests don't run {}", program);
    }
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .with_context(|| format!("Failed to run: {} {}", program, args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} {} failed: {}", program, args.join(" "), stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Fetch current user email via `jira me`.
pub async fn fetch_my_email() -> Result<String> {
    run_cmd("jira", &["me"]).await
}

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

/// The sub-tasks among `tickets` and under them, from Jira's REST search. Empty when Jira can't
/// be asked (no `JIRA_API_TOKEN`, offline), which leaves rows flat until the next refresh.
async fn load_subtasks<'a>(tickets: impl Iterator<Item = &'a Ticket>) -> Vec<Subtask> {
    let mut keys: Vec<String> = tickets.map(|ticket| ticket.key.clone()).collect();
    keys.sort();
    keys.dedup();
    crate::jira_rest::subtasks(&keys).await.unwrap_or_default()
}

/// Parse a line of tab-separated ticket output into a Ticket.
/// Expected columns: key, status, assignee, summary
/// Summary is last because the jira CLI uses tab-padding for alignment,
/// which inserts extra tabs after long text fields. Putting summary last
/// avoids corrupting the status/assignee parsing.
fn parse_ticket_line(line: &str) -> Option<Ticket> {
    // Filter out empty fields caused by tab-padding alignment
    let fields: Vec<&str> = line.split('\t').filter(|s| !s.is_empty()).collect();
    if fields.len() < 3 {
        return None;
    }

    let key = fields[0].trim().to_string();
    if key.is_empty() {
        return None;
    }

    let status_str = fields[1].trim();
    // When assignee is empty, jira-cli tab padding can collapse to 3 fields after filtering.
    // In that case, treat field 2 as summary.
    let (assignee, summary) = if fields.len() == 3 {
        (None, fields[2].trim().to_string())
    } else {
        (
            Some(fields[2].trim().to_string()).filter(|s| !s.is_empty()),
            fields[3..]
                .iter()
                .map(|s| s.trim())
                .collect::<Vec<_>>()
                .join(" "),
        )
    };

    Some(Ticket {
        key,
        summary,
        status: status_str.to_string(),
        assignee,
        assignee_email: None,
        reporter: None,
        description: None,
        labels: Vec::new(),
        epic_key: None,
        epic_name: None,
        parent_key: None,
        updated: None,
        detail_loaded: false,
        activity: Vec::new(),
    })
}

/// Fetch tickets for a JQL query with pagination.
async fn fetch_tickets_for_query(config: &AppConfig, query: &str) -> Result<Vec<Ticket>> {
    let mut all_tickets = Vec::new();
    let mut from = 0usize;
    let page_size = 100usize;
    let project = &config.jira.project;

    loop {
        let paginate = format!("{}:{}", from, page_size);
        let output = match run_cmd(
            "jira",
            &[
                "issue",
                "list",
                "-p",
                project,
                "-q",
                query,
                "--plain",
                "--no-headers",
                "--columns",
                "key,status,assignee,summary",
                "--paginate",
                &paginate,
            ],
        )
        .await
        {
            Ok(output) => output,
            Err(e) => {
                // jira-cli returns exit code 1 for empty JQL results.
                if e.to_string().contains("No result found for given query") {
                    break;
                }
                return Err(e);
            }
        };

        if output.is_empty() {
            break;
        }

        let batch: Vec<Ticket> = output.lines().filter_map(parse_ticket_line).collect();
        let batch_len = batch.len();
        all_tickets.extend(batch);

        if batch_len < page_size {
            break;
        }
        from += page_size;
    }

    Ok(all_tickets)
}

/// Fetch epic children using both company-managed (Epic Link) and team-managed (parent) style links.
async fn fetch_children_for_epic(
    config: &AppConfig,
    epic_key: &str,
    epic_summary: &str,
) -> Result<Vec<Ticket>> {
    let epic_link_query = format!("\"Epic Link\" = {}", epic_key);
    let parent_query = format!("parent = {}", epic_key);

    let (epic_link_result, parent_result) = tokio::join!(
        fetch_tickets_for_query(config, &epic_link_query),
        fetch_tickets_for_query(config, &parent_query)
    );

    let mut children_by_key: HashMap<String, Ticket> = HashMap::new();
    let mut success_count = 0usize;
    let mut errors: Vec<String> = Vec::new();

    match epic_link_result {
        Ok(tickets) => {
            success_count += 1;
            for mut t in tickets {
                t.epic_key = Some(epic_key.to_string());
                t.epic_name = Some(epic_summary.to_string());
                children_by_key.entry(t.key.clone()).or_insert(t);
            }
        }
        Err(e) => errors.push(format!("Epic Link query error: {}", e)),
    }

    match parent_result {
        Ok(tickets) => {
            success_count += 1;
            for mut t in tickets {
                t.epic_key = Some(epic_key.to_string());
                t.epic_name = Some(epic_summary.to_string());
                children_by_key.entry(t.key.clone()).or_insert(t);
            }
        }
        Err(e) => errors.push(format!("parent query error: {}", e)),
    }

    if success_count == 0 {
        anyhow::bail!(
            "Failed to fetch children for {} via Epic Link and parent queries. {}",
            epic_key,
            errors.join(" | ")
        );
    }

    let mut children: Vec<Ticket> = children_by_key.into_values().collect();
    children.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(children)
}

/// Fetch full ticket detail as JSON via `jira issue view KEY --raw`.
/// Returns the ticket with description populated.
pub async fn fetch_ticket_detail(key: &str) -> Result<Ticket> {
    let output = run_cmd("jira", &["issue", "view", key, "--raw"]).await?;
    let json: serde_json::Value = serde_json::from_str(&output)
        .with_context(|| format!("Failed to parse JSON for {}", key))?;
    let mut ticket = ticket_from_issue(&json, crate::jira_rest::epic_link_field().as_deref())
        .with_context(|| format!("No issue in Jira's answer for {}", key))?;
    ticket.detail_loaded = true;
    Ok(ticket)
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
/// tickets nobody has taken that are the team's by Assigned Teams. jira-cli used to add the
/// project and an order; a REST search needs both spelled out. The order keeps pages stable
/// while tickets change underneath them.
fn lists_jql(config: &AppConfig, assignee_emails: &[&str], scope: TicketFetchScope) -> String {
    let assignees = assignee_emails
        .iter()
        .map(|email| format!("\"{}\"", email))
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
        "project = \"{}\" AND ((assignee in ({assignees}) AND {statuses}) \
         OR (assignee is EMPTY AND \"Assigned Teams\" = \"{}\" AND status in {active})) \
         ORDER BY key",
        config.jira.project, config.jira.team_name
    )
}

/// Splits one search's tickets into My Work and Team, each sorted by key. Jira's email for an
/// assignee is mapped to the roster's entry ignoring case, since Team groups by exact email;
/// tickets with no assignee become the Unassigned member's. A ticket assigned to someone
/// outside `members` can't be placed and is left out.
fn bucket_tickets(
    mut found: Vec<Ticket>,
    members: &[TeamMember],
    my_email: &str,
) -> (Vec<Ticket>, Vec<Ticket>) {
    found.sort_by(|a, b| a.key.cmp(&b.key));
    found.dedup_by(|a, b| a.key == b.key);
    let (mut mine, mut team) = (Vec::new(), Vec::new());
    for mut ticket in found {
        if ticket.assignee.is_none() {
            ticket.assignee = Some(UNASSIGNED_TEAM_NAME.to_string());
            ticket.assignee_email = Some(UNASSIGNED_TEAM_EMAIL.to_string());
        } else if let Some(member) = members.iter().find(|member| {
            ticket
                .assignee_email
                .as_deref()
                .is_some_and(|email| email.eq_ignore_ascii_case(&member.email))
        }) {
            ticket.assignee_email = Some(member.email.clone());
            if member.email.eq_ignore_ascii_case(my_email) {
                mine.push(ticket.clone());
            }
        } else {
            continue;
        }
        team.push(ticket);
    }
    (mine, team)
}

/// `jql` limited to `project`, with an order, as `jira issue list -q` made it. A filter's own
/// ORDER BY is kept (after the project, outside the parentheses); without one, newest first.
fn scoped_jql(project: &str, jql: &str) -> String {
    // ponytail: the last "order by" is taken as the clause, so one inside a quoted string
    // fails the query; parse JQL properly if filters ever need it.
    let split = jql.to_ascii_lowercase().rfind("order by");
    let (condition, order) = match split {
        Some(at) => (jql[..at].trim(), jql[at..].trim()),
        None => (jql.trim(), "ORDER BY created DESC"),
    };
    if condition.is_empty() {
        format!("project = \"{project}\" {order}")
    } else {
        format!("project = \"{project}\" AND ({condition}) {order}")
    }
}

/// Fetch all epics and their children.
async fn fetch_epics(config: &AppConfig) -> Result<Vec<Epic>> {
    const MAX_EPIC_CHILD_FETCH_CONCURRENCY: usize = 8;

    let mut from = 0usize;
    let page_size = 100usize;
    let mut epic_stubs_map: HashMap<String, String> = HashMap::new();
    let project = &config.jira.project;

    loop {
        let paginate = format!("{}:{}", from, page_size);
        let epics_output = run_cmd(
            "jira",
            &[
                "issue",
                "list",
                "-t",
                "Epic",
                "-p",
                project,
                "--plain",
                "--no-headers",
                "--columns",
                "key,status,summary",
                "--paginate",
                &paginate,
            ],
        )
        .await?;

        if epics_output.is_empty() {
            break;
        }

        let mut batch_count = 0usize;
        for line in epics_output.lines() {
            let fields: Vec<&str> = line.split('\t').filter(|s| !s.is_empty()).collect();
            if fields.len() >= 2 {
                let key = fields[0].trim().to_string();
                // Summary is after status (field 2+)
                let summary = if fields.len() > 2 {
                    fields[2..]
                        .iter()
                        .map(|s| s.trim())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    String::new()
                };
                if !key.is_empty() {
                    epic_stubs_map.entry(key).or_insert(summary);
                    batch_count += 1;
                }
            }
        }

        if batch_count < page_size {
            break;
        }
        from += page_size;
    }

    let mut epic_stubs: Vec<(String, String)> = epic_stubs_map.into_iter().collect();
    epic_stubs.sort_by(|a, b| a.0.cmp(&b.0));

    let epic_count = epic_stubs.len();
    if epic_count == 0 {
        return Ok(Vec::new());
    }

    let config_arc = std::sync::Arc::new(config.clone());
    let mut epics_by_index: Vec<Option<Epic>> = vec![None; epic_count];
    let mut iter = epic_stubs.into_iter().enumerate();
    let mut tasks = tokio::task::JoinSet::new();

    let initial_workers = MAX_EPIC_CHILD_FETCH_CONCURRENCY.min(epic_count);
    for _ in 0..initial_workers {
        if let Some((idx, (epic_key, epic_summary))) = iter.next() {
            let cfg = config_arc.clone();
            tasks.spawn(async move {
                let children = match fetch_children_for_epic(&cfg, &epic_key, &epic_summary).await {
                    Ok(children) => children,
                    Err(e) => {
                        eprintln!("Warning: {}. Showing this epic with no related tickets.", e);
                        Vec::new()
                    }
                };

                (
                    idx,
                    Epic {
                        key: epic_key,
                        summary: epic_summary,
                        children,
                    },
                )
            });
        }
    }

    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok((idx, epic)) => {
                epics_by_index[idx] = Some(epic);
            }
            Err(e) => {
                eprintln!("Warning: epic fetch task failed: {}", e);
            }
        }

        if let Some((idx, (epic_key, epic_summary))) = iter.next() {
            let cfg = config_arc.clone();
            tasks.spawn(async move {
                let children = match fetch_children_for_epic(&cfg, &epic_key, &epic_summary).await {
                    Ok(children) => children,
                    Err(e) => {
                        eprintln!("Warning: {}. Showing this epic with no related tickets.", e);
                        Vec::new()
                    }
                };

                (
                    idx,
                    Epic {
                        key: epic_key,
                        summary: epic_summary,
                        children,
                    },
                )
            });
        }
    }

    let mut epics = Vec::with_capacity(epic_count);
    let mut dropped = 0usize;
    for epic in epics_by_index {
        if let Some(epic) = epic {
            epics.push(epic);
        } else {
            dropped += 1;
        }
    }
    if dropped > 0 {
        eprintln!(
            "Warning: dropped {} epic rows due to unexpected task failure.",
            dropped
        );
    }

    let found = load_subtasks(epics.iter().flat_map(|epic| &epic.children)).await;
    subtasks::add_to_epics(&mut epics, &found);

    Ok(epics)
}

const EPICS_CACHE_PREFIX: &str = "lazyjira_epics_cache";
const DETAILS_CACHE_PREFIX: &str = "lazyjira_ticket_details_cache";
const FULL_CACHE_PREFIX: &str = "lazyjira_full_cache";
const MY_EMAIL_PREFIX: &str = "lazyjira_my_email";

fn cache_file_name(prefix: &str, project: &str) -> String {
    format!("{prefix}_{project}.json")
}

/// Where every cache lives: `~/.cache/lazyjira/`, or the temp dir without a `HOME`.
fn cache_dir_in(home: Option<std::ffi::OsString>) -> PathBuf {
    match home {
        Some(home) => PathBuf::from(home).join(".cache").join(CACHE_DIR_NAME),
        None => std::env::temp_dir().join(CACHE_DIR_NAME),
    }
}

fn cache_dir() -> PathBuf {
    // Tests keep off the real ~/.cache.
    cache_dir_in(if cfg!(test) {
        None
    } else {
        std::env::var_os("HOME")
    })
}

fn cache_path(prefix: &str, project: &str) -> PathBuf {
    cache_dir().join(cache_file_name(prefix, project))
}

fn read_cache_file<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache_file(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create cache directory: {}", dir.display()))?;
    }
    let json = serde_json::to_string(value).context("Failed to serialize cache")?;
    std::fs::write(path, json)
        .with_context(|| format!("Failed to write cache file: {}", path.display()))
}

fn now_unix_secs() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}

pub fn load_startup_cache_snapshot(project: &str) -> Option<StartupCacheSnapshot> {
    let snapshot: CacheSnapshot = read_cache_file(&cache_path(FULL_CACHE_PREFIX, project))?;
    let age_secs = now_unix_secs().saturating_sub(snapshot.saved_at_unix_secs);
    Some(StartupCacheSnapshot {
        cache: snapshot.cache,
        age_secs,
    })
}

pub fn save_full_cache_snapshot(project: &str, cache: &Cache) -> Result<()> {
    let snapshot = CacheSnapshot {
        saved_at_unix_secs: now_unix_secs(),
        cache: cache.clone(),
    };
    write_cache_file(&cache_path(FULL_CACHE_PREFIX, project), &snapshot)
}

fn load_epics_cache(project: &str) -> Vec<Epic> {
    read_cache_file(&cache_path(EPICS_CACHE_PREFIX, project)).unwrap_or_default()
}

fn save_epics_cache(project: &str, epics: &[Epic]) -> Result<()> {
    write_cache_file(&cache_path(EPICS_CACHE_PREFIX, project), &epics)
}

fn load_details_cache(project: &str) -> HashMap<String, Ticket> {
    read_cache_file(&cache_path(DETAILS_CACHE_PREFIX, project)).unwrap_or_default()
}

fn load_my_email(project: &str) -> Option<String> {
    read_cache_file(&cache_path(MY_EMAIL_PREFIX, project))
}

fn save_my_email(project: &str, email: &str) -> Result<()> {
    write_cache_file(&cache_path(MY_EMAIL_PREFIX, project), &email)
}

/// Ask `jira me` for the current user's email, and remember it for later refreshes.
pub async fn refresh_my_email(project: &str) -> Result<String> {
    let email = fetch_my_email().await?;
    save_my_email(project, &email)?;
    Ok(email)
}

/// The remembered email, so a refresh doesn't wait on `jira me`; asks Jira only the first time.
async fn my_email(project: &str) -> Result<String> {
    match load_my_email(project) {
        Some(email) => Ok(email),
        None => refresh_my_email(project).await,
    }
}

/// Fills in what only a ticket's detail has: description, reporter and activity. Everything the
/// list search returns (labels, assignee, epic, parent) stays as that search read it, since
/// the cache is never invalidated and would bring back what has since changed in Jira.
fn hydrate_ticket_from_details_cache(
    ticket: &mut Ticket,
    details_by_key: &HashMap<String, Ticket>,
) {
    let Some(detail) = details_by_key.get(&ticket.key) else {
        return;
    };

    // Only mark fully loaded if the cached detail has reporter (added later).
    // Old cache entries missing reporter will be re-fetched once, then stay cached.
    ticket.detail_loaded = detail.reporter.is_some();
    ticket.description = detail.description.clone();
    if detail.reporter.is_some() {
        ticket.reporter = detail.reporter.clone();
    }
    if !detail.activity.is_empty() {
        ticket.activity = detail.activity.clone();
    }
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

/// Ticket details fetched so far. A refresh hydrates from this in-memory copy instead of
/// reading the details cache file, and a background task writes it back to that file.
#[derive(Clone)]
pub struct DetailCache {
    by_key: Arc<Mutex<HashMap<String, Ticket>>>,
    changed: mpsc::UnboundedSender<()>,
}

impl DetailCache {
    /// Read the details cache file once, and start the task that writes it back.
    pub fn spawn(project: &str) -> Self {
        let (details, changed) = Self::new(load_details_cache(project));
        tokio::spawn(write_details(
            cache_path(DETAILS_CACHE_PREFIX, project),
            details.by_key.clone(),
            changed,
        ));
        details
    }

    fn new(by_key: HashMap<String, Ticket>) -> (Self, mpsc::UnboundedReceiver<()>) {
        let (changed, rx) = mpsc::unbounded_channel();
        let details = DetailCache {
            by_key: Arc::new(Mutex::new(by_key)),
            changed,
        };
        (details, rx)
    }

    /// Keep a freshly fetched detail. False when the writer has stopped, so it won't reach disk.
    pub fn record(&self, mut detail: Ticket) -> bool {
        detail.detail_loaded = true;
        lock_details(&self.by_key).insert(detail.key.clone(), detail);
        self.changed.send(()).is_ok()
    }

    /// Fill a list read's tickets, including epic children, with the details already fetched.
    pub fn hydrate(&self, cache: &mut Cache) {
        let by_key = lock_details(&self.by_key);
        let epic_children = cache.epics.iter_mut().flat_map(|epic| &mut epic.children);
        for ticket in cache
            .my_tickets
            .iter_mut()
            .chain(&mut cache.team_tickets)
            .chain(epic_children)
        {
            hydrate_ticket_from_details_cache(ticket, &by_key);
        }
    }
}

fn lock_details(
    by_key: &Mutex<HashMap<String, Ticket>>,
) -> MutexGuard<'_, HashMap<String, Ticket>> {
    by_key.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Write the details once changes go quiet, and once more when the app closes the channel.
async fn write_details(
    path: PathBuf,
    by_key: Arc<Mutex<HashMap<String, Ticket>>>,
    mut changed: mpsc::UnboundedReceiver<()>,
) {
    let flush_after_idle = Duration::from_millis(750);
    while changed.recv().await.is_some() {
        while let Ok(Some(())) = timeout(flush_after_idle, changed.recv()).await {}
        if let Err(e) = write_cache_file(&path, &*lock_details(&by_key)) {
            eprintln!("Warning: failed to persist details cache: {}", e);
        }
    }
}

async fn fetch_with_scope(config: &AppConfig, scope: TicketFetchScope) -> Result<Cache> {
    let mut team_members = config.team_members();

    let project = &config.jira.project;
    let my_email = my_email(project).await?;
    if !team_members
        .iter()
        .any(|member| member.email.eq_ignore_ascii_case(&my_email))
    {
        team_members.push(TeamMember {
            name: name_from_email(&my_email),
            email: my_email.clone(),
        });
    }
    let mut epics = load_epics_cache(project);

    // A failed search returns here, so the caller keeps showing the last snapshot.
    let emails: Vec<&str> = team_members.iter().map(|m| m.email.as_str()).collect();
    let found = crate::jira_rest::search(&lists_jql(config, &emails, scope), LIST_FIELDS).await?;
    let (mut my_tickets, mut team_tickets) = bucket_tickets(found, &team_members, &my_email);
    if team_tickets
        .iter()
        .any(|ticket| ticket.assignee_email.as_deref() == Some(UNASSIGNED_TEAM_EMAIL))
    {
        team_members.push(TeamMember {
            name: UNASSIGNED_TEAM_NAME.to_string(),
            email: UNASSIGNED_TEAM_EMAIL.to_string(),
        });
    }

    attach_epics_to_tickets(&mut my_tickets, &mut team_tickets, &epics);
    reconcile_epic_child_statuses(&mut epics, &my_tickets, &team_tickets);

    Ok(Cache {
        my_tickets,
        team_tickets,
        epics,
        team_members,
    })
}

/// Fetch active (non-Done) tickets first for fast startup accuracy.
pub async fn fetch_active_only(config: &AppConfig) -> Result<Cache> {
    fetch_with_scope(config, TicketFetchScope::ActiveOnly).await
}

/// Fetch active + recently done tickets for a complete cache refresh.
pub async fn fetch_all(config: &AppConfig) -> Result<Cache> {
    fetch_with_scope(config, TicketFetchScope::ActiveAndRecentDone).await
}

/// Add a comment to a ticket via `jira issue comment add`.
pub async fn add_comment(key: &str, body: &str) -> Result<()> {
    run_cmd(
        "jira",
        &["issue", "comment", "add", key, body, "--no-input"],
    )
    .await?;
    Ok(())
}

/// Assign a ticket to a user via `jira issue assign`.
pub async fn assign_ticket(key: &str, email: &str) -> Result<()> {
    run_cmd("jira", &["issue", "assign", key, email]).await?;
    Ok(())
}

/// Edit ticket fields via `jira issue edit`.
pub async fn edit_ticket(
    key: &str,
    summary: Option<&str>,
    labels: Option<&[String]>,
    description: Option<&str>,
) -> Result<()> {
    let mut args = vec!["issue", "edit", key, "--no-input"];

    if let Some(s) = summary {
        args.push("-s");
        args.push(s);
    }

    if let Some(lbls) = labels {
        for label in lbls {
            args.push("-l");
            args.push(label);
        }
    }

    if let Some(body) = description {
        args.extend(["-b", body]);
    }
    run_cmd("jira", &args).await?;
    Ok(())
}

/// Run an arbitrary JQL query and return matching tickets.
pub async fn fetch_jql_query(config: &AppConfig, jql: &str) -> Result<Vec<Ticket>> {
    crate::jira_rest::search(&scoped_jql(&config.jira.project, jql), LIST_FIELDS).await
}

/// Create a new ticket via `jira issue create`, with optional body and labels.
pub async fn create_ticket_with_fields(
    project: &str,
    issue_type: &str,
    summary: &str,
    assignee_email: Option<&str>,
    epic_key: Option<&str>,
    description: Option<&str>,
    labels: Option<&[String]>,
) -> Result<String> {
    let mut args: Vec<String> = vec![
        "issue".to_string(),
        "create".to_string(),
        "-t".to_string(),
        issue_type.to_string(),
        "-s".to_string(),
        summary.to_string(),
        "--no-input".to_string(),
        "-p".to_string(),
        project.to_string(),
    ];

    if let Some(email) = assignee_email {
        args.push("-a".to_string());
        args.push(email.to_string());
    }

    if let Some(ek) = epic_key {
        args.push("-P".to_string());
        args.push(ek.to_string());
    }

    if let Some(body) = description {
        if !body.trim().is_empty() {
            args.push("-b".to_string());
            args.push(body.to_string());
        }
    }

    if let Some(values) = labels {
        for label in values {
            if !label.trim().is_empty() {
                args.push("-l".to_string());
                args.push(label.to_string());
            }
        }
    }

    let args_ref = args.iter().map(|s| s.as_str()).collect::<Vec<_>>();
    let output = run_cmd("jira", &args_ref).await?;
    // jira-cli typically outputs something like "Issue AMP-1234 created"
    // Extract the key
    let key = output
        .split_whitespace()
        .find(|w| w.contains('-'))
        .map(|w| w.to_string())
        .unwrap_or(output.trim().to_string());
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{JiraConfig, StatusConfig};
    use std::collections::BTreeMap;

    fn test_ticket(key: &str, status: &str) -> Ticket {
        Ticket {
            key: key.to_string(),
            summary: format!("Summary for {}", key),
            status: status.to_string(),
            assignee: None,
            assignee_email: None,
            reporter: None,
            description: None,
            labels: Vec::new(),
            epic_key: None,
            epic_name: None,
            parent_key: None,
            updated: None,
            detail_loaded: false,
            activity: Vec::new(),
        }
    }

    #[test]
    fn parse_ticket_line_handles_empty_assignee() {
        let line = "AMP-2842\tNeeds Triage\t\t\t\tevals cli export doesn't support packages";
        let ticket = parse_ticket_line(line).expect("ticket should parse");
        assert_eq!(ticket.key, "AMP-2842");
        assert_eq!(ticket.assignee, None);
        assert_eq!(
            ticket.summary,
            "evals cli export doesn't support packages".to_string()
        );
    }

    #[test]
    fn parse_ticket_line_handles_assignee_and_summary() {
        let line = "AMP-2815\tIn Progress\tMohammad Mazraeh\tRun evals ci in Olympus in parallel";
        let ticket = parse_ticket_line(line).expect("ticket should parse");
        assert_eq!(ticket.key, "AMP-2815");
        assert_eq!(ticket.assignee, Some("Mohammad Mazraeh".to_string()));
        assert_eq!(
            ticket.summary,
            "Run evals ci in Olympus in parallel".to_string()
        );
    }

    #[test]
    fn parse_ticket_line_keeps_the_real_status_name() {
        let line = "DEMO-7\tResolved\tSam Doe\tShip it";
        let ticket = parse_ticket_line(line).expect("ticket should parse");
        assert_eq!(ticket.status, "Resolved");
    }

    #[test]
    fn caches_from_before_real_status_names_are_refetched() {
        // Written before #19: `status` held the collapsed enum, and `jira_status` the name
        // only when known. Loading it fails, so the app fetches from Jira instead of
        // showing Resolved tickets as Closed.
        let json = r#"{"saved_at_unix_secs": 1, "cache": {"my_tickets": [
            {"key": "DEMO-1", "summary": "One", "status": "Closed", "jira_status": "Resolved",
             "assignee": null, "assignee_email": null, "description": null, "labels": [],
             "epic_key": null, "epic_name": null, "url": "https://jira.example.com/browse/DEMO-1"}
          ], "team_tickets": [], "epics": [], "team_members": []}}"#;
        assert!(serde_json::from_str::<CacheSnapshot>(json).is_err());
        let details = r#"{"DEMO-1": {"key": "DEMO-1", "summary": "One", "status": {"Other": "Backlog"},
            "assignee": null, "assignee_email": null, "description": null, "labels": [],
            "epic_key": null, "epic_name": null, "url": "https://jira.example.com/browse/DEMO-1"}}"#;
        assert!(serde_json::from_str::<HashMap<String, Ticket>>(details).is_err());

        let cache = Cache {
            my_tickets: vec![test_ticket("DEMO-1", "Resolved")],
            ..Cache::empty()
        };
        let snapshot = CacheSnapshot {
            saved_at_unix_secs: 1,
            cache,
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let loaded: CacheSnapshot = serde_json::from_str(&json).expect("new cache should load");
        assert_eq!(loaded.cache.my_tickets[0].status, "Resolved");
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

    #[test]
    fn one_search_fills_my_work_and_team_by_the_roster_email() {
        let found = vec![
            assigned("AMP-3", "To Do", "Sam", "sam.chen@example.com"),
            assigned("AMP-2", "Done", "Alex", "alex.rivera@example.com"),
            assigned("AMP-1", "To Do", "Alex", "alex.rivera@example.com"),
            test_ticket("AMP-4", "To Do"),
        ];
        let (mine, team) = bucket_tickets(found, &roster(), "alex.rivera@example.com");
        let keys = |tickets: &[Ticket]| tickets.iter().map(|t| t.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&mine), ["AMP-1", "AMP-2"]);
        assert_eq!(keys(&team), ["AMP-1", "AMP-2", "AMP-3", "AMP-4"]);
    }

    #[test]
    fn jiras_email_is_mapped_to_the_roster_entry_ignoring_case() {
        let found = vec![
            assigned("AMP-1", "To Do", "Sam C.", "Sam.Chen@Example.com"),
            assigned("AMP-2", "To Do", "Alex R.", "ALEX.RIVERA@example.com"),
        ];
        // `jira me` can differ in case from Jira's own email too.
        let (mine, team) = bucket_tickets(found, &roster(), "Alex.Rivera@example.com");
        assert_eq!(
            team[0].assignee_email.as_deref(),
            Some("sam.chen@example.com")
        );
        assert_eq!(
            team[1].assignee_email.as_deref(),
            Some("alex.rivera@example.com")
        );
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].key, "AMP-2");
        // The name is Jira's own: a display name that differs from the roster's still groups.
        assert_eq!(team[0].assignee.as_deref(), Some("Sam C."));
    }

    #[test]
    fn tickets_without_an_assignee_go_to_the_unassigned_row() {
        let (mine, team) = bucket_tickets(
            vec![test_ticket("AMP-9", "To Do")],
            &roster(),
            "alex.rivera@example.com",
        );
        assert!(mine.is_empty());
        assert_eq!(team[0].assignee.as_deref(), Some("Unassigned"));
        assert_eq!(team[0].assignee_email.as_deref(), Some("__unassigned__"));
    }

    #[test]
    fn a_ticket_assigned_to_someone_outside_the_roster_is_left_out() {
        let found = vec![assigned("AMP-5", "To Do", "Pat", "pat@example.com")];
        let (mine, team) = bucket_tickets(found, &roster(), "alex.rivera@example.com");
        assert!(mine.is_empty() && team.is_empty());
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
    fn a_saved_filter_is_scoped_to_the_project_the_way_jira_cli_did() {
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
    fn the_details_cache_fills_only_what_the_list_search_does_not_return() {
        let mut fresh = assigned("AMP-1", "To Do", "Alex R.", "alex.rivera@example.com");
        fresh.labels = vec!["new".into()];
        fresh.epic_key = Some("AMP-100".into());
        let mut stale = test_ticket("AMP-1", "Blocked");
        stale.assignee = Some("Old Owner".into());
        stale.assignee_email = Some("old@example.com".into());
        stale.labels = vec!["old".into()];
        stale.epic_key = Some("AMP-50".into());
        stale.parent_key = Some("AMP-7".into());
        stale.description = Some("Body".into());
        stale.reporter = Some("Pat".into());
        let cached = HashMap::from([("AMP-1".to_string(), stale)]);

        hydrate_ticket_from_details_cache(&mut fresh, &cached);

        assert_eq!(fresh.status, "To Do");
        assert_eq!(fresh.assignee.as_deref(), Some("Alex R."));
        assert_eq!(
            fresh.assignee_email.as_deref(),
            Some("alex.rivera@example.com")
        );
        assert_eq!(fresh.labels, ["new"]);
        assert_eq!(fresh.epic_key.as_deref(), Some("AMP-100"));
        // A re-parented sub-task must not get its old parent back from the cache.
        assert_eq!(fresh.parent_key, None);
        assert_eq!(fresh.description.as_deref(), Some("Body"));
        assert_eq!(fresh.reporter.as_deref(), Some("Pat"));
        assert!(fresh.detail_loaded);
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

    #[test]
    fn my_email_is_remembered_per_project() {
        let _ = std::fs::remove_file(cache_path(MY_EMAIL_PREFIX, "EMAILTEST_A"));
        assert_eq!(load_my_email("EMAILTEST_A"), None);
        save_my_email("EMAILTEST_A", "me@example.com").unwrap();

        assert_eq!(
            load_my_email("EMAILTEST_A").as_deref(),
            Some("me@example.com")
        );
        assert_eq!(load_my_email("EMAILTEST_B"), None);
    }

    #[test]
    fn caches_live_in_the_lazyjira_dir_under_home_per_project() {
        let dir = cache_dir_in(Some("/home/me".into()));
        assert_eq!(dir, PathBuf::from("/home/me/.cache/lazyjira"));

        for prefix in [DETAILS_CACHE_PREFIX, EPICS_CACHE_PREFIX, MY_EMAIL_PREFIX] {
            assert_eq!(
                cache_path(prefix, "AMP").parent(),
                Some(cache_dir().as_path())
            );
            assert_ne!(cache_path(prefix, "AMP"), cache_path(prefix, "DEMO"));
        }
    }

    #[test]
    fn an_old_temp_dir_epics_cache_is_ignored() {
        let epics = vec![Epic {
            key: "OLDTMP-1".into(),
            summary: "Old".into(),
            children: vec![],
        }];
        let old = std::env::temp_dir().join("lazyjira_epics_cache_OLDTMP.json");
        std::fs::write(&old, serde_json::to_string(&epics).unwrap()).unwrap();
        let _ = std::fs::remove_file(cache_path(EPICS_CACHE_PREFIX, "OLDTMP"));

        assert!(load_epics_cache("OLDTMP").is_empty());

        save_epics_cache("OLDTMP", &epics).unwrap();
        assert_eq!(load_epics_cache("OLDTMP")[0].key, "OLDTMP-1");
        let _ = std::fs::remove_file(old);
    }

    #[test]
    fn a_refresh_hydrates_from_details_recorded_in_memory() {
        let (details, _changed) = DetailCache::new(HashMap::new());
        let mut detail = test_ticket("DEMO-1", "In Progress");
        detail.description = Some("Body".into());
        detail.reporter = Some("Sam Doe".into());
        details.record(detail);

        let mut cache = Cache {
            my_tickets: vec![test_ticket("DEMO-1", "To Do")],
            team_tickets: vec![test_ticket("DEMO-1", "To Do")],
            epics: vec![Epic {
                key: "DEMO-9".into(),
                summary: "Epic".into(),
                children: vec![test_ticket("DEMO-1", "To Do")],
            }],
            ..Cache::empty()
        };
        details.hydrate(&mut cache);

        for ticket in [
            &cache.my_tickets[0],
            &cache.team_tickets[0],
            &cache.epics[0].children[0],
        ] {
            assert_eq!(ticket.description.as_deref(), Some("Body"));
            assert!(ticket.detail_loaded);
            // The list read's status stands; a detail never overrides it here.
            assert_eq!(ticket.status, "To Do");
        }
    }
}
