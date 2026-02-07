# lazyjira follow-ups (2026-02-07)

## In progress
- [x] Cache rich ticket detail fields (description, labels, assignee, epic info, status) so Enter/detail opens with no network latency.
- [x] Hydrate rich fields from local cache during startup for both My Work and Team datasets.
- [x] Persist rich detail cache updates whenever a background detail fetch completes.

## UI/UX follow-ups
- [x] Add `?` keybindings/help overlay menu (all tabs) and wire open/close behavior.
- [x] Show the keybinding hint in the status/footer bar.
- [x] Verify Done/Focus/search keybindings are listed in the help overlay.
- [x] In Epics tab search mode, keep keyboard navigation available.
- [x] Add an accurate per-epic progress bar chart (% complete).
- [x] Add Labels as a separate column in My Work view.
- [x] Add Labels as a separate column in Team view.
- [x] Extend search filtering to include labels (My Work + Team + Epics child rows).

## Team view behavior
- [x] Include Christian in Team view (do not skip self in team aggregation).
- [x] Keep Team "done" grouped under a smaller subheader.
- [x] Confirm status column width still fits `Ready for Work` with self included.

## Validation
- [x] Run `cargo fmt`.
- [x] Run `cargo check`.
- [ ] Manual smoke test:
- [ ] `?` opens/closes help.
- [ ] Team view includes Christian.
- [ ] Enter on a previously loaded ticket detail is instant.
- [ ] Epics search can navigate with keyboard.
- [ ] Epic progress bar % matches done/total math.
- [ ] Labels render correctly in My Work + Team columns.
- [ ] Searching by label filters correctly.
