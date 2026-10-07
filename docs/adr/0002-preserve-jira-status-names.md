# Preserve Jira status names and centralize status rules

Tickets retain Jira's actual status names so distinct workflow positions remain visible instead of collapsing into a fixed set of built-in names. Configuration determines display order and which statuses count as done; shared status rules apply that interpretation to grouping, visibility, active counts, and epic progress. Built-in status knowledge remains for shortcuts, default colors, and fallback interpretation of unlisted names.

Recorded from the existing design in [the architecture notes](../architecture.md#statuses) and [the status rules](../../src/cache.rs).
