---
name: verify-lazyjira
description: Drive the real lazyjira TUI in a private tmux session against the demo's fake Jira (no real Jira, config or cache), press keys like a user, and capture screens, Jira-side state and outbound `jira`/`open`/`pbcopy` calls as proof. Use to prove a UI or behavior change works in the running app, beyond `cargo test`.
---

# Verify lazyjira

lazyjira is a terminal UI. The helper `.claude/skills/verify-lazyjira/bin/verify` runs the release build inside a tmux server of its own (`tmux -L lazyjira-verify-<run>`), with a throwaway `HOME`, the demo config (`docs/demo/config.toml`) and two fakes from `docs/demo/bin/`:

- `fake_jira_rest.py` serves every read and every move. Moves persist in its memory until `down`.
- `jira`, the fake jira-cli, answers `jira me` and accepts comment, assign, edit and create without changing any data.

Shims in the run's `shims/` dir log each `jira`, `open` (browser) and `pbcopy` (clipboard) call to `calls.log`, so nothing reaches your real browser, clipboard or Jira. `date` is pinned to a September Wednesday, so spooky season never shows.

Every command takes a run id first. Pick a fresh one per verification (`[A-Za-z0-9_-]+`). Different ids run side by side safely.

```bash
V=.claude/skills/verify-lazyjira/bin/verify
```

## Launch

```bash
$V <run> up        # cargo build --release, start fake REST, start the TUI; waits for DEMO-101 on screen
```

Ready means `ready: run=<run> evidence=...` was printed: the list has loaded from the fake Jira. A build failure or a timeout prints the screen and exits non-zero. The terminal is 160x45.

## Doctor

Run first whenever anything looks off. Read-only.

```bash
$V <run> doctor
```

All lines must be `ok`. `FAIL lazyjira exited` prints the last screen (a panic shows there). `STALE build` means a source file is newer than `target/release/lazyjira`, so `down` then `up` again. `up` rebuilds every time.

## Drive

```bash
$V <run> keys <tmux key names...>   # e.g. keys Down Enter, keys Escape, keys m, keys S-Tab, keys C-j
$V <run> type <literal text>        # typed as-is, e.g. type DEMO-201
$V <run> wait '<ERE regex>' [secs]  # poll the screen until it matches (default 10s); prints the screen on timeout
$V <run> snap <name>                # print the screen, save evidence/<name>.txt (+ .ansi with colors)
$V <run> issue <KEY>                # the ticket's status/assignee/updated as the fake Jira holds it
$V <run> calls                      # every jira/open/pbcopy call the app made, in order
```

Reach a ticket by key, never by counting rows: `keys /`, `type DEMO-201`, `wait 'READY FOR WORK \(1\)'`, `keys Down`, `keys Enter`, `wait 'Status +●'`. Search selects the group header first, so Enter without Down does nothing.

Wait on text, not on sleeps. Status-bar messages (`Moved DEMO-201 to In Progress`, `Comment added to DEMO-205`, `Assigned DEMO-205`) are the most dependable signals that an action finished.

The fake data lives in `docs/demo/bin/jira` (`TICKETS`, `SUBTASKS`, `EPICS`, `TEAM`). You are Alex Rivera. Every ticket offers the same four transitions: `[p]` Start Progress, `[v]` Send to Review, `[b]` Block, `[c]` Done. None asks for a resolution.

## Evidence

Everything goes to `target/verify/<run>/evidence/` (gitignored):

- `<name>.txt` / `<name>.ansi`: screens from `snap`. Snap the action (picker or form open) and the result, not only the end.
- `jira-state.txt`: every `issue` lookup, appended. Take one before and one after a mutation.
- `calls.log`: outbound commands, copied there by `calls` and by `down`.

Proof standards:
- Drive the user path: keys and mouse-free flows as in `README.md`. Don't edit cache files or config to fake state.
- Moves: prove all three of the detail's `Status`, `issue <KEY>`, and the list after `r` (refresh).
- Comment, assign, edit and create change nothing in the fake data. The proof is the `calls.log` line, such as `jira issue assign DEMO-205 priya.shah@example.com`, plus the status-bar message. Never claim the change persisted in Jira.
- Settings (`S`) rewrites `target/verify/<run>/home/.config/lazyjira/config.toml`. Copy that into `evidence/` before `down`, which deletes `home/`.

## Cleanup

```bash
$V <run> down
```

This kills only this run's tmux server and fake REST pid, then deletes `home/`, `shims/` and logs. `evidence/` survives. Run it after every failed attempt too. Delete `target/verify/<run>/` yourself once the proof is reported.

## Features

[features/README.md](features/README.md) is the verification map: one file per user-facing feature, with recipes. Update it when a feature's keys, text or flow change.
