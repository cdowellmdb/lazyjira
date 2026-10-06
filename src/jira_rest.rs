//! The Jira REST calls jira-cli can't make: listing a ticket's transitions with their fields,
//! sending one transition by id, and reading which sub-tasks sit under which parent.
//!
//! Uses jira-cli's `server`, `auth_type` and `login` settings and the `JIRA_API_TOKEN`
//! environment variable, so no extra setup is needed where jira-cli already works.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::cache::Ticket;
use crate::jira_issue::parse_search_page;
use crate::transitions::{parse_transitions, Transition};

/// Lists the transitions Jira offers for `key`, with their fields.
pub async fn get_transitions(key: &str) -> Result<Vec<Transition>> {
    shared()?.transitions(key).await
}

/// Sends transition `id` for `key`, with a resolution only when `resolution_id` is given.
pub async fn transition(key: &str, id: &str, resolution_id: Option<&str>) -> Result<()> {
    shared()?.transition(key, id, resolution_id).await
}

/// A sub-task as Jira's search returns it, with the ticket it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct Subtask {
    pub key: String,
    pub parent_key: String,
    pub summary: String,
    pub status: String,
    pub assignee: Option<String>,
    pub assignee_email: Option<String>,
    pub labels: Vec<String>,
}

/// The sub-tasks among `keys` and the sub-tasks under them, each with its parent.
pub async fn subtasks(keys: &[String]) -> Result<Vec<Subtask>> {
    shared()?.subtasks(keys).await
}

/// The tickets matching `jql` with `fields` filled in, following Jira's pages. The Epic Link
/// field from jira-cli's config is requested as well, so `epic_key` is read when the ticket has
/// one.
pub async fn search(jql: &str, fields: &[&str]) -> Result<Vec<Ticket>> {
    shared()?.search(jql, fields).await
}

/// The client for this session, built on first use. `JIRA_API_TOKEN` can't change while the app
/// runs, so a setup error (such as a missing token) is kept and reported on every call.
fn shared() -> Result<&'static JiraRest> {
    // Tests must never reach the real Jira, even on a machine where JIRA_API_TOKEN is set.
    if cfg!(test) {
        bail!("tests don't call Jira");
    }
    static CLIENT: OnceLock<std::result::Result<JiraRest, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| JiraRest::from_jira_cli().map_err(|e| format!("{:#}", e)))
        .as_ref()
        .map_err(|e| anyhow!("{}", e))
}

fn missing_token_error() -> anyhow::Error {
    anyhow!(
        "JIRA_API_TOKEN is not set. lazyjira reads tickets and sends moves through Jira's \
         REST API and needs the same token as jira-cli"
    )
}

/// The top-level jira-cli settings lazyjira needs. The rest of the file is ignored.
#[derive(Deserialize)]
struct JiraCliConfig {
    server: String,
    auth_type: Option<String>,
    login: Option<String>,
    #[serde(default)]
    epic: Option<EpicSettings>,
}

/// jira-cli's `epic` settings: `link` is the id of the Epic Link custom field.
#[derive(Deserialize)]
struct EpicSettings {
    link: Option<String>,
}

/// The id of the Epic Link custom field (`customfield_10857` on one instance, something else on
/// another), as jira-cli's config names it. Read once; `None` when the config doesn't say.
pub fn epic_link_field() -> Option<String> {
    static FIELD: OnceLock<Option<String>> = OnceLock::new();
    FIELD
        .get_or_init(|| {
            let yaml = std::fs::read_to_string(jira_cli_config_path().ok()?).ok()?;
            configured_epic_link(&yaml)
        })
        .clone()
}

fn configured_epic_link(yaml: &str) -> Option<String> {
    let config: JiraCliConfig = serde_yaml::from_str(yaml).ok()?;
    config.epic?.link.filter(|link| !link.trim().is_empty())
}

/// Browser links use the same Jira instance as jira-cli, without needing a REST token.
pub fn server_url() -> Result<String> {
    let path = jira_cli_config_path()?;
    let yaml = std::fs::read_to_string(&path)
        .with_context(|| format!("Couldn't read jira-cli's config {}", path.display()))?;
    configured_server(&yaml)
}

fn configured_server(yaml: &str) -> Result<String> {
    let config: JiraCliConfig =
        serde_yaml::from_str(yaml).context("jira-cli's config has no usable `server` setting")?;
    let url = reqwest::Url::parse(config.server.trim()).context("Invalid Jira server URL")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("Jira server must be an HTTP or HTTPS URL");
    }
    Ok(config.server.trim().trim_end_matches('/').to_string())
}

