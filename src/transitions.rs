//! A ticket's workflow transitions, as Jira lists them, and the rules for choosing one.
//!
//! Each issue type has its own workflow, and a transition's name can differ from the status it
//! leads to: an epic in Backlog reaches In Progress through "Resume Progress". So moves choose
//! from the ticket's own transitions and send the chosen one by id.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cache::Status;

/// A transition Jira offers for one ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub id: String,
    pub name: String,
    /// Name of the status the transition leads to.
    pub to_name: String,
    /// The resolution field on the transition screen, if there is one.
    pub resolution: Option<ResolutionField>,
    /// Id and required flag of every field on the transition screen, sorted by id.
    fields: Vec<(String, bool)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionField {
    pub required: bool,
    pub allowed: Vec<Resolution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
struct TransitionsResponse {
    transitions: Vec<RawTransition>,
}

#[derive(Deserialize)]
struct RawTransition {
    id: String,
    name: String,
    to: RawStatus,
    #[serde(default)]
    fields: BTreeMap<String, RawField>,
}

#[derive(Deserialize)]
struct RawStatus {
    name: String,
}

#[derive(Deserialize)]
struct RawField {
    #[serde(default)]
    required: bool,
    /// Only the resolution's values are read, and other fields' values vary in shape.
    #[serde(default, rename = "allowedValues")]
    allowed_values: Vec<serde_json::Value>,
}

impl From<RawTransition> for Transition {
    fn from(raw: RawTransition) -> Self {
        let resolution = raw.fields.get("resolution").map(|field| ResolutionField {
            required: field.required,
            allowed: field
                .allowed_values
                .iter()
                .filter_map(|value| {
                    Some(Resolution {
                        id: value.get("id")?.as_str()?.to_string(),
                        name: value.get("name")?.as_str()?.to_string(),
                    })
                })
                .collect(),
        });
        Transition {
            id: raw.id,
            name: raw.name,
            to_name: raw.to.name,
            resolution,
            fields: raw
                .fields
                .into_iter()
                .map(|(id, field)| (id, field.required))
                .collect(),
        }
    }
}

/// Parses Jira's answer to `GET /rest/api/2/issue/{key}/transitions?expand=transitions.fields`.
pub fn parse_transitions(body: &str) -> Result<Vec<Transition>> {
    let response: TransitionsResponse =
        serde_json::from_str(body).context("Jira's list of transitions could not be read")?;
    Ok(merge_indistinguishable(
        response.transitions.into_iter().map(Transition::from),
    ))
}

/// Keeps one transition, the one with the lowest id, from each group that shares a name,
/// destination and set of fields. Jira can offer such twins (DSCI stories have two
/// "Ready for Work" transitions). The user can't tell them apart, so listing both, or refusing
/// a shortcut as ambiguous, would only make them guess. The lowest id is simply a stable pick.
fn merge_indistinguishable(transitions: impl IntoIterator<Item = Transition>) -> Vec<Transition> {
    let mut kept: Vec<Transition> = Vec::new();
    for transition in transitions {
        match kept.iter_mut().find(|k| k.looks_like(&transition)) {
            Some(twin) if id_order(&transition.id) < id_order(&twin.id) => *twin = transition,
            Some(_) => {}
            None => kept.push(transition),
        }
    }
    kept
}

/// Jira ids are numbers, so "9" sorts before "10".
fn id_order(id: &str) -> (u64, &str) {
    (id.parse().unwrap_or(u64::MAX), id)
}

impl Transition {
    /// Whether the user sees no difference between `self` and `other`: everything but the id matches.
    fn looks_like(&self, other: &Transition) -> bool {
        self.name == other.name
            && self.to_name == other.to_name
            && self.fields == other.fields
            && self.resolution == other.resolution
    }

    /// Whether this transition matches a status shortcut: it leads to a status that
    /// `Status::from_str` maps onto `status`. So `Closed` matches Done, Closed and Resolved.
    pub fn leads_to(&self, status: &Status) -> bool {
        Status::from_str(&self.to_name) == *status
    }

    /// Picker text: "Resume Progress → In Progress", or only the name when it is the destination.
    /// The id is added when another of the ticket's `transitions` has the same name.
    pub fn label(&self, transitions: &[Transition]) -> String {
        let mut label = if self.name == self.to_name {
            self.name.clone()
        } else {
            format!("{} → {}", self.name, self.to_name)
        };
        if transitions
            .iter()
            .any(|t| t.name == self.name && t.id != self.id)
        {
            label.push_str(&format!(" (id {})", self.id));
        }
        label
    }

    /// The resolution id to send when the user chose `choice` (`None` is "No resolution").
    /// Err says why this transition can't be sent with that choice.
    pub fn resolution_to_send(
        &self,
        choice: Option<&Resolution>,
    ) -> Result<Option<String>, String> {
        let Some(field) = &self.resolution else {
            return Ok(None);
        };
        match choice {
            Some(chosen) if field.allowed.iter().any(|r| r.id == chosen.id) => {
                Ok(Some(chosen.id.clone()))
            }
            _ if !field.required => Ok(None),
            Some(chosen) => Err(format!(
                "\"{}\" requires a resolution and doesn't allow {}",
                self.name, chosen.name
            )),
            None if field.allowed.is_empty() => Err(format!(
                "\"{}\" requires a resolution, but Jira offers none to choose from",
                self.name
            )),
            None => Err(format!("\"{}\" requires a resolution", self.name)),
        }
    }
}

/// The choices for one resolution prompt covering `fields`: `None` ("No resolution") first when
/// any of them is optional, then every allowed value once, in the order Jira lists them.
pub fn resolution_choices<'a>(
    fields: impl IntoIterator<Item = &'a ResolutionField>,
) -> Vec<Option<Resolution>> {
    let mut any_optional = false;
    let mut values: Vec<Resolution> = Vec::new();
    for field in fields {
        any_optional |= !field.required;
        for resolution in &field.allowed {
            if !values.iter().any(|v| v.id == resolution.id) {
                values.push(resolution.clone());
            }
        }
    }
    let no_resolution = any_optional.then_some(None);
    no_resolution
        .into_iter()
        .chain(values.into_iter().map(Some))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An epic in Backlog, shaped like Jira's real answer (made-up ids and URLs).
    pub const BACKLOG_EPIC: &str = r#"{
      "expand": "transitions",
      "transitions": [
        {"id": "812", "name": "Cancelled", "description": "", "opsbarSequence": 10,
         "to": {"self": "https://jira.example.com/rest/api/2/status/1", "name": "Cancelled", "id": "101",
                "statusCategory": {"id": 3, "key": "done", "name": "Done"}},
         "fields": {}},
        {"id": "824", "name": "Resume Progress", "description": "", "opsbarSequence": 20,
         "to": {"name": "In Progress", "id": "103"},
         "fields": {
           "assignee": {"required": false, "schema": {"type": "user", "system": "assignee"},
                        "name": "Assignee", "fieldId": "assignee", "operations": ["set"],
                        "autoCompleteUrl": "https://jira.example.com/rest/api/latest/user/search?username="},
           "customfield_20002": {"required": false, "schema": {"type": "date"}, "name": "Target Date",
                                 "fieldId": "customfield_20002", "operations": ["set"]}
         }},
        {"id": "87", "name": "Ready", "to": {"name": "Open", "id": "104"}, "fields": {}},
        {"id": "848", "name": "Waiting for Input", "to": {"name": "Waiting for Input", "id": "105"},
         "fields": {"assignee": {"required": false, "name": "Assignee", "fieldId": "assignee"}}}
      ]
    }"#;

    pub fn transition(id: &str, name: &str, to: &str) -> Transition {
        Transition {
            id: id.to_string(),
            name: name.to_string(),
            to_name: to.to_string(),
            resolution: None,
            fields: Vec::new(),
        }
    }

    pub fn with_resolution(
        mut t: Transition,
        required: bool,
        allowed: &[(&str, &str)],
    ) -> Transition {
        t.resolution = Some(ResolutionField {
            required,
            allowed: allowed
                .iter()
                .map(|(id, name)| Resolution {
                    id: id.to_string(),
                    name: name.to_string(),
                })
                .collect(),
        });
        t.fields = vec![("resolution".to_string(), required)];
        t
    }

    pub fn resolution(id: &str, name: &str) -> Resolution {
        Resolution {
            id: id.to_string(),
            name: name.to_string(),
        }
    }

    fn names(transitions: &[Transition]) -> Vec<(&str, &str)> {
        transitions
            .iter()
            .map(|t| (t.name.as_str(), t.to_name.as_str()))
            .collect()
    }

    #[test]
    fn parses_names_destinations_and_missing_resolution() {
        let transitions = parse_transitions(BACKLOG_EPIC).unwrap();
        assert_eq!(
            names(&transitions),
            [
                ("Cancelled", "Cancelled"),
                ("Resume Progress", "In Progress"),
                ("Ready", "Open"),
                ("Waiting for Input", "Waiting for Input"),
            ]
        );
        assert_eq!(transitions[1].id, "824");
        assert!(transitions.iter().all(|t| t.resolution.is_none()));
    }

    #[test]
    fn parses_required_and_optional_resolution_fields() {
        let body = r#"{"transitions": [
          {"id": "805", "name": "Done", "to": {"name": "Done"}, "fields": {
            "resolution": {"required": true, "name": "Resolution", "fieldId": "resolution",
              "allowedValues": [
                {"self": "https://jira.example.com/rest/api/2/resolution/1", "name": "Fixed", "id": "101"},
                {"self": "https://jira.example.com/rest/api/2/resolution/2", "name": "Won't Fix", "id": "102"}]},
            "components": {"required": false, "allowedValues": [{"id": "109", "value": "Other shape"}]}
          }},
          {"id": "815", "name": "Resolve", "to": {"name": "Resolved"}, "fields": {
            "resolution": {"required": false, "allowedValues": [{"name": "Duplicate", "id": "103"}]}
          }}
        ]}"#;
        let transitions = parse_transitions(body).unwrap();
        assert_eq!(
            transitions[0].resolution,
            Some(ResolutionField {
                required: true,
                allowed: vec![resolution("101", "Fixed"), resolution("102", "Won't Fix")],
            })
        );
        assert_eq!(
            transitions[1].resolution,
            Some(ResolutionField {
                required: false,
                allowed: vec![resolution("103", "Duplicate")],
            })
        );
    }

    #[test]
    fn rejects_a_body_that_is_not_a_transition_list() {
        let error = parse_transitions(r#"{"errorMessages": ["nope"]}"#).unwrap_err();
        assert!(format!("{:#}", error).contains("transitions could not be read"));
    }

    #[test]
    fn identical_duplicates_become_one_with_the_lowest_id() {
        let body = r#"{"transitions": [
          {"id": "87", "name": "Ready for Work", "to": {"name": "Ready for Work"},
           "fields": {"assignee": {"required": false}}},
          {"id": "836", "name": "Ready for Work", "to": {"name": "Ready for Work"},
           "fields": {"assignee": {"required": false}}},
          {"id": "861", "name": "Waiting for Approval", "to": {"name": "Waiting for Approval"}, "fields": {}},
          {"id": "862", "name": "Request LGTM", "to": {"name": "Waiting for Approval"}, "fields": {}}
        ]}"#;
        let transitions = parse_transitions(body).unwrap();
        let ids: Vec<&str> = transitions.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["87", "861", "862"]);

        // The lowest id wins even when Jira lists it second.
        let reversed = parse_transitions(&body.replace("\"87\"", "\"999\"")).unwrap();
        assert_eq!(reversed[0].id, "836");
    }

    #[test]
    fn same_name_with_different_fields_stays_separate_and_shows_its_id() {
        let plain = transition("87", "Ready for Work", "Ready for Work");
        let with_fields = Transition {
            fields: vec![("assignee".to_string(), false)],
            ..transition("836", "Ready for Work", "Ready for Work")
        };
        let transitions = merge_indistinguishable([plain, with_fields]);
        assert_eq!(transitions.len(), 2);
        assert_eq!(transitions[0].label(&transitions), "Ready for Work (id 87)");
        assert_eq!(
            transitions[1].label(&transitions),
            "Ready for Work (id 836)"
        );
    }

    #[test]
    fn labels_show_the_destination_only_when_it_differs() {
        let transitions = parse_transitions(BACKLOG_EPIC).unwrap();
        let labels: Vec<String> = transitions.iter().map(|t| t.label(&transitions)).collect();
        assert_eq!(
            labels,
            [
                "Cancelled",
                "Resume Progress → In Progress",
                "Ready → Open",
                "Waiting for Input"
            ]
        );
    }

    #[test]
    fn shortcuts_match_the_destination_not_the_name() {
        let transitions = parse_transitions(BACKLOG_EPIC).unwrap();
        let matching = |status: Status| -> Vec<&str> {
            transitions
                .iter()
                .filter(|t| t.leads_to(&status))
                .map(|t| t.name.as_str())
                .collect()
        };
        assert!(matching(Status::Closed).is_empty());
        assert_eq!(matching(Status::InProgress), ["Resume Progress"]);

        let story = [
            transition("870", "Closed", "Closed"),
            transition("871", "Resolved", "Resolved"),
            transition("805", "Done", "Done"),
            transition("855", "Won't Do", "Won't Do"),
        ];
        let closing: Vec<&str> = story
            .iter()
            .filter(|t| t.leads_to(&Status::Closed))
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(closing, ["870", "871", "805"]);
    }

    #[test]
    fn resolution_rules_follow_the_transition_field() {
        let fixed = resolution("101", "Fixed");
        let declined = resolution("107", "Declined");
        let required =
            with_resolution(transition("805", "Done", "Done"), true, &[("101", "Fixed")]);
        let optional = with_resolution(
            transition("815", "Resolve", "Resolved"),
            false,
            &[("101", "Fixed")],
        );
        let none = transition("870", "Closed", "Closed");
        let required_empty = with_resolution(transition("806", "Done", "Done"), true, &[]);

        assert_eq!(
            required.resolution_to_send(Some(&fixed)),
            Ok(Some("101".into()))
        );
        assert!(required.resolution_to_send(Some(&declined)).is_err());
        assert!(required.resolution_to_send(None).is_err());
        assert_eq!(optional.resolution_to_send(Some(&declined)), Ok(None));
        assert_eq!(optional.resolution_to_send(None), Ok(None));
        assert_eq!(none.resolution_to_send(Some(&fixed)), Ok(None));
        assert_eq!(
            required_empty.resolution_to_send(None),
            Err("\"Done\" requires a resolution, but Jira offers none to choose from".into())
        );
    }

    #[test]
    fn resolution_choices_merge_fields_and_offer_none_only_when_optional() {
        let required = with_resolution(
            transition("805", "Done", "Done"),
            true,
            &[("101", "Fixed"), ("102", "Won't Fix")],
        );
        let optional = with_resolution(
            transition("815", "Resolve", "Resolved"),
            false,
            &[("102", "Won't Fix"), ("103", "Duplicate")],
        );
        assert_eq!(
            resolution_choices(required.resolution.iter()),
            [
                Some(resolution("101", "Fixed")),
                Some(resolution("102", "Won't Fix"))
            ]
        );
        assert_eq!(
            resolution_choices(required.resolution.iter().chain(&optional.resolution)),
            [
                None,
                Some(resolution("101", "Fixed")),
                Some(resolution("102", "Won't Fix")),
                Some(resolution("103", "Duplicate")),
            ]
        );
    }
}
