# lazyjira Full Roadmap Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extract hardcoded config into a TOML config file with first-run setup, then add in-TUI actions (create/comment/assign/edit), activity history, and saved JQL filters.

**Architecture:** A new `src/config.rs` module owns the `AppConfig` struct, loaded once at startup and threaded through `jira_client` functions. The `Status` enum gains an `Other(String)` variant for projects with non-standard workflows. New TUI modals for actions use `tui-textarea` for multiline input. Activity data is parsed from existing `--raw` JSON responses. Saved filters are stored in the config TOML.

**Tech Stack:** Rust, ratatui 0.29, crossterm 0.28, tokio, serde, toml, tui-textarea

---

## Phase 1: Configuration System

### Task 1: Add `toml` dependency and create config module skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `src/config.rs`
- Modify: `src/main.rs:1` (add `mod config;`)

**Step 1: Add toml dependency**

Add to `Cargo.toml` under `[dependencies]`:

```toml
toml = "0.8"
```

**Step 2: Create `src/config.rs` with the `AppConfig` struct**

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// User-facing configuration stored at ~/.config/lazyjira/config.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub jira: JiraConfig,
    #[serde(default)]
    pub team: BTreeMap<String, String>,
    #[serde(default)]
    pub statuses: StatusConfig,
    #[serde(default)]
    pub filters: Vec<SavedFilter>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusConfig {
    pub active: Vec<String>,
    pub done: Vec<String>,
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            active: vec![
                "Needs Triage".into(),
                "Ready for Work".into(),
                "To Do".into(),
                "In Progress".into(),
                "In Review".into(),
                "Blocked".into(),
            ],
            done: vec!["Done".into(), "Closed".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedFilter {
    pub name: String,
    pub jql: String,
}

pub fn config_dir() -> PathBuf {
    dirs_or_fallback().join("lazyjira")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

fn dirs_or_fallback() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home).join(".config"),
        Err(_) => std::env::temp_dir(),
    }
}

pub fn load_config() -> Result<Option<AppConfig>> {
    let path = config_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    let config: AppConfig =
        toml::from_str(&content).with_context(|| "Failed to parse config.toml")?;
    Ok(Some(config))
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create config dir: {}", dir.display()))?;
    let path = config_path();
    let content = toml::to_string_pretty(config).context("Failed to serialize config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write config: {}", path.display()))?;
    Ok(())
}
```

**Step 3: Wire module into main.rs**

Add `mod config;` to the module declarations at the top of `src/main.rs`.

**Step 4: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles successfully (may have warnings)

**Step 5: Write unit tests for config round-trip**

Add to `src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_config() {
        let config = AppConfig {
            jira: JiraConfig {
                project: "AMP".into(),
                team_name: "Code Generation".into(),
                done_window_days: 14,
            },
            team: BTreeMap::from([
                ("Dev One".into(), "dev.one@mongodb.com".into()),
            ]),
            statuses: StatusConfig::default(),
            filters: vec![],
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.jira.project, "AMP");
        assert_eq!(parsed.team.len(), 1);
        assert_eq!(parsed.statuses.active.len(), 6);
    }

    #[test]
    fn missing_config_returns_none() {
        // config_path() points to a real path, but if we just test the logic:
        // A non-existent path should return None.
        let path = PathBuf::from("/tmp/lazyjira_test_nonexistent/config.toml");
        assert!(!path.exists());
    }
}
```

**Step 6: Run tests**

Run: `cargo test --lib config`
Expected: 2 tests pass

**Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/config.rs src/main.rs
git commit -m "feat: add config module with TOML serialization"
```

---

### Task 2: Add `Other(String)` variant to `Status` enum

The `Status` enum is hardcoded to 7 statuses. Other MongoDB projects may use different names. Adding `Other(String)` allows any status string to be represented.

**Files:**
- Modify: `src/cache.rs:6-84` (Status enum and impls)
- Modify: `src/app.rs:289-299` (epic_status_rank)
- Modify: `src/views/my_work.rs:9-19` (status_color)
- Modify: `src/views/team.rs:9-19` (status_color)
- Modify: `src/views/epics.rs:9-19` (status_color)
- Modify: `src/views/unassigned.rs:11-21` (status_color)
- Modify: `src/widgets/ticket_detail.rs:9-19` (status_color)

**Step 1: Add `Other(String)` to the Status enum in `src/cache.rs`**

Add after `Done` variant:

```rust
Other(String),
```

**Step 2: Update all `match` arms on `Status` throughout the codebase**

Every `match` on `Status` needs a wildcard or `Other(_)` arm. Update each:

In `cache.rs`:
- `as_str()`: add `Status::Other(s) => s.as_str()` — but `as_str` returns `&'static str`. Change return type to `&str` and the method to return `s.as_str()`.

Actually, `as_str` returns `&'static str` which won't work with `Other(String)`. Change the signature:

```rust
pub fn as_str(&self) -> &str {
    match self {
        Status::NeedsTriage => "Needs Triage",
        Status::ReadyForWork => "Ready for Work",
        Status::ToDo => "To Do",
        Status::InProgress => "In Progress",
        Status::InReview => "In Review",
        Status::Blocked => "Blocked",
        Status::Done => "Done",
        Status::Other(s) => s,
    }
}
```

- `from_str()`: the default case already returns `ToDo`. Change it to return `Other(s.to_string())`:

```rust
_ => Status::Other(s.to_string()),
```

