# lazyjira

A fast, cache-first Rust TUI for viewing Jira tickets. Built with Ratatui.

## Build & Run

```bash
cargo build --release
cargo run --release
lazyjira --dev
```

Binary name: `lazyjira`

`lazyjira --dev` (or `lazyjira --rebuild`) forces a rebuild from source and runs the app.
Use `lazyjira --dev-release` for an optimized rebuild.

## Architecture

- **src/main.rs** — Entry point, terminal setup, event loop, key handling
- **src/app.rs** — App state, tab management, selection tracking, cache mutations
- **src/cache.rs** — Data types (Ticket, Epic, TeamMember, Status, Cache)
- **src/jira_client.rs** — Shells out to `jira` CLI, parses output
- **src/views/** — Tab renderers (my_work.rs, team.rs, epics.rs)
- **src/widgets/** — Overlays/widgets (`ticket_detail.rs`, `keybindings_help.rs`)

## Key Design Decisions

### Jira CLI column parsing
The `jira` CLI (`ankitpokhrel/jira-cli`) uses tab-padding for visual alignment in `--plain` output. Longer text fields get fewer padding tabs, shorter ones get more. **Always put summary/text fields LAST** in `--columns` to avoid corrupting fixed fields. Filter empty strings from tab splits.

### Detail data uses JSON + local cache
List queries use `--plain --no-headers --columns key,status,assignee,summary` for speed. Rich ticket detail (description, labels, assignee, status, epic linkage) comes from `jira issue view KEY --raw`, is cached locally, and is hydrated on startup for fast detail open. Missing details are prefetched in the background.

### View/state ordering must match
The team view sorts members by active ticket count (most active first). Any code that maps `selected_index` to a ticket key (in `app.rs`) MUST use the same sort order as the view renderer. Use `app.sorted_team_members()` for this.

### Current UX behavior
- Team view includes the current user (if not in `team.yml`, inferred from `jira me` email).
- My Work and Team include a separate Labels column.
- Search matches ticket key/summary/assignee/labels and team member name/email.
- `Enter` works while search is active (opens detail for selected row).
- Epics child rows are sorted by status with Done at the bottom.
- Epics show an accurate progress bar and percentage complete.

## Jira Configuration

- **Project:** AMP
- **Jira instance:** jira.mongodb.org
- **Team roster:** `~/.claude/skills/jira/team.yml`
- **Auth:** Via existing `jira` CLI authentication (`~/.config/.jira/.config.yml`)

## Dependencies

- `ratatui` 0.29 + `crossterm` 0.28 — TUI rendering
- `tokio` — async runtime for parallel CLI calls
- `serde` + `serde_json` + `serde_yaml` — JSON/YAML parsing
- `anyhow` — error handling

## Notes

Use `README.md` as the current onboarding doc for run instructions and keybindings.

## Commit Messages
Follow @COMMIT_STYLING.md
