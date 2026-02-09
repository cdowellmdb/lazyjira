# lazyjira

A terminal UI for Jira focused on fast triage and team visibility.

## Features

- Five tabs: `My Work`, `Team`, `Epics`, `Unassigned`, `Filters`
- Status-grouped ticket views with focus filters
- Epic relationship mapping + progress bars
- Rich ticket detail with description, labels, assignee, epic, activity history
- In-TUI actions: create tickets, comment, assign, edit fields, move status
- Multi-select + bulk move/assign from list views
- Saved JQL filters with persistent config
- Local caching for fast startup and detail open

## Setup

On first run, lazyjira prompts for your Jira project key and team name, then writes `~/.config/lazyjira/config.toml`. Edit the file directly to add team members, custom statuses, or saved filters.

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
| `Tab` | Switch to results / next tab |
| `Shift+Tab` | Back to sidebar |
| `Enter` | Run filter (sidebar) / open ticket (results) |
| `n` | New filter |
| `e` | Edit filter |
| `x` | Delete filter |

## Configuration

Config lives at `~/.config/lazyjira/config.toml`:

```toml
[jira]
project = "AMP"
team_name = "Code Generation"
done_window_days = 14

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