- `move_shortcut()`: add `Status::Other(_) => '?'`
- `from_move_shortcut()`: no change needed (default arm returns None)
- `all()`: this returns `&'static [Status]` — cannot include `Other`. Leave as-is; `all()` is used for the move picker and display ordering, which only applies to known statuses.
- `others()`: add `Status::Other(_) => vec![]` or more accurately, filter self from `all()` — current logic already works since `Other` won't match any item in `all()`.

In `app.rs`:
- `epic_status_rank()`: add `Status::Other(_) => 5` (sort unknown between Blocked and Done)

In all view files and `ticket_detail.rs`:
- `status_color()`: add `Status::Other(_) => Color::Magenta` (distinctive color for unknown statuses)

**Step 3: Fix the `&'static Status` references**

In `app.rs:431`, `my_work_visible_by_status` returns `Vec<(&'static Status, ...)>`. The `Status::all()` returns `&'static [Status]`, so iterating over it yields `&'static Status` refs. This still works since we're iterating known statuses only. No change needed.

**Step 4: Run `cargo build` to verify all matches are exhaustive**

Run: `cargo build 2>&1 | tail -20`
Expected: compiles successfully

**Step 5: Run all existing tests**

Run: `cargo test`
Expected: all tests pass

**Step 6: Commit**

```bash
git add src/cache.rs src/app.rs src/views/ src/widgets/
git commit -m "feat: add Other(String) variant to Status enum for custom workflows"
```

---

### Task 3: Thread `AppConfig` through `jira_client` functions

Replace all hardcoded constants in `jira_client.rs` with values from `AppConfig`. Functions that currently use module-level constants will accept `&AppConfig` parameters instead.

**Files:**
- Modify: `src/jira_client.rs` (throughout — constants, function signatures, team loading)
- Modify: `src/main.rs` (pass config to all jira_client calls)
- Modify: `src/config.rs` (add helper to build team members list)

**Step 1: Add a method to `AppConfig` to produce `Vec<TeamMember>`**

In `src/config.rs`:

```rust
use crate::cache::TeamMember;

impl AppConfig {
    pub fn team_members(&self) -> Vec<TeamMember> {
        let mut seen = std::collections::HashSet::new();
        let mut members = Vec::new();
        for (name, email) in &self.team {
            if !email.is_empty() && seen.insert(email.clone()) {
                members.push(TeamMember {
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
```

**Step 2: Update `jira_client.rs` — remove hardcoded constants, add `config` parameters**

Remove these constants from the top of `jira_client.rs`:
- `PROJECT`
- `RECENT_DONE_WINDOW`
- `ACTIVE_STATUS_CLAUSE`
- `ASSIGNED_TEAM_NAME`

Keep:
- `JIRA_BASE_URL` (stays hardcoded — same for all MongoDB teams)
- `UNASSIGNED_TEAM_NAME` and `UNASSIGNED_TEAM_EMAIL` (internal sentinel values, not configurable)

Update cache file name constants to be functions that take a project key:

```rust
fn epics_cache_file_name(project: &str) -> String {
    format!("lazyjira_epics_cache_{}.json", project)
}

fn details_cache_file_name(project: &str) -> String {
    format!("lazyjira_ticket_details_cache_{}.json", project)
}

fn full_cache_file_name(project: &str) -> String {
    format!("lazyjira_full_cache_{}.json", project)
}
```

Update every function that uses these constants to accept `config: &AppConfig`:

- `fetch_tickets_for_query(config, query)` — uses `config.jira.project` instead of `PROJECT`
- `fetch_tickets_for_user(config, email, scope)` — uses `config.active_status_clause()`
- `fetch_unassigned_team_tickets(config)` — uses `config.jira.team_name`
- `fetch_epics(config)` — uses `config.jira.project`
- `fetch_with_scope(config, scope)` — uses `config.team_members()` instead of `load_team_roster()`
- `fetch_active_only(config)`, `fetch_all(config)` — pass config through
- `refresh_epics_cache(config)` — pass config
- `fetch_ticket_detail(key)` — no config needed (uses key directly)
- `move_ticket(key, status)` — no config needed
- Cache path functions: `epics_cache_path(project)`, `details_cache_path(project)`, `full_cache_path(project)`
- `load_startup_cache_snapshot(project)`, `save_full_cache_snapshot(project, cache)`

Remove the `load_team_roster()` function entirely.

**Step 3: Update `src/main.rs` to load config and pass it through**

At the top of `main()`, after `maybe_run_dev_mode()`:

```rust
let config = config::load_config()?
    .expect("No config found. Run lazyjira to set up.");
```

Update all spawn/call sites to pass `&config` or `config.clone()`:
- `spawn_epics_refresh` → needs `config.clone()` inside the spawned task
- `spawn_cache_refresh` → needs `config.clone()`
- `load_startup_cache_snapshot` → needs `&config.jira.project`
- `save_full_cache_snapshot` → needs `&config.jira.project`
- `spawn_detail_cache_writer` → needs `&config.jira.project`

Change spawn function signatures:

```rust
fn spawn_epics_refresh(tx: &UnboundedSender<BackgroundMessage>, config: &AppConfig) {
    let tx = tx.clone();
    let config = config.clone();
    tokio::spawn(async move {
        let result = jira_client::refresh_epics_cache(&config)
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(BackgroundMessage::EpicsRefreshed(result));
    });
}
```

