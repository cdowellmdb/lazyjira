# Use Jira CLI with REST for transitions

Lazyjira uses the existing Jira CLI for ticket reads, creation, comments, assignment, and field edits, and calls Jira REST directly for transition discovery and execution because the CLI does not expose the transition details the move workflow needs. Moves use each ticket's available transition IDs and allowed resolutions, since workflows vary and a transition's name can differ from its destination status. The REST adapter reuses Jira CLI configuration and the existing token so this split does not require a separate setup flow.

**Update:** ticket reads (lists, epics, details) now use Jira's REST API too ([ADR 0005](0005-read-lists-over-jira-rest-search.md)), which supersedes the read half of this decision. jira-cli still answers `jira me`, creates tickets, comments, assigns and edits fields.

Recorded from the existing design in [the architecture notes](../architecture.md#moves-use-jiras-transitions) and [the transition implementation](../../src/jira_rest.rs).
