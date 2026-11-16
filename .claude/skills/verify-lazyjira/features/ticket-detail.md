# Ticket detail

A user opens a ticket from any list and reads its fields, its description rendered from Jira wiki markup, and its comments. From there they can step to the previous or next ticket in the list, zoom, and open the actions (move, comment, assign, edit, activity, browser).

## Sub-features

- `detail-open` opens the detail: its first line is `<KEY>  N of M` with a muted `×` at the right end. At 120 columns or wider it sits beside the list, which stays on the left (50 cells, keys and summaries, one `│` divider between). Below 120 columns it replaces the list. The key hints are the bottom line while it is open.
- `detail-fields` shows Status, Assignee, Reporter, Epic and Labels.
- `detail-markup` renders headings (bold, no color), bullets, quotes, tables, links, mentions, inline code (a chip) and backslash escapes in the description.
- `detail-scroll` scrolls with `j/k`, `Space`, `g/G`.
- `detail-step` moves to the previous / next ticket with `Left/Right`.
- `detail-zoom` toggles the detail over the whole content area with `z`.
- `detail-close` closes with `Esc`.

## How to get to it (user POV)

- `Enter` on a ticket row in My Work, Team, Unassigned, Epics or Filters results.
- Click the selected row again (mouse; this harness doesn't click).

## Driving it with verify

Preconditions:

- Fresh run.

- **Open by search.** `$V <run> keys /`, `$V <run> type DEMO-201`, `$V <run> wait '● Ready for Work 1'`, `$V <run> keys Down`, `$V <run> keys Enter`, `$V <run> wait 'DEMO-201  1 of 1 .*×'`.
- **Fields and markup.** `$V <run> snap detail-open`. The screen shows `Status    ● Ready for Work`, `Assignee  Alex Rivera`, `Reporter  Priya Shah`, `Epic      DEMO-200 · Search performance`, one blank row under the rule, then DEMO-201's own description: `Problem statement` with `facet_counts` and `title:*` as code, `(see DEMO-190)` with no backslash, the `Findings` table (`Query │ Hits │ Time │ Cached`), the `Proposed fix` quote (`▎ `) and bullets (`• `), and `Slow cases`. At 160 columns `Slow cases` is an aligned table (`Query │ Cause │ Seen`). At 120 or 80 it is records: `title:* red`, then `  Cause  wildcard skips ...` and `  Seen   12,400 hits ...` under it. Read `$V <run> snap` ANSI (`evidence/<name>.ansi`) for the look: code has a background and no red, headings are bold with no color, and bullets and the `▎` are the muted gray.
- **Right edge.** The text and the rules end two blank cells before the scrollbar's thumb column (the pane's last), so no line touches it.
- **Scroll** (*not yet driven*). `$V <run> keys G`, then snap. The end of the description and comments shows.
- **Step** (*not yet driven*). Open from the unfiltered list instead (`Escape` the search first, `keys Down Enter`), then `$V <run> keys Right`. The key in the title changes and `N of M` counts up.
- **Layouts.** Resize with `tmux -L lazyjira-verify-<run> resize-window -x 160 -y 45`. At 160 and 120 columns `snap` shows the list left of a `│` column and the detail right of it. At 119 and 80 the detail fills the area and no list shows. `$V <run> keys z` at 160 gives the full-area detail (the top bar stays), `z` again the split.
- **Close.** `$V <run> keys Escape`. The list shows again.

## Gotchas

- Opened from a search, the list holds only the matches, so `Left/Right` has nowhere to go (`1 of 1`).
- `o` opens the browser. Here it only logs `open http://127.0.0.1:<port>/browse/<KEY>` to `calls.log`. Prove it with `calls`, not the screen.
- The stale line ("Untouched for weeks…") comes from the fake data's old dates. It's expected.