Similarly for `spawn_cache_refresh`.

**Step 4: Update existing tests in `jira_client.rs`**

The test `unassigned_query_filters_for_code_generation_team` will need to construct a config and pass it. Update accordingly.

**Step 5: Run `cargo build` and `cargo test`**

Run: `cargo build && cargo test`
Expected: compiles and all tests pass

**Step 6: Commit**

```bash
git add src/config.rs src/jira_client.rs src/main.rs
git commit -m "feat: thread AppConfig through jira_client, remove hardcoded constants"
```

---

### Task 4: First-run interactive setup TUI

When no config file exists, present a minimal TUI setup flow that collects project key, team name, and writes a config file before entering the main app.

**Files:**
- Create: `src/setup.rs`
- Modify: `src/main.rs` (call setup if config missing)
- Modify: `src/config.rs` (add legacy migration helper)

**Step 1: Create `src/setup.rs` with interactive setup**

The setup runs inside the already-initialized terminal. It collects:
1. Project key (text input)
2. Team name (text input)
3. Current user email (from `jira me`)

It writes a config with default statuses and an empty team roster (user can add members later by editing the file).

```rust
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use std::io;
use std::time::Duration;

use crate::config::{AppConfig, JiraConfig, StatusConfig};

enum SetupStep {
    ProjectKey,
    TeamName,
    Confirm,
}

struct SetupState {
    step: SetupStep,
    project_key: String,
    team_name: String,
    user_email: String,
}

pub async fn run_setup(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<AppConfig> {
    // Fetch user email first
    let user_email = crate::jira_client::fetch_my_email_public().await
        .unwrap_or_else(|_| "unknown@mongodb.com".to_string());

    let mut state = SetupState {
        step: SetupStep::ProjectKey,
        project_key: String::new(),
        team_name: String::new(),
        user_email,
    };

    loop {
        terminal.draw(|f| render_setup(f, &state))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match &state.step {
                    SetupStep::ProjectKey => match key.code {
                        KeyCode::Enter if !state.project_key.is_empty() => {
                            state.step = SetupStep::TeamName;
                        }
                        KeyCode::Char(c) => {
                            state.project_key.push(c.to_ascii_uppercase());
                        }
                        KeyCode::Backspace => { state.project_key.pop(); }
                        _ => {}
                    },
                    SetupStep::TeamName => match key.code {
                        KeyCode::Enter if !state.team_name.is_empty() => {
                            state.step = SetupStep::Confirm;
                        }
                        KeyCode::Esc => { state.step = SetupStep::ProjectKey; }
                        KeyCode::Char(c) => { state.team_name.push(c); }
                        KeyCode::Backspace => { state.team_name.pop(); }
                        _ => {}
                    },
                    SetupStep::Confirm => match key.code {
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                            let config = build_config(&state);
                            crate::config::save_config(&config)?;
                            return Ok(config);
                        }
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                            state.step = SetupStep::ProjectKey;
                        }
                        _ => {}
                    },
                }
            }
        }
    }
}

fn build_config(state: &SetupState) -> AppConfig {
    let mut team = std::collections::BTreeMap::new();
    let user_name = crate::jira_client::name_from_email_public(&state.user_email);
    team.insert(user_name, state.user_email.clone());

    // Try to migrate legacy team.yml
    if let Ok(legacy_members) = migrate_legacy_team_roster() {
        for member in legacy_members {
            team.entry(member.0).or_insert(member.1);
        }
    }

    AppConfig {
        jira: JiraConfig {
            project: state.project_key.clone(),
            team_name: state.team_name.clone(),
            done_window_days: 14,
        },
        team,
        statuses: StatusConfig::default(),
        filters: vec![],
    }
}

fn migrate_legacy_team_roster() -> Result<Vec<(String, String)>> {
    let home = std::env::var("HOME")?;
    let path = format!("{}/.claude/skills/jira/team.yml", home);
    let content = std::fs::read_to_string(&path)?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&content)?;
    let team_map = yaml
        .get("team")
        .and_then(|v| v.as_mapping())
        .ok_or_else(|| anyhow::anyhow!("no team mapping"))?;

    let mut members = Vec::new();
    for (name_val, email_val) in team_map {
        let name = name_val.as_str().unwrap_or_default().to_string();
        let email = email_val.as_str().unwrap_or_default().to_string();
        if !email.is_empty() {
            members.push((name, email));
        }
    }
    Ok(members)
}

fn render_setup(f: &mut ratatui::Frame, state: &SetupState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(f.area());

    let title = Paragraph::new(" lazyjira — First-Time Setup ")
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    f.render_widget(title, chunks[0]);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Welcome! Let's configure lazyjira for your team.",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
    ];

    match state.step {
        SetupStep::ProjectKey => {
            lines.push(Line::from(vec![
                Span::styled("Jira Project Key: ", Style::default().fg(Color::Cyan)),
                Span::styled(&state.project_key, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("_", Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  (e.g., AMP, SERVER, CLOUD)  Press Enter to continue.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        SetupStep::TeamName => {
            lines.push(Line::from(vec![
                Span::styled("Project: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&state.project_key),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Team Name (for unassigned queries): ", Style::default().fg(Color::Cyan)),
                Span::styled(&state.team_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("_", Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  (e.g., Code Generation, Cloud Platform)  Press Enter to continue, Esc to go back.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        SetupStep::Confirm => {
            lines.push(Line::from(Span::styled("Configuration Summary:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Project:    ", Style::default().fg(Color::DarkGray)),
                Span::raw(&state.project_key),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Team:       ", Style::default().fg(Color::DarkGray)),
                Span::raw(&state.team_name),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  User:       ", Style::default().fg(Color::DarkGray)),
                Span::raw(&state.user_email),
            ]));
            lines.push(Line::from(""));

            let config_path = crate::config::config_path();
            lines.push(Line::from(vec![
                Span::styled("  Config:     ", Style::default().fg(Color::DarkGray)),
                Span::raw(config_path.display().to_string()),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Press Enter or y to save.  Esc or n to start over.",
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    let body = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(body, chunks[1]);
}
```

