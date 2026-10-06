//! The Jira REST calls lazyjira makes itself: the paginated search every list read and the
//! detail prefetch use, reading one issue, listing a ticket's transitions with their fields, and
//! sending one transition by id.
//!
//! Uses jira-cli's `server`, `auth_type`, `login` and `epic.link` settings and the
//! `JIRA_API_TOKEN` environment variable, so no extra setup is needed where jira-cli already
//! works. The search is `POST /rest/api/2/search`, which Jira Server and Data Center serve
//! (see ADR 0005).

use std::future::Future;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::cache::Ticket;
use crate::jira_issue::{parse_search_page, ticket_from_issue};
use crate::transitions::{parse_transitions, Transition};

/// The ticket's page in the Jira web UI.
pub fn browse_url(key: &str) -> Result<String> {
    Ok(format!("{}/browse/{}", server_url()?, key))
}

/// An error with its whole chain, as the user sees it: "Couldn't reach Jira: connection refused"
/// rather than just the outermost "Couldn't reach Jira".
pub fn describe(error: &anyhow::Error) -> String {
    format!("{:#}", error)
}

/// Lists the transitions Jira offers for `key`, with their fields.
pub async fn get_transitions(key: &str) -> Result<Vec<Transition>> {
    shared()?.transitions(key).await
}

/// Sends transition `id` for `key`, with a resolution only when `resolution_id` is given.
pub async fn transition(key: &str, id: &str, resolution_id: Option<&str>) -> Result<()> {
    shared()?.transition(key, id, resolution_id).await
}

/// The tickets matching `jql` with `fields` filled in, following Jira's pages. The Epic Link
/// field from jira-cli's config is requested as well, so `epic_key` is read when the ticket has
/// one.
pub async fn search(jql: &str, fields: &[&str]) -> Result<Vec<Ticket>> {
    shared()?.search(jql, fields).await
}

/// The ticket `key`, with `fields` and the Epic Link filled in as for `search`. Unlike a search,
/// which reads Jira's index and can lag behind a change made a moment ago (a move), this reads
/// the issue itself, so it's the read for a ticket the user just opened or moved.
pub async fn issue(key: &str, fields: &[&str]) -> Result<Ticket> {
    shared()?.issue(key, fields).await
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
        .get_or_init(|| JiraRest::from_jira_cli().map_err(|e| describe(&e)))
        .as_ref()
        .map_err(|e| anyhow!("{}", e))
}

/// The top-level jira-cli settings lazyjira needs. The rest of the file is ignored.
#[derive(Deserialize)]
struct JiraCliConfig {
    server: String,
    auth_type: Option<String>,
    login: Option<String>,
}

/// The id of the Epic Link custom field (`customfield_10857` on one instance, something else on
/// another), as jira-cli's config names it. Read once; `None` when the config doesn't say, as
/// for team-managed projects, whose epics are found through `parent` instead.
pub fn epic_link_field() -> Option<String> {
    static FIELD: OnceLock<Option<String>> = OnceLock::new();
    FIELD
        .get_or_init(|| {
            let yaml = std::fs::read_to_string(jira_cli_config_path().ok()?).ok()?;
            configured_epic_link(&yaml)
        })
        .clone()
}

/// `epic.link` from jira-cli's config. Read as loose YAML, so an odd `epic` section costs the
/// Epic Link field and nothing else.
fn configured_epic_link(yaml: &str) -> Option<String> {
    let config: serde_yaml::Value = serde_yaml::from_str(yaml).ok()?;
    let link = config["epic"]["link"].as_str()?.trim();
    (!link.is_empty()).then(|| link.to_string())
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
    validated_server(&config)
}

/// The config's `server` as a base URL without a trailing slash, if it is an HTTP(S) one.
fn validated_server(config: &JiraCliConfig) -> Result<String> {
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
}

