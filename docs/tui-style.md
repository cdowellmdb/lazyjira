# TUI style guide

How lazyjira looks, and the rules behind it. Read this before changing colors, borders, spacing, footers, overlays or glyphs. The target is the polish of the Codex TUI. Its own rules live in `codex-rs/tui/styles.md` in the openai/codex repo, and this guide adapts them to lazyjira.

## Principles

1. **Weight and dimness carry hierarchy, color does not.** Primary text uses the default foreground. Headers, group names, the active tab and the selected key are bold. Secondary text (epic, age, labels, assignee, footer descriptions, counts) is dim.
2. **One accent.** Cyan marks keys, focus and the active tab, and nothing else. Yellow headers and blue info text are gone.
3. **Color means state.** Green is done or success. Red is blocked or error. Each status keeps a colored `●` so a board scans quickly: In Progress yellow, Ready for Work blue, Needs Triage magenta, In Review cyan, Blocked red, Done green. Unknown statuses keep the `special` role.
4. **Few frames.** No title box, no tab-bar box, no outer content border, no `│` between columns. Columns align with padding. One dim rule sits under the top line. A dim `│` is allowed only as the divider between the list and the detail pane.
5. **Airy.** One blank line between groups. Two spaces of indent per nesting level, with `└` before a sub-task. The selected row gets a `›` marker and a subtle background across its full width.
6. **Marking is a mode.** The `☐` column appears only after `Space` or `A`, with a bulk bar (`2 selected  B move or assign · u clear · A all`) in place of the old `sel:0` footer item.
7. **No emoji.** Every glyph is single width, so columns and overlays never misalign. Stale tickets show their age in dim text with no icon.
8. **Footers degrade, they never cut.** Hints read `key action · key action`, with bold keys and dim words. They are ordered by importance and dropped from the right as the terminal narrows. `? more` always stays.
9. **Overlays do not leak.** An overlay clears everything behind it. Forms, pickers and bulk actions dock as one panel under the list with a single rule above it. Detail sits beside the list at 120 columns or wider and replaces it below that. Help and Settings are full-width sheets.
10. **Narrow terminals drop columns, not hints.** At 80 columns the epic and labels columns go first. Key, summary and age stay.

## Colors

Renderers draw with the terminal's ANSI colors, and `theme.rs` recolors the finished frame by role. Keep that design. The roles map as follows.

| Role | Use |
| --- | --- |
| `text` | Default foreground. Most text. |
| `muted` | Secondary text, rules, hints. |
| `selection` | Background of the selected row. |
| `accent` | Keys, focus, the active tab, links. |
| `highlight` | In Progress dot. No longer headers. |
| `info` | Ready for Work dot. |
| `special` | Needs Triage dot and unknown statuses. |
| `success` | Done. |
| `error` | Blocked and errors. |

Every preset must keep readable contrast for `text`, `muted`, `accent` and each status dot against its `background` and against `selection`. A test enforces this.

## Visual target

These screens are the agreed mockup, built from the demo's fake data. Bold, dim, tint and color are described in the notes under the "Colors" section above and in principles 1 to 6.