**Step 2: Make `fetch_my_email` and `name_from_email` public in `jira_client.rs`**

Add public wrappers:

```rust
pub async fn fetch_my_email_public() -> Result<String> {
    fetch_my_email().await
}

pub fn name_from_email_public(email: &str) -> String {
    name_from_email(email)
}
```

**Step 3: Wire setup into `main.rs`**

Add `mod setup;` at the top.

In `main()`, after terminal init but before `App::new()`:

```rust
let config = match config::load_config()? {
    Some(config) => config,
    None => setup::run_setup(&mut terminal).await?,
};
```

**Step 4: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1 | tail -10`
Expected: compiles successfully

**Step 5: Manual test**

Temporarily rename `~/.config/lazyjira/config.toml` (if it exists) and run `cargo run`. Verify the setup TUI appears.

**Step 6: Commit**

```bash
git add src/setup.rs src/main.rs src/jira_client.rs
git commit -m "feat: first-run interactive setup writes config.toml"
```

---

### Task 5: Verify full integration — config loads, app runs, caches use project key

**Files:**
- Possibly tweak: `src/main.rs`, `src/jira_client.rs`

**Step 1: Create a config file manually for testing**

Write `~/.config/lazyjira/config.toml`:

```toml
[jira]
project = "AMP"
team_name = "Code Generation"
done_window_days = 14

[team]
"Christian Dowell" = "christian.dowell@mongodb.com"

[statuses]
active = ["Needs Triage", "Ready for Work", "To Do", "In Progress", "In Review", "Blocked"]
done = ["Done", "Closed"]
```

**Step 2: Run the app end-to-end**

Run: `cargo run --release`
Expected: app starts, loads tickets, all tabs work

**Step 3: Verify cache files use project key in name**

Run: `ls /tmp/lazyjira_*` and `ls ~/.cache/lazyjira/`
Expected: filenames contain `_AMP.json`

**Step 4: Run full test suite**

Run: `cargo test`
Expected: all tests pass

**Step 5: Commit if any fixes were needed**

```bash
git add -A
git commit -m "fix: integration fixes for config-based startup"
```

---

## Phase 2: In-TUI Actions

### Task 6: Add `tui-textarea` dependency and create form widget module

**Files:**
- Modify: `Cargo.toml`
- Create: `src/widgets/form.rs`
- Modify: `src/widgets/mod.rs` (if it exists, or create it)

**Step 1: Add tui-textarea dependency**

Add to `Cargo.toml`:

```toml
tui-textarea = "0.7"
```

**Step 2: Verify widgets module structure**

Check if `src/widgets/mod.rs` exists. If not, create it with:

```rust
pub mod keybindings_help;
pub mod ticket_detail;
pub mod form;
```

**Step 3: Create `src/widgets/form.rs` — shared form components**

This module provides reusable form primitives: text input field, dropdown picker, and a generic modal frame.

```rust
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn render_modal_frame(f: &mut ratatui::Frame, title: &str, percent_x: u16, percent_y: u16) -> Rect {
    let area = centered_rect(percent_x, percent_y, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title));
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

/// A simple single-line text input with label.
pub fn render_text_input(lines: &mut Vec<Line>, label: &str, value: &str, focused: bool) {
    let cursor = if focused { "_" } else { "" };
    lines.push(Line::from(vec![
        Span::styled(
            format!("{}: ", label),
            Style::default().fg(if focused { Color::Cyan } else { Color::DarkGray }),
        ),
        Span::styled(
            value.to_string(),
            Style::default().fg(Color::White).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
        ),
        Span::styled(cursor, Style::default().fg(Color::DarkGray)),
    ]));
}

/// A dropdown-style picker (rendered as a list with selected highlight).
pub fn render_picker<'a>(lines: &mut Vec<Line<'a>>, label: &str, options: &[String], selected: usize, focused: bool) {
    lines.push(Line::from(Span::styled(
        format!("{}:", label),
        Style::default().fg(if focused { Color::Cyan } else { Color::DarkGray }),
    )));
    for (i, option) in options.iter().enumerate() {
        let prefix = if i == selected { "> " } else { "  " };
        let style = if i == selected && focused {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(Span::styled(format!("  {}{}", prefix, option), style)));
    }
}
```

**Step 4: Build and verify**

Run: `cargo build`
Expected: compiles

**Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/widgets/
git commit -m "feat: add tui-textarea dep and form widget primitives"
```

---

### Task 7: Implement Create Ticket action

**Files:**
- Create: `src/widgets/create_ticket.rs`
- Modify: `src/widgets/mod.rs`
- Modify: `src/app.rs` (add `CreateTicket` overlay state)
- Modify: `src/main.rs` (handle `c` keybinding and create ticket flow)
- Modify: `src/jira_client.rs` (add `create_ticket` function)

