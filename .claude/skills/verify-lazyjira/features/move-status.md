# Move status

A user moves one ticket to another status from its detail view. The picker lists the transitions Jira offers. The ticket keeps its status until Jira confirms, then the detail, the list and Jira all agree.

## Sub-features

- `move-open` opens the picker from ticket detail with `m`.
- `move-pick` selects a transition with its status letter (`p/w/n/t/v/b/c`) or `j/k`.
- `move-confirm` sends the selected move with `Enter` or `y`.
- `move-now` sends at once with the uppercase letter (`P`, `C`, ...).
- `move-cancel` closes the picker with `Esc` and sends nothing.
- `move-done-confetti` throws confetti when an active ticket moves to Done.

## How to get to it (user POV)

- Open a ticket (`Enter` on its row), press `m`.
- Click the move option in ticket detail (mouse; this harness doesn't click).

## Driving it with verify

Preconditions:

- Fresh run. `$V <run> issue DEMO-201` reports `status=Ready for Work`.

- **Open the ticket.** `$V <run> keys /`, `$V <run> type DEMO-201`, `$V <run> wait 'READY FOR WORK \(1\)'`, `$V <run> keys Down`, `$V <run> keys Enter`, `$V <run> wait 'Status +● Ready for Work'`.
- **Open the picker.** `$V <run> keys m`, `$V <run> wait 'Move from Ready for Work:'`. The lines `[p] Start Progress → In Progress`, `[v] Send to Review → In Review`, `[b] Block → Blocked` and `[c] Done` appear.
- **Pick by status.** `$V <run> keys p`, `$V <run> wait 'Move with Start Progress → In Progress\? Press Enter or y'`, `$V <run> snap move-pick`.
- **Confirm.** `$V <run> keys y`, `$V <run> wait 'Status +● In Progress'`. The status bar reads `Moved DEMO-201 to In Progress`. `$V <run> snap move-confirm`.
- **Jira agrees.** `$V <run> issue DEMO-201` reports `status=In Progress` and an `updated` of today.
- **List agrees.** `$V <run> keys Escape`, `$V <run> keys Escape` (closes detail, then search), `$V <run> keys r`, `$V <run> wait 'DEMO-201 .*  0m '`. DEMO-201 now sits under IN PROGRESS with Updated `0m`. `$V <run> snap move-list`.
- **Move now** (*not yet driven*). Open another active ticket, `$V <run> keys m`, `$V <run> keys C`. Expect `Moved <KEY> to Done` with no confirmation prompt.

## Gotchas

- The fake Jira offers the same four transitions for every ticket, whatever its status, and never a resolution field. The resolution picker and "no matching transition" can't be reached here. Unit tests in `transitions.rs` / `move_picker.rs` cover them.
- `y` or `Enter` before any letter confirms the highlighted first option, `[p]`.
- Moves persist in the fake until `down`. A second recipe on the same run starts from the moved state.
- Confetti animates. Snap within about 2s of a Done move to catch it.
