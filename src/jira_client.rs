use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::cache::{ActivityEntry, ActivityKind, Cache, Epic, Status, TeamMember, Ticket};
use crate::config::AppConfig;

const JIRA_BASE_URL: &str = "https://jira.mongodb.org/browse";
const UNASSIGNED_TEAM_NAME: &str = "Unassigned";
const UNASSIGNED_TEAM_EMAIL: &str = "__unassigned__";
const FULL_CACHE_DIR_NAME: &str = "lazyjira";

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
    let url = format!("{}/{}", JIRA_BASE_URL, key);

    Some(Ticket {
        key,
        summary,
        status: Status::from_str(status_str),
        assignee,
        assignee_email: None,
        description: None,
        labels: Vec::new(),
        epic_key: None,
        epic_name: None,
        detail_loaded: false,
        url,
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
async fn fetch_children_for_epic(config: &AppConfig, epic_key: &str, epic_summary: &str) -> Result<Vec<Ticket>> {
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

    let fields = json.get("fields").context("No fields in response")?;

    let summary = fields["summary"].as_str().unwrap_or("").to_string();
    let status = fields["status"]["name"].as_str().unwrap_or("To Do");
    let assignee = fields["assignee"]["displayName"]
        .as_str()
        .map(|s| s.to_string());
    let assignee_email = fields["assignee"]["emailAddress"]
        .as_str()
        .map(|s| s.to_string());
    let description = fields["description"].as_str().map(|s| s.to_string());
    let labels = fields["labels"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let epic_key = fields["customfield_12551"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // Try parent field for epic link
            fields["parent"]["key"].as_str().map(|s| s.to_string())
        });

    let mut activity = Vec::new();

    // Parse changelog
    if let Some(histories) = json.get("changelog")
        .and_then(|c| c.get("histories"))
        .and_then(|h| h.as_array())
    {
        for history in histories {
            let timestamp = history["created"].as_str().unwrap_or("").to_string();
            let author = history["author"]["displayName"].as_str().unwrap_or("Unknown").to_string();
            let author_email = history["author"]["emailAddress"].as_str().map(|s| s.to_string());

            if let Some(items) = history["items"].as_array() {
                for item in items {
                    let field = item["field"].as_str().unwrap_or("");
                    let from_str = item["fromString"].as_str().unwrap_or("").to_string();
                    let to_str = item["toString"].as_str().unwrap_or("").to_string();

                    let kind = match field {
                        "status" => ActivityKind::StatusChange { from: from_str, to: to_str },
                        "assignee" => ActivityKind::AssigneeChange {
                            from: Some(from_str).filter(|s| !s.is_empty()),
                            to: Some(to_str).filter(|s| !s.is_empty()),
                        },
                        _ => ActivityKind::FieldChange { field: field.to_string(), from: from_str, to: to_str },
                    };

                    activity.push(ActivityEntry {
                        timestamp: timestamp.clone(),
                        author: author.clone(),
                        author_email: author_email.clone(),
                        kind,
                    });
                }
            }
        }
    }

    // Parse comments
    if let Some(comments) = json.get("fields")
        .and_then(|f| f.get("comment"))
        .and_then(|c| c.get("comments"))
        .and_then(|c| c.as_array())
    {
        for comment in comments {
            let timestamp = comment["created"].as_str().unwrap_or("").to_string();
            let author = comment["author"]["displayName"].as_str().unwrap_or("Unknown").to_string();
            let author_email = comment["author"]["emailAddress"].as_str().map(|s| s.to_string());
            let body = comment["body"].as_str().unwrap_or("").to_string();

            activity.push(ActivityEntry {
                timestamp,
                author,
                author_email,
                kind: ActivityKind::Comment { body },
            });
        }
    }

    // Sort newest first
    activity.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let ticket_key = json["key"].as_str().unwrap_or(key).to_string();
    let url = format!("{}/{}", JIRA_BASE_URL, ticket_key);

    Ok(Ticket {
        key: ticket_key,
        summary,
        status: Status::from_str(status),
        assignee,
        assignee_email,
        description,
        labels,
        epic_key,
        epic_name: None,
        detail_loaded: true,
        url,
        activity,
    })
}

/// Fetch tickets assigned to a specific user, setting assignee_email on results.
async fn fetch_tickets_for_user(config: &AppConfig, email: &str, scope: TicketFetchScope) -> Result<Vec<Ticket>> {
    let active_query = format!(
        "assignee = \"{}\" AND status in {}",
        email, config.active_status_clause()
    );

    let mut tickets_by_key: HashMap<String, Ticket> = HashMap::new();

    match scope {
        TicketFetchScope::ActiveOnly => {
            for mut ticket in fetch_tickets_for_query(config, &active_query).await? {
                ticket.assignee_email = Some(email.to_string());
                tickets_by_key.insert(ticket.key.clone(), ticket);
            }
        }
        TicketFetchScope::ActiveAndRecentDone => {
            let recent_done_query = format!(
                "assignee = \"{}\" AND status in {} AND updated >= {}",
                email, config.done_status_clause(), config.done_window()
            );
            let (active_result, done_result) = tokio::join!(
                fetch_tickets_for_query(config, &active_query),
                fetch_tickets_for_query(config, &recent_done_query)
            );
            for mut ticket in active_result? {
                ticket.assignee_email = Some(email.to_string());
                tickets_by_key.insert(ticket.key.clone(), ticket);
            }
            for mut ticket in done_result? {
                ticket.assignee_email = Some(email.to_string());
                tickets_by_key.insert(ticket.key.clone(), ticket);
            }
        }
    }

    let mut tickets: Vec<Ticket> = tickets_by_key.into_values().collect();
    tickets.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(tickets)
}

fn unassigned_team_active_query(config: &AppConfig) -> String {
    format!(
        "assignee is EMPTY AND \"Assigned Teams\" = \"{}\" AND status in {}",
        config.jira.team_name, config.active_status_clause()
    )
}

async fn fetch_unassigned_team_tickets(config: &AppConfig) -> Result<Vec<Ticket>> {
    let mut tickets = fetch_tickets_for_query(config, &unassigned_team_active_query(config)).await?;
    for ticket in &mut tickets {
        ticket.assignee = Some(UNASSIGNED_TEAM_NAME.to_string());
        ticket.assignee_email = Some(UNASSIGNED_TEAM_EMAIL.to_string());
    }
    tickets.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(tickets)
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

    Ok(epics)
}

fn epics_cache_file_name(project: &str) -> String {
    format!("lazyjira_epics_cache_{}.json", project)
}

fn details_cache_file_name(project: &str) -> String {
    format!("lazyjira_ticket_details_cache_{}.json", project)
}

fn full_cache_file_name(project: &str) -> String {
    format!("lazyjira_full_cache_{}.json", project)
}

fn epics_cache_path(project: &str) -> PathBuf {
    std::env::temp_dir().join(epics_cache_file_name(project))
}

fn details_cache_path(project: &str) -> PathBuf {
    std::env::temp_dir().join(details_cache_file_name(project))
}

fn full_cache_dir() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(".cache").join(FULL_CACHE_DIR_NAME),
        Err(_) => std::env::temp_dir().join(FULL_CACHE_DIR_NAME),
    }
}