```text
1. My Work, airy list (110 columns)
 lazyjira   My Work   Team   Epics   Unassigned   Filters                                            Settings 
──────────────────────────────────────────────────────────────────────────────────────────────────────────────


 ● In Progress 2
 › DEMO-101  ▾   Add saved payment methods to checkout        Checkout redesign       7w  checkout, frontend  
     └ DEMO-111  Migrate stored cards to the new vault        Checkout redesign      10w  checkout

 ● Ready for Work 1
   DEMO-201      Profile slow facet queries                   Search performance      9w  search, perf

 ● Needs Triage 1
   DEMO-205      Search returns stale results after reindex   Search performance      8w  search

 ● In Review 1
   DEMO-102      Fix tax rounding for multi-currency carts    Checkout redesign       7w  checkout, billing

 ● Blocked 1
   DEMO-150      Rotate payment provider API keys             Checkout redesign       8w  security

 ● Done 1  (d to show)


 Tab switch · j/k move · Space mark · Enter open · m move · / search · ? more

2. After pressing Space: the checkbox column appears, with a bulk bar
 lazyjira   My Work   Team   Epics   Unassigned   Filters                                            Settings 
──────────────────────────────────────────────────────────────────────────────────────────────────────────────


 ● In Progress 2
 › ☑ DEMO-101  Add saved payment methods to checkout                                                          
   ☑ └ DEMO-111  Migrate stored cards to the new vault
   ☐   DEMO-201  Profile slow facet queries

 2 selected   B move or assign · u clear · A all

3. Enter on a ticket (120+ columns): list and detail side by side
 lazyjira   My Work   Team   Epics   Unassigned   Filters                                                      Settings 
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
 In Progress 2                                    │ DEMO-101  1 of 7
 › DEMO-101  Add saved payment methods…           │ Add saved payment methods to checkout
     └ DEMO-111  Migrate stored cards…            │ 
                                                  │ Status    ● In Progress
 Ready for Work 1                                 │ Assignee  Alex Rivera
   DEMO-201  Profile slow facet queries           │ Reporter  Priya Shah
                                                  │ Epic      DEMO-100 · Checkout redesign
 Needs Triage 1                                   │ Labels    checkout  frontend
   DEMO-205  Search returns stale results         │ Updated   7 weeks ago
                                                  │ 
 To Do 1                                          │ Context
   DEMO-112  DEMO-101 › Add card toke…            │ Customers hit this during peak checkout traffic;
                                                  │ see the incident review for details.
 In Review 1                                      │ 
   DEMO-102  Fix tax rounding for multi…          │ Acceptance criteria

 ↑↓ scroll · ←→ prev/next · m move · C comment · a assign · e edit · Esc close

4. Pressing m: one docked panel instead of a popup
 lazyjira   My Work   Team   Epics   Unassigned   Filters                                            Settings 
──────────────────────────────────────────────────────────────────────────────────────────────────────────────


 ● In Progress 2
 › DEMO-101  ▾   Add saved payment methods to checkout        Checkout redesign       7w  checkout, frontend  
     └ DEMO-111  Migrate stored cards to the new vault        Checkout redesign      10w  checkout

──────────────────────────────────────────────────────────────────────────────────────────────────────────────
 Move DEMO-101 from In Progress
  › p  ● In Progress
    v  ● In Review
    b  ● Blocked
    c  ● Done
 Enter select · Esc cancel

5. 80 columns: fewer columns, footer drops items from the right
 lazyjira   My Work   Team   Epics   Unassigned   Filters              Settings 
────────────────────────────────────────────────────────────────────────────────


 ● In Progress 2
 › DEMO-101  Add saved payment methods…                                 7w     
   └ DEMO-111  Migrate stored cards to the…                            10w

 ● Ready for Work 1
   DEMO-201  Profile slow facet queries                                 9w

 Tab switch · j/k move · Enter open · ? more
```

## Build order

The work ships as a stack of four PRs, each based on the one below.

1. **Foundation.** This guide, a shared style module for the roles above, the graceful footer, emoji removal, and `TestBackend` snapshot and contrast tests.
2. **Chrome and lists.** The one-line top bar, no column bars, marking mode, airy groups and the `›` selection on all five tabs.
3. **Overlays.** The docked panel, the side-by-side detail view, and full-width help and settings.
4. **Narrow widths and docs.** The 80-column layout, README and keybinding docs, and re-recorded demo GIFs once at the end.

## Rules for new UI

- Take a style from the shared module. Do not write `Color::Cyan` or `Modifier::BOLD` inline in a renderer.
- Add a `TestBackend` snapshot for any new screen, at 80 and 120 columns.
- Test wide characters with `unicode-width`, not `str::len`.
- Update this guide when a rule changes.