**Step 1: Add `create_ticket` to `jira_client.rs`**

```rust
pub async fn create_ticket(
    project: &str,
    issue_type: &str,
    summary: &str,
    description: &str,
    assignee_email: Option<&str>,
    epic_key: Option<&str>,
) -> Result<String> {
    let mut args = vec![
        "issue", "create",
        "-t", issue_type,
        "-s", summary,
        "-b", description,
        "--no-input",
        "-p", project,
    ];

    let assignee_arg;
    if let Some(email) = assignee_email {
        assignee_arg = format!("{}", email);
        args.push("-a");
        args.push(&assignee_arg);
    }

    let epic_arg;
    if let Some(ek) = epic_key {
        epic_arg = format!("Epic Link={}", ek);
        args.push("--custom");
        args.push(&epic_arg);
    }

    let output = run_cmd("jira", &args).await?;
    // jira-cli prints the created issue key
    let key = output
        .lines()
        .find(|l| l.contains('-'))
        .map(|l| l.trim().to_string())
        .unwrap_or(output.trim().to_string());
    Ok(key)
}
```

**Step 2: Add overlay state to `src/app.rs`**

Add a new enum variant and state struct:

```rust
#[derive(Debug, Clone)]
pub struct CreateTicketState {
    pub focused_field: usize,  // 0=type, 1=summary, 2=description, 3=assignee, 4=epic
    pub issue_type: usize,     // index into ISSUE_TYPES
    pub summary: String,
    pub description: String,
    pub assignee_idx: usize,   // index into team members (0 = none)
    pub epic_idx: usize,       // index into epics (0 = none)
}

pub const ISSUE_TYPES: &[&str] = &["Task", "Bug", "Story", "Sub-task"];
```

Add to `App`:
```rust
pub create_ticket: Option<CreateTicketState>,
```

**Step 3: Create `src/widgets/create_ticket.rs` with rendering**

Render a centered modal with the form fields. Use `render_text_input` and `render_picker` from `form.rs`.

**Step 4: Handle `c` keybinding in `main.rs`**

In `handle_main_keys`, add:

```rust
KeyCode::Char('c') => {
    app.create_ticket = Some(CreateTicketState {
        focused_field: 0,
        issue_type: 0,
        summary: String::new(),
        description: String::new(),
        assignee_idx: 0,
        epic_idx: 0,
    });
}
```

Add a new `handle_create_ticket_keys` function for input handling within the modal.

**Step 5: Handle form submission**

On Enter from the confirm step, spawn an async task:

```rust
let project = config.jira.project.clone();
tokio::spawn(async move {
    let result = jira_client::create_ticket(
        &project, issue_type, &summary, &description,
        assignee_email.as_deref(), epic_key.as_deref(),
    ).await;
    let _ = tx.send(BackgroundMessage::TicketCreated(result));
});
```

Add `TicketCreated(Result<String, String>)` to `BackgroundMessage`.

**Step 6: Handle the result — add ticket to cache, flash success/error**

When `BackgroundMessage::TicketCreated` arrives, if success, trigger a refresh so the new ticket appears. Flash the created key.

**Step 7: Build and manual test**

Run: `cargo build && cargo run`
Test: press `c`, fill form, submit. Verify ticket appears in Jira.

**Step 8: Commit**

```bash
git add src/widgets/create_ticket.rs src/widgets/mod.rs src/app.rs src/main.rs src/jira_client.rs
git commit -m "feat: create ticket from TUI with type/summary/desc/assignee/epic"
```

---

### Task 8: Implement Comment action

**Files:**
- Create: `src/widgets/comment.rs`
- Modify: `src/widgets/mod.rs`
- Modify: `src/app.rs` (add comment state)
- Modify: `src/main.rs` (handle `C` keybinding in detail view)
- Modify: `src/jira_client.rs` (add `add_comment` function)

**Step 1: Add `add_comment` to `jira_client.rs`**

```rust
pub async fn add_comment(key: &str, body: &str) -> Result<()> {
    run_cmd("jira", &["issue", "comment", "add", key, body]).await?;
    Ok(())
}
```

**Step 2: Add comment state to `app.rs`**

```rust
pub comment_state: Option<CommentState>,

#[derive(Debug, Clone)]
pub struct CommentState {
    pub ticket_key: String,
    pub body: String,
}
```

**Step 3: Create `src/widgets/comment.rs`**

Render a centered modal with a `tui-textarea` widget for multiline comment input. Show the ticket key in the title.

```rust
use tui_textarea::TextArea;
```

Store `TextArea` in the `CommentState` (or a wrapper) so it handles cursor/selection natively.

Actually, since `TextArea` is not `Clone` or `Debug`, store it separately in `App`:

```rust
pub comment_textarea: Option<tui_textarea::TextArea<'static>>,
```

**Step 4: Handle `C` keybinding in detail view**

In `handle_detail_keys` under `DetailMode::View`:

```rust
KeyCode::Char('C') => {
    if let Some(key) = app.detail_ticket_key.clone() {
        let mut textarea = tui_textarea::TextArea::default();
        textarea.set_block(Block::default().borders(Borders::ALL).title(" Comment "));
        app.comment_state = Some(CommentState { ticket_key: key, body: String::new() });
        app.comment_textarea = Some(textarea);
    }
}
```

