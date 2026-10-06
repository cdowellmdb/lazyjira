//! What goes through the `jira` CLI: `jira me`, creating tickets, comments, assignment and field
//! edits, plus the browser URL of a ticket. Ticket reads are in `jira_reads`.

use anyhow::{Context, Result};
use tokio::process::Command;

use crate::local_cache::{load_my_email, save_my_email};

/// The ticket's page in the Jira web UI.
pub fn browse_url(key: &str) -> Result<String> {
    Ok(format!(
        "{}/browse/{}",
        crate::jira_rest::server_url()?,
        key
    ))
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

/// Ask `jira me` for the current user's email, and remember it for later refreshes.
pub async fn refresh_my_email(project: &str) -> Result<String> {
    let email = crate::cache::normalize_email(&fetch_my_email().await?);
    save_my_email(project, &email)?;
    Ok(email)
}

/// The remembered email, so a refresh doesn't wait on `jira me`; asks Jira only the first time.
pub async fn my_email(project: &str) -> Result<String> {
    match load_my_email(project) {
        Some(email) => Ok(email),
        None => refresh_my_email(project).await,
    }
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