fn full_cache_path(project: &str) -> PathBuf {
    full_cache_dir().join(full_cache_file_name(project))
}

fn now_unix_secs() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}

pub fn load_startup_cache_snapshot(project: &str) -> Option<StartupCacheSnapshot> {
    let path = full_cache_path(project);
    let content = std::fs::read_to_string(&path).ok()?;
    let snapshot: CacheSnapshot = serde_json::from_str(&content).ok()?;
    let age_secs = now_unix_secs().saturating_sub(snapshot.saved_at_unix_secs);
    Some(StartupCacheSnapshot {
        cache: snapshot.cache,
        age_secs,
    })
}

pub fn save_full_cache_snapshot(project: &str, cache: &Cache) -> Result<()> {
    let dir = full_cache_dir();
    std::fs::create_dir_all(&dir).with_context(|| {
        format!(
            "Failed to create persistent cache directory: {}",
            dir.display()
        )
    })?;

    let path = full_cache_path(project);
    let snapshot = CacheSnapshot {
        saved_at_unix_secs: now_unix_secs(),
        cache: cache.clone(),
    };
    let json = serde_json::to_string(&snapshot).context("Failed to serialize full cache")?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write full cache snapshot: {}", path.display()))?;
    Ok(())
}

fn load_epics_cache(project: &str) -> Vec<Epic> {
    let path = epics_cache_path(project);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };

    match serde_json::from_str::<Vec<Epic>>(&content) {
        Ok(epics) => epics,
        Err(_) => Vec::new(),
    }
}

