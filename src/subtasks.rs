//! Sub-tasks under their parents: the row order that puts a sub-task after its parent, and how
//! the parent links Jira's search returns reach tickets and epics.

use std::collections::{HashMap, HashSet};

use crate::cache::{Epic, Ticket};
use crate::jira_rest::Subtask;

/// A row's place in a parent's family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// A ticket with this many sub-tasks drawn under it.
    Parent(usize),
    /// A sub-task drawn under its parent.
    Child,
}

/// `tickets` with each sub-task moved to just after its parent, when the parent is among them,
/// each with its place in the family. Everything else keeps its order, and no ticket is dropped:
/// Jira sub-tasks can't have sub-tasks, so a parent is never itself nested. A sub-task whose
/// parent isn't among `tickets` has no family.
pub fn nest(tickets: Vec<&Ticket>) -> Vec<(&Ticket, Option<Family>)> {
    let keys: HashSet<&str> = tickets.iter().map(|ticket| ticket.key.as_str()).collect();
    let mut under: HashMap<&str, Vec<&Ticket>> = HashMap::new();
    let mut roots = Vec::new();
    for ticket in tickets {
        match ticket
            .parent_key
            .as_deref()
            .filter(|parent| keys.contains(parent))
        {
            Some(parent) => under.entry(parent).or_default().push(ticket),
            None => roots.push(ticket),
        }
    }
    let mut rows = Vec::new();
    for ticket in roots {
        let children = under.remove(ticket.key.as_str()).unwrap_or_default();
        rows.push((
            ticket,
            (!children.is_empty()).then_some(Family::Parent(children.len())),
        ));
        rows.extend(
            children
                .into_iter()
                .map(|child| (child, Some(Family::Child))),
        );
    }
    rows
}

/// Sets `parent_key` on each of `tickets` that is one of `subtasks`.
pub fn set_parents<'a>(tickets: impl IntoIterator<Item = &'a mut Ticket>, subtasks: &[Subtask]) {
    let parents: HashMap<&str, &str> = subtasks
        .iter()
        .map(|subtask| (subtask.key.as_str(), subtask.parent_key.as_str()))
        .collect();
    for ticket in tickets {
        if let Some(parent) = parents.get(ticket.key.as_str()) {
            ticket.parent_key = Some(parent.to_string());
        }
    }
}

/// Sets `parent_key` on the epics' children that are sub-tasks, and adds the sub-tasks under an
/// epic's children to that epic. Jira doesn't link a sub-task to the epic itself, so without
/// this a ticket vanishes from its epic once it becomes a sub-task.
pub fn add_to_epics(epics: &mut [Epic], subtasks: &[Subtask]) {
    set_parents(
        epics.iter_mut().flat_map(|epic| epic.children.iter_mut()),
        subtasks,
    );
    for epic in epics {
        let have: HashSet<String> = epic
            .children
            .iter()
            .map(|ticket| ticket.key.clone())
            .collect();
        for subtask in subtasks {
            if have.contains(&subtask.parent_key) && !have.contains(&subtask.key) {
                epic.children.push(Ticket {
                    key: subtask.key.clone(),
                    summary: subtask.summary.clone(),
                    status: subtask.status.clone(),
                    assignee: subtask.assignee.clone(),
                    assignee_email: subtask.assignee_email.clone(),
                    reporter: None,
                    description: None,
                    labels: subtask.labels.clone(),
                    epic_key: Some(epic.key.clone()),
                    epic_name: Some(epic.summary.clone()),
                    parent_key: Some(subtask.parent_key.clone()),
                    detail_loaded: false,
                    activity: Vec::new(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(key: &str, parent: Option<&str>) -> Ticket {
        let mut ticket = Ticket::for_test(key, "To Do");
        ticket.parent_key = parent.map(String::from);
        ticket
    }

    fn subtask(key: &str, parent: &str) -> Subtask {
        Subtask {
            key: key.into(),
            parent_key: parent.into(),
            summary: format!("{key} summary"),
            status: "On Deck".into(),
            assignee: Some("Alex".into()),
            assignee_email: None,
            labels: vec![],
        }
    }

    fn keys(rows: &[(&Ticket, Option<Family>)]) -> Vec<String> {
        rows.iter().map(|(ticket, _)| ticket.key.clone()).collect()
    }

    #[test]
    fn a_sub_task_follows_its_parent_and_the_rest_keep_their_order() {
        let tickets = [
            ticket("A-1", None),
            ticket("A-9", Some("A-3")),
            ticket("A-2", None),
            ticket("A-3", None),
            ticket("A-8", Some("A-1")),
            ticket("A-7", Some("A-1")),
        ];
        let rows = nest(tickets.iter().collect());
        assert_eq!(
            keys(&rows),
            ["A-1", "A-8", "A-7", "A-2", "A-3", "A-9"],
            "children keep their relative order under the parent"
        );
        let families: Vec<_> = rows.iter().map(|(_, family)| *family).collect();
        assert_eq!(
            families,
            [
                Some(Family::Parent(2)),
                Some(Family::Child),
                Some(Family::Child),
                None,
                Some(Family::Parent(1)),
                Some(Family::Child),
            ]
        );
    }

    #[test]
    fn a_sub_task_whose_parent_is_elsewhere_stays_where_it_was_with_no_family() {
        let tickets = [ticket("A-5", Some("A-1")), ticket("A-6", None)];
        let rows = nest(tickets.iter().collect());
        assert_eq!(keys(&rows), ["A-5", "A-6"]);
        assert!(rows.iter().all(|(_, family)| family.is_none()));
    }

    #[test]
    fn parents_are_set_on_the_tickets_that_are_sub_tasks() {
        let mut tickets = [ticket("A-1", None), ticket("A-2", None)];
        set_parents(tickets.iter_mut(), &[subtask("A-2", "A-1")]);
        assert_eq!(tickets[0].parent_key, None);
        assert_eq!(tickets[1].parent_key.as_deref(), Some("A-1"));
    }

    #[test]
    fn epics_gain_the_sub_tasks_of_their_children_once() {
        let mut epics = vec![
            Epic {
                key: "E-1".into(),
                summary: "Epic one".into(),
                children: vec![ticket("A-1", None), ticket("A-2", None)],
            },
            Epic {
                key: "E-2".into(),
                summary: "Epic two".into(),
                children: vec![ticket("B-1", None)],
            },
        ];
        // A-2 is already a child, as it was before it became a sub-task of A-1.
        let subtasks = [subtask("A-5", "A-1"), subtask("A-2", "A-1")];
        add_to_epics(&mut epics, &subtasks);
        add_to_epics(&mut epics, &subtasks);

        let added: Vec<_> = epics[0].children.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(added, ["A-1", "A-2", "A-5"]);
        assert_eq!(epics[0].children[1].parent_key.as_deref(), Some("A-1"));
        let new = &epics[0].children[2];
        assert_eq!(new.parent_key.as_deref(), Some("A-1"));
        assert_eq!(new.epic_key.as_deref(), Some("E-1"));
        assert_eq!(new.epic_name.as_deref(), Some("Epic one"));
        assert_eq!(new.assignee.as_deref(), Some("Alex"));
        assert_eq!(epics[1].children.len(), 1);
    }
}
