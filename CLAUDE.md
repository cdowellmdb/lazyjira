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
Use `lazyjira --dev-release` for an optimized rebuild. Dev mode prefers the lazyjira checkout in the current directory or its parents, then falls back to `CARGO_MANIFEST_DIR`. It prints the manifest path before building, so a binary installed from Git cannot silently rebuild its cached checkout while you're working in another checkout.

`.githooks/pre-commit` runs `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` when Rust files are staged. Enable it per clone with `git config core.hooksPath .githooks`. Don't bypass it with `--no-verify` unless asked; fix what it reports.

Tests never reach Jira: `jira_client::run_cmd` and `jira_rest` refuse to run under `cfg(test)`. Tests and docs use made-up ticket keys like `DEMO-1`.

## Architecture

When naming domain concepts, use [CONTEXT.md](CONTEXT.md). Before changing Jira transport, status interpretation, or move consistency, read the corresponding decision in [docs/adr/](docs/adr/).

- **src/main.rs** — Entry point, terminal setup, event loop, key handling, `--dev` flags
- **src/app.rs** — App state, tab management, selection tracking, cache mutations
- **src/cache.rs** — Data types (Ticket, Epic, TeamMember, Status, Cache)
- **src/config.rs** — `config.toml` schema, defaults, load/save
- **src/settings.rs** — In-app team, epic, startup, and theme preferences
- **src/theme.rs** — Color themes: presets, custom themes from `[themes]`, and the pass that recolors a drawn frame
- **src/mouse.rs** — Hit targets registered by renderers and mouse event handling
- **src/setup.rs** — First-run setup screen (project key, team name); imports the legacy `team.yml` roster
- **src/jira_client.rs** — The `jira` CLI: `jira me`, creating tickets, comments, assignment and field edits
- **src/jira_reads.rs** — What a refresh reads from Jira, all through `jira_rest`: the list search for My Work/Team/Unassigned, saved filters, epics with their children and sub-tasks, and the current user's email (`my_email`: remembered through `local_cache`, asked of `jira me` only when none is)
- **src/jira_search.rs** — Searching Jira a few queries at a time (`search_in_order`) and reading ticket details through those searches (`fetch_ticket_detail` for one fresh read, `fetch_ticket_details` for batched prefetch)
- **src/lists.rs** — What a read found, shaped without I/O: `bucket_tickets` splits one search into My Work/Team/Unassigned, `group_by_epic` puts children under their epics, and `attach_epics_to_tickets` and `reconcile_epic_child_statuses` keep tickets and epics in step
- **src/local_cache.rs** — The files kept between runs in `~/.cache/lazyjira/` (snapshot, epics, details, the current user's email) and `DetailCache`, the in-memory copy of the details that list reads hydrate from
- **src/bounded.rs** — Runs tasks a few at a time (`for_each_bounded`): the reads' searches and the bulk actions' Jira calls share it
- **src/jira_rest.rs** — Jira REST client using jira-cli's config and `JIRA_API_TOKEN`: the paginated `search` (JQL + fields in, tickets out), `issue` (one issue, read itself rather than through the search index), moves (list a ticket's transitions, send one by id), and a ticket's browser URL
- **src/jql.rs** — The JQL of the list, epic, sub-task and saved-filter reads, built safely: `quote` for a value, `key_chunks` and `is_key` so only ticket-key-shaped text reaches a list, `KEYS_PER_SEARCH`, and the builders `lists_jql`, `scoped_jql`, `epics_jql`, `epic_children_jqls` and `subtasks_jqls`
- **src/jira_issue.rs** — Pure parser from Jira's issue JSON (`{key, fields}`: a search result or a single issue) to a `Ticket`, including comments as activity
- **src/subtasks.rs** — Sub-task hierarchy: `nest` orders rows so a sub-task follows its parent, `is_nested` tells renderers which rows are drawn under one, and `add_to_epics` adds the sub-tasks under an epic's children to that epic
- **src/transitions.rs** — Transition model and parsing, and the rules for matching shortcuts and resolutions
- **src/move_picker.rs** — Single-ticket move picker state and keys; returns the Jira call to make instead of making it
- **src/bulk_plan.rs** — What a bulk move/assign sends for each ticket, and why others are skipped
- **src/bulk_actions.rs** — Bulk move/assign state, progression, and completion; returns work for `main.rs` to execute. Dismissed operations still update tickets, but their results cannot replace a newer modal.
- **src/moves.rs** — Single-ticket move tracking: pending moves, Jira-confirmed moves (to ignore stale reads), and rejected moves awaiting dismissal
- **src/bulk_upload.rs** — CSV parsing and validation for bulk ticket creation
- **src/views/** — Tab renderers (`my_work.rs`, `team.rs`, `epics.rs`, `unassigned.rs`, `filters.rs`, shared helpers in `common.rs`)
- **src/widgets/** — Overlays (`ticket_detail.rs`, `keybindings_help.rs`, `activity.rs`, `assign.rs`, `bulk_actions.rs`, `bulk_upload.rs`, `comment.rs`, `create_ticket.rs`, `edit_fields.rs`, `form.rs`, `move_failure.rs`), and `markup.rs`, which renders Jira wiki markup in descriptions (emphasis, links, mentions, lists, tables, quotes, code) as styled lines wrapped to the popup width

## Before changing behaviour

Read the named section of [docs/architecture.md](docs/architecture.md) before changing:

- **Epic reads or epic progress**: "Epics come from batched searches"
- **My Work, Team or Unassigned reads, list JQL, or email matching**: "Lists come from one Jira search"
- **Ticket details, their cache or prefetch**: "Detail data uses JSON + local cache"
- **Cache files, their location or format**: "Local caches"
- **Status order, done-ness or `[statuses]`**: "Statuses"
- **Moves or the move picker**: "Moves use Jira's transitions" and "Moves wait for Jira"
- **Grouped rows, selection, tab switching or mouse targets**: "Visible rows"
- **Sub-tasks, parents or folding**: "Sub-tasks nest under their parent"
- **Colors or themes**: "Themes recolor the finished frame"
- **Columns, marks, done-group folding or the detail overlay's scrolling**: "Current UX behavior"

## Configuration

- **Config file:** `~/.config/lazyjira/config.toml`, created by the first-run setup. Holds the project key, team name, team roster (`[team]`), statuses, saved filters, and `[preferences]`. An old top-level `resolutions` list is ignored (moves offer the resolutions Jira allows). Saved filters and preferences rewrite it. `S` applies team and epic changes during the session.
- **Browser URLs:** derive them when opened with `jira_rest::browse_url`, using jira-cli's `server`. URLs are not stored in tickets or caches.
- **Unassigned tab:** queries the `Assigned Teams` custom field using `jira.team_name`.
- **Legacy roster:** `~/.claude/skills/jira/team.yml` is only read once, during first-run setup, to seed `[team]`.
- **Auth:** Via existing `jira` CLI authentication (`~/.config/.jira/.config.yml`, or `$JIRA_CONFIG_FILE`). List reads, detail reads and moves call Jira's REST API with that file's `server`, `auth_type`, `login` and `epic.link`, and `JIRA_API_TOKEN`; lazyjira assumes Jira Server or Data Center (ADR 0005). A list refresh checks the setup (`jira_rest::ensure_ready`) before it runs `jira me`, so a missing `JIRA_API_TOKEN` is the error shown, not whatever jira-cli says first.
- **Caches:** the snapshot, epics, ticket details and the current user's email, per project in `~/.cache/lazyjira/` (`local_cache`).

## Dependencies

- `ratatui` 0.29 + `crossterm` 0.28 — TUI rendering
- `tui-textarea` — text input in forms
- `unicode-width` — column widths when wrapping rendered markup
- `tokio` — async runtime for parallel CLI calls
- `reqwest` (rustls, no OpenSSL) — Jira REST calls for list and detail reads and moves
- `serde` + `serde_json` + `serde_yaml` + `toml` — JSON/YAML/TOML parsing
- `csv` — bulk upload parsing
- `anyhow` — error handling

## Demo recording

`docs/demo/record.sh` re-records `docs/images/demo.gif` with VHS (`docs/demo/record.sh docs/demo/themes.tape` re-records `docs/images/themes.gif`). It runs the app with a throwaway `HOME`/`TMPDIR` and puts `docs/demo/bin/jira` (a fake `jira` CLI with made-up data) first on `PATH`. It also starts `docs/demo/bin/fake_jira_rest.py`, which serves `POST /rest/api/2/search` and `GET /rest/api/2/issue/KEY` from the fake CLI's data (it imports `bin/jira`), and writes a jira-cli config in the throwaway `HOME` whose `server` points at it, with `epic.link` and `JIRA_API_TOKEN=demo`. Never record against a real Jira instance. The fake CLI answers only `jira me` and no-op writes; if you change which `jira` subcommands or flags the app uses, update it to match. If you change the JQL or fields a search sends, update the fake's evaluator (`evaluate`/`matches` in `bin/jira`) and `base_fields`; its Assigned Teams clause compares the value with `TEAM_NAME`, which must match the demo config's `jira.team_name`. Moves aren't served, so the demo doesn't show them. Re-record the GIFs once per PR, after its last merge, rather than per ticket.
## Notes

`README.md` is the onboarding doc: run instructions, keybindings and user-visible behaviour.

## Commit Messages
Follow @COMMIT_STYLING.md

## Coding Standards and PRs
Follow @CODING_STANDARDS.md: the four required reviews run before any PR (draft included) is raised, and PR bodies use the `/pr` template plus a Reviews section.
