//! The Jira REST calls behind moves, which jira-cli can't make: listing a ticket's transitions
//! with their fields, and sending one transition by id.
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

use crate::transitions::{parse_transitions, Transition};

/// Lists the transitions Jira offers for `key`, with their fields.
pub async fn get_transitions(key: &str) -> Result<Vec<Transition>> {
    shared()?.transitions(key).await
}

/// Sends transition `id` for `key`, with a resolution only when `resolution_id` is given.
pub async fn transition(key: &str, id: &str, resolution_id: Option<&str>) -> Result<()> {
    shared()?.transition(key, id, resolution_id).await
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

/// The top-level jira-cli settings lazyjira needs. The rest of the file is ignored.
#[derive(Deserialize)]
struct JiraCliConfig {
    server: String,
    auth_type: Option<String>,
    login: Option<String>,
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
            .context(
                "JIRA_API_TOKEN is not set. lazyjira sends moves through Jira's REST API \
                 and needs the same token as jira-cli",
            )?;
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
            server: config.server.trim_end_matches('/').to_string(),
            auth,
        })
    }

    fn transitions_request(&self, method: Method, key: &str, query: &str) -> RequestBuilder {
        let url = format!(
            "{}/rest/api/2/issue/{}/transitions{}",
            self.server, key, query
        );
        let request = self.http.request(method, url);
        match &self.auth {
            Auth::Bearer(token) => request.bearer_auth(token),
            Auth::Basic { login, token } => request.basic_auth(login, Some(token)),
        }
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
