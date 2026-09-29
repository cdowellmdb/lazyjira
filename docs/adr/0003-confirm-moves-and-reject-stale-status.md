# Confirm moves before changing status and guard against stale reads

A ticket's displayed status changes only after Jira confirms its move, and a rejected move remains visible until dismissed. Concurrent reads are stamped when requested so a result requested before a confirmed move cannot restore the older status. This preserves confirmed moves while allowing background detail, ticket, epic, and filter reads to continue.

Recorded from the existing design in [CLAUDE.md](../../CLAUDE.md#moves-wait-for-jira) and [move tracking](../../src/moves.rs); this decision concerns moves, not assignment or field-edit confirmation behavior.
