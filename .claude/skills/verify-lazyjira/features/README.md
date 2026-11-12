# lazyjira verification map

The maintained source for verifying lazyjira's user-facing behavior. Read this index, then use the matching feature file as the recipe. `$V` is `.claude/skills/verify-lazyjira/bin/verify`.

## Baseline preconditions

- `$V <run> up` printed `ready`, and `$V <run> doctor` is all `ok`.
- The run is fresh: nothing has moved, and My Work shows DEMO-101, DEMO-201, DEMO-205, DEMO-102 and DEMO-150 in their demo statuses.
- Never drive a tmux session or fake Jira you didn't start with this run id.

## Driving conventions

- Reach tickets with search (`/`, key, `Down`, `Enter`), not by row position.
- Use `wait` on screen text after every action that loads or sends something. Use `sleep` only for keys that redraw locally.
- Treat commands as literal. tmux key names: `Enter`, `Escape`, `Tab`, `S-Tab`, `Down`, `C-j`.
- Start a fresh run for a recipe that needs the baseline after another recipe mutated it.

## Proof and skip reporting

- Snap the screen when the action is open (picker, form) and after its result.
- A mutation's proof includes a second view: `issue <KEY>` for moves, `calls.log` for jira-cli writes.
- Name the feature and sub-feature ID with each snap (`snap move-confirm`).
- Report an entry point you couldn't drive with the command tried and what was missing. Don't count it as covered by another path.
- Recipes marked *not yet driven* were written from the README and the screens seen so far. On first use, fix any wait text that doesn't match and drop the mark.

## Feature entry contract

Each feature file has an H1, one paragraph of user-visible behavior, then exactly four H2s in this order: `Sub-features`, `How to get to it (user POV)`, `Driving it with verify`, `Gotchas`.

## Features

- [Move status](./move-status.md): single-ticket move picker with Jira-side proof. Driven 2026-11-11.
- [Comment and assign](./comment-and-assign.md): jira-cli writes from ticket detail, proven through `calls.log`. Driven 2026-11-11.
- [Ticket detail](./ticket-detail.md): opening by search, fields, markup, prev/next, zoom. Opening driven 2026-11-11; the rest *not yet driven*.
- [Search](./search.md): `/` search in a list tab, match and empty states. Match and empty states seen 2026-11-11; clearing *not yet driven*.
- [Tabs and lists](./tabs-and-lists.md): the five tabs, status groups, status focus, folding, done toggle, refresh. *Not yet driven* beyond My Work loading.
- [Wrap summaries](./wrap-summaries.md): the Settings toggle, wrapped rows in every tab, highlight and scroll. Driven 2026-11-11.
