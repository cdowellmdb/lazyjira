use std::collections::HashMap;

use anyhow::{Context, Result};
use tokio::process::Command;

use crate::cache::{Cache, Epic, Status, TeamMember, Ticket};

const JIRA_BASE_URL: &str = "https://jira.mongodb.org/browse";
const PROJECT: &str = "AMP";

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
async fn fetch_my_email() -> Result<String> {
    run_cmd("jira", &["me"]).await
}

/// Load team roster from ~/.claude/skills/jira/team.yml, deduplicating by email.
fn load_team_roster() -> Result<Vec<TeamMember>> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let path = format!("{}/.claude/skills/jira/team.yml", home);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read team file: {}", path))?;

    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&content).context("Failed to parse team.yml")?;

    let team_map = yaml
        .get("team")
        .and_then(|v| v.as_mapping())
        .context("Expected 'team' mapping in team.yml")?;

    let mut seen_emails: HashMap<String, ()> = HashMap::new();
    let mut members = Vec::new();

    for (name_val, email_val) in team_map {
        let name = name_val.as_str().unwrap_or_default().to_string();
        let email = email_val.as_str().unwrap_or_default().to_string();

        if !email.is_empty() && !seen_emails.contains_key(&email) {
            seen_emails.insert(email.clone(), ());
            members.push(TeamMember { name, email });
        }
    }

    Ok(members)
}

/// Parse a line of tab-separated ticket output into a Ticket.
/// Expected columns: key, summary, status, assignee[, type]
fn parse_ticket_line(line: &str) -> Option<Ticket> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() < 3 {
        return None;
    }

    let key = fields[0].trim().to_string();
    if key.is_empty() {
        return None;
    }

    let summary = fields[1].trim().to_string();
    let status_str = fields[2].trim();
    let assignee = fields
        .get(3)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let url = format!("{}/{}", JIRA_BASE_URL, key);

    Some(Ticket {
        key,
        summary,
        status: Status::from_str(status_str),
        assignee: assignee.clone(),
        assignee_email: None,
        description: None,
        epic_key: None,
        epic_name: None,
        url,
    })
}

/// Fetch tickets assigned to a specific user, setting assignee_email on results.
async fn fetch_tickets_for_user(email: &str) -> Result<Vec<Ticket>> {
    let assignee_flag = format!("-a{}", email);
    let output = run_cmd(
        "jira",
        &[
            "issue",
            "list",
            &assignee_flag,
            "-s",
            "To Do",
            "-s",
            "In Progress",
            "-s",
            "In Review",
            "-s",
            "Blocked",
            "-s",
            "Done",
            "-p",
            PROJECT,
            "--plain",
            "--no-headers",
            "--columns",
            "key,summary,status,assignee,type",
        ],
    )
    .await?;

    Ok(output
        .lines()
        .filter_map(|line| {
            let mut ticket = parse_ticket_line(line)?;
            ticket.assignee_email = Some(email.to_string());
            Some(ticket)
        })
        .collect())
}

/// Fetch all epics and their children concurrently.
async fn fetch_epics() -> Result<Vec<Epic>> {
    let epics_output = run_cmd(
        "jira",
        &[
            "issue",
            "list",
            "-t",
            "Epic",
            "-p",
            PROJECT,
            "--plain",
            "--no-headers",
            "--columns",
            "key,summary,status",
        ],
    )
    .await?;

    let epic_stubs: Vec<(String, String)> = epics_output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() >= 2 {
                let key = fields[0].trim().to_string();
                let summary = fields[1].trim().to_string();
                if !key.is_empty() {
                    return Some((key, summary));
                }
            }
            None
        })
        .collect();

    // Fetch children for each epic concurrently
    let mut handles = Vec::new();
    for (epic_key, epic_summary) in epic_stubs {
        handles.push(tokio::spawn(async move {
            let query = format!("parent={}", epic_key);
            let children_output = run_cmd(
                "jira",
                &[
                    "issue",
                    "list",
                    "-q",
                    &query,
                    "--plain",
                    "--no-headers",
                    "--columns",
                    "key,summary,status,assignee",
                ],
            )
            .await
            .unwrap_or_default();

            let children: Vec<Ticket> = children_output
                .lines()
                .filter_map(|line| {
                    let mut t = parse_ticket_line(line)?;
                    t.epic_key = Some(epic_key.clone());
                    t.epic_name = Some(epic_summary.clone());
                    Some(t)
                })
                .collect();

            Epic {
                key: epic_key,
                summary: epic_summary,
                children,
            }
        }));
    }

    let mut epics = Vec::new();
    for handle in handles {
        epics.push(handle.await?);
    }

    Ok(epics)
}

/// Fetch all Jira data concurrently and return a populated Cache.
pub async fn fetch_all() -> Result<Cache> {
    let team_members = load_team_roster().unwrap_or_default();

    // Fetch my email and epics concurrently
    let (my_email_result, epics_result) = tokio::join!(fetch_my_email(), fetch_epics(),);

    let my_email = my_email_result?;
    let epics = epics_result?;

    // Fetch my tickets
    let my_tickets = fetch_tickets_for_user(&my_email).await?;

    // Fetch team tickets concurrently (skip self to avoid duplicating my_tickets)
    let mut team_handles = Vec::new();
    for member in &team_members {
        if member.email == my_email {
            continue;
        }
        let email = member.email.clone();
        team_handles.push(tokio::spawn(async move {
            fetch_tickets_for_user(&email).await.unwrap_or_default()
        }));
    }

    let mut team_tickets = Vec::new();
    for handle in team_handles {
        let tickets = handle.await?;
        team_tickets.extend(tickets);
    }

    Ok(Cache {
        my_tickets,
        team_tickets,
        epics,
        team_members,
        my_email,
    })
}

/// Move a ticket to a new status via `jira issue move`.
pub async fn move_ticket(key: &str, status: &str) -> Result<()> {
    run_cmd("jira", &["issue", "move", key, status]).await?;
    Ok(())
}