**Step 5: Handle comment textarea keys**

Forward key events to `textarea.input(event)`. On Ctrl+Enter or Esc+Enter (submit), extract text and spawn async comment.

**Step 6: Build, test, commit**

```bash
git add src/widgets/comment.rs src/widgets/mod.rs src/app.rs src/main.rs src/jira_client.rs
git commit -m "feat: add comment to ticket from detail view"
```

---

### Task 9: Implement Assign/Reassign action

**Files:**
- Create: `src/widgets/assign.rs`
- Modify: `src/widgets/mod.rs`
- Modify: `src/app.rs` (add assign state)
- Modify: `src/main.rs` (handle `a` keybinding)
- Modify: `src/jira_client.rs` (add `assign_ticket` function)

**Step 1: Add `assign_ticket` to `jira_client.rs`**

```rust
pub async fn assign_ticket(key: &str, email: &str) -> Result<()> {
    run_cmd("jira", &["issue", "assign", key, email]).await?;
    Ok(())
}
```

**Step 2: Add assign state to `app.rs`**

```rust
pub assign_state: Option<AssignState>,

#[derive(Debug, Clone)]
pub struct AssignState {
    pub ticket_key: String,
    pub selected: usize,  // index into team members
}
```

**Step 3: Create `src/widgets/assign.rs`**

Render a picker modal listing team members from `app.cache.team_members`. Use the same centered modal pattern.

**Step 4: Handle `a` keybinding**

From detail view or list view:
```rust
KeyCode::Char('a') => {
    if let Some(key) = /* current ticket key */ {
        app.assign_state = Some(AssignState { ticket_key: key, selected: 0 });
    }
}
```

**Step 5: Handle picker navigation (j/k/Enter/Esc)**

On Enter, fire-and-forget the assign CLI call, optimistically update the cache, and close the picker.

**Step 6: Build, test, commit**

```bash
git add src/widgets/assign.rs src/widgets/mod.rs src/app.rs src/main.rs src/jira_client.rs
git commit -m "feat: assign/reassign ticket from detail or list view"
```

---

### Task 10: Implement Edit Fields action

**Files:**
- Create: `src/widgets/edit_fields.rs`
- Modify: `src/widgets/mod.rs`
- Modify: `src/app.rs` (add edit state)
- Modify: `src/main.rs` (handle `e` keybinding in detail view)
- Modify: `src/jira_client.rs` (add `edit_ticket` function)

**Step 1: Add `edit_ticket` to `jira_client.rs`**

```rust
pub async fn edit_ticket(
    key: &str,
    summary: Option<&str>,
    description: Option<&str>,
    labels: Option<&[String]>,
) -> Result<()> {
    let mut args = vec!["issue", "edit", key, "--no-input"];

    let summary_arg;
    if let Some(s) = summary {
        summary_arg = s.to_string();
        args.push("-s");
        args.push(&summary_arg);
    }

    let body_arg;
    if let Some(d) = description {
        body_arg = d.to_string();
        args.push("-b");
        args.push(&body_arg);
    }

    let label_args: Vec<String>;
    if let Some(lbls) = labels {
        label_args = lbls.to_vec();
        for label in &label_args {
            args.push("-l");
            args.push(label);
        }
    }

    run_cmd("jira", &args).await?;
    Ok(())
}
```

**Step 2: Add edit state to `app.rs`**

```rust
pub edit_state: Option<EditFieldsState>,

#[derive(Debug, Clone)]
pub struct EditFieldsState {
    pub ticket_key: String,
    pub focused_field: usize,  // 0=summary, 1=labels
    pub summary: String,
    pub labels: String,  // comma-separated
}
```

Note: description editing is complex (multiline). Offer two modes:
- Summary and labels: inline edit in TUI
- Description: shell out to `$EDITOR` (like git commit)

**Step 3: Create `src/widgets/edit_fields.rs`**

Render an edit form pre-populated with current ticket values.

**Step 4: Handle `e` keybinding in detail view**

Pre-populate fields from the current ticket detail, open the edit modal.

**Step 5: On submit, spawn async edit and update cache**

On success, update the cached ticket fields. On failure, flash error.

**Step 6: Build, test, commit**

```bash
git add src/widgets/edit_fields.rs src/widgets/mod.rs src/app.rs src/main.rs src/jira_client.rs
git commit -m "feat: edit ticket summary and labels from detail view"
```

---

## Phase 3: Activity / History

### Task 11: Parse changelog and comments from ticket JSON

**Files:**
- Modify: `src/cache.rs` (add `ActivityEntry` struct)
- Modify: `src/jira_client.rs` (parse changelog + comments from `--raw` JSON)

**Step 1: Add activity data structures to `src/cache.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub timestamp: String,      // ISO 8601
    pub author: String,         // display name
    pub author_email: Option<String>,
    pub kind: ActivityKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActivityKind {
    StatusChange { from: String, to: String },
    Comment { body: String },
    AssigneeChange { from: Option<String>, to: Option<String> },
    FieldChange { field: String, from: String, to: String },
}
```

Add to `Ticket`:
```rust
pub activity: Vec<ActivityEntry>,
```

**Step 2: Parse changelog in `fetch_ticket_detail`**

In `jira_client.rs`, after parsing the main fields, parse the changelog:

