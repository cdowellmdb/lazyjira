//! Building JQL safely: quoting a value, and keeping only what is shaped like a ticket key in a
//! list, so config, saved filters and keys from a cache can't change the query they go in. The
//! list, epic, sub-task and saved-filter queries are built here.

use crate::config::AppConfig;

/// How many keys one `key in (…)` or `parent in (…)` search holds.
pub const KEYS_PER_SEARCH: usize = 50;

/// Whether `key` looks like a Jira key, safe to put in a JQL list.
pub fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The keys of `keys` that look like Jira keys, `KEYS_PER_SEARCH` to a chunk, each chunk for one
/// search's list (`.join(",")`). A key that doesn't look like one is left out before the keys
/// are counted, so it can't break the query or take a place from a real key.
pub fn key_chunks(keys: &[String]) -> Vec<Vec<&str>> {
    let usable: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|key| is_key(key))
        .collect();
    usable
        .chunks(KEYS_PER_SEARCH)
        .map(<[&str]>::to_vec)
        .collect()
}

/// `text` as a JQL string literal, quotes included: a `"` or `\` in it can't end the string or
/// change the query.
pub fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Which tickets the list search covers.
#[derive(Debug, Clone, Copy)]
pub enum TicketFetchScope {
    ActiveOnly,
    ActiveAndRecentDone,
}

/// The one search behind My Work, Team and Unassigned: the active tickets of everyone in
/// `assignee_emails`, plus those done inside the window for the full scope, and the active
/// tickets nobody has taken that are the team's by Assigned Teams. A REST search needs the
/// project and an order spelled out. The order keeps pages stable while tickets change
/// underneath them.
pub fn lists_jql(config: &AppConfig, assignee_emails: &[&str], scope: TicketFetchScope) -> String {
    let assignees = assignee_emails
        .iter()
        .map(|email| quote(email))
        .collect::<Vec<_>>()
        .join(", ");
    let active = config.active_status_clause();
    let statuses = match scope {
        TicketFetchScope::ActiveOnly => format!("status in {active}"),
        TicketFetchScope::ActiveAndRecentDone => format!(
            "(status in {active} OR (status in {} AND updated >= {}))",
            config.done_status_clause(),
            config.done_window()
        ),
    };
    format!(
        "project = {} AND ((assignee in ({assignees}) AND {statuses}) \
         OR (assignee is EMPTY AND \"Assigned Teams\" = {} AND status in {active})) \
         ORDER BY key",
        quote(&config.jira.project),
        quote(&config.jira.team_name)
    )
}

/// Every sub-task in the project, in one search. Jira doesn't link a sub-task to its parent's
/// epic, so the epics read finds them here and places them by `parent`. The order keeps pages
/// stable while tickets change underneath them.
pub fn subtasks_jql(project: &str) -> String {
    format!(
        "project = {} AND issuetype in subTaskIssueTypes() ORDER BY key",
        quote(project)
    )
}

/// Every ticket in the project that names an epic through the Epic Link field (company-managed
/// projects), in one search. Asking for the children of 50 epics by key took Jira about a second
/// a search where it was measured, and had to wait for the epics' keys; this waits for nothing.
/// It also finds the children of other projects' epics, which belong to no epic here. Only for
/// when jira-cli's config knows the field. The order keeps pages stable while tickets change
/// underneath them.
pub fn children_by_epic_link_jql(project: &str) -> String {
    format!(
        "project = {} AND \"Epic Link\" is not EMPTY ORDER BY key",
        quote(project)
    )
}

/// The searches that find the tickets naming one of `epic_keys` as their `parent` (team-managed
/// projects), `KEYS_PER_SEARCH` epics to a search: Jira Server can't ask for every ticket with a
/// parent. A key that isn't shaped like a ticket key is left out. The order keeps pages stable
/// while tickets change underneath them.
pub fn children_by_parent_jqls(project: &str, epic_keys: &[String]) -> Vec<String> {
    key_chunks(epic_keys)
        .into_iter()
        .map(|chunk| {
            format!(
                "project = {} AND parent in ({}) ORDER BY key",
                quote(project),
                chunk.join(",")
            )
        })
        .collect()
}

/// A saved filter's `jql` limited to `project`, with an order. The filter's own ORDER BY is
/// kept (after the project, outside the parentheses); without one, newest first.
pub fn scoped_jql(project: &str, jql: &str) -> String {
    // ponytail: the last "order by" is taken as the clause, so one inside a quoted string
    // fails the query; parse JQL properly if filters ever need it.
    let split = jql.to_ascii_lowercase().rfind("order by");
    let (condition, order) = match split {
        Some(at) => (jql[..at].trim(), jql[at..].trim()),
        None => (jql.trim(), "ORDER BY created DESC"),
    };
    let project = quote(project);
    if condition.is_empty() {
        format!("project = {project} {order}")
    } else {
        format!("project = {project} AND ({condition}) {order}")
    }
}

