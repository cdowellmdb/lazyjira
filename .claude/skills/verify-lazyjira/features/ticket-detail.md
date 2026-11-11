# Ticket detail

A user opens a ticket from any list and reads its fields, its description rendered from Jira wiki markup, and its comments. From there they can step to the previous or next ticket in the list, zoom, and open the actions (move, comment, assign, edit, activity, browser).

## Sub-features

- `detail-open` opens the popup titled `[×]  <KEY>` with `N of M` in the corner.
- `detail-fields` shows Status, Assignee, Reporter, Epic and Labels.
- `detail-markup` renders headings, numbered lists, tables, links and mentions in the description.
- `detail-scroll` scrolls with `j/k`, `Space`, `g/G`.
- `detail-step` moves to the previous / next ticket with `Left/Right`.
- `detail-zoom` toggles full screen with `z`.
- `detail-close` closes with `Esc`.

## How to get to it (user POV)

- `Enter` on a ticket row in My Work, Team, Unassigned, Epics or Filters results.
- Click the selected row again (mouse; this harness doesn't click).

## Driving it with verify

Preconditions:

- Fresh run.

- **Open by search.** `$V <run> keys /`, `$V <run> type DEMO-201`, `$V <run> wait 'READY FOR WORK \(1\)'`, `$V <run> keys Down`, `$V <run> keys Enter`, `$V <run> wait '\[×\]  DEMO-201'`.
- **Fields and markup.** `$V <run> snap detail-open`. The screen shows `Status    ● Ready for Work`, `Assignee  Alex Rivera`, `Reporter  Priya Shah`, `Epic      DEMO-200 · Search performance`, the `Acceptance criteria` numbered list and the `Rollout` table (`Stage │ Traffic │ Owner`).
- **Scroll** (*not yet driven*). `$V <run> keys G`, then snap. The end of the description and comments shows.
- **Step** (*not yet driven*). Open from the unfiltered list instead (`Escape` the search first, `keys Down Enter`), then `$V <run> keys Right`. The key in the title changes and `N of M` counts up.
- **Zoom** (*not yet driven*). `$V <run> keys z`. The popup fills the terminal and the tab bar is hidden. `z` again restores it.
- **Close.** `$V <run> keys Escape`. The list shows again.

## Gotchas

- Opened from a search, the list holds only the matches, so `Left/Right` has nowhere to go (`1 of 1`).
- `o` opens the browser. Here it only logs `open http://127.0.0.1:<port>/browse/<KEY>` to `calls.log`. Prove it with `calls`, not the screen.
- The stale line ("Untouched for weeks…") comes from the fake data's old dates. It's expected.