impl JiraRest {
    fn from_jira_cli() -> Result<Self> {
        let path = jira_cli_config_path()?;
        let yaml = std::fs::read_to_string(&path)
            .with_context(|| format!("Couldn't read jira-cli's config {}", path.display()))?;
        let token = std::env::var("JIRA_API_TOKEN")
            .ok()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "JIRA_API_TOKEN is not set. lazyjira reads tickets and sends moves through \
                     Jira's REST API and needs the same token as jira-cli"
                )
            })?;
        Self::new(&yaml, token).with_context(|| format!("Using {}", path.display()))
    }

    fn new(config_yaml: &str, token: String) -> Result<Self> {
        let config: JiraCliConfig = serde_yaml::from_str(config_yaml)
            .context("jira-cli's config has no usable `server` setting")?;
        let server = validated_server(&config)?;
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
        Ok(Self { http, server, auth })
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

    async fn issue(&self, key: &str, fields: &[&str]) -> Result<Ticket> {
        let epic_link = epic_link_field();
        let fields = with_epic_link(fields, epic_link.as_deref());
        let response = self
            .request(Method::GET, &issue_path(key, &fields)?)
            .send()
            .await
            .context("Couldn't reach Jira")?;
        let json: Value = serde_json::from_str(&successful_body(response).await?)
            .context("Jira's answer isn't JSON")?;
        ticket_from_issue(&json, epic_link.as_deref())
            .with_context(|| format!("Jira's answer for {key} has no key"))
    }

    async fn search(&self, jql: &str, fields: &[&str]) -> Result<Vec<Ticket>> {
        let epic_link = epic_link_field();
        let fields = &with_epic_link(fields, epic_link.as_deref());
        read_pages(
            |start_at| async move {
                let response = self
                    .request(Method::POST, "search")
                    .json(&search_body(jql, fields, start_at))
                    .send()
                    .await
                    .context("Couldn't reach Jira")?;
                successful_body(response).await
            },
            epic_link.as_deref(),
        )
        .await
    }
}

/// `fields` and the Epic Link field, when jira-cli's config names one, so `epic_key` is read.
fn with_epic_link<'a>(fields: &[&'a str], epic_link: Option<&'a str>) -> Vec<&'a str> {
    fields.iter().copied().chain(epic_link).collect()
}

/// Everything a search matches, a page at a time. `fetch(start_at)` asks Jira for the page that
/// starts there and returns its body.
async fn read_pages<F, Fut>(mut fetch: F, epic_link: Option<&str>) -> Result<Vec<Ticket>>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let mut found = Vec::new();
    let mut start_at = 0;
    loop {
        let page = parse_search_page(&fetch(start_at).await?, epic_link)?;
        found.extend(page.tickets);
        // The next page starts after what the server sent, since it can cap a page below
        // `SEARCH_PAGE_SIZE`; an empty page ends the search rather than being asked for again.
        if page.sent == 0 || start_at + page.sent >= page.total {
            return Ok(found);
        }
        start_at += page.sent;
    }
}

/// What a page asks for. A server that allows fewer is followed by what it sends, see
/// `read_pages`; the instance this was measured on takes 500. Pages of 100 cost three
/// round-trips for a team of 270 tickets, 500 only one.
const SEARCH_PAGE_SIZE: usize = 500;

/// The path, under the API root, that reads `key` with just `fields`.
fn issue_path(key: &str, fields: &[&str]) -> Result<String> {
    if !crate::jql::is_key(key) {
        bail!("{key:?} isn't a ticket key");
    }
    Ok(format!("issue/{key}?fields={}", fields.join(",")))
}