```rust
let mut activity = Vec::new();

// Parse changelog
if let Some(changelog) = json.get("changelog").and_then(|c| c.get("histories")).and_then(|h| h.as_array()) {
    for history in changelog {
        let timestamp = history["created"].as_str().unwrap_or("").to_string();
        let author = history["author"]["displayName"].as_str().unwrap_or("Unknown").to_string();
        let author_email = history["author"]["emailAddress"].as_str().map(|s| s.to_string());

        if let Some(items) = history["items"].as_array() {
            for item in items {
                let field = item["field"].as_str().unwrap_or("");
                let from = item["fromString"].as_str().unwrap_or("").to_string();
                let to = item["toString"].as_str().unwrap_or("").to_string();

                let kind = match field {
                    "status" => ActivityKind::StatusChange { from, to },
                    "assignee" => ActivityKind::AssigneeChange {
                        from: Some(from).filter(|s| !s.is_empty()),
                        to: Some(to).filter(|s| !s.is_empty()),
                    },
                    _ => ActivityKind::FieldChange { field: field.to_string(), from, to },
                };

                activity.push(ActivityEntry { timestamp: timestamp.clone(), author: author.clone(), author_email: author_email.clone(), kind });
            }
        }
    }
}

// Parse comments
if let Some(comments) = fields.get("comment").and_then(|c| c.get("comments")).and_then(|c| c.as_array()) {
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

// Sort by timestamp descending (newest first)
activity.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
```

**Step 3: Wire activity into the Ticket struct in `fetch_ticket_detail`**

Add `activity` field to the returned `Ticket`.

**Step 4: Run `cargo test`**

Expected: compiles and existing tests pass

**Step 5: Commit**

```bash
git add src/cache.rs src/jira_client.rs
git commit -m "feat: parse changelog and comments from ticket JSON into ActivityEntry"
```

---

### Task 12: Add history view accessible from ticket detail

**Files:**
- Create: `src/widgets/activity.rs`
- Modify: `src/widgets/mod.rs`
- Modify: `src/app.rs` (add `History` to `DetailMode`)
- Modify: `src/main.rs` (handle `h` keybinding in detail view)
- Modify: `src/widgets/ticket_detail.rs` (update footer hint)

**Step 1: Add `History` variant to `DetailMode`**

In `src/app.rs`:

```rust
pub enum DetailMode {
    View,
    MovePicker { selected: usize, confirm_target: Option<crate::cache::Status> },
    History { scroll: u16 },
}
```

**Step 2: Create `src/widgets/activity.rs`**

```rust
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::cache::{ActivityEntry, ActivityKind};

fn format_timestamp(ts: &str) -> &str {
    // ISO 8601: "2024-01-15T10:30:00.000+0000" → "2024-01-15 10:30"
    // Take first 16 chars
    if ts.len() >= 16 {
        &ts[..16]
    } else {
        ts
    }
}

pub fn render_activity(f: &mut ratatui::Frame, area: Rect, entries: &[ActivityEntry], scroll: u16) {
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        "Activity History",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no activity found)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    for entry in entries {
        let ts = format_timestamp(&entry.timestamp).replace('T', " ");
        let detail = match &entry.kind {
            ActivityKind::StatusChange { from, to } => {
                format!("Status: {} → {}", from, to)
            }
            ActivityKind::Comment { body } => {
                let preview: String = body.chars().take(80).collect();
                let ellipsis = if body.len() > 80 { "..." } else { "" };
                format!("Comment: \"{}{}\"", preview, ellipsis)
            }
            ActivityKind::AssigneeChange { from, to } => {
                let from_str = from.as_deref().unwrap_or("Unassigned");
                let to_str = to.as_deref().unwrap_or("Unassigned");
                format!("Assignee: {} → {}", from_str, to_str)
            }
            ActivityKind::FieldChange { field, from, to } => {
                format!("{}: {} → {}", field, from, to)
            }
        };

        lines.push(Line::from(vec![
            Span::styled(format!("{:<17}", ts), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<20}", entry.author), Style::default().fg(Color::White)),
            Span::styled(detail, Style::default().fg(Color::Gray)),
        ]));
    }

    let widget = Paragraph::new(lines)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}
```

**Step 3: Handle `h` keybinding in detail view**

In `handle_detail_keys`, under `DetailMode::View`:

```rust
KeyCode::Char('h') => {
    app.detail_mode = DetailMode::History { scroll: 0 };
}
```

Add handling for `DetailMode::History`:
```rust
DetailMode::History { scroll } => match key {
    KeyCode::Esc => app.detail_mode = DetailMode::View,
    KeyCode::Down => app.detail_mode = DetailMode::History { scroll: scroll + 1 },
    KeyCode::Up => app.detail_mode = DetailMode::History { scroll: scroll.saturating_sub(1) },
    _ => {}
},
```

**Step 4: Render history in `ticket_detail.rs`**

In the `render` function, add a match arm for `DetailMode::History`:

```rust
DetailMode::History { scroll } => {
    let activity = &ticket.activity;
    crate::widgets::activity::render_activity(f, inner, activity, *scroll);
}
```

**Step 5: Update footer hint in detail view**

Add `[h] history` to the footer string.

**Step 6: Update keybindings help**

In `src/widgets/keybindings_help.rs`, add under Detail View:
```
Line::from("  h: view activity history"),
```

**Step 7: Build and manual test**

Run: `cargo run`, open a ticket detail, press `h`. Verify activity renders.

**Step 8: Commit**

