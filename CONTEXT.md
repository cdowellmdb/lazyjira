# Lazyjira

Lazyjira is a terminal workspace for viewing and updating Jira tickets across personal work, a team, epics, and saved filters.

## Language

**Ticket**:
A Jira issue with a key, summary, status, and optional assignee, labels, description, and epic relationship.
_Avoid_: Task when referring to every ticket type; task is one type of ticket.

**Status**:
The name Jira gives a ticket's current workflow position, such as In Progress or Resolved.
_Avoid_: Transition when referring to the ticket's current position.

**Done**:
The classification of statuses treated as finished work for visibility, active counts, and epic progress. Several differently named statuses can count as done.
_Avoid_: Closed as a synonym for every done status; Closed is a specific status name.

**Transition**:
A workflow action Jira currently offers for a ticket, leading to a destination status. Different transitions can lead to the same status.
_Avoid_: Status when referring to the action.

**Move**:
An attempt to change a ticket's status through one of its available transitions.

**Resolution**:
A value describing how a ticket was resolved, offered or required by a transition. It is distinct from the destination status.

**Epic**:
A Jira issue grouping related child tickets, with progress measured by how many children count as done.

**Sub-task**:
A Jira issue that belongs to a parent ticket. Views show it under its parent when the parent is in the same group, and otherwise lead its summary with the parent's key. Epic progress counts sub-tasks of an epic's children as part of the epic.
_Avoid_: Child ticket when referring to a ticket in an epic; that is an epic's child, not a sub-task.

**Team roster**:
The people included in the Team view. Membership is distinct from a ticket's Assigned Teams value.

**Assigned Teams**:
The Jira field used to identify a team's unassigned work.

**My Work**:
The view of tickets assigned to the current Jira user.

**Team**:
The view of work grouped by assignee, including the current user and other people in the team roster.

**Unassigned**:
Tickets with no assignee that match the chosen Assigned Teams value, also available in their own view.

**Saved filter**:
A named JQL query whose matching tickets can be viewed in the Filters tab.
_Avoid_: Search when referring to a saved JQL query.

**Search**:
A text filter over loaded tickets and, in the Team view, team members.

**Jira search**:
A JQL query sent to Jira's REST search endpoint to read tickets (`jira_rest::search`), for a list, an epic's children, sub-tasks or a batch of details. A saved filter's JQL is sent through it too.
_Avoid_: Search for a call to Jira; Search is the local text filter.

**Status focus**:
A restriction of My Work or Team to one displayed active status.

**Ticket detail**:
The expanded view of a ticket's fields, description, and comments.

**Activity**:
A ticket's history of field changes and comments.

**Bulk action**:
A move or assignment applied to selected tickets, with success, failure, or a reason for skipping each ticket.
_Avoid_: Bulk upload when updating existing tickets.

**Bulk upload**:
Creation of tickets from CSV rows after preview and validation.
