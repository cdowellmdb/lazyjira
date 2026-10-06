//! The files lazyjira keeps between runs, per project under `~/.cache/lazyjira/`: the full
//! snapshot, the epics, ticket details and the current user's email. `DetailCache` is the
//! in-memory copy of the details that refreshes hydrate from.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::cache::{Cache, Epic, Ticket};

const CACHE_DIR_NAME: &str = "lazyjira";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CacheSnapshot {
    saved_at_unix_secs: u64,
    cache: Cache,
}

#[derive(Debug, Clone)]
pub struct StartupCacheSnapshot {
    pub cache: Cache,
    pub age_secs: u64,
}

const EPICS_CACHE_PREFIX: &str = "lazyjira_epics_cache";
const DETAILS_CACHE_PREFIX: &str = "lazyjira_ticket_details_cache";
const FULL_CACHE_PREFIX: &str = "lazyjira_full_cache";
const MY_EMAIL_PREFIX: &str = "lazyjira_my_email";

fn cache_file_name(prefix: &str, project: &str) -> String {
    format!("{prefix}_{project}.json")
}

/// Where every cache lives: `~/.cache/lazyjira/`, or the temp dir without a `HOME`.
fn cache_dir_in(home: Option<std::ffi::OsString>) -> PathBuf {
    match home {
        Some(home) => PathBuf::from(home).join(".cache").join(CACHE_DIR_NAME),
        None => std::env::temp_dir().join(CACHE_DIR_NAME),
    }
}

fn cache_dir() -> PathBuf {
    // Tests keep off the real ~/.cache.
    cache_dir_in(if cfg!(test) {
        None
    } else {
        std::env::var_os("HOME")
    })
}

fn cache_path(prefix: &str, project: &str) -> PathBuf {
    cache_dir().join(cache_file_name(prefix, project))
}

fn read_cache_file<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache_file(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create cache directory: {}", dir.display()))?;
    }
    let json = serde_json::to_string(value).context("Failed to serialize cache")?;
    std::fs::write(path, json)
        .with_context(|| format!("Failed to write cache file: {}", path.display()))
}

fn now_unix_secs() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}

pub fn load_startup_cache_snapshot(project: &str) -> Option<StartupCacheSnapshot> {
    let snapshot: CacheSnapshot = read_cache_file(&cache_path(FULL_CACHE_PREFIX, project))?;
    let age_secs = now_unix_secs().saturating_sub(snapshot.saved_at_unix_secs);
    Some(StartupCacheSnapshot {
        cache: snapshot.cache,
        age_secs,
    })
}

pub fn save_full_cache_snapshot(project: &str, cache: &Cache) -> Result<()> {
    let snapshot = CacheSnapshot {
        saved_at_unix_secs: now_unix_secs(),
        cache: cache.clone(),
    };
    write_cache_file(&cache_path(FULL_CACHE_PREFIX, project), &snapshot)
}

pub fn load_epics_cache(project: &str) -> Vec<Epic> {
    read_cache_file(&cache_path(EPICS_CACHE_PREFIX, project)).unwrap_or_default()
}

pub fn save_epics_cache(project: &str, epics: &[Epic]) -> Result<()> {
    write_cache_file(&cache_path(EPICS_CACHE_PREFIX, project), &epics)
}

fn load_details_cache(project: &str) -> HashMap<String, Ticket> {
    read_cache_file(&cache_path(DETAILS_CACHE_PREFIX, project)).unwrap_or_default()
}

pub fn load_my_email(project: &str) -> Option<String> {
    read_cache_file(&cache_path(MY_EMAIL_PREFIX, project))
}

pub fn save_my_email(project: &str, email: &str) -> Result<()> {
    write_cache_file(&cache_path(MY_EMAIL_PREFIX, project), &email)
}

/// Fills in what only a ticket's detail has: description, reporter and activity. Everything the
/// list search returns (labels, assignee, epic, parent) stays as that search read it, since
/// the cache is never invalidated and would bring back what has since changed in Jira.
fn hydrate_ticket_from_details_cache(
    ticket: &mut Ticket,
    details_by_key: &HashMap<String, Ticket>,
) {
    let Some(detail) = details_by_key.get(&ticket.key) else {
        return;
    };

    // Only mark fully loaded if the cached detail has reporter (added later).
    // Old cache entries missing reporter will be re-fetched once, then stay cached.
    ticket.detail_loaded = detail.reporter.is_some();
    ticket.description = detail.description.clone();
    if detail.reporter.is_some() {
        ticket.reporter = detail.reporter.clone();
    }
    if !detail.activity.is_empty() {
        ticket.activity = detail.activity.clone();
    }
}

