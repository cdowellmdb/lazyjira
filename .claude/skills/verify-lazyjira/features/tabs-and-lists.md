# Tabs and lists

A user switches between My Work, Team, Epics, Unassigned and Filters. Each lists tickets in status groups, can focus one status, fold groups, hide done tickets, and refresh while keeping the selection.

## Sub-features

- `tabs-switch` moves between tabs with `Tab` / `S-Tab`.
- `tabs-groups` shows status groups like `● In Progress 2`, with sub-tasks under their parents.
- `tabs-focus` focuses one status with `f` / `F`, then back to all.
- `tabs-fold` folds the current group with `z`, every other group with `Z`.
- `tabs-done` toggles done statuses with `d`.
- `tabs-refresh` refreshes with `r` and keeps the selected ticket.
- `tabs-narrow` draws My Work and Team as key, summary and age (Team adds status) at 80 columns, and brings Epic and Labels back as the terminal widens.
- `filters-run` runs a saved filter (`My bugs`, `Blocked`) with `Enter` on the Filters sidebar.

## How to get to it (user POV)

- Launch: My Work is the start tab.
- `Tab` cycles My Work → Team → Epics → Unassigned → Filters.
- Click a tab name (mouse; this harness doesn't click).

## Driving it with verify

Preconditions:

- Fresh run, on My Work.

- **My Work.** `$V <run> wait '● In Progress 2'`. DEMO-101 shows with sub-task DEMO-111 under it, and DEMO-112 reads `DEMO-101 › Add card tokenization endpoint`. `$V <run> snap tabs-my-work`.
- **Team** (*not yet driven*). `$V <run> keys Tab`, `$V <run> wait 'DEMO-204'`. Tickets from Priya, Sam and Jordan show.
- **Epics** (*not yet driven*). `$V <run> keys Tab`, `$V <run> wait 'Checkout redesign'`. DEMO-100, DEMO-200 and DEMO-300 show with progress.
- **Unassigned** (*not yet driven*). `$V <run> keys Tab`, `$V <run> wait 'DEMO-400'`. DEMO-106, DEMO-206, DEMO-304 and DEMO-400 show.
- **Filters** (*not yet driven*). `$V <run> keys Tab`, `$V <run> wait 'My bugs'`, `$V <run> keys Enter`, `$V <run> wait 'DEMO-205'`. My bugs lists DEMO-102 and DEMO-205.
- **Status focus** (*not yet driven*). Back on My Work, `$V <run> keys f`. Only the first status group shows. `F` steps back, and cycling past the last returns to all.
- **Narrow.** `tmux -L lazyjira-verify-<run> resize-window -x 80 -y 24` on My Work: no Epic or Labels column, the summaries whole (`Add saved payment methods to checkout`), the age at the right edge and `? more` in the footer. `snap tabs-narrow-80`. At `-x 120` the Epic (`Checkout redesign`) and Labels columns are back; Team keeps both at 120 and loses Labels at 119. At `-x 60` the top bar has no Settings button. Resize back to `-x 160 -y 45` after.
- **Settings at 80x24.** `$V <run> keys S` at 80x24 shows all five Starting tab options (`Filters` last) and all five themes (`catppuccin` last). `snap settings-80`, then `keys Escape`.
- **Help's close hint.** `$V <run> keys ?` at 80x24 (help scrolls): only `Esc/? close` on the last row closes it, not the scroll hint before it. The harness has no mouse, so this is proven by `ui_tests::sheets`, not driven.
- **Refresh.** `$V <run> keys r`. The list stays on screen while it reloads.

## Gotchas

- Done groups start folded in My Work. DEMO-110 is hidden until you unfold or press `d`.
- Team includes you (Alex Rivera) per the README. Check that before calling an extra row a bug.
- The ages come from the fake data's dates relative to today. They change as the real clock moves. A stale ticket shows its age with no icon.
