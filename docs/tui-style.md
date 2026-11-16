# TUI style guide

How lazyjira looks, and the rules behind it. Read this before changing colors, borders, spacing, footers, overlays or glyphs. The target is the polish of the Codex TUI. Its own rules live in `codex-rs/tui/styles.md` in the openai/codex repo, and this guide adapts them to lazyjira.

## Principles

Each principle and each mockup names the PR in the build order below that lands it. All four are in the code: PR 1 (the foundation), PR 2 (the top bar, lists and marking mode), PR 3 (overlays) and PR 4 (narrow widths and docs).

1. **Weight and dimness carry hierarchy, color does not.** (PR 1: the roles. PR 2: the lists.) Primary text uses the default foreground. Headers, group names, the active tab and the selected key are bold. Secondary text (epic, age, labels, assignee, footer descriptions, counts) is dim. In this guide, dim means the muted role, which is ANSI DarkGray that `theme.rs` recolors. It never means `Modifier::DIM`, which a theme can't reach.
2. **One accent.** (PR 2) Cyan marks keys, focus and the active tab, and nothing else. Yellow headers and blue info text are gone.
3. **Color means state.** (PR 2) Green is done or success. Red is blocked or error. Each status keeps a colored `●` so a board scans quickly: In Progress yellow, Ready for Work blue, Needs Triage magenta, In Review cyan, Blocked red, Done green. Unknown statuses keep the `special` role.
4. **Few frames.** (PR 2) No title box, no tab-bar box, no outer content border, no `│` between columns. Columns align with padding. One dim rule sits under the top line. A dim `│` is allowed only as the divider between the list and the detail pane.
5. **Airy.** (PR 2) One blank line between groups. Two spaces of indent per nesting level, with `└` before a sub-task. The selected row gets a `›` marker and a subtle background across its full width.
6. **Marking is a mode.** (PR 2) The `☐` column appears only after `Space` or `A`, with a bulk bar (`2 selected   B move or assign · u clear · A all`) in place of the old `sel:0` footer item.
7. **No emoji.** (PR 1) Every glyph is single width, so columns and overlays never misalign. Stale tickets show their age in dim text with no icon.
8. **Footers degrade, they never cut.** (PR 1) Hints read `key action · key action`, with bold keys and dim words. Each hint has a priority. As the terminal narrows, hints are dropped by priority, lowest importance first, then from the right within a priority, and never half a hint. `? more` always stays.
9. **Overlays do not leak.** (PR 3, in the code) An overlay clears everything behind it. Forms, pickers, bulk actions and notices dock as one panel under the list with a single rule above it and no box, taking the rows they need up to 60% of the content area. Detail sits beside the list at 120 columns or wider (the list 50 cells, one dim `│` between) and replaces it below that or after `z`. Help and Settings are full-width sheets: a bold title with a close cell, the content, and a dim `Esc/? close` hint line. While a detail is open its hints replace the footer, wrapped onto as many rows as the width needs so every key stays clickable, with a message (an error, a move waiting on Jira) on a row above them. `ui_tests` check that no list text shows through and no border glyph is drawn.
10. **Narrow terminals drop columns, not hints.** (PR 4, in the code) Epic and Labels are drawn at their full width or not at all, and are dropped, Labels first, once Summary would keep fewer than 37 columns (`SUMMARY_MIN`). At 80 columns My Work and Team show key, summary and age, and Team its status, for the demo's keys and for keys of 7 characters or more (with shorter keys and no sub-tasks Epic still fits at 80); Summary takes the freed width and ends `...` where it is cut. The footer never shows half a hint (`? more` stays; at 60x20 `Space mark` and `q quit` drop), and group headers, the top bar and the bulk bar fit at 80x24 and 60x20.

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

3. Enter on a ticket (120+ columns): list and detail side by side (PR 3, in the code; the drawn screen differs from this mockup in the ways listed under "Mockup differences")
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

4. Pressing m in ticket detail: the move panel docks under the list instead of a popup (PR 3, in the code; see "Mockup differences")
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