fn save_epics_cache(project: &str, epics: &[Epic]) -> Result<()> {
    let path = epics_cache_path(project);
    let json = serde_json::to_string(epics).context("Failed to serialize epics cache")?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write epics cache file: {}", path.display()))?;
    Ok(())
}

fn load_details_cache(project: &str) -> HashMap<String, Ticket> {
    let path = details_cache_path(project);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => return HashMap::new(),
    };

    serde_json::from_str::<HashMap<String, Ticket>>(&content).unwrap_or_default()
}

fn save_details_cache(project: &str, details_by_key: &HashMap<String, Ticket>) -> Result<()> {
    let path = details_cache_path(project);
    let json =
        serde_json::to_string(details_by_key).context("Failed to serialize details cache")?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write details cache file: {}", path.display()))?;
    Ok(())
}

fn hydrate_ticket_from_details_cache(
    ticket: &mut Ticket,
    details_by_key: &HashMap<String, Ticket>,
) {
    let Some(detail) = details_by_key.get(&ticket.key) else {
        return;
    };

    ticket.detail_loaded = true;
    ticket.description = detail.description.clone();
    ticket.labels = detail.labels.clone();
    if detail.assignee.is_some() {
        ticket.assignee = detail.assignee.clone();
    }
    if detail.assignee_email.is_some() {
        ticket.assignee_email = detail.assignee_email.clone();
    }
    if detail.epic_key.is_some() {
        ticket.epic_key = detail.epic_key.clone();
    }
    if detail.epic_name.is_some() {
        ticket.epic_name = detail.epic_name.clone();
    }
    if !detail.activity.is_empty() {
        ticket.activity = detail.activity.clone();
    }
}