/// Ticket details fetched so far. A refresh hydrates from this in-memory copy instead of
/// reading the details cache file, and a background task writes it back to that file.
#[derive(Clone)]
pub struct DetailCache {
    by_key: Arc<Mutex<HashMap<String, Ticket>>>,
    changed: mpsc::UnboundedSender<()>,
}

impl DetailCache {
    /// Read the details cache file once, and start the task that writes it back.
    pub fn spawn(project: &str) -> Self {
        let (details, changed) = Self::new(load_details_cache(project));
        tokio::spawn(write_details(
            cache_path(DETAILS_CACHE_PREFIX, project),
            details.by_key.clone(),
            changed,
        ));
        details
    }

    fn new(by_key: HashMap<String, Ticket>) -> (Self, mpsc::UnboundedReceiver<()>) {
        let (changed, rx) = mpsc::unbounded_channel();
        let details = DetailCache {
            by_key: Arc::new(Mutex::new(by_key)),
            changed,
        };
        (details, rx)
    }

    /// Keep a freshly fetched detail. False when the writer has stopped, so it won't reach disk.
    pub fn record(&self, mut detail: Ticket) -> bool {
        detail.detail_loaded = true;
        lock_details(&self.by_key).insert(detail.key.clone(), detail);
        self.changed.send(()).is_ok()
    }

    /// Fill a list read's tickets, including epic children, with the details already fetched.
    pub fn hydrate(&self, cache: &mut Cache) {
        let by_key = lock_details(&self.by_key);
        let epic_children = cache.epics.iter_mut().flat_map(|epic| &mut epic.children);
        for ticket in cache
            .my_tickets
            .iter_mut()
            .chain(&mut cache.team_tickets)
            .chain(epic_children)
        {
            hydrate_ticket_from_details_cache(ticket, &by_key);
        }
    }
}

fn lock_details(
    by_key: &Mutex<HashMap<String, Ticket>>,
) -> MutexGuard<'_, HashMap<String, Ticket>> {
    by_key.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Write the details once changes go quiet, and once more when the app closes the channel.
