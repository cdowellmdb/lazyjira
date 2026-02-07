# lazyjira — Design Document

A fast, cache-first TUI for viewing Jira tickets. Built with Rust + Ratatui.

## Overview

lazyjira pulls all relevant Jira data on startup via the `jira` CLI, caches it in memory, and lets you navigate instantly across three views. Think `lazygit` but for Jira.

## Views

### Tab 1: My Work

Your tickets grouped by status, sorted newest-first within each group.

```
 My Work          Team          Epics
─────────────────────────────────────────────────
 IN PROGRESS (2)
  AMP-341  Add session resumption         Epic: Auth
  AMP-298  Fix connection pooling          Epic: Perf

 TO DO (3)
  AMP-412  Rate limiting middleware        Epic: Auth
  AMP-405  Add retry logic                Epic: Perf
  AMP-399  Update SDK error types         Epic: SDK

 IN REVIEW (1)
  AMP-287  Connection string parser        Epic: SDK

 DONE (5 most recent)
  AMP-260  Add TLS support                Epic: Auth
```

### Tab 2: Team

One section per team member. Shows non-Done tickets. Members with more active work sort to the top.

```
 My Work          Team          Epics
─────────────────────────────────────────────────
 Nathan        ● AMP-320 In Progress   Add driver metrics
               ● AMP-315 In Review     Update changelog

 Darshak       ● AMP-333 In Progress   Fix cursor leak

 Gui           (no active tickets)
```

### Tab 3: Epics

Each epic with a progress bar and status breakdown.

```
 My Work          Team          Epics
─────────────────────────────────────────────────
 AMP-200  Auth Overhaul        ████████░░░░  8/15  (53%)
          In Progress: 3  To Do: 4  In Review: 2  Done: 8

 AMP-180  Performance          ██████░░░░░░  6/14  (43%)
          In Progress: 2  To Do: 5  In Review: 1  Done: 6
```

## Ticket Detail

Press `Enter` on any ticket to open a detail overlay:

```
┌─ AMP-341 ─────────────────────────────────────┐
│ Add session resumption                         │
│                                                │
│ Status: In Progress    Assignee: Christian      │
│ Epic: AMP-200 (Auth Overhaul)                  │
│                                                │
│ Description:                                   │
│ Users lose their session when the driver        │
│ reconnects after a network blip. Need to        │
│ cache session tokens and resume on reconnect.   │
│                                                │
│ [Esc] close   [o] open in browser   [m] move   │
└────────────────────────────────────────────────┘
```

## Status Move

Press `m` from the detail overlay to change ticket status:

```
┌─ AMP-341 ─────────────────────────────────────┐
│ Move to:                                       │
│   > To Do                                      │
│     In Review                                  │
│     Blocked                                    │
│     Done                                       │
│                                                │
│ [Enter] confirm   [Esc] cancel                 │
└────────────────────────────────────────────────┘
```

- Shells out to `jira issue move AMP-341 "<Status>"`
- Optimistic cache update (instant UI response)
- Reverts on CLI failure with error flash

## Key Bindings

| Key     | Action                              |
|---------|-------------------------------------|
| `Tab`   | Cycle between tabs                  |
| `Enter` | Open ticket detail                  |
| `Esc`   | Close detail / cancel               |
| `r`     | Refresh (re-fetch from Jira CLI)    |
| `o`     | Open ticket in browser              |
| `m`     | Move ticket status (from detail)    |
| `/`     | Filter/search within current view   |
| `q`     | Quit                                |
| `j`/`k` | Navigate up/down (vim-style)        |

## Architecture

### Data Flow

1. `main.rs` starts → spawns async tasks via `tokio` to call `jira` CLI in parallel
2. `jira_client.rs` runs 3 parallel subprocess calls, parses JSON output
3. Results stored in `cache.rs` (in-memory structs)
4. Views in `views/` read from cache to render
5. `r` triggers full cache refresh → views re-render
6. Status moves write via `jira_client.rs` → update cache optimistically

### Project Structure

```
lazyjira/
├── Cargo.toml
├── src/
│   ├── main.rs               # Entry point, terminal setup, app loop
│   ├── app.rs                # App state, tab management, key handling
│   ├── jira_client.rs        # Shells out to jira CLI, parses JSON output
│   ├── cache.rs              # In-memory cache, data types
│   ├── views/
│   │   ├── mod.rs
│   │   ├── my_work.rs        # My Work tab rendering
│   │   ├── team.rs           # Team tab rendering
│   │   └── epics.rs          # Epics tab rendering
│   └── widgets/
│       ├── mod.rs
│       └── ticket_detail.rs  # Detail overlay + status mover
```

### Dependencies

- `ratatui` — TUI rendering
- `crossterm` — terminal backend
- `tokio` — async runtime (parallel CLI calls)
- `serde` + `serde_json` — parse jira CLI JSON output
- `serde_yaml` — parse team.yml

### Binary Name

`lazyjira`

## Cache Strategy

- Fetch on startup only
- Manual refresh with `r`
- No background polling

## Team Roster

Reads from `~/.claude/skills/jira/team.yml`. 12 members on the Code Generation team.

## Jira Configuration

- **Project:** AMP
- **Auth:** Via existing `jira` CLI authentication
- **Statuses:** To Do, In Progress, In Review, Blocked, Done

## Non-Goals

- No ticket creation (use `jira` CLI or `/jira` skill)
- No commenting
- No editing ticket fields
- No sprint tracking (team uses epic-based + kanban flow)