5. 80 columns: Epic and Labels are dropped, Summary takes their width, and the footer drops hints from the right (PR 4, in the code, as the app draws it with the demo's data)
 lazyjira   My Work   Team   Epics   Unassigned   Filters            Settings
────────────────────────────────────────────────────────────────────────────────

 ● In Progress 2
   DEMO-101 ▼    Add saved payment methods to checkout                        8w
     └ DEMO-111  Migrate stored cards to the new vault                       10w

 ● Ready for Work 1
   DEMO-201      Profile slow facet queries                                   9w

 ● Needs Triage 1
   DEMO-205      Search returns stale results after reindex                   8w

 ● Done 1  (z to open)
 Tab switch · j/k move · Space mark · Enter open · / search · q quit · ? more
```

## Build order

The work ships as a stack of four PRs, each based on the one below.

1. **Foundation.** This guide, a shared style module for the roles above, the graceful footer, emoji removal, and `TestBackend` snapshot and contrast tests.
2. **Chrome and lists.** The one-line top bar, no column bars, marking mode, airy groups and the `›` selection on all five tabs.
3. **Overlays (landed).** The docked panel, the side-by-side detail view, and full-width help and settings.
4. **Narrow widths and docs (landed).** The 80-column layout, the first-run setup sheet, README and keybinding docs, and re-recorded demo GIFs.

## Rules for new UI

- Take a style from `src/style.rs`. Do not write `Color::Cyan` or `Modifier::BOLD` inline in a renderer.
- Add a `TestBackend` snapshot for any new screen, at 80 and 120 columns.
- Test wide characters with `unicode-width`, not `str::len`.
- Update this guide when a rule changes.

### Mockup differences

Mockups 1 and 2 are the approved sketches and show a list with no Epic or Labels columns. The code draws them with those columns wherever the width allows (86 and 110 columns on My Work with the demo's keys), marked or not, because marking never adds or drops a column. The code draws mockups 3, 4 and 5 with these differences, kept on purpose:

- Group headers keep PR 2's colored `●` and its summary column is cut with `...` where the mockup shows a single `…` and drops the dots. Truncation and the headers are PR 2 and PR 4's, and the 50-cell list pane only has room for a key and a clipped summary.
- Mockup 3 shows the detail's `Updated` and `Customers hit this...` lines as examples. The pane shows exactly what the detail showed before (status, assignee, reporter, epic, labels, the stale quip, description, comments), so there is no `Updated` row.
- The detail's `[×]` close cell ends the header line, and the activity view has a `Back` button. The mockup has neither.
- The mockup's move panel has `›  p  ● In Progress` rows. The panel shows the same lines the picker always showed (`[p] Start Progress → In Progress`), because the picker's text is unchanged, and its title reads `Move DEMO-101 from In Progress` with the status muted.
- The panel has a `[×]` on its title line and its keys in the hint line are clickable. A form's buttons (`Submit`, `Cancel`, `Editor`) share the last row with its hints.
- The detail's hints take two rows below 121 columns (at 120 and at 80), split as evenly as whole hints allow, and one row from 121. The mockup's one line fits only above that. A message takes a row above them, so the content is a row shorter.
- A move failure panel is as tall as Jira's wrapped error needs, up to the 60% cap, and `j/k` scroll it when it is still taller. The bulk result scrolls the same way.
- The bulk upload preview drops its `Path:` line and its blank lines when the full chrome would leave under 3 ticket rows, which is a body of 11 rows or fewer, so at 80x24 it shows 5 rows instead of 1.
- Click cells that changed size from the boxed overlays before PR 3, because the boxes went away. Not every target kept its cell:
  - A single-line text field (Summary, Labels, Name, CSV path) is 1 row, the label and the value side by side. In a box it was 3 rows. A field of more rows is its label row and its value rows, and an option list is its label and one row per option.
  - Help's close hint (`Esc/? close`) is a target 11 wide and 1 row tall on the sheet's last row, after the scroll hint's 28 cells when help scrolls (at 80x24) and from column 2 when it fits. It was the whole 76 wide row until PR 4, and 68 wide and 3 tall in its box. The scroll hint names several keys, so it has no target, as a footer hint without a single key has none.
  `ui_tests` pin the cells of every panel and sheet at 80x24, so a change to one shows as a diff there.
- The detail pane's last column is a scrollbar thumb when the text is longer than the pane, with no track drawn, so `│` stays the divider's alone.
- Mockup 5 is what the app draws at 80 columns with the demo's data: a cut summary would end `...` rather than `…`, the group header under the cursor shows only the tint, and the folded Done group reads `(z to open)`.
- Epic and Labels are not squeezed: a column is drawn full or dropped, so between 86 and 109 columns My Work shows Epic without Labels (Team: 100 to 119, with a 14 wide status), and marking a ticket changes none of this, because the checkbox takes its two cells from Summary. The thresholds shift with the keys and statuses drawn, because they are measured as the room Summary keeps.
- Settings gives Starting tab and Theme six rows, so all five tabs and the first five themes show at 80x24. Theme has more than five options (the built-in presets and any custom themes), so it scrolls past them with no cue.
- The first-run setup is a sheet: a bold title, labelled fields with a cursor, and buttons on the last row that the keys drive (they are not clickable, the setup has never read the mouse). It runs before any theme exists, so it draws the terminal's own colors from the `style` roles.

### Inline colors still in the code

PR 1 moved the selected row and secondary text onto `src/style.rs`. PR 2 moved the top bar, the status line (`style::error` and `style::warning`), the list views and Filters' panes. PR 3 moved the overlays: the detail, the activity view, the forms, the panel and sheet frames, help, settings and the text selection and Copy button in `mouse.rs`. PR 4 moved the first-run setup. What still names a color or modifier inline:

- `widgets/markup.rs`: the colors Jira wiki markup asks for in a description or comment (headings, links, code, mentions, table rules, panels). They follow the content, not a role, so they stay inline. Keep them to ANSI colors so a theme still recolors them.
- `views/common.rs` and `views/my_work.rs`: `Modifier::BOLD` added to a style from `style.rs` (`muted` removes it, the empty-scene text adds it).
- `delight.rs`: confetti, the rainbow bar and the quit receipt, which are decoration.

### Known limits

- While a detail is open, the mouse wheel over the list scrolls the detail, not the list. A click on a list row shows that ticket.
- The move failure panel's wrap count is a port of ratatui 0.29's word wrap, counting `char`s where ratatui counts graphemes. A multi-character grapheme (an emoji with a joiner) in Jira's error can make the panel a row too tall or short. The wrap test would need a new ratatui version's behavior to change.