/// No `Debug`, so the token can't end up in a log or error message.
enum Auth {
    Bearer(String),
    Basic { login: String, token: String },
}

struct JiraRest {
    http: reqwest::Client,
    /// Base URL without a trailing slash, e.g. `https://jira.example.com`.
    server: String,
    auth: Auth,
    /// See [`epic_link_field`].
    epic_link: Option<String>,
}

impl JiraRest {
    fn from_jira_cli() -> Result<Self> {
        let path = jira_cli_config_path()?;
        let yaml = std::fs::read_to_string(&path)
            .with_context(|| format!("Couldn't read jira-cli's config {}", path.display()))?;
        let token = std::env::var("JIRA_API_TOKEN")
            .ok()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(missing_token_error)?;
        Self::new(&yaml, token).with_context(|| format!("Using {}", path.display()))
    }

    fn new(config_yaml: &str, token: String) -> Result<Self> {
        let config: JiraCliConfig = serde_yaml::from_str(config_yaml)
            .context("jira-cli's config has no usable `server` setting")?;
        let auth = match config.auth_type.as_deref().unwrap_or_default() {
            "bearer" => Auth::Bearer(token),
            // jira-cli treats a missing auth_type as basic.
            "basic" | "" => Auth::Basic {
                login: config
                    .login
                    .filter(|login| !login.trim().is_empty())
                    .context("jira-cli's config has no `login`, which basic auth needs")?,
                token,
            },
            other => bail!(
                "jira-cli's auth_type \"{}\" isn't supported. lazyjira supports basic and bearer",
                other
            ),
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("lazyjira/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("Couldn't set up the HTTP client")?;
        Ok(Self {
            http,
            server: configured_server(config_yaml)?,
            auth,
            epic_link: configured_epic_link(config_yaml),
        })
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let request = self
            .http
            .request(method, format!("{}/rest/api/2/{}", self.server, path));
        match &self.auth {
            Auth::Bearer(token) => request.bearer_auth(token),
            Auth::Basic { login, token } => request.basic_auth(login, Some(token)),
        }
    }

    fn transitions_request(&self, method: Method, key: &str, query: &str) -> RequestBuilder {
        self.request(method, &format!("issue/{}/transitions{}", key, query))
    }

    async fn transitions(&self, key: &str) -> Result<Vec<Transition>> {
        let response = self
            .transitions_request(Method::GET, key, "?expand=transitions.fields")
            .send()
            .await
            .context("Couldn't reach Jira")?;
        parse_transitions(&successful_body(response).await?)
    }

    async fn transition(&self, key: &str, id: &str, resolution_id: Option<&str>) -> Result<()> {
        let response = self
            .transitions_request(Method::POST, key, "")
            .json(&transition_body(id, resolution_id))
            .send()
            .await
            .context("Couldn't reach Jira")?;
        successful_body(response).await?;
        Ok(())
    }

    async fn search(&self, jql: &str, fields: &[&str]) -> Result<Vec<Ticket>> {
        let fields = search_fields(fields, self.epic_link.as_deref());
        self.search_pages(jql, &fields, |body| {
            parse_search_page(body, self.epic_link.as_deref())
        })
        .await
    }

    /// Searches `KEYS_PER_SEARCH` keys at a time, following Jira's pages.
    async fn subtasks(&self, keys: &[String]) -> Result<Vec<Subtask>> {
        let mut found = Vec::new();
        for chunk in keys.chunks(KEYS_PER_SEARCH) {
            if let Some(jql) = subtasks_jql(chunk) {
                found.extend(
                    self.search_pages(&jql, &SUBTASK_FIELDS, parse_subtasks)
                        .await?,
                );
            }
        }
        Ok(found)
    }

    /// Everything `jql` matches, a page at a time. `parse` reads one page's body into its items
    /// and the total number of matches.
    async fn search_pages<T>(
        &self,
        jql: &str,
        fields: &[&str],
        parse: impl Fn(&str) -> Result<(Vec<T>, usize)>,
    ) -> Result<Vec<T>> {
        let mut found = Vec::new();
        let mut start_at = 0;
        loop {
            let response = self
                .request(Method::POST, "search")
                .json(&search_body(jql, fields, start_at))
                .send()
                .await
                .context("Couldn't reach Jira")?;
            let (page, total) = parse(&successful_body(response).await?)?;
            let next = next_start(start_at, page.len(), total);
            found.extend(page);
            match next {
                Some(next) => start_at = next,
                None => return Ok(found),
            }
        }
    }
}

const KEYS_PER_SEARCH: usize = 50;
/// Jira allows up to 1000 a page, and a server that allows fewer is followed by what it sends
/// (`next_start`). Pages of 100 cost three round-trips for a team of 270 tickets, 500 only one.
const SEARCH_PAGE_SIZE: usize = 500;
const SUBTASK_FIELDS: [&str; 5] = ["summary", "status", "assignee", "labels", "parent"];

/// JQL for the sub-tasks among `keys` and under them. Keys that don't look like Jira keys are
/// left out so they can't break the query; `None` when none are left.
fn subtasks_jql(keys: &[String]) -> Option<String> {
    let keys: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|key| {
            !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .collect();
    if keys.is_empty() {
        return None;
    }
    let list = keys.join(",");
    Some(format!(
        "(key in ({list}) OR parent in ({list})) AND issuetype in subTaskIssueTypes()"
    ))
}

/// `fields` and, when the instance has one, the Epic Link custom field.
fn search_fields<'a>(fields: &[&'a str], epic_link: Option<&'a str>) -> Vec<&'a str> {
    fields.iter().copied().chain(epic_link).collect()
}

fn search_body(jql: &str, fields: &[&str], start_at: usize) -> Value {
    json!({
        "jql": jql,
        "startAt": start_at,
        "maxResults": SEARCH_PAGE_SIZE,
        "fields": fields,
    })
}

/// Where the next page starts after one of `page_len` items at `start_at`, `None` once all
/// `total` matches are read. Follows what the server sent, since it can cap a page below
/// `SEARCH_PAGE_SIZE`, and stops on an empty page rather than asking for it again.
fn next_start(start_at: usize, page_len: usize, total: usize) -> Option<usize> {
    let next = start_at + page_len;
    (page_len > 0 && next < total).then_some(next)
}

/// One page of search results: its sub-tasks (an issue with no parent is skipped) and the total
/// number of matches.
fn parse_subtasks(body: &str) -> Result<(Vec<Subtask>, usize)> {
    let json: Value = serde_json::from_str(body).context("Jira's search answer isn't JSON")?;
    let text = |value: &Value| value.as_str().map(str::to_string);
    let subtasks = json["issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|issue| {
            let fields = &issue["fields"];
            Some(Subtask {
                key: text(&issue["key"])?,
                parent_key: text(&fields["parent"]["key"])?,
                summary: text(&fields["summary"]).unwrap_or_default(),
                status: text(&fields["status"]["name"]).unwrap_or_default(),
                assignee: text(&fields["assignee"]["displayName"]),
                assignee_email: text(&fields["assignee"]["emailAddress"]),
                labels: fields["labels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(text)
                    .collect(),
            })
        })
        .collect();
    let total = json["total"].as_u64().unwrap_or(0) as usize;
    Ok((subtasks, total))
}

/// `$JIRA_CONFIG_FILE`, else `~/.config/.jira/.config.yml`, as jira-cli does.
fn jira_cli_config_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("JIRA_CONFIG_FILE").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/.jira/.config.yml"))
}

fn transition_body(id: &str, resolution_id: Option<&str>) -> Value {
    let mut body = json!({ "transition": { "id": id } });
    if let Some(resolution_id) = resolution_id {
        body["fields"] = json!({ "resolution": { "id": resolution_id } });
    }
    body
}

async fn successful_body(response: Response) -> Result<String> {
    let status = response.status();
    let body = response
        .text()
        .await
        .context("Couldn't read Jira's answer")?;
    if !status.is_success() {
        bail!(error_text(status, &body));
    }
    Ok(body)
}

/// Readable text for a failed call: the HTTP status, then Jira's `errorMessages` and each entry
/// of its `errors` map (field: message). A body that isn't JSON, like an HTML error page, is left out.
fn error_text(status: StatusCode, body: &str) -> String {
    let mut lines = vec![format!("Jira answered {}.", status)];
    if let Ok(json) = serde_json::from_str::<Value>(body) {
        let messages = json["errorMessages"].as_array().into_iter().flatten();
        lines.extend(messages.filter_map(Value::as_str).map(str::to_string));
        let errors = json["errors"].as_object().into_iter().flatten();
        lines.extend(
            errors.filter_map(|(field, message)| Some(format!("{}: {}", field, message.as_str()?))),
        );
    }
    if status == StatusCode::UNAUTHORIZED {
        lines.push("Check JIRA_API_TOKEN and the auth_type in jira-cli's config.".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "---\nauth_type: bearer\nboard:\n  id: 1\n  name: Board\n\
                          epic:\n  name: customfield_1\ninstallation: Local\n\
                          login: someone@example.com\nproject:\n  key: DEMO\n\
                          server: https://jira.example.com/\ntimezone: UTC\n";

    #[test]
    fn browser_server_keeps_context_paths_and_needs_no_auth_settings() {
        assert_eq!(
            configured_server("server: https://jira.example.com/jira/\n").unwrap(),
            "https://jira.example.com/jira"
        );
        assert!(configured_server("server: file:///tmp/jira").is_err());
        assert!(configured_server("server: not-a-url").is_err());
    }

    #[test]
    fn the_epic_link_field_comes_from_jira_clis_epic_settings() {
        let with_link = CONFIG.replace(
            "epic:\n  name: customfield_1\n",
            "epic:\n  name: customfield_10858\n  link: customfield_10857\n",
        );
        assert_eq!(
            configured_epic_link(&with_link).as_deref(),
            Some("customfield_10857")
        );
        // An instance without the field (jira-cli doesn't write one), or no config, is no link.
        assert_eq!(configured_epic_link(CONFIG), None);
        assert_eq!(
            configured_epic_link("server: https://jira.example.com\n"),
            None
        );
        assert_eq!(configured_epic_link("not: [yaml"), None);
    }

    #[test]
    fn the_client_keeps_the_epic_link_field_for_its_searches() {
        let with_link = CONFIG.replace(
            "epic:\n  name: customfield_1\n",
            "epic:\n  name: customfield_10858\n  link: customfield_10857\n",
        );
        let client = JiraRest::new(&with_link, "t0ken".to_string()).unwrap();
        assert_eq!(client.epic_link.as_deref(), Some("customfield_10857"));
        assert_eq!(
            search_fields(&["summary", "status"], client.epic_link.as_deref()),
            ["summary", "status", "customfield_10857"]
        );
        assert_eq!(search_fields(&["summary"], None), ["summary"]);
    }

    #[test]
    fn a_search_asks_for_one_page_of_the_fields_at_the_offset() {
        assert_eq!(
            search_body("project = DEMO", &["summary", "labels"], 200),
            json!({
                "jql": "project = DEMO",
                "startAt": 200,
                "maxResults": 500,
                "fields": ["summary", "labels"],
            })
        );
    }

    #[test]
    fn a_search_follows_pages_until_the_total_is_read() {
        // Exactly one full page: no second request.
        assert_eq!(next_start(0, 500, 500), None);
        // One more match than a page holds: the second page starts at the 501st.
        assert_eq!(next_start(0, 500, 501), Some(500));
        assert_eq!(next_start(500, 1, 501), None);
        // A server that caps pages below ours is followed by what it actually sent.
        assert_eq!(next_start(0, 50, 120), Some(50));
        assert_eq!(next_start(50, 50, 120), Some(100));
        assert_eq!(next_start(100, 20, 120), None);
        // An empty page ends the search even if the total says more, rather than looping.
        assert_eq!(next_start(40, 0, 120), None);
    }

    #[test]
    fn a_missing_token_says_what_it_is_needed_for() {
        // Setup errors are kept for the session, so they must name every read that needs the token.
        let error = format!("{:#}", missing_token_error());
        assert!(error.starts_with("JIRA_API_TOKEN is not set"));
        assert!(error.contains("reads tickets"));
    }

    #[test]
    fn reads_server_and_bearer_auth_from_jira_cli_config() {
        let client = JiraRest::new(CONFIG, "t0ken".to_string()).unwrap();
        assert_eq!(client.server, "https://jira.example.com");
        assert!(matches!(client.auth, Auth::Bearer(ref token) if token == "t0ken"));
    }

    #[test]
    fn basic_auth_uses_login_and_is_the_default() {
        for config in [
            CONFIG.replace("auth_type: bearer", "auth_type: basic"),
            CONFIG.replace("auth_type: bearer\n", ""),
        ] {
            let client = JiraRest::new(&config, "t0ken".to_string()).unwrap();
            assert!(matches!(
                client.auth,
                Auth::Basic { ref login, ref token }
                    if login == "someone@example.com" && token == "t0ken"
            ));
        }
    }

    #[test]
    fn unusable_config_fails_with_a_clear_message() {
        let error = |config: &str| {
            format!(
                "{:#}",
                JiraRest::new(config, "t0ken".to_string()).err().unwrap()
            )
        };
        assert!(
            error(&CONFIG.replace("server: https://jira.example.com/\n", ""))
                .contains("no usable `server`")
        );
        assert!(error(
            &CONFIG
                .replace("auth_type: bearer", "auth_type: basic")
                .replace("login: someone@example.com\n", "")
        )
        .contains("no `login`"));
        assert!(error(&CONFIG.replace("bearer", "mtls")).contains("\"mtls\" isn't supported"));
    }

    #[test]
    fn transition_body_adds_resolution_only_when_given() {
        assert_eq!(
            transition_body("824", None),
            json!({ "transition": { "id": "824" } })
        );
        assert_eq!(
            transition_body("805", Some("101")),
            json!({ "transition": { "id": "805" }, "fields": { "resolution": { "id": "101" } } })
        );
    }

    #[test]
    fn subtask_search_covers_sub_tasks_among_and_under_the_keys() {
        let keys = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();
        assert_eq!(
            subtasks_jql(&keys(&["DSCI-1", "DSCI-2"])).unwrap(),
            "(key in (DSCI-1,DSCI-2) OR parent in (DSCI-1,DSCI-2)) \
             AND issuetype in subTaskIssueTypes()"
        );
        // Anything that isn't shaped like a key is dropped rather than put in the query.
        assert_eq!(
            subtasks_jql(&keys(&["DSCI-1", "x\") OR 1=1", ""])).unwrap(),
            "(key in (DSCI-1) OR parent in (DSCI-1)) AND issuetype in subTaskIssueTypes()"
        );
        assert_eq!(subtasks_jql(&keys(&["no good"])), None);
        assert_eq!(subtasks_jql(&[]), None);
    }

    #[test]
    fn parses_sub_tasks_with_their_parent_and_skips_issues_without_one() {
        let body = r#"{"total": 3, "issues": [
            {"key": "DSCI-3265", "fields": {"summary": "Run AX", "status": {"name": "On Deck"},
              "assignee": {"displayName": "Alex", "emailAddress": "alex@example.com"},
              "labels": ["mage"], "parent": {"key": "DSCI-3244"}}},
            {"key": "DSCI-3266", "fields": {"summary": "Pins", "status": {"name": "Closed"},
              "assignee": null, "labels": [], "parent": {"key": "DSCI-3244"}}},
            {"key": "DSCI-1", "fields": {"summary": "Not a sub-task", "parent": null}}]}"#;
        let (subtasks, total) = parse_subtasks(body).unwrap();
        assert_eq!(total, 3);
        assert_eq!(
            subtasks,
            vec![
                Subtask {
                    key: "DSCI-3265".into(),
                    parent_key: "DSCI-3244".into(),
                    summary: "Run AX".into(),
                    status: "On Deck".into(),
                    assignee: Some("Alex".into()),
                    assignee_email: Some("alex@example.com".into()),
                    labels: vec!["mage".into()],
                },
                Subtask {
                    key: "DSCI-3266".into(),
                    parent_key: "DSCI-3244".into(),
                    summary: "Pins".into(),
                    status: "Closed".into(),
                    assignee: None,
                    assignee_email: None,
                    labels: vec![],
                },
            ]
        );
        assert!(parse_subtasks("<html>").is_err());
    }

    #[test]
    fn error_text_lists_error_messages_and_field_errors() {
        let body = r#"{"errorMessages": ["It is not valid to transition this issue."],
                       "errors": {"resolution": "Resolution is required.", "assignee": "Unknown user."}}"#;
        assert_eq!(
            error_text(StatusCode::BAD_REQUEST, body),
            "Jira answered 400 Bad Request.\n\
             It is not valid to transition this issue.\n\
             assignee: Unknown user.\n\
             resolution: Resolution is required."
        );
    }

    #[test]
    fn error_text_skips_html_and_hints_at_auth() {
        assert_eq!(
            error_text(
                StatusCode::UNAUTHORIZED,
                "<html><body>Unauthorized</body></html>"
            ),
            "Jira answered 401 Unauthorized.\n\
             Check JIRA_API_TOKEN and the auth_type in jira-cli's config."
        );
        assert_eq!(
            error_text(
                StatusCode::NOT_FOUND,
                r#"{"errorMessages": [], "errors": {}}"#
            ),
            "Jira answered 404 Not Found."
        );
    }
}
