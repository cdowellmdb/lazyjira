# Search

In a list tab, `/` searches tickets by key, summary, labels and team members. The list narrows to matching rows under their status groups, and a query with no match says so.

## Sub-features

- `search-open` starts a search with `/`. The query shows on the bottom line as `/<text>`.
- `search-match` narrows the list to matches, keeping their status groups.
- `search-open-result` opens a match with `Down` then `Enter`.
- `search-empty` shows `No tickets found` when nothing matches.
- `search-exit` ends the search with `Esc` and restores the full list.

## How to get to it (user POV)

- Press `/` on My Work, Team, Epics or Unassigned.
- On Filters, `/` searches the results pane (see `README.md`).

## Driving it with verify

Preconditions:

- Fresh run, on My Work.

- **Match by key.** `$V <run> keys /`, `$V <run> type DEMO-201`, `$V <run> wait 'READY FOR WORK \(1\)'`. Only DEMO-201 shows, and the line under the headers reads `all rows · epic Search performance · search, perf`. `$V <run> snap search-match`.
- **Open a result.** `$V <run> keys Down`, `$V <run> keys Enter`, `$V <run> wait '\[×\]  DEMO-201'`. Close with `$V <run> keys Escape`.
- **Empty state.** `$V <run> type zzz`, `$V <run> wait 'No tickets found'`. `$V <run> snap search-empty`.
- **Exit** (*not yet driven*). `$V <run> keys Escape`, `$V <run> wait 'DEMO-101'`. The full list shows again.

## Gotchas

- The selection starts on the group header. `Enter` without `Down` does nothing, and the search stays open.
- While the search is open, letters type into the query. `m`, `q` and other keys don't act until `Esc`.
- After a ticket opened from search closes, the search is still active. Typing appends to it.
