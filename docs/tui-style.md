# TUI style guide

How lazyjira looks, and the rules behind it. Read this before changing colors, borders, spacing, footers, overlays or glyphs. The target is the polish of the Codex TUI. Its own rules live in `codex-rs/tui/styles.md` in the openai/codex repo, and this guide adapts them to lazyjira.

## Principles

Each principle and each mockup names the PR in the build order below that lands it. PR 1 (the foundation) and PR 2 (the top bar, lists and marking mode) are in the code. The others are the target.

1. **Weight and dimness carry hierarchy, color does not.** (PR 1: the roles. PR 2: the lists.) Primary text uses the default foreground. Headers, group names, the active tab and the selected key are bold. Secondary text (epic, age, labels, assignee, footer descriptions, counts) is dim. In this guide, dim means the muted role, which is ANSI DarkGray that `theme.rs` recolors. It never means `Modifier::DIM`, which a theme can't reach.
2. **One accent.** (PR 2) Cyan marks keys, focus and the active tab, and nothing else. Yellow headers and blue info text are gone.
3. **Color means state.** (PR 2) Green is done or success. Red is blocked or error. Each status keeps a colored `●` so a board scans quickly: In Progress yellow, Ready for Work blue, Needs Triage magenta, In Review cyan, Blocked red, Done green. Unknown statuses keep the `special` role.
4. **Few frames.** (PR 2) No title box, no tab-bar box, no outer content border, no `│` between columns. Columns align with padding. One dim rule sits under the top line. A dim `│` is allowed only as the divider between the list and the detail pane.
5. **Airy.** (PR 2) One blank line between groups. Two spaces of indent per nesting level, with `└` before a sub-task. The selected row gets a `›` marker and a subtle background across its full width.
6. **Marking is a mode.** (PR 2) The `☐` column appears only after `Space` or `A`, with a bulk bar (`2 selected   B move or assign · u clear · A all`) in place of the old `sel:0` footer item.
7. **No emoji.** (PR 1) Every glyph is single width, so columns and overlays never misalign. Stale tickets show their age in dim text with no icon.
8. **Footers degrade, they never cut.** (PR 1) Hints read `key action · key action`, with bold keys and dim words. Each hint has a priority. As the terminal narrows, hints are dropped by priority, lowest importance first, then from the right within a priority, and never half a hint. `? more` always stays.
9. **Overlays do not leak.** (PR 3) An overlay clears everything behind it. Forms, pickers and bulk actions dock as one panel under the list with a single rule above it. Detail sits beside the list at 120 columns or wider and replaces it below that. Help and Settings are full-width sheets.
10. **Narrow terminals drop columns, not hints.** (PR 4) At 80 columns the epic and labels columns go first. Key, summary and age stay.

Two decisions of PR 2:

- A selected group header shows only the tint, not `›`, because its glyph slot holds the dot or fold arrow.
- Team, Epics and Unassigned headers keep `▼ name` plus counts, and status-grouped lists use `● Name count`.

## Colors

Renderers draw with the terminal's ANSI colors, and `theme.rs` recolors the finished frame by role. Keep that design. The roles map as follows.

| Role | Use |
| --- | --- |
| `text` | Default foreground. Most text. |
| `muted` | Secondary text, rules, hints. |
| `subtle` | Secondary text on the selected row, and behind buttons. |
| `selection` | Background of the selected row. |
| `accent` | Keys, focus, the active tab, links. |
| `highlight` | In Progress dot. No longer headers. |
| `info` | Ready for Work dot. |
| `special` | Needs Triage dot and unknown statuses. |
| `success` | Done. |
| `error` | Blocked and errors. |

Every preset must keep readable contrast, measured as the WCAG 2 ratio. `text` on `background` needs 4.5. `text` on `selection`, and every other role (`muted`, `subtle`, `accent`, `highlight`, `info`, `special`, `success`, `error`) on both `background` and `selection`, need 3.0. A test enforces this for every preset.

## Visual target

