# Lazyjira

Lazyjira is a terminal workspace for viewing and updating Jira tickets across personal work, a team, epics, and saved filters.

## Language

**Ticket**:
A Jira issue with a key, summary, status, and optional assignee, labels, description, and epic relationship.
_Avoid_: Task when referring to every ticket type; task is one type of ticket.

**Status**:
The name Jira gives a ticket's current workflow position, such as In Progress or Resolved. A read whose answer has no status shows Unknown, which is never a real status.
_Avoid_: Transition when referring to the ticket's current position.

**Done**:
The classification of statuses treated as finished work for visibility, active counts, and epic progress. Several differently named statuses can count as done.
_Avoid_: Closed as a synonym for every done status; Closed is a specific status name.

**Dropped**:
A done status for work the team decided not to do, such as Won't Do. It is done for visibility and active counts, but an epic's progress leaves it out, and a move to it isn't shipped.
_Avoid_: Cancelled as a synonym for every dropped status; Cancelled is a specific status name.

**Transition**:
A workflow action Jira currently offers for a ticket, leading to a destination status. Different transitions can lead to the same status.
_Avoid_: Status when referring to the action.

**Move**:
An attempt to change a ticket's status through one of its available transitions.

**Resolution**:
A value describing how a ticket was resolved, offered or required by a transition. It is distinct from the destination status.

**Epic**:
A Jira issue grouping related child tickets, with progress measured by how many of its children that aren't dropped count as done.

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
A restriction of My Work, Team or Epics to one displayed active status. In Epics it applies to an epic's children, and an epic with no child in that status is hidden.

**Ticket detail**:
The expanded view of a ticket's fields, description, and comments.

**Activity**:
A ticket's comments with their authors and times, newest first. Lazyjira's reads don't ask Jira for field changes.

**Bulk action**:
A move or assignment applied to selected tickets, with success, failure, or a reason for skipping each ticket.
_Avoid_: Bulk upload when updating existing tickets.

**Shipped**:
A ticket a confirmed move took from an active status to a done one, not a dropped one, during the session. The quit receipt lists shipped tickets.

**Stale**:
An active ticket whose Jira `updated` is at least three weeks old (dusty), or at least 90 days old (a fossil, whose row fades). Done tickets are never stale.

**Bulk upload**:
Creation of tickets from CSV rows after preview and validation.