/// All the project's epics, in an order that keeps pages stable while epics change underneath
/// them.
pub fn epics_jql(project: &str) -> String {
    format!(
        "project = {} AND issuetype = Epic ORDER BY key",
        quote(project)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{JiraConfig, StatusConfig};
    use std::collections::BTreeMap;

    #[test]
    fn quote_keeps_a_value_inside_its_string() {
        assert_eq!(quote("Platform Team"), "\"Platform Team\"");
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(r"back\slash"), r#""back\\slash""#);
        // The backslash is escaped before the quote, so an escaped quote can't be forged.
        assert_eq!(quote(r#"\""#), r#""\\\"""#);
    }

    fn keys(count: usize) -> Vec<String> {
        (1..=count).map(|n| format!("DEMO-{n}")).collect()
    }

    #[test]
    fn only_keys_shaped_like_jira_keys_reach_a_jql_list() {
        let keys: Vec<String> = ["DEMO-1", "x\") OR 1=1", "", "AB_2"]
            .map(String::from)
            .into();
        assert_eq!(key_chunks(&keys), [["DEMO-1", "AB_2"]]);
        assert!(key_chunks(&["no good".to_string()]).is_empty());
        assert!(key_chunks(&[]).is_empty());
        assert!(is_key("DEMO-3244") && !is_key("DEMO 1") && !is_key(""));
    }

    #[test]
    fn keys_are_chunked_by_the_search_limit_with_malformed_ones_taking_no_place() {
        let sizes =
            |keys: &[String]| -> Vec<usize> { key_chunks(keys).iter().map(Vec::len).collect() };
        assert_eq!(sizes(&keys(50)), [50]);
        let fifty_one = keys(51);
        assert_eq!(sizes(&fifty_one), [50, 1]);
        assert_eq!(key_chunks(&fifty_one)[1], ["DEMO-51"]);
        // A malformed key among 50 real ones doesn't push the 50th into a second search.
        let mut with_bad = keys(50);
        with_bad.insert(10, "bad key".to_string());
        assert_eq!(sizes(&with_bad), [50]);
    }

    #[test]
    fn the_epic_list_is_every_epic_in_the_project() {
        assert_eq!(
            epics_jql("DEMO"),
            "project = \"DEMO\" AND issuetype = Epic ORDER BY key"
        );
    }

    fn test_config() -> AppConfig {
        AppConfig {
            jira: JiraConfig {
                project: "DEMO".into(),
                team_name: "Payments Platform".into(),
                done_window_days: 14,
                epics_i_care_about: vec![],
            },
            team: BTreeMap::new(),
            statuses: StatusConfig {
                active: vec!["In Progress".into(), "To Do".into()],
                done: vec!["Done".into(), "Closed".into()],
                dropped: vec![],
            },
            filters: vec![],
            preferences: Default::default(),
            themes: Default::default(),
        }
    }

    #[test]
    fn dropped_statuses_never_reach_the_list_search() {
        let without = test_config();
        let mut with = test_config();
        with.statuses.dropped = vec!["Won't Do".into(), "Not In This Project".into()];
        for scope in [
            TicketFetchScope::ActiveOnly,
            TicketFetchScope::ActiveAndRecentDone,
        ] {
            let jql = lists_jql(&with, &["alex@example.com"], scope);
            assert_eq!(jql, lists_jql(&without, &["alex@example.com"], scope));
            assert!(!jql.contains("Won't Do"), "{jql}");
        }
    }

    #[test]
    fn the_list_search_covers_the_roster_and_the_teams_unassigned_work() {
        let config = test_config();
        let emails = ["alex@example.com", "sam@example.com"];
        assert_eq!(
            lists_jql(&config, &emails, TicketFetchScope::ActiveOnly),
            "project = \"DEMO\" AND (\
             (assignee in (\"alex@example.com\", \"sam@example.com\") \
              AND status in (\"In Progress\", \"To Do\")) \
             OR (assignee is EMPTY AND \"Assigned Teams\" = \"Payments Platform\" \
              AND status in (\"In Progress\", \"To Do\"))) ORDER BY key"
        );
    }

    #[test]
    fn the_full_list_search_adds_recently_done_tickets_inside_the_window() {
        let mut config = test_config();
        config.jira.done_window_days = 7;
        let jql = lists_jql(
            &config,
            &["alex@example.com"],
            TicketFetchScope::ActiveAndRecentDone,
        );
        assert_eq!(
            jql,
            "project = \"DEMO\" AND (\
             (assignee in (\"alex@example.com\") AND (status in (\"In Progress\", \"To Do\") \
              OR (status in (\"Done\", \"Closed\") AND updated >= -7d))) \
             OR (assignee is EMPTY AND \"Assigned Teams\" = \"Payments Platform\" \
              AND status in (\"In Progress\", \"To Do\"))) ORDER BY key"
        );
    }

    fn epic_keys(count: usize) -> Vec<String> {
        (1..=count).map(|n| format!("DEMO-{n}")).collect()
    }

    #[test]
    fn epic_link_children_and_sub_tasks_are_each_one_project_wide_search() {
        assert_eq!(
            children_by_epic_link_jql("DEMO"),
            "project = \"DEMO\" AND \"Epic Link\" is not EMPTY ORDER BY key"
        );
        assert_eq!(
            subtasks_jql("DEMO"),
            "project = \"DEMO\" AND issuetype in subTaskIssueTypes() ORDER BY key"
        );
    }

    #[test]
    fn children_by_parent_are_searched_fifty_epics_at_a_time() {
        let search =
            |list: String| format!("project = \"DEMO\" AND parent in ({list}) ORDER BY key");
        // Exactly 50 epics: one search holding all of them.
        let one = children_by_parent_jqls("DEMO", &epic_keys(50));
        assert_eq!(one, [search(epic_keys(50).join(","))]);
        // 51 epics: a second search that holds only the 51st.
        assert_eq!(
            children_by_parent_jqls("DEMO", &epic_keys(51)),
            [one[0].clone(), search("DEMO-51".to_string())]
        );
        assert!(children_by_parent_jqls("DEMO", &[]).is_empty());
    }

    #[test]
    fn epic_keys_that_are_not_shaped_like_keys_stay_out_of_the_query() {
        let keys = vec!["DEMO-1".to_string(), "x\") OR 1=1".to_string()];
        assert_eq!(
            children_by_parent_jqls("DEMO", &keys),
            ["project = \"DEMO\" AND parent in (DEMO-1) ORDER BY key"]
        );
        // A chunk with nothing left to ask about sends no search.
        assert!(children_by_parent_jqls("DEMO", &["no good".to_string()]).is_empty());
    }

    #[test]
    fn a_malformed_epic_key_takes_no_place_in_a_search() {
        let search =
            |list: String| format!("project = \"DEMO\" AND parent in ({list}) ORDER BY key");
        // 50 usable keys and a malformed one: still one search of exactly the 50 epics.
        let mut keys = epic_keys(50);
        keys.insert(10, "no good".to_string());
        assert_eq!(
            children_by_parent_jqls("DEMO", &keys),
            [search(epic_keys(50).join(","))]
        );
        // The 51st usable key starts the second search, alone.
        keys.push("DEMO-51".to_string());
        assert_eq!(
            children_by_parent_jqls("DEMO", &keys),
            [
                search(epic_keys(50).join(",")),
                search("DEMO-51".to_string())
            ]
        );
    }

    #[test]
    fn a_saved_filter_is_limited_to_the_project_and_keeps_its_own_order() {
        assert_eq!(
            scoped_jql("DEMO", "type = Bug AND assignee = currentUser()"),
            "project = \"DEMO\" AND (type = Bug AND assignee = currentUser()) \
             ORDER BY created DESC"
        );
        // The filter's own ordering wins, and stays outside the parentheses.
        assert_eq!(
            scoped_jql("DEMO", "status = Blocked order by updated ASC"),
            "project = \"DEMO\" AND (status = Blocked) order by updated ASC"
        );
        assert_eq!(
            scoped_jql("DEMO", "ORDER BY priority DESC"),
            "project = \"DEMO\" ORDER BY priority DESC"
        );
        assert_eq!(
            scoped_jql("DEMO", "  "),
            "project = \"DEMO\" ORDER BY created DESC"
        );
    }

    #[test]
    fn jql_built_from_config_cannot_be_broken_by_a_quote_in_it() {
        let mut config = test_config();
        config.jira.team_name = r#"Team "A""#.into();
        config.statuses.active = vec![r#"On "Hold""#.into()];
        let jql = lists_jql(&config, &["a@example.com"], TicketFetchScope::ActiveOnly);
        assert!(jql.contains(r#""Assigned Teams" = "Team \"A\"""#), "{jql}");
        assert!(jql.contains(r#"status in ("On \"Hold\"")"#), "{jql}");
        assert_eq!(
            epics_jql(r#"A"B"#),
            r#"project = "A\"B" AND issuetype = Epic ORDER BY key"#
        );
        assert_eq!(
            scoped_jql(r#"A"B"#, "type = Bug"),
            r#"project = "A\"B" AND (type = Bug) ORDER BY created DESC"#
        );
        assert_eq!(
            children_by_parent_jqls(r#"A"B"#, &epic_keys(1)),
            [r#"project = "A\"B" AND parent in (DEMO-1) ORDER BY key"#]
        );
        assert_eq!(
            children_by_epic_link_jql(r#"A"B"#),
            r#"project = "A\"B" AND "Epic Link" is not EMPTY ORDER BY key"#
        );
        assert_eq!(
            subtasks_jql(r#"A"B"#),
            r#"project = "A\"B" AND issuetype in subTaskIssueTypes() ORDER BY key"#
        );
    }
}
