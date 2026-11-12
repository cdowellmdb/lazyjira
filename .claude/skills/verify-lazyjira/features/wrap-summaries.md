# Wrap summaries

With `preferences.wrap_summaries` on, a ticket row's summary word-wraps onto further lines under the Summary column in all five tabs, instead of ending in `...`. Epic headers and group headers stay on one line. Selecting a wrapped ticket highlights every line it takes.

## Sub-features

- `wrap-setting` turns wrapping on in Settings (`S`), in the "Wrap summaries" box, and saves it to `config.toml`.
- `wrap-rows` wraps ticket summaries in My Work, Team, Epics, Unassigned and Filters results.
- `wrap-epic-header` keeps an Epics header's summary on one line.
- `wrap-highlight` highlights all of a selected ticket's lines and nothing else.
- `wrap-scroll` scrolls so a selected wrapped ticket's last line stays on screen.

## How to get to it (user POV)

- Press `S`, `Shift+Tab` to "Wrap summaries", `↓` to On, `Enter` to save.
- Or set `wrap_summaries = true` under `[preferences]` in `~/.config/lazyjira/config.toml`.

## Driving it with verify

Preconditions:

- Fresh run, on My Work.

- **Turn it on.** `$V <run> keys S`, `$V <run> wait 'Wrap summaries'`, `$V <run> keys S-Tab Down`, `$V <run> snap wrap-setting`, `$V <run> keys Enter`, `$V <run> wait 'DEMO-101'`. Copy `target/verify/<run>/home/.config/lazyjira/config.toml` to `evidence/`; it holds `wrap_summaries = true`.
- **Make summaries wrap.** At 160 columns the demo summaries fit. Narrow the window: `tmux -L lazyjira-verify-<run> resize-window -t "$(tmux -L lazyjira-verify-<run> ls -F '#S' | head -1)" -x 80 -y 30`. My Work then shows DEMO-101's summary as `Add saved` / `payment methods` / `to checkout`. `$V <run> snap wrap-rows`.
- **Highlight.** `$V <run> keys j`, `$V <run> snap wrap-highlight`. In `evidence/wrap-highlight.ansi`, DEMO-101's three lines carry the dark gray background and DEMO-111's don't.
- **Other tabs.** `$V <run> keys Tab` for each tab and snap it. On Filters, `Enter` runs "My bugs" and the results wrap. On Epics, the `DEMO-100    Checkout redesign` header stays one line.
- **Scroll.** Resize to `-y 16`, go back to My Work, and press `j` until the last ticket is selected. Its last summary line shows above the panel's bottom border.

## Gotchas

- `S-Tab` once from the first field lands on "Wrap summaries" (the sixth field). Theme is two `S-Tab`s.
- At 80 columns, Team's and Filters' Summary columns are about 10 wide, so long words like "tokenization" break mid-word. That's expected.
- `down` deletes `home/`, so copy `config.toml` first.
