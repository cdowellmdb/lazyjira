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
- **src/widgets/** — Ticket detail overlay (ticket_detail.rs)

## Key Design Decisions

### Jira CLI column parsing
The `jira` CLI (`ankitpokhrel/jira-cli`) uses tab-padding for visual alignment in `--plain` output. Longer text fields get fewer padding tabs, shorter ones get more. **Always put summary/text fields LAST** in `--columns` to avoid corrupting fixed fields. Filter empty strings from tab splits.

### Detail view uses JSON
List queries use `--plain --no-headers --columns key,status,assignee,summary` for speed. But the detail overlay fetches full JSON via `jira issue view KEY --raw` for accurate status, assignee, and description.

### View/state ordering must match
The team view sorts members by active ticket count (most active first). Any code that maps `selected_index` to a ticket key (in `app.rs`) MUST use the same sort order as the view renderer. Use `app.sorted_team_members()` for this.

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

## Design Doc

Full design with wireframes: `docs/plans/2026-02-06-lazyjira-design.md`