These screens are the agreed mockup, built from the demo's fake data. Bold, dim, tint and color are described in the notes under the "Colors" section above and in principles 1 to 6.

```text
1. My Work, airy list (120 columns) (PR 2; the footer is PR 1)
 lazyjira   My Work   Team   Epics   Unassigned   Filters                                                      Settings 
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────


 ● In Progress 2
 › DEMO-101 ▼    Add saved payment methods to checkout        Checkout redesign       7w  checkout, frontend  
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


 Tab switch · j/k move · Space mark · Enter open · B bulk · / search · r refresh · q quit · ? more          done hidden 

2. After pressing Space: the checkbox column appears, with a bulk bar (PR 2; the checkbox glyph is whatever PR 2 chooses, the app draws ☒ today and this mockup matches)
 lazyjira   My Work   Team   Epics   Unassigned   Filters                                            Settings 
──────────────────────────────────────────────────────────────────────────────────────────────────────────────


 ● In Progress 2
 › ☒ DEMO-101 ▼    Add saved payment methods to checkout                                                    
   ☒   └ DEMO-111  Migrate stored cards to the new vault
   ☐ DEMO-201      Profile slow facet queries

 2 selected   B move or assign · u clear · A all

3. Enter on a ticket (120+ columns): list and detail side by side (PR 3)
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

4. Pressing m in ticket detail: the move panel docks under the list instead of a popup (PR 3)
 lazyjira   My Work   Team   Epics   Unassigned   Filters                                            Settings 
──────────────────────────────────────────────────────────────────────────────────────────────────────────────


 ● In Progress 2
 › DEMO-101 ▼    Add saved payment methods to checkout        Checkout redesign       7w  checkout, frontend  
     └ DEMO-111  Migrate stored cards to the new vault        Checkout redesign      10w  checkout

──────────────────────────────────────────────────────────────────────────────────────────────────────────────
 Move DEMO-101 from In Progress
  › p  ● In Progress
    v  ● In Review
    b  ● Blocked
    c  ● Done
 Enter select · Esc cancel

5. 80 columns: fewer columns, footer drops items from the right (PR 4; the footer is what PR 1 draws)
 lazyjira   My Work   Team   Epics   Unassigned   Filters              Settings 
────────────────────────────────────────────────────────────────────────────────


 ● In Progress 2
 › DEMO-101  Add saved payment methods…                                 7w     
   └ DEMO-111  Migrate stored cards to the…                            10w

 ● Ready for Work 1
   DEMO-201  Profile slow facet queries                                 9w

 Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more
```

## Build order

The work ships as a stack of four PRs, each based on the one below.

1. **Foundation.** This guide, a shared style module for the roles above, the graceful footer, emoji removal, and `TestBackend` snapshot and contrast tests.
2. **Chrome and lists.** The one-line top bar, no column bars, marking mode, airy groups and the `›` selection on all five tabs.
3. **Overlays.** The docked panel, the side-by-side detail view, and full-width help and settings.
4. **Narrow widths and docs.** The 80-column layout, README and keybinding docs, and re-recorded demo GIFs once at the end.

## Rules for new UI

- Take a style from `src/style.rs`. Do not write `Color::Cyan` or `Modifier::BOLD` inline in a renderer.
- Add a `TestBackend` snapshot for any new screen, at 80 and 120 columns.
- Test wide characters with `unicode-width`, not `str::len`.
- Update this guide when a rule changes.

### Inline colors still in the code

PR 1 moved the selected row and secondary text onto `src/style.rs`. PR 2 moved the top bar, the status line (`style::error` and `style::warning`), the list views and Filters' panes, so `views/` and `ui()` no longer name a color inline except these:

- `views/common.rs` and `views/my_work.rs`: `Modifier::BOLD` added to a style from `style.rs` (`muted` removes it, the empty-scene text adds it).
- `mouse.rs`: the text selection highlight (`LightBlue`) and the Copy button text (`Cyan`).
- The overlays in `src/widgets/` (detail, help, forms, bulk, activity): their muted text and highlights. PR 3 rebuilds them.
