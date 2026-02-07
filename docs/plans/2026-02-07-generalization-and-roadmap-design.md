# lazyjira Generalization & Roadmap Design

## Goal

Make lazyjira usable by other teams at MongoDB — same Jira instance, different projects/teams/workflows. Then layer on new capabilities that reduce trips to the Jira web UI.

## Phase 1: Configuration System

Extract all hardcoded values into `~/.config/lazyjira/config.toml`. On first run with no config file, the app enters an interactive setup flow.

### Config file structure

```toml
[jira]
project = "AMP"
team_name = "Code Generation"
done_window_days = 14

[team]
"Christian Dowell" = "christian.dowell@mongodb.com"
"Nate Smith" = "nate.smith@mongodb.com"

[statuses]
# Auto-populated from Jira on first run, editable after
active = ["Needs Triage", "Ready for Work", "To Do", "In Progress", "In Review", "Blocked"]
done = ["Done", "Closed"]
```

### First-run interactive setup

1. Detect missing config file.
2. Run `jira me` to get current user email.
3. Prompt for project key (text input).
4. Prompt for team name (text input).
5. Query Jira for available statuses in that project, let user pick which are "active".
6. Write the config file.
7. Prompt: "Add team members now or later?" — if now, enter name/email pairs.

### What stays hardcoded (for now)

- Base URL (`jira.mongodb.org`) — same for all MongoDB teams.
- Custom field IDs (e.g. `customfield_12551` for epic links) — instance-level, not project-level.

### Cache changes

Cache filenames become project-aware: `lazyjira_full_cache_{project}.json`, `lazyjira_epics_cache_{project}.json`, etc. This avoids collisions if someone works across multiple projects.

### Migration

On first run, if the legacy team roster at `~/.claude/skills/jira/team.yml` exists, offer to import it into the new config file.

---

## Phase 2: In-TUI Actions

Four new actions, each building on the last. All operate through the existing `jira` CLI.

### Create ticket

- Keybinding: `c` from any list view.
- Modal form with fields: Type (dropdown), Summary (text), Description (multiline), Assignee (picker from team roster), Epic (picker from cached epics list).
- Runs `jira issue create` under the hood.
- On success, adds the ticket to the local cache immediately and selects it.

### Comment

- Keybinding: `C` from the detail view.
- Multiline text input modal.
- Runs `jira issue comment add KEY "body"`.
- Appends the comment to cached detail for instant display.

### Assign / reassign

- Keybinding: `a` from detail or list view.
- Picker populated from team roster in config.
- Runs `jira issue assign KEY "email"`.
- Optimistic cache update (same pattern as existing status move).

### Edit fields

- Keybinding: `e` from the detail view.
- Editable form pre-populated with current values (summary, description, labels).
- Runs `jira issue edit KEY --summary "..." --label "..."` etc.
- Updates cache on success.

### Text editing

Multiline input uses `tui-textarea` crate for in-TUI editing. For long-form text (descriptions), shell out to `$EDITOR` as a fallback option.

---

## Phase 3: Activity / History

### History view

- Accessible from detail view via `h` keybinding.
- Scrollable timeline showing: status changes, comments, assignee changes, with timestamps and authors.
- Data sourced from `jira issue view KEY --raw` JSON (`changelog` and `comment` sections).
- Cached alongside ticket details so repeat views are instant.

### Rendering

Each entry as a compact line:

```
2024-01-15  christian.dowell  Status: In Progress -> In Review
2024-01-14  nate.smith        Comment: "Looks good, just one nit..."
```

---

## Phase 4: Saved Filters

### Storage

Filters stored in the config file:

```toml
[[filters]]
name = "My Blocked Tickets"
jql = "project = AMP AND assignee = currentUser() AND status = Blocked"

[[filters]]
name = "Recent P1s"
jql = "project = AMP AND priority = Highest AND created >= -7d"
```

### UI

- Filter manager accessible from a keybinding or dedicated tab.
- Create, edit, delete filters from the TUI.
- Two creation modes: raw JQL input, or simple form builder (status, assignee, label, date range).
- Results render in the same table format as My Work, with full search/detail support.

### Opt-in

First-run setup does not create any filters. They accumulate as the user needs them.

---

## Phasing & Dependencies

```
Phase 1 (Config) ──> Phase 2 (Actions)
                ──> Phase 3 (Activity)
                ──> Phase 4 (Filters)
```

Phase 1 is the foundation — everything else depends on configuration being flexible. Phases 2-4 are independent of each other and can be reordered based on priority.

## Non-goals (for now)

- Multi-instance support (non-MongoDB Jira).
- Sprint tracking / board views.
- Publishing to crates.io.
- Plugin / extension system.
