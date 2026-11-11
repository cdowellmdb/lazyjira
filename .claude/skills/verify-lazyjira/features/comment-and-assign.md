# Comment and assign

From ticket detail, a user comments on a ticket (`C`) or reassigns it (`a`). lazyjira sends each through the `jira` CLI and reports it in the status bar. Assign also updates the detail's Assignee at once.

## Sub-features

- `comment-open` opens the `Comment on <KEY>` form.
- `comment-submit` sends the comment with `Enter`. `C-j` (Ctrl+J) inserts a newline instead.
- `assign-open` opens the `Assign <KEY>` picker listing the roster plus `Unassigned`.
- `assign-filter` narrows the picker by typing.
- `assign-submit` assigns with `Enter`.
- `edit-fields` (`e`: summary, labels, description) and `create` (`c` on a list tab) send `jira issue edit` / `jira issue create` (*not yet driven*).

## How to get to it (user POV)

- In ticket detail, press `C` to comment or `a` to assign.
- Click Submit / Assign in the form (mouse; this harness doesn't click).

## Driving it with verify

Preconditions:

- Fresh run. DEMO-205 is assigned to Alex Rivera.

- **Open the ticket.** `$V <run> keys /`, `$V <run> type DEMO-205`, `$V <run> wait 'NEEDS TRIAGE \(1\)'`, `$V <run> keys Down`, `$V <run> keys Enter`, `$V <run> wait 'Status +●'`.
- **Open the comment form.** `$V <run> keys C`, `$V <run> wait 'Comment on DEMO-205'`. The form shows `Submit    Cancel    Editor`.
- **Submit.** `$V <run> type "Verified via tmux"`, `$V <run> keys Enter`, `$V <run> wait 'Comment added to DEMO-205'`, `$V <run> snap comment-submit`.
- **Open the assign picker.** `$V <run> keys a`, `$V <run> wait 'Assign DEMO-205'`. It lists Alex Rivera, Jordan Lee, Priya Shah, Sam Chen and `Unassigned (__unassigned__)`.
- **Filter and assign.** `$V <run> type Priya`, `$V <run> keys Enter`, `$V <run> wait 'Assigned DEMO-205'`. The detail shows `Assignee  Priya Shah`. `$V <run> snap assign-submit`.
- **Proof.** `$V <run> calls` lists, in order, `jira issue comment add DEMO-205 Verified\ via\ tmux --no-input` and `jira issue assign DEMO-205 priya.shah@example.com`.

## Gotchas

- The fake `jira` CLI accepts writes and changes nothing. After `r`, DEMO-205 is Alex Rivera's again and no comment shows in activity (`h`). The `calls.log` line is the side-effect proof. Don't treat the revert as a bug.
- `issue <KEY>` can't prove a comment or assign: the fake REST never sees them.
- In the comment form, `Enter` submits. Use `keys C-j` for a multiline comment.
