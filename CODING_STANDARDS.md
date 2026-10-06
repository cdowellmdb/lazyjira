# Coding standards

Reviewers apply these rules to every change; `code-review` reads them for its Standards axis. The rules are judgement calls unless they say otherwise.

## Required reviews

Code quality and architecture are release requirements. Before raising any PR, run all four reviews: `ponytail:ponytail-review`, `code-review` (both Standards and Spec), `thermo-nuclear-code-quality-review`, and `pragmatic-programmer`. Address every P0, P1 and P2 finding and re-review the affected changes before opening the PR. These reviews are mandatory for every PR.

## Promised bounds have boundary tests

When a docstring, comment or ADR promises a limit, a unit, or what is retained, a test exercises the exact boundary and asserts the exact value. An inequality does not prove what was kept.

For example, "searches 50 keys at a time" needs a test with exactly 50 keys (one search) and one with 51 (two searches, the second holding only the 51st key). `assert!(searches.len() <= 2)` passes even when a key is dropped. Test the promised unit too: a column width needs wide characters (`unicode-width`, not `str::len`), and a byte limit needs multi-byte input split at the boundary.

## PR bodies

PR bodies follow the `/pr` template (Summary, Evidence, Merge Danger) plus a short Reviews section with evidence that the four required reviews ran: findings by severity, what was fixed, and what was skipped and why. Keep long review logs and decision tables in the spec or ticket.

When the Summary is a file tree, list only the changed files in a `diff` block so GitHub colors each line by status:

```diff
+ src/jira_search.rs        NEW
- src/old_module.rs         REMOVED
! src/jira_client.rs        list reads → REST search
- src/views/old_name.rs     RENAMED ↓
+ src/views/new_name.rs
```

Keep each note to a few words; put anything longer below the tree.

## After opening a PR

There is no CI on pull requests (the only workflow builds releases from tags), so the checks run locally: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (both enforced by `.githooks/pre-commit`) and `cargo test` pass on the PR's head before it's marked ready. If the PR has GitHub checks, follow them with `gh pr checks <pr> --watch` until they pass, and fix any failure. Read every Copilot review comment: fix the ones that hold, and reply to each with the fixing commit or why no change is needed. Re-run the affected reviews after any fix. Don't merge; the user does. When the user authorizes a merge, merge only after the local checks pass on the PR's head and any GitHub checks are green.