async fn write_details(
    path: PathBuf,
    by_key: Arc<Mutex<HashMap<String, Ticket>>>,
    mut changed: mpsc::UnboundedReceiver<()>,
) {
    let flush_after_idle = Duration::from_millis(750);
    while changed.recv().await.is_some() {
        while let Ok(Some(())) = timeout(flush_after_idle, changed.recv()).await {}
        if let Err(e) = write_cache_file(&path, &*lock_details(&by_key)) {
            eprintln!("Warning: failed to persist details cache: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ticket(key: &str, status: &str) -> Ticket {
        Ticket::for_test(key, status)
    }

    #[test]
    fn caches_from_before_real_status_names_are_refetched() {
        // Written before #19: `status` held the collapsed enum, and `jira_status` the name
        // only when known. Loading it fails, so the app fetches from Jira instead of
        // showing Resolved tickets as Closed.
        let json = r#"{"saved_at_unix_secs": 1, "cache": {"my_tickets": [
            {"key": "DEMO-1", "summary": "One", "status": "Closed", "jira_status": "Resolved",
             "assignee": null, "assignee_email": null, "description": null, "labels": [],
             "epic_key": null, "epic_name": null, "url": "https://jira.example.com/browse/DEMO-1"}
          ], "team_tickets": [], "epics": [], "team_members": []}}"#;
        assert!(serde_json::from_str::<CacheSnapshot>(json).is_err());
        let details = r#"{"DEMO-1": {"key": "DEMO-1", "summary": "One", "status": {"Other": "Backlog"},
            "assignee": null, "assignee_email": null, "description": null, "labels": [],
            "epic_key": null, "epic_name": null, "url": "https://jira.example.com/browse/DEMO-1"}}"#;
        assert!(serde_json::from_str::<HashMap<String, Ticket>>(details).is_err());

        let cache = Cache {
            my_tickets: vec![test_ticket("DEMO-1", "Resolved")],
            ..Cache::empty()
        };
        let snapshot = CacheSnapshot {
            saved_at_unix_secs: 1,
            cache,
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let loaded: CacheSnapshot = serde_json::from_str(&json).expect("new cache should load");
        assert_eq!(loaded.cache.my_tickets[0].status, "Resolved");
    }

    #[test]
    fn the_details_cache_fills_only_what_the_list_search_does_not_return() {
        let mut fresh = test_ticket("AMP-1", "To Do");
        fresh.assignee = Some("Alex R.".to_string());
        fresh.assignee_email = Some("alex.rivera@example.com".to_string());
        fresh.labels = vec!["new".into()];
        fresh.epic_key = Some("AMP-100".into());
        let mut stale = test_ticket("AMP-1", "Blocked");
        stale.assignee = Some("Old Owner".into());
        stale.assignee_email = Some("old@example.com".into());
        stale.labels = vec!["old".into()];
        stale.epic_key = Some("AMP-50".into());
        stale.parent_key = Some("AMP-7".into());
        stale.description = Some("Body".into());
        stale.reporter = Some("Pat".into());
        let cached = HashMap::from([("AMP-1".to_string(), stale)]);

        hydrate_ticket_from_details_cache(&mut fresh, &cached);

        assert_eq!(fresh.status, "To Do");
        assert_eq!(fresh.assignee.as_deref(), Some("Alex R."));
        assert_eq!(
            fresh.assignee_email.as_deref(),
            Some("alex.rivera@example.com")
        );
        assert_eq!(fresh.labels, ["new"]);
        assert_eq!(fresh.epic_key.as_deref(), Some("AMP-100"));
        // A re-parented sub-task must not get its old parent back from the cache.
        assert_eq!(fresh.parent_key, None);
        assert_eq!(fresh.description.as_deref(), Some("Body"));
        assert_eq!(fresh.reporter.as_deref(), Some("Pat"));
        assert!(fresh.detail_loaded);
    }

    #[test]
    fn my_email_is_remembered_per_project() {
        let _ = std::fs::remove_file(cache_path(MY_EMAIL_PREFIX, "EMAILTEST_A"));
        assert_eq!(load_my_email("EMAILTEST_A"), None);
        save_my_email("EMAILTEST_A", "me@example.com").unwrap();

        assert_eq!(
            load_my_email("EMAILTEST_A").as_deref(),
            Some("me@example.com")
        );
        assert_eq!(load_my_email("EMAILTEST_B"), None);
    }

    #[test]
    fn caches_live_in_the_lazyjira_dir_under_home_per_project() {
        let dir = cache_dir_in(Some("/home/me".into()));
        assert_eq!(dir, PathBuf::from("/home/me/.cache/lazyjira"));

        for prefix in [DETAILS_CACHE_PREFIX, EPICS_CACHE_PREFIX, MY_EMAIL_PREFIX] {
            assert_eq!(
                cache_path(prefix, "AMP").parent(),
                Some(cache_dir().as_path())
            );
            assert_ne!(cache_path(prefix, "AMP"), cache_path(prefix, "DEMO"));
        }
    }

    #[test]
    fn an_old_temp_dir_epics_cache_is_ignored() {
        let epics = vec![Epic {
            key: "OLDTMP-1".into(),
            summary: "Old".into(),
            children: vec![],
        }];
        let old = std::env::temp_dir().join("lazyjira_epics_cache_OLDTMP.json");
        std::fs::write(&old, serde_json::to_string(&epics).unwrap()).unwrap();
        let _ = std::fs::remove_file(cache_path(EPICS_CACHE_PREFIX, "OLDTMP"));

        assert!(load_epics_cache("OLDTMP").is_empty());

        save_epics_cache("OLDTMP", &epics).unwrap();
        assert_eq!(load_epics_cache("OLDTMP")[0].key, "OLDTMP-1");
        let _ = std::fs::remove_file(old);
    }

    #[test]
    fn a_refresh_hydrates_from_details_recorded_in_memory() {
        let (details, _changed) = DetailCache::new(HashMap::new());
        let mut detail = test_ticket("DEMO-1", "In Progress");
        detail.description = Some("Body".into());
        detail.reporter = Some("Sam Doe".into());
        details.record(detail);

        let mut cache = Cache {
            my_tickets: vec![test_ticket("DEMO-1", "To Do")],
            team_tickets: vec![test_ticket("DEMO-1", "To Do")],
            epics: vec![Epic {
                key: "DEMO-9".into(),
                summary: "Epic".into(),
                children: vec![test_ticket("DEMO-1", "To Do")],
            }],
            ..Cache::empty()
        };
        details.hydrate(&mut cache);

        for ticket in [
            &cache.my_tickets[0],
            &cache.team_tickets[0],
            &cache.epics[0].children[0],
        ] {
            assert_eq!(ticket.description.as_deref(), Some("Body"));
            assert!(ticket.detail_loaded);
            // The list read's status stands; a detail never overrides it here.
            assert_eq!(ticket.status, "To Do");
        }
    }
}