fn search_body(jql: &str, fields: &[&str], start_at: usize) -> Value {
    json!({
        "jql": jql,
        "startAt": start_at,
        "maxResults": SEARCH_PAGE_SIZE,
        "fields": fields,
    })
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
    fn an_odd_epic_section_costs_the_epic_link_and_nothing_else() {
        let odd = CONFIG.replace("epic:\n  name: customfield_1\n", "epic: not-a-mapping\n");
        assert_eq!(configured_epic_link(&odd), None);
        // Moves and browser links still work.
        assert!(JiraRest::new(&odd, "t0ken".to_string()).is_ok());
        assert_eq!(configured_server(&odd).unwrap(), "https://jira.example.com");
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

    /// A Jira that answers each page `start_at` asks for with `answer(start_at)`: a body with
    /// `total` matches, `keys` of them here, the keys numbered from `start_at + 1`.
    fn page_body(start_at: usize, total: usize, keys: usize) -> String {
        let issues: Vec<_> = (1..=keys)
            .map(|n| json!({"key": format!("DEMO-{}", start_at + n), "fields": {}}))
            .collect();
        json!({"total": total, "issues": issues}).to_string()
    }

    /// Runs `read_pages` against `answer`, returning the offsets asked for and the keys found.
    async fn read(answer: impl Fn(usize) -> Result<String>) -> Result<(Vec<usize>, Vec<String>)> {
        let asked = std::cell::RefCell::new(Vec::new());
        let found = read_pages(
            |start_at| {
                asked.borrow_mut().push(start_at);
                let body = answer(start_at);
                async move { body }
            },
            None,
        )
        .await?;
        let keys = found.into_iter().map(|ticket| ticket.key).collect();
        Ok((asked.into_inner(), keys))
    }

    #[tokio::test]
    async fn a_search_asks_for_the_next_page_where_the_last_one_ended() {
        // Jira capping pages at 50 of 120 matches: three pages, in order, none repeated.
        let (asked, keys) = read(|start| Ok(page_body(start, 120, (120 - start).min(50))))
            .await
            .unwrap();
        assert_eq!(asked, [0, 50, 100]);
        assert_eq!(keys.len(), 120);
        assert_eq!(
            (keys[0].as_str(), keys[119].as_str()),
            ("DEMO-1", "DEMO-120")
        );

        // Exactly one full page of matches is one request, not a second empty one.
        let (asked, keys) = read(|start| Ok(page_body(start, 500, 500))).await.unwrap();
        assert_eq!((asked, keys.len()), (vec![0], 500));
        // One more match than a page holds: a second request for the 501st alone.
        let (asked, keys) =
            read(|start| Ok(page_body(start, 501, if start == 0 { 500 } else { 1 })))
                .await
                .unwrap();
        assert_eq!((asked, keys.len()), (vec![0, 500], 501));
    }

    #[tokio::test]
    async fn a_page_with_an_issue_that_has_no_key_still_moves_the_offset_by_what_jira_sent() {
        // Three matches; the first page's second issue has no key, so only two can be read.
        let first = json!({"total": 3, "issues": [
            {"key": "DEMO-1", "fields": {}}, {"fields": {}}]})
        .to_string();
        let (asked, keys) = read(|start| match start {
            0 => Ok(first.clone()),
            _ => Ok(page_body(start, 3, 1)),
        })
        .await
        .unwrap();
        assert_eq!(asked, [0, 2]);
        assert_eq!(keys, ["DEMO-1", "DEMO-3"]);
    }

    #[tokio::test]
    async fn an_empty_page_ends_the_search_and_a_missing_total_fails_it() {
        // The total promises more than Jira has: stop rather than ask for the same page forever.
        let (asked, keys) =
            read(|start| Ok(page_body(start, 120, if start == 0 { 50 } else { 0 })))
                .await
                .unwrap();
        assert_eq!((asked, keys.len()), (vec![0, 50], 50));

        // No total: an error, not the first page passed off as the whole list.
        let error = read(|_| Ok(r#"{"issues": []}"#.to_string()))
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("no total"), "{error:#}");
        // A failed request is the search's failure, whichever page it was.
        let error = read(|start| match start {
            0 => Ok(page_body(0, 120, 50)),
            _ => Err(anyhow!("Jira answered 503.")),
        })
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "Jira answered 503.");
    }

    #[test]
    fn one_issue_is_read_by_key_with_the_fields_asked_for() {
        assert_eq!(
            issue_path("DEMO-1", &["summary", "status", "customfield_10857"]).unwrap(),
            "issue/DEMO-1?fields=summary,status,customfield_10857"
        );
        // The key goes into a URL path, so anything but a key is refused.
        for key in ["DEMO-1/transitions", "../DEMO-1", "DEMO 1", ""] {
            assert!(issue_path(key, &["summary"]).is_err(), "{key:?}");
        }
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