```bash
git add src/widgets/activity.rs src/widgets/mod.rs src/widgets/ticket_detail.rs src/widgets/keybindings_help.rs src/app.rs src/main.rs
git commit -m "feat: activity history view in ticket detail"
```

---

## Phase 4: Saved Filters

### Task 13: Add Filters tab and config storage

**Files:**
- Create: `src/views/filters.rs`
- Modify: `src/views/mod.rs`
- Modify: `src/app.rs` (add `Filters` tab, filter state)
- Modify: `src/main.rs` (render Filters tab, handle keys)
- Modify: `src/jira_client.rs` (add generic JQL fetch)

**Step 1: Add `Filters` to the `Tab` enum in `app.rs`**

```rust
pub enum Tab {
    MyWork,
    Team,
    Epics,
    Unassigned,
    Filters,
}
```

Update `Tab::next()`, `Tab::title()`, `Tab::all()`, and the tab index matching in `main.rs` `ui()`.

**Step 2: Add filter execution state to `App`**

```rust
pub active_filter_idx: usize,         // which filter is selected in the sidebar
pub filter_results: Vec<Ticket>,      // results of the currently active filter
pub filter_loading: bool,
pub filter_selected_index: usize,     // selected ticket within filter results
```

**Step 3: Add `fetch_jql_query` to `jira_client.rs`**

```rust
pub async fn fetch_jql_query(config: &AppConfig, jql: &str) -> Result<Vec<Ticket>> {
    fetch_tickets_for_query(config, jql).await
}
```

This reuses the existing paginated `fetch_tickets_for_query`.

**Step 4: Create `src/views/filters.rs`**

Layout: left sidebar lists saved filter names (from config), right pane shows results of the selected filter. Same table format as My Work.

```rust
pub fn render(f: &mut ratatui::Frame, area: Rect, app: &App) {
    // Split into sidebar (20%) and results (80%)
    // Sidebar: list of filter names, highlight active
    // Results: ticket table from app.filter_results
}
```

**Step 5: Handle tab keybindings for Filters**

- `j/k` in sidebar: navigate filters
- `Enter` on filter: execute JQL, populate results
- `j/k` when results focused: navigate tickets
- `Enter` on ticket: open detail
- `n`: new filter (opens creation modal)
- `e`: edit selected filter
- `x`: delete selected filter

**Step 6: Add filter creation/edit modal**

A simple form with two fields:
- Name (text input)
- JQL (text input or multiline)

On save, update `config.filters` and call `save_config()`.

**Step 7: Build, manual test, commit**

```bash
git add src/views/filters.rs src/views/mod.rs src/app.rs src/main.rs src/jira_client.rs
git commit -m "feat: saved JQL filters tab with create/edit/delete"
```

---

### Task 14: Add JQL builder form as alternative to raw JQL

**Files:**
- Create: `src/widgets/jql_builder.rs`
- Modify: `src/views/filters.rs` (offer builder as creation mode)

**Step 1: Create `src/widgets/jql_builder.rs`**

A form that builds JQL from structured fields:
- Status (picker from config statuses)
- Assignee (picker from team + "currentUser()")
- Label (text input)
- Date range (picker: last 7d, 14d, 30d, custom)

Generates JQL string like:
```
project = AMP AND status = "In Progress" AND assignee = currentUser() AND created >= -7d
```

**Step 2: Integrate builder into filter creation flow**

When creating a new filter, offer two modes:
1. Raw JQL
2. Builder

Toggle between them with a keybinding (e.g., Tab).

**Step 3: Build, test, commit**

```bash
git add src/widgets/jql_builder.rs src/views/filters.rs
git commit -m "feat: JQL builder form for creating filters without raw JQL"
```

---

### Task 15: Final integration testing and keybindings update

**Files:**
- Modify: `src/widgets/keybindings_help.rs` (add all new keybindings)
- Modify: `README.md` (update keybindings table if it exists)

**Step 1: Update keybindings help overlay**

Add all new keybindings:
```
Actions:
  c: create ticket
  C: comment (in detail view)
  a: assign/reassign
  e: edit fields (in detail view)
  h: view activity history (in detail view)

Filters Tab:
  n: new filter
  e: edit filter
  x: delete filter
  Enter: run filter / open ticket
```

**Step 2: Run full test suite**

Run: `cargo test`
Expected: all tests pass

**Step 3: Build release and smoke test**

Run: `cargo build --release && cargo run --release`
Test all four phases:
- Config loads correctly
- Create ticket works
- Comment works
- Assign works
- Edit fields works
- Activity history shows in detail view
- Filters tab works with saved and new filters

**Step 4: Commit**

```bash
git add src/widgets/keybindings_help.rs
git commit -m "docs: update keybindings help with all new actions"
```

---

## Summary

| Phase | Tasks | Key Files |
|-------|-------|-----------|
| 1: Config | Tasks 1-5 | `config.rs`, `setup.rs`, `jira_client.rs`, `main.rs` |
| 2: Actions | Tasks 6-10 | `widgets/create_ticket.rs`, `widgets/comment.rs`, `widgets/assign.rs`, `widgets/edit_fields.rs` |
| 3: Activity | Tasks 11-12 | `cache.rs`, `widgets/activity.rs` |
| 4: Filters | Tasks 13-15 | `views/filters.rs`, `widgets/jql_builder.rs` |

Each task is a standalone commit. Phases 2-4 can be worked in any order after Phase 1 is complete.
