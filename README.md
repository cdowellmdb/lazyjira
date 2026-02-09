# lazyjira

A terminal UI for Jira focused on fast triage and team visibility.

## Features

- Five tabs: `My Work`, `Team`, `Epics`, `Unassigned`, `Filters`
- Status-grouped ticket views with focus filters
- Epic relationship mapping + progress bars
- Optional epic focus list for the Epics tab (`epics_i_care_about`)
- Rich ticket detail with description, labels, assignee, epic, activity history
- In-TUI actions: create tickets, comment, assign, edit fields, move status
- Multi-select + bulk move/assign from list views
- Bulk CSV upload for mass ticket creation with mandatory preview
- Saved JQL filters with persistent config
- Local caching for fast startup and detail open

## Setup

On first run, lazyjira prompts for your Jira project key and team name, then writes `~/.config/lazyjira/config.toml`. Edit the file directly to add team members, custom statuses, saved filters, or an epic focus list for the Epics tab.

### Requirements

- Rust toolchain (`cargo`)
- [`jira`](https://github.com/ankitpokhrel/jira-cli) CLI authenticated and in your `$PATH`

## Run

```bash
cargo run
```

## Install / Update

```bash
cargo install --path . --force
```

Dev rebuild:

```bash
lazyjira --dev            # debug build + run
lazyjira --dev-release    # release build + run
```

## Keybindings

### Global

| Key | Action |
|-----|--------|
| `Tab` | Next tab |
| `j/k` | Navigate |
| `Space` | Toggle ticket/group selection |
| `A` | Select all visible tickets |
| `u` | Clear selected tickets |
| `B` | Open bulk action menu |
| `U` | Open bulk CSV upload |
| `Enter` | Open detail |
| `/` | Search |
| `c` | Create ticket |
| `d` | Toggle Done visibility |
| `p/w/n/v` | Focus status filter |
| `r` | Refresh |
| `?` | Keybindings help |
| `q` | Quit |

### Detail View

| Key | Action |
|-----|--------|
| `Esc` | Close |
| `Up/Down` | Scroll |
| `o` | Open in browser |
| `m` | Move status |
| `C` | Comment |
| `a` | Assign/reassign |
| `e` | Edit summary + labels |
| `h` | Activity history |

Move picker: `p/w/n/t/v/b/d` to select + confirm, uppercase to move immediately.

### Filters Tab

| Key | Action |
|-----|--------|
| `j/k` | Navigate within focused pane |
| `Space` | Toggle selection (results pane) |
| `A` | Select all results (results pane) |
| `u` | Clear selection (results pane) |
| `B` | Open bulk actions (results pane) |
| `U` | Open bulk CSV upload |
| `Tab` | Switch to results / next tab |
| `Shift+Tab` | Back to sidebar |
| `Enter` | Run filter (sidebar) / open ticket (results) |
| `n` | New filter |
| `e` | Edit filter |
| `x` | Delete filter |

## Bulk CSV Upload

Use `U` to open the bulk CSV upload modal from any main view.

Flow:
1. Enter a CSV file path.
2. Preview parsed rows, warnings, and validation errors.
3. Submit only when preview has zero invalid rows.

CSV rules (V1):
- Required header: `summary`
- Optional headers: `type,assignee_email,epic_key,labels,description`
- `type` defaults to `Task` and must be one of `Task`, `Bug`, `Story`
- `labels` uses `|` separators in one cell (example: `frontend|urgent`)
- `epic_key` must match a known cached epic
- Row limit: 500 rows per upload

Warnings:
- Duplicate summary against existing visible tickets
- Duplicate summary within the CSV

Warnings do not block submission. Validation errors do.

## Configuration

Config lives at `~/.config/lazyjira/config.toml`:

```toml
[jira]
project = "AMP"
team_name = "Code Generation"
done_window_days = 14
# Keep this in the [jira] section.

epics_i_care_about = ["AMP-100", "AMP-200"] # optional; empty/missing = show all epics; list order controls Epics tab order

[team]
"Alice Smith" = "alice.smith@example.com"
"Bob Jones" = "bob.jones@example.com"

[statuses]
active = ["Needs Triage", "Ready for Work", "To Do", "In Progress", "In Review", "Blocked"]
done = ["Done", "Closed"]

[[filters]]
name = "My bugs"
jql = "type = Bug AND assignee = currentUser()"

[[filters]]
name = "Recent P1s"
jql = "priority = P1 AND created >= -7d"
```

## Cache

- Startup loads a persisted snapshot, then refreshes active tickets, then recently done.
- Epic relationships and ticket detail are cached locally and refreshed in the background.
- Cache files are project-scoped (`~/.cache/lazyjira/`, `/tmp/lazyjira_*`).
