# lazyjira

A fast, cache-first Rust TUI for viewing Jira tickets. Built with Ratatui.

## Build & Run

```bash
cargo build --release
cargo run --release
cargo test
lazyjira --dev
```

Binary name: `lazyjira`

`lazyjira --dev` (or `lazyjira --rebuild`) forces a rebuild from source and runs the app.
Use `lazyjira --dev-release` for an optimized rebuild. Both rebuild from the checkout the binary was compiled in (`CARGO_MANIFEST_DIR`), so they only work for binaries built from a local checkout, not release downloads.

## Architecture

- **src/main.rs** — Entry point, terminal setup, event loop, key handling, `--dev` flags
- **src/app.rs** — App state, tab management, selection tracking, cache mutations
- **src/cache.rs** — Data types (Ticket, Epic, TeamMember, Status, Cache)
- **src/config.rs** — `config.toml` schema, defaults, load/save
- **src/setup.rs** — First-run setup screen (project key, team name); imports the legacy `team.yml` roster
- **src/jira_client.rs** — Shells out to `jira` CLI, parses output, reads/writes local caches
- **src/jira_rest.rs** — Jira REST client for moves (list a ticket's transitions, send one by id), using jira-cli's config and `JIRA_API_TOKEN`
- **src/transitions.rs** — Transition model and parsing, and the rules for matching shortcuts and resolutions
- **src/move_picker.rs** — Single-ticket move picker state and keys; returns the Jira call to make instead of making it
- **src/bulk_plan.rs** — What a bulk move/assign sends for each ticket, and why others are skipped
- **src/moves.rs** — Single-ticket move tracking: pending moves, Jira-confirmed moves (to ignore stale reads), and rejected moves awaiting dismissal
- **src/bulk_upload.rs** — CSV parsing and validation for bulk ticket creation
- **src/views/** — Tab renderers (`my_work.rs`, `team.rs`, `epics.rs`, `unassigned.rs`, `filters.rs`, shared helpers in `common.rs`)
- **src/widgets/** — Overlays (`ticket_detail.rs`, `keybindings_help.rs`, `activity.rs`, `assign.rs`, `bulk_actions.rs`, `bulk_upload.rs`, `comment.rs`, `create_ticket.rs`, `edit_fields.rs`, `form.rs`, `move_failure.rs`)

## Key Design Decisions

### Jira CLI column parsing
The `jira` CLI (`ankitpokhrel/jira-cli`) uses tab-padding for visual alignment in `--plain` output. Longer text fields get fewer padding tabs, shorter ones get more. **Always put summary/text fields LAST** in `--columns` to avoid corrupting fixed fields. Filter empty strings from tab splits.

### Detail data uses JSON + local cache
List queries use `--plain --no-headers --columns key,status,assignee,summary` for speed. Rich ticket detail (description, labels, assignee, status, epic linkage) comes from `jira issue view KEY --raw`, is cached locally, and is hydrated on startup for fast detail open. Missing details are prefetched in the background.

### Statuses
`Status::from_str` maps workflow status names onto the canonical `Status` variants (`Done`/`Closed`/`Resolved` all become `Status::Closed`). Unknown names become `Status::Other` and are grouped after the canonical statuses (`cache::group_by_status`). The `[statuses]` config only controls which statuses the JQL queries load.

`Ticket::jira_status` keeps Jira's real status name next to the enum (issue #19 will make it the only one). Read it with `Ticket::status_name()`, which falls back to the enum for caches written before the field existed, and change a status with `Ticket::set_status(name)` so both stay in step. Views still group by the enum.

### Moves use Jira's transitions
Each issue type has its own workflow, and a transition's name can differ from the status it leads to, so moves never send status names. The picker lists the ticket's transitions from `GET /rest/api/2/issue/{key}/transitions?expand=transitions.fields` and sends the chosen one by id (`jira_rest`). Shortcuts match a transition's destination through `Status::from_str`. Transitions that differ only by id are merged when parsed. A resolution is asked for only when the transition has a resolution field, from that field's allowed values. Tests can't reach Jira: `jira_rest` refuses to build its client under `cfg(test)`, and the picker returns a `JiraCall` for `main.rs` to start.

### Moves wait for Jira
A single-ticket move changes the ticket only after Jira reports success (`BackgroundMessage::TicketMoved`); a failure keeps the status and shows the error until dismissed. Every Jira read (detail fetch, cache/epics refresh, filter query) carries `requested_at = app.moves.now()`. Reads requested before a ticket's latest confirmed move must not overwrite its status: use `App::enrich_ticket(key, requested_at, detail)`, `App::replace_cache(cache, requested_at)` or `App::reapply_moves_since` rather than writing statuses directly.

### View/state ordering must match
The team view sorts members by active ticket count (most active first). Any code that maps `selected_index` to a ticket key (in `app.rs`) MUST use the same sort order as the view renderer. Use `app.sorted_team_members()` for this.

### Current UX behavior
- Team view includes the current user (if not in the `[team]` config, inferred from `jira me` email).
- My Work and Team include a separate Labels column.
- Search matches ticket key/summary/assignee/labels and team member name/email.
- `Enter` works while search is active (opens detail for selected row).
- Epics child rows are sorted by status with Done at the bottom.
- Epics show an accurate progress bar and percentage complete.

## Configuration

- **Config file:** `~/.config/lazyjira/config.toml`, created by the first-run setup. Holds the project key, team name, team roster (`[team]`), statuses, and saved filters. An old top-level `resolutions` list is ignored (moves offer the resolutions Jira allows). The app rewrites it when saved filters change.
- **Jira instance:** `jira.mongodb.org`, hardcoded for browser links (`JIRA_BASE_URL` in `src/jira_client.rs`, plus `src/main.rs`).
- **Unassigned tab:** queries the `Assigned Teams` custom field using `jira.team_name`.
- **Legacy roster:** `~/.claude/skills/jira/team.yml` is only read once, during first-run setup, to seed `[team]`.
- **Auth:** Via existing `jira` CLI authentication (`~/.config/.jira/.config.yml`, or `$JIRA_CONFIG_FILE`). Moves call Jira's REST API with that file's `server`, `auth_type` and `login`, and `JIRA_API_TOKEN`.
- **Caches:** full snapshot in `~/.cache/lazyjira/`; epics and ticket-detail caches in the system temp dir. All are per project.

## Dependencies

- `ratatui` 0.29 + `crossterm` 0.28 — TUI rendering
- `tui-textarea` — text input in forms
- `tokio` — async runtime for parallel CLI calls
- `reqwest` (rustls, no OpenSSL) — Jira REST calls for moves
- `serde` + `serde_json` + `serde_yaml` + `toml` — JSON/YAML/TOML parsing
- `csv` — bulk upload parsing
- `anyhow` — error handling

## Demo recording

`docs/demo/record.sh` re-records `docs/images/demo.gif` with VHS. It runs the app with a throwaway `HOME`/`TMPDIR` and puts `docs/demo/bin/jira` (a fake `jira` CLI with made-up data) first on `PATH`. Never record against a real Jira instance. If you change which `jira` subcommands, flags, or columns the app uses, update the fake CLI to match. Moves use the REST API rather than the CLI, so the demo doesn't show them.

## Notes

Use `README.md` as the current onboarding doc for run instructions and keybindings.

## Commit Messages
Follow @COMMIT_STYLING.md