fn hydrate_tickets_from_details_cache(
    tickets: &mut [Ticket],
    details_by_key: &HashMap<String, Ticket>,
) {
    for ticket in tickets {
        hydrate_ticket_from_details_cache(ticket, details_by_key);
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

    let attach_epic = |ticket: &mut Ticket| {
        if let Some((epic_key, epic_name)) = epic_by_ticket.get(&ticket.key) {
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

pub fn spawn_detail_cache_writer(project: &str) -> mpsc::UnboundedSender<Ticket> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Ticket>();
    let project = project.to_string();

    tokio::spawn(async move {
        let mut details_by_key = load_details_cache(&project);

        while let Some(mut detail) = rx.recv().await {
            detail.detail_loaded = true;
            details_by_key.insert(detail.key.clone(), detail);

            while let Ok(mut queued) = rx.try_recv() {
                queued.detail_loaded = true;
                details_by_key.insert(queued.key.clone(), queued);
            }

            if let Err(e) = save_details_cache(&project, &details_by_key) {
                eprintln!("Warning: failed to persist details cache: {}", e);
            }
        }
    });

    tx
}

async fn fetch_with_scope(config: &AppConfig, scope: TicketFetchScope) -> Result<Cache> {
    let mut team_members = config.team_members();

    let my_email = fetch_my_email().await?;
    if !team_members.iter().any(|member| member.email == my_email) {
        team_members.push(TeamMember {
            name: name_from_email(&my_email),
            email: my_email.clone(),
        });
    }
    let project = &config.jira.project;
    let details_by_key = load_details_cache(project);
    let mut epics = load_epics_cache(project);

    let mut my_tickets = fetch_tickets_for_user(config, &my_email, scope).await?;

    // Seed team view with my current tickets to avoid refetching self.
    let mut team_tickets = my_tickets.clone();
    let mut team_handles = Vec::new();
    for member in &team_members {
        if member.email == my_email {
            continue;
        }
        let email = member.email.clone();
        let cfg = config.clone();
        team_handles.push(tokio::spawn(async move {
            fetch_tickets_for_user(&cfg, &email, scope).await
        }));
    }

    for handle in team_handles {
        let tickets = handle.await??;
        team_tickets.extend(tickets);
    }

    let unassigned_team_tickets = fetch_unassigned_team_tickets(config).await?;
    if !unassigned_team_tickets.is_empty()
        && !team_members
            .iter()
            .any(|member| member.email == UNASSIGNED_TEAM_EMAIL)
    {
        team_members.push(TeamMember {
            name: UNASSIGNED_TEAM_NAME.to_string(),
            email: UNASSIGNED_TEAM_EMAIL.to_string(),
        });
    }
    team_tickets.extend(unassigned_team_tickets);

    attach_epics_to_tickets(&mut my_tickets, &mut team_tickets, &epics);
    hydrate_tickets_from_details_cache(&mut my_tickets, &details_by_key);
    hydrate_tickets_from_details_cache(&mut team_tickets, &details_by_key);
    for epic in &mut epics {
        hydrate_tickets_from_details_cache(&mut epic.children, &details_by_key);
    }

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

/// Move a ticket to a new status via `jira issue move`.
pub async fn move_ticket(key: &str, status: &str) -> Result<()> {
    run_cmd("jira", &["issue", "move", key, status]).await?;
    Ok(())
}

/// Add a comment to a ticket via `jira issue comment add`.
pub async fn add_comment(key: &str, body: &str) -> Result<()> {
    run_cmd("jira", &["issue", "comment", "add", key, body]).await?;
    Ok(())
}

/// Assign a ticket to a user via `jira issue assign`.
pub async fn assign_ticket(key: &str, email: &str) -> Result<()> {
    run_cmd("jira", &["issue", "assign", key, email]).await?;
    Ok(())
}

/// Edit ticket fields via `jira issue edit`.
pub async fn edit_ticket(key: &str, summary: Option<&str>, labels: Option<&[String]>) -> Result<()> {
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

    run_cmd("jira", &args).await?;
    Ok(())
}

/// Create a new ticket via `jira issue create`.
pub async fn create_ticket(
    project: &str,
    issue_type: &str,
    summary: &str,
    assignee_email: Option<&str>,
    epic_key: Option<&str>,
) -> Result<String> {
    let mut args = vec![
        "issue", "create",
        "-t", issue_type,
        "-s", summary,
        "--no-input",
        "-p", project,
    ];

    let assignee_str;
    if let Some(email) = assignee_email {
        assignee_str = email.to_string();
        args.push("-a");
        args.push(&assignee_str);
    }

    // Epic link via parent field
    let parent_str;
    if let Some(ek) = epic_key {
        parent_str = ek.to_string();
        args.push("--parent");
        args.push(&parent_str);
    }

    let output = run_cmd("jira", &args).await?;
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
    use std::collections::BTreeMap;
    use crate::config::{JiraConfig, StatusConfig};

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
    fn unassigned_query_filters_for_team_name_from_config() {
        let config = AppConfig {
            jira: JiraConfig { project: "AMP".into(), team_name: "Code Generation".into(), done_window_days: 14 },
            team: BTreeMap::new(),
            statuses: StatusConfig::default(),
            filters: vec![],
        };
        let query = unassigned_team_active_query(&config);
        assert!(query.contains("assignee is EMPTY"));
        assert!(query.contains("\"Assigned Teams\" = \"Code Generation\""));
    }
}
