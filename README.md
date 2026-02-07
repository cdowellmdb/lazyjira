# lazyjira

`lazyjira` is a terminal UI for Jira focused on fast triage and team visibility.

## Features

- Three tabs: `My Work`, `Team`, and `Epics`
- Status-grouped ticket views with focus filters
- Epic relationship mapping + progress bars
- Rich ticket detail modal (description, labels, assignee, epic)
- Optimistic ticket status move flow
- Local caching for fast startup and fast detail open

## Requirements

- Rust toolchain (`cargo`)
- Jira CLI available as `jira` in your shell
- Team roster file at `~/.claude/skills/jira/team.yml` with a `team` mapping of `name: email`

## Run

```bash
cargo run
```

## Install / Update CLI Binary

If you run `lazyjira` directly from your shell, reinstall after code changes so the global binary matches your latest local code:

```bash
cargo install --path . --force
```

Dev rebuild modes:

```bash
cargo run -- --dev
cargo run -- --dev-release
```

## Keybindings

- `Tab`: switch tab
- `j/k` or `Up/Down`: navigate
- `Enter`: open detail
- `/`: search
- `d`: toggle Done visibility (My Work + Team)
- `p`: focus In Progress
- `w`: focus Ready for Work
- `n`: focus Needs Triage
- `v`: focus In Review
- `r`: refresh
- `?`: open keybindings menu
- `q`: quit

Detail modal:

- `Esc`: close
- `o`: open ticket in browser
- `m`: open move picker

## Data/Cache Notes

- Startup first loads a persisted full-cache snapshot (if present), then refreshes active tickets, then refreshes recently updated done tickets.
- Epic relationships and rich ticket detail are cached in temp files and refreshed in the background.
