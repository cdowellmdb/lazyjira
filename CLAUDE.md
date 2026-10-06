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
- **src/jira_client.rs** — The `jira` CLI: `jira me` and the remembered email (`my_email`), creating tickets, comments, assignment and field edits, and a ticket's browser URL
- **src/jira_reads.rs** — What a refresh reads from Jira, all through `jira_rest`: the list JQL and bucketing one search into My Work/Team/Unassigned, saved filters, epics with their children and sub-tasks, and ticket details (`fetch_ticket_detail` for one fresh read, `fetch_ticket_details` for batched prefetch)
- **src/local_cache.rs** — The files kept between runs in `~/.cache/lazyjira/` (snapshot, epics, details, the current user's email) and `DetailCache`, the in-memory copy of the details that list reads hydrate from
- **src/jira_rest.rs** — Jira REST client using jira-cli's config and `JIRA_API_TOKEN`: the paginated `search` (JQL + fields in, tickets out), `issue` (one issue, read itself rather than through the search index), and moves (list a ticket's transitions, send one by id)
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

## Key Design Decisions

### Epics come from batched searches
The Epics tab's `jira_reads::fetch_epics` reads through `jira_rest::search` too, not one `jira` process per epic: one search lists the epics (`epics_jql`), then `epic_children_jqls` builds one children search per `jira_rest::KEYS_PER_SEARCH` (50) epic keys, and `load_subtasks` one sub-task search per 50 children (`subtasks_jqls`). A project with 400 epics and 2000 children needs about 50 searches, so `search_each` runs `SEARCHES_AT_ONCE` (4) at a time and `search_in_order` returns their tickets in query order. A child names its epic through the Epic Link field (company-managed projects) or `parent` (team-managed); the `"Epic Link"` clause is left out when jira-cli's config has no `epic.link`, since the field would then be unreadable and the JQL could fail, so only `parent` links are found there (the README lists this). `group_by_epic` puts each child under the epic its `epic_key` names (the Epic Link value, else a non-sub-task's `parent`), once each and sorted by key; a child of an epic that isn't listed is dropped. `subtasks::add_to_epics` adds the sub-tasks of the children, which Jira doesn't link to the epic, so progress counts what it did before. Any failed search, sub-tasks included, fails the refresh (`refresh_epics_cache` returns the error), so the epics already on screen and in the cache stay instead of being replaced by epics with a different progress.

### Lists come from one Jira search
My Work, Team and Unassigned come from one paginated `jira_rest::search` ([ADR 0005](docs/adr/0005-read-lists-over-jira-rest-search.md)), not per-member CLI queries. `jira_reads::lists_jql` builds it from the config: the roster's active tickets (plus those done inside the window for the full scope) and the active tickets nobody has taken that are the team's by Assigned Teams. A REST search needs the project and an order spelled out, so every builder adds them (`scoped_jql` for saved filters, keeping their own ORDER BY), and every config string in a query goes through `jira_rest::jql_quote`. Each ticket arrives with assignee name and email, labels, parent, `updated` and the Epic Link, parsed by `jira_issue::ticket_from_issue`. Emails are normalized (`cache::normalize_email`) where they enter the app (Jira's issues, the roster, `jira me`), so everything after compares them exactly. `bucket_tickets` splits the results: a ticket is placed on the roster by email, else by the assignee's display name, since Jira can hide emails; an assignee it can't place joins Team under their email (or display name) rather than dropping the ticket; no assignee becomes the Unassigned member's; My Work is the tickets of the current user's email. The Epic Link field id is jira-cli's `epic.link` (`jira_rest::epic_link_field`), never a hard-coded `customfield_` id. A search answer without a `total` is an error, not a short list. A failed search (including a missing `JIRA_API_TOKEN`) propagates out of `fetch_with_scope`, so `replace_cache` isn't called and the last snapshot stays; with no snapshot, startup restores the terminal and exits with the error.

### Detail data uses JSON + local cache
Rich ticket detail (description, reporter, comments as activity) is cached locally, and is hydrated on startup for fast detail open. Hydration (`hydrate_ticket_from_details_cache`) fills only those detail-only fields and the stored `detail_loaded`: labels, assignee, epic and parent come from the list search and the cache must not bring back stale values. The details cache file is read once at startup into `local_cache::DetailCache` (`App::details`); `fetch_active_only` and `fetch_all` hydrate from that in-memory copy before returning, so no caller can forget to, and a background task writes it back, holding the lock only while it serializes. Missing details are prefetched in the background by `jira_reads::fetch_ticket_details`: one `jira_rest::search` (`key in (...)`, asking for the list fields plus `DETAIL_ONLY_FIELDS`) per `KEYS_PER_SEARCH` (50) tickets, `SEARCHES_AT_ONCE` (4) chunks at a time, and each ticket's result goes to the app in its own `TicketDetailFetched`, so `enrich_ticket` and the move guard see each as a single-ticket detail. A chunk that fails (or a ticket Jira leaves out) fails only those keys: they stay missing and are asked for again on the next refresh, and the other chunks are still read. The cache is never invalidated, so opening a ticket's detail always fetches Jira's copy too (`App::open_fresh_detail`, through `jira_rest::issue`, as does the refetch after a move): the cached detail shows at once and is replaced when the fetch lands. That read is `GET /issue/{key}`, not a search, because the search reads Jira's index, which can lag a move made a moment ago. Neither read asks for the changelog, so activity is comments. `jira_client::run_cmd` and `jira_rest` refuse to run under `cfg(test)`, so tests that open details never reach Jira.

### Statuses
`Ticket::status` is Jira's own status name ("Resolved", "On Deck"), or `cache::UNKNOWN_STATUS` when a read's answer has none (so it shows plainly instead of passing as To Do; `enrich_ticket` never copies it over a real status). Views group, label and compare by it. `cache::StatusRules`, built from the `[statuses]` config, decides everything else about a status: the one display order (`active` in config order, then unlisted statuses in first-seen order, then `done`) and whether it's done (`is_done`: `d` hides it, epic progress counts it, Team puts it in the done split and leaves it out of active counts). Get it from `app.status_rules()`; don't sort statuses or test for done anywhere else. Names match case-insensitively. A name the config doesn't list takes the place of a listed name that `Status::from_str` reads the same way (so "Resolved" follows "Done"); failing that, Closed-like names are done and anything else is active.

The `Status` enum is only Jira-independent knowledge: move shortcut keys, default colors (`views::common::status_color`), and that fallback. `Status::from_str` reads Done/Closed/Resolved as `Status::Closed` and unknown names as `Status::Other`.

The `[statuses]` config also controls which statuses the JQL queries load.

### Moves use Jira's transitions
Each issue type has its own workflow, and a transition's name can differ from the status it leads to, so moves never send status names. The picker lists the ticket's transitions from `GET /rest/api/2/issue/{key}/transitions?expand=transitions.fields` and sends the chosen one by id (`jira_rest`). Shortcuts match a transition's destination through `Status::from_str`. Transitions that differ only by id are merged when parsed. A resolution is asked for only when the transition has a resolution field, from that field's allowed values. Tests can't reach Jira: `jira_rest` refuses to build its client under `cfg(test)`, and the picker returns a `JiraCall` for `main.rs` to start.

### Moves wait for Jira
A single-ticket move changes the ticket only after Jira reports success (`BackgroundMessage::TicketMoved`); a failure keeps the status and shows the error until dismissed. Every Jira read (detail fetch, cache/epics refresh, filter query) carries `requested_at = app.moves.now()`. Reads requested before a ticket's latest confirmed move must not overwrite its status: use `App::enrich_ticket(key, requested_at, detail)`, `App::replace_cache(cache, requested_at)` or `App::reapply_moves_since` rather than writing statuses directly.

### Visible rows
App's grouped view methods return `VisibleGroup` values with header metadata, totals, and occurrence indices. Collapsed groups retain their header and totals but have no ticket rows. Navigation and renderers use these same groups; renderers compare supplied indices with `selected_index`. Keep tab-specific formatting in `views/`. Team members are ordered by active ticket count (most active first), with active tickets before done tickets.

Use `App::switch_tab` to restore each tab's position, search, and status focus. Cache updates preserve the selected ticket occurrence and group through `mark_cache_changed`; use `replace_cache`, `replace_epics`, and the ticket update helpers so the old visible rows are retained before data changes.

Renderers register mouse targets for the rows actually drawn, after scrolling. `ui` clears targets before each overlay, so covered controls cannot receive clicks. Mouse actions share keyboard handlers and preserve confirmation steps. Text fields use the existing `tui-textarea` dependency and `widgets::form` helpers.

### Sub-tasks nest under their parent
`Ticket::parent_key` names a sub-task's parent. It comes from Jira's REST search on every list read: from the list search itself for My Work, Team, Unassigned and filters (`fetch_with_scope`, `fetch_jql_query`) and for epics' children, and from the sub-task search (`jira_reads::load_subtasks`, through the same `search`; [ADR 0004](docs/adr/0004-read-subtask-parents-over-rest.md)) for the sub-tasks `fetch_epics` adds. Never take it from the detail cache: `hydrate_ticket_from_details_cache` skips it so a re-parented ticket doesn't keep its old parent, and only a freshly fetched detail reaches it, through `enrich_ticket`. A failed search fails the read, so rows stay as they were.

The Epics fetch also adds the sub-tasks of an epic's children to that epic's `children` (`subtasks::add_to_epics`), because Jira doesn't link a sub-task to the epic. They count in epic progress. Each tab passes its rows through `subtasks::nest` when building `VisibleGroup`s; Team nests its active and done rows separately, so a sub-task never moves between them. Renderers get a row's key and summary from `views::common::ticket_cells`: a sub-task whose parent is in the same rows is indented, and one whose parent isn't (a different status group, another assignee) leads its summary with the parent's key. `jira_issue::ticket_from_issue` reads a sub-task's `parent` as `parent_key`, never as an epic.

Folding a parent adds its key to `App::collapsed_parents` (one set for every tab). `nest` tags each row with its `Family` (a parent with a sub-task count, or a child); `index_groups` drops the children of folded parents from the rows but keeps them in `total`, and `VisibleGroup::family` carries the tags to renderers and to `selected_fold_parent`. So row counts that must include hidden rows (Team's header counts) come from `total`, never from `tickets.len()`, and Team splits active from done by status, not position. `z` goes through `App::toggle_fold_at_cursor`: the selected parent or sub-task folds its parent, anything else folds its group.

### Themes recolor the finished frame
Renderers draw with plain ANSI colors, each used for one job: `DarkGray` is muted text as a foreground and the selected row as a background, `Cyan` is keys and focus, `Yellow` is headers and the chosen option, and so on (`Theme::role` lists them). `ui` ends with `Theme::apply`, which swaps those colors in the buffer for the theme's roles, like a terminal color scheme. So keep drawing with the ANSI color for the job, not a theme field, and code that finds things by color (the footer's key hints, the editor cursor) keeps working. Unset roles, and every role of `default`, leave the terminal's color. While preferences are open, `ui` applies the highlighted theme instead of `App::theme`, which is how the picker previews.

### Current UX behavior
- Team view includes the current user (if not in the `[team]` config, inferred from `jira me` email).
- My Work and Team include a separate Labels column.
- Search matches ticket key/summary/assignee/labels and team member name/email.
- `Enter` works while search is active (opens detail for selected row).
- `f`/`F` cycle the status focus (My Work and Team) through the statuses shown, in display order, then back to all (`App::cycle_status_focus`). Closed isn't in the cycle; `d` shows and hides it.
- Epics child rows are sorted by status with Done at the bottom.
- Epics show an accurate progress bar and percentage complete.
- The detail overlay shows the ticket's fields, its description (Jira markup, via `widgets/markup.rs`) and its comments, oldest first. The body is pre-wrapped to the overlay's width, so its line count is its height: the renderer records the scroll limit in `App::detail_scroll_max` for the scroll keys.
- In the detail overlay, Left/Right step to the previous/next ticket in the list under it (epics, for an epic's detail) and move the list selection with it (`App::step_detail`). `z` toggles full screen.

## Configuration

- **Config file:** `~/.config/lazyjira/config.toml`, created by the first-run setup. Holds the project key, team name, team roster (`[team]`), statuses, saved filters, and `[preferences]`. An old top-level `resolutions` list is ignored (moves offer the resolutions Jira allows). Saved filters and preferences rewrite it. `S` applies team and epic changes during the session.
- **Browser URLs:** derive them when opened with `jira_client::browse_url`, using jira-cli's `server` through `jira_rest::server_url`. URLs are not stored in tickets or caches.
- **Unassigned tab:** queries the `Assigned Teams` custom field using `jira.team_name`.
- **Legacy roster:** `~/.claude/skills/jira/team.yml` is only read once, during first-run setup, to seed `[team]`.
- **Auth:** Via existing `jira` CLI authentication (`~/.config/.jira/.config.yml`, or `$JIRA_CONFIG_FILE`). List reads, detail reads and moves call Jira's REST API with that file's `server`, `auth_type`, `login` and `epic.link`, and `JIRA_API_TOKEN`; lazyjira assumes Jira Server or Data Center (ADR 0005).
- **Caches:** full snapshot, epics, ticket details, and the current user's email (from `jira me`, refreshed in the background at startup once one is remembered) all live in `~/.cache/lazyjira/`, per project. Tests use a directory per test run in the temp dir instead (`local_cache::cache_dir`), so parallel runs don't share files. Old caches in the system temp dir are ignored. A cache that fails to parse is ignored and refetched, which is how format changes are handled: tickets store their status as `status_name`, so caches from before real status names are refetched once.

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

`docs/demo/record.sh` re-records `docs/images/demo.gif` with VHS (`docs/demo/record.sh docs/demo/themes.tape` re-records `docs/images/themes.gif`). It runs the app with a throwaway `HOME`/`TMPDIR` and puts `docs/demo/bin/jira` (a fake `jira` CLI with made-up data) first on `PATH`. It also starts `docs/demo/bin/fake_jira_rest.py`, which serves `POST /rest/api/2/search` and `GET /rest/api/2/issue/KEY` from the fake CLI's data (it imports `bin/jira`), and writes a jira-cli config in the throwaway `HOME` whose `server` points at it, with `epic.link` and `JIRA_API_TOKEN=demo`. Never record against a real Jira instance. The fake CLI answers only `jira me` and no-op writes; if you change which `jira` subcommands or flags the app uses, update it to match. If you change the JQL or fields a search sends, update the fake's evaluator (`evaluate`/`matches` in `bin/jira`) and `base_fields`; its Assigned Teams clause compares the value with `TEAM_NAME`, which must match the demo config's `jira.team_name`. Moves aren't served, so the demo doesn't show them.

## Notes

Use `README.md` as the current onboarding doc for run instructions and keybindings.

## Commit Messages
Follow @COMMIT_STYLING.md

## Coding Standards and PRs
Follow @CODING_STANDARDS.md: the four required reviews run before any PR (draft included) is raised, and PR bodies use the `/pr` template plus a Reviews section.
