use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level application configuration, persisted as TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub jira: JiraConfig,
    #[serde(default)]
    pub team: BTreeMap<String, String>,
    #[serde(default)]
    pub statuses: StatusConfig,
    #[serde(default = "default_resolutions")]
    pub resolutions: Vec<String>,
    #[serde(default)]
    pub filters: Vec<SavedFilter>,
}

/// Jira project and team settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraConfig {
    pub project: String,
    pub team_name: String,
    #[serde(default = "default_done_window_days")]
    pub done_window_days: u32,
}

fn default_done_window_days() -> u32 {
    14
}

/// Which status names are considered active vs done.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusConfig {
    #[serde(default = "default_active_statuses")]
    pub active: Vec<String>,
    #[serde(default = "default_done_statuses")]
    pub done: Vec<String>,
}

fn default_active_statuses() -> Vec<String> {
    vec![
        "Needs Triage".to_string(),
        "Ready for Work".to_string(),
        "To Do".to_string(),
        "In Progress".to_string(),
        "In Review".to_string(),
        "Blocked".to_string(),
    ]
}

fn default_done_statuses() -> Vec<String> {
    vec!["Done".to_string(), "Closed".to_string()]
}

pub fn default_resolutions() -> Vec<String> {
    vec![
        "Done".to_string(),
        "Duplicate".to_string(),
        "Won't Do".to_string(),
        "Cannot Reproduce".to_string(),
        "Community Answered".to_string(),
        "Declined".to_string(),
        "Fixed".to_string(),
        "Gone away".to_string(),
        "Incomplete".to_string(),
        "Won't Fix".to_string(),
        "Works as Designed".to_string(),
    ]
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            active: default_active_statuses(),
            done: default_done_statuses(),
        }
    }
}

/// A saved JQL filter with a display name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedFilter {
    pub name: String,
    pub jql: String,
}

impl AppConfig {
    pub fn team_members(&self) -> Vec<crate::cache::TeamMember> {
        let mut seen = std::collections::HashSet::new();
        let mut members = Vec::new();
        for (name, email) in &self.team {
            if !email.is_empty() && seen.insert(email.clone()) {
                members.push(crate::cache::TeamMember {
                    name: name.clone(),
                    email: email.clone(),
                });
            }
        }
        members
    }

    pub fn active_status_clause(&self) -> String {
        let quoted: Vec<String> = self.statuses.active.iter().map(|s| format!("\"{}\"", s)).collect();
        format!("({})", quoted.join(", "))
    }

    pub fn done_status_clause(&self) -> String {
        let quoted: Vec<String> = self.statuses.done.iter().map(|s| format!("\"{}\"", s)).collect();
        format!("({})", quoted.join(", "))
    }

    pub fn done_window(&self) -> String {
        format!("-{}d", self.jira.done_window_days)
    }
}

/// Returns the lazyjira config directory path (`~/.config/lazyjira/`).
pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".config").join("lazyjira"))
}

/// Returns the config file path (`~/.config/lazyjira/config.toml`).
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

/// Load the config from disk. Returns `Ok(None)` if the file does not exist.
pub fn load_config() -> Result<Option<AppConfig>> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let config: AppConfig =
        toml::from_str(&content).context("Failed to parse config.toml")?;
    Ok(Some(config))
}

/// Write the config to disk, creating the directory if needed.
pub fn save_config(config: &AppConfig) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create config directory: {}", dir.display()))?;
    let path = dir.join("config.toml");
    let content = toml::to_string_pretty(config).context("Failed to serialize config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write config file: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> AppConfig {
        let mut team = BTreeMap::new();
        team.insert("alice".to_string(), "alice@example.com".to_string());
        team.insert("bob".to_string(), "bob@example.com".to_string());

        AppConfig {
            jira: JiraConfig {
                project: "AMP".to_string(),
                team_name: "Code Generation".to_string(),
                done_window_days: 14,
            },
            team,
            statuses: StatusConfig::default(),
            resolutions: default_resolutions(),
            filters: vec![SavedFilter {
                name: "My bugs".to_string(),
                jql: "type = Bug AND assignee = currentUser()".to_string(),
            }],
        }
    }

    #[test]
    fn round_trip_serialization() {
        let config = sample_config();
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: AppConfig = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(parsed.jira.project, config.jira.project);
        assert_eq!(parsed.jira.team_name, config.jira.team_name);
        assert_eq!(parsed.jira.done_window_days, config.jira.done_window_days);
        assert_eq!(parsed.team.len(), config.team.len());
        assert_eq!(parsed.team.get("alice"), Some(&"alice@example.com".to_string()));
        assert_eq!(parsed.statuses.active, config.statuses.active);
        assert_eq!(parsed.statuses.done, config.statuses.done);
        assert_eq!(parsed.filters.len(), 1);
        assert_eq!(parsed.filters[0].name, "My bugs");
        assert_eq!(parsed.filters[0].jql, config.filters[0].jql);
    }

    #[test]
    fn defaults_applied_when_fields_omitted() {
        let minimal_toml = r#"
[jira]
project = "TEST"
team_name = "My Team"
"#;
        let config: AppConfig = toml::from_str(minimal_toml).expect("parse minimal config");

        assert_eq!(config.jira.project, "TEST");
        assert_eq!(config.jira.team_name, "My Team");
        assert_eq!(config.jira.done_window_days, 14);
        assert_eq!(config.statuses.active, default_active_statuses());
        assert_eq!(config.statuses.done, default_done_statuses());
        assert!(config.team.is_empty());
        assert!(config.filters.is_empty());
    }

    #[test]
    fn status_config_default_matches_hardcoded_statuses() {
        let defaults = StatusConfig::default();
        assert!(defaults.active.contains(&"Needs Triage".to_string()));
        assert!(defaults.active.contains(&"Ready for Work".to_string()));
        assert!(defaults.active.contains(&"To Do".to_string()));
        assert!(defaults.active.contains(&"In Progress".to_string()));
        assert!(defaults.active.contains(&"In Review".to_string()));
        assert!(defaults.active.contains(&"Blocked".to_string()));
        assert_eq!(defaults.active.len(), 6);

        assert!(defaults.done.contains(&"Done".to_string()));
        assert!(defaults.done.contains(&"Closed".to_string()));
        assert_eq!(defaults.done.len(), 2);
    }

    #[test]
    fn done_window_days_default_is_14() {
        assert_eq!(default_done_window_days(), 14);
    }
}
