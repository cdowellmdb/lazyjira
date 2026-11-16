# First-run setup

With no `config.toml`, lazyjira opens a setup sheet: a bold title, the project key, then the team name, then a review of what it will save. `Enter` continues and `Esc` goes back; on the review `Enter` or `y` saves and `Esc` or `n` starts over.

## Sub-features

- `setup-project` types the project key in capitals, letters and digits only, `Backspace` deletes, `Enter` needs one character.
- `setup-team` types the team name, `Esc` goes back to the project key with both kept.
- `setup-confirm` shows project, team, your email from `jira me` and the config path, with Save and Start over buttons.

## How to get to it (user POV)

Start lazyjira with an empty `HOME`, so there is no `~/.config/lazyjira/config.toml`.

## Driving it with verify

`verify up` always starts with the demo config, so this one is driven by hand in a private tmux server, with the fake `jira` CLI first on `PATH` (it answers `jira me`). Nothing real is read.

```bash
H=$(mktemp -d); mkdir -p "$H/tmp"; T="tmux -L lazyjira-verify-setup"
$T new-session -d -x 80 -y 24 "env HOME=$H TMPDIR=$H/tmp PATH=$PWD/docs/demo/bin:$PATH JIRA_API_TOKEN=demo $PWD/target/release/lazyjira"
$T capture-pane -p                     # First-time setup, Project key, Continue
$T send-keys d e m o; $T send-keys Enter
$T send-keys P a y; $T capture-pane -p # Team name field, Back button
$T send-keys Enter; $T capture-pane -p # Save this configuration?, Save and Start over
$T kill-server; rm -rf "$H"
```

Resize with `$T resize-window -x 60 -y 20` to check the 60x20 sheet. Do not press `y` or `Enter` on the review unless `HOME` is the throwaway one: it writes `config.toml` there.

## Gotchas

- The setup reads no mouse, so its buttons are drawn but not clickable.
- A long config path is cut at the sheet's right edge.
- `target/release/lazyjira` must be current: `verify <run> up` rebuilds it.
