//! The files lazyjira keeps between runs, per project under `~/.cache/lazyjira/`: the full
//! snapshot, the epics, ticket details and the current user's email. `DetailCache` is the
//! in-memory copy of the details that refreshes hydrate from.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
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

/// Where every cache lives: `~/.cache/lazyjira/`, or the temp dir without a `HOME`.
fn cache_dir_in(home: Option<std::ffi::OsString>) -> PathBuf {
    match home {
        Some(home) => PathBuf::from(home).join(".cache").join(CACHE_DIR_NAME),
        None => std::env::temp_dir().join(CACHE_DIR_NAME),
    }
}

fn cache_dir() -> PathBuf {
    if cfg!(test) {
        // Tests keep off the real ~/.cache, and off the files of another test run sharing this
        // temp dir.
        return std::env::temp_dir().join(format!("lazyjira-test-{}", std::process::id()));
    }
    cache_dir_in(std::env::var_os("HOME"))
}

/// The file of cache `prefix` for `project` in `dir`.
fn cache_file(dir: &Path, prefix: &str, project: &str) -> PathBuf {
    dir.join(format!("{prefix}_{project}.json"))
}

fn cache_path(prefix: &str, project: &str) -> PathBuf {
    cache_file(&cache_dir(), prefix, project)
}

fn read_cache_file<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache_file(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let json = serde_json::to_string(value).context("Failed to serialize cache")?;
    write_cache_text(path, &json)
}

/// Writes a cache file that only this user can read: descriptions and comments are in it. The
/// directory is made private too, and a file an earlier build made readable is tightened.
fn write_cache_text(path: &Path, json: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("Failed to create cache directory: {}", dir.display()))?;
    }
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(json.as_bytes())
    };
    write().with_context(|| format!("Failed to write cache file: {}", path.display()))
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

/// The remembered email, normalized as it is read: a file written by an earlier build or edited
/// by hand may spell it differently, and everything after compares emails exactly.
pub fn load_my_email(project: &str) -> Option<String> {
    let email: String = read_cache_file(&cache_path(MY_EMAIL_PREFIX, project))?;
    Some(crate::cache::normalize_email(&email))
}

/// Remembers `email` for later refreshes. Not being able to write the file only costs the next
/// refresh a `jira me`, so it neither fails this refresh nor prints (the screen is the app's).
pub fn remember_my_email(project: &str, email: &str) {
    let _ = write_cache_file(&cache_path(MY_EMAIL_PREFIX, project), &email);
}

/// Fills in what only a ticket's detail has: description, reporter and activity, and whether
/// the detail was loaded, as it was stored. Everything the list search returns (labels,
/// assignee, epic, parent) stays as that search read it, since the cache is never invalidated
/// and would bring back what has since changed in Jira.
fn hydrate_ticket_from_details_cache(
    ticket: &mut Ticket,
    details_by_key: &HashMap<String, Ticket>,
) {
    let Some(detail) = details_by_key.get(&ticket.key) else {
        return;
    };

    ticket.detail_loaded = detail.detail_loaded;
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
    /// The file the writer saves to, for `close`; none when nothing is saved.
    path: Option<PathBuf>,
}

/// How long details must stop changing before the writer saves them.
const FLUSH_AFTER_IDLE: Duration = Duration::from_millis(750);

impl DetailCache {
    /// Read the details cache file once, and start the task that writes it back.
    pub fn spawn(project: &str) -> Self {
        let path = cache_path(DETAILS_CACHE_PREFIX, project);
        let (details, changed) = Self::new(load_details_cache(project), Some(path.clone()));
        tokio::spawn(write_details(
            path,
            details.by_key.clone(),
            changed,
            FLUSH_AFTER_IDLE,
        ));
        details
    }

    /// A cache with nothing in it and no writer, so what's recorded stays in memory only.
    pub fn in_memory() -> Self {
        Self::new(HashMap::new(), None).0
    }

    fn new(
        by_key: HashMap<String, Ticket>,
        path: Option<PathBuf>,
    ) -> (Self, mpsc::UnboundedReceiver<()>) {
        let (changed, rx) = mpsc::unbounded_channel();
        let details = DetailCache {
            by_key: Arc::new(Mutex::new(by_key)),
            changed,
            path,
        };
        (details, rx)
    }

    /// Keep a freshly fetched detail. False when the writer has stopped, so it won't reach disk.
    pub fn record(&self, detail: Ticket) -> bool {
        lock_details(&self.by_key).insert(detail.key.clone(), detail);
        self.changed.send(()).is_ok()
    }

    /// Saves the details, for when the app quits: details recorded in the last moments are still
    /// inside the writer's quiet period then. It writes the file itself rather than asking the
    /// writer, so a refresh still holding the writer's channel open, or a stalled writer, can't
    /// hold up quitting. If the writer happens to be saving at that moment too, the file may
    /// come out torn; a file that doesn't parse is ignored and refetched, as any cache is.
    pub fn close(&self) {
        if let Some(path) = &self.path {
            save_details(path, &self.by_key);
        }
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

/// Saves the details to `path`. A failure costs only a refetch next run, and the screen is the
/// app's, so it isn't printed.
fn save_details(path: &Path, by_key: &Mutex<HashMap<String, Ticket>>) {
    // The guard lives for this statement only: the disk write below never holds the lock that
    // recording and hydrating, which the UI waits on, take.
    let Ok(json) = serde_json::to_string(&*lock_details(by_key)) else {
        return;
    };
    let _ = write_cache_text(path, &json);
}

/// Saves the details once they've stayed unchanged for `idle`, and once more when the channel
/// closes.
async fn write_details(
    path: PathBuf,
    by_key: Arc<Mutex<HashMap<String, Ticket>>>,
    mut changed: mpsc::UnboundedReceiver<()>,
    idle: Duration,
) {
    while changed.recv().await.is_some() {
        while let Ok(Some(())) = timeout(idle, changed.recv()).await {}
        save_details(&path, &by_key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ticket(key: &str, status: &str) -> Ticket {
        Ticket::for_test(key, status)
    }

    /// How many `Remove`s are alive. The test cache directory goes with the last one, and the
    /// lock makes a test starting up wait out a removal, so it never loses the directory it is
    /// about to write into.
    static LIVE: Mutex<usize> = Mutex::new(0);

    /// Removes the cache files a test wrote, even when it fails, and the test cache directory
    /// once no test is using it. Made before the test writes anything.
    struct Remove(Vec<PathBuf>);

    impl Remove {
        fn new(paths: Vec<PathBuf>) -> Self {
            *LIVE.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            Remove(paths)
        }
    }

    impl Drop for Remove {
        fn drop(&mut self) {
            for path in &self.0 {
                let _ = std::fs::remove_file(path);
            }
            let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
            *live -= 1;
            if *live == 0 {
                let _ = std::fs::remove_dir(cache_dir());
            }
        }
    }

    /// A project name no other test or test run shares.
    fn project(name: &str) -> String {
        format!("{name}{}", std::process::id())
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
        stale.detail_loaded = true;
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
    fn a_cached_detail_counts_as_loaded_as_it_was_stored_whether_or_not_it_has_a_reporter() {
        // Jira has no reporter for a deleted user, and such a ticket must not be fetched again
        // on every refresh; an entry stored before its detail was read isn't loaded either.
        let mut no_reporter = test_ticket("AMP-1", "To Do");
        no_reporter.detail_loaded = true;
        let mut not_loaded = test_ticket("AMP-2", "To Do");
        not_loaded.reporter = Some("Pat".into());
        let cached = HashMap::from([
            ("AMP-1".to_string(), no_reporter),
            ("AMP-2".to_string(), not_loaded),
        ]);

        let mut one = test_ticket("AMP-1", "To Do");
        hydrate_ticket_from_details_cache(&mut one, &cached);
        let mut two = test_ticket("AMP-2", "To Do");
        hydrate_ticket_from_details_cache(&mut two, &cached);

        assert!(one.detail_loaded);
        assert!(!two.detail_loaded);
    }

    #[test]
    fn my_email_is_remembered_per_project() {
        let (a, b) = (project("EMAIL_A"), project("EMAIL_B"));
        let _remove = Remove::new(vec![cache_path(MY_EMAIL_PREFIX, &a)]);
        assert_eq!(load_my_email(&a), None);
        remember_my_email(&a, "me@example.com");

        assert_eq!(load_my_email(&a).as_deref(), Some("me@example.com"));
        assert_eq!(load_my_email(&b), None);
    }

    #[test]
    fn a_remembered_email_is_read_in_the_form_the_app_compares() {
        // A file from before emails were normalized, or edited by hand.
        let project = project("EMAIL_CASE");
        let path = cache_path(MY_EMAIL_PREFIX, &project);
        let _remove = Remove::new(vec![path.clone()]);
        write_cache_file(&path, &" Me@Example.COM ").unwrap();

        assert_eq!(load_my_email(&project).as_deref(), Some("me@example.com"));
    }

    #[test]
    fn an_email_that_cannot_be_saved_is_only_not_remembered() {
        // A file stands where the cache directory for this project would be made.
        let project = project("BLOCKED");
        let blocker = cache_path(MY_EMAIL_PREFIX, &format!("{project}-dir"));
        let _remove = Remove::new(vec![blocker.clone()]);
        std::fs::create_dir_all(blocker.parent().unwrap()).unwrap();
        std::fs::write(&blocker, "in the way").unwrap();
        let unwritable = format!("{project}-dir.json/x");

        remember_my_email(&unwritable, "me@example.com");

        assert_eq!(load_my_email(&unwritable), None);
    }

    #[test]
    fn caches_live_in_the_lazyjira_dir_under_home_per_project() {
        let dir = cache_dir_in(Some("/home/me".into()));
        assert_eq!(dir, PathBuf::from("/home/me/.cache/lazyjira"));
        // Without a HOME they fall back to the temp dir, still under lazyjira.
        assert_eq!(
            cache_dir_in(None),
            std::env::temp_dir().join("lazyjira").as_path()
        );

        assert_eq!(
            cache_file(&dir, EPICS_CACHE_PREFIX, "AMP"),
            PathBuf::from("/home/me/.cache/lazyjira/lazyjira_epics_cache_AMP.json")
        );
        assert_eq!(
            cache_file(&dir, DETAILS_CACHE_PREFIX, "AMP"),
            PathBuf::from("/home/me/.cache/lazyjira/lazyjira_ticket_details_cache_AMP.json")
        );
        assert_eq!(
            cache_file(&dir, MY_EMAIL_PREFIX, "DEMO"),
            PathBuf::from("/home/me/.cache/lazyjira/lazyjira_my_email_DEMO.json")
        );
    }

    #[test]
    fn an_old_temp_dir_epics_cache_is_ignored() {
        let project = project("OLDTMP");
        let epics = vec![Epic {
            key: "OLDTMP-1".into(),
            summary: "Old".into(),
            children: vec![],
        }];
        // Where the epics cache lived before it moved: straight in the system temp dir.
        let old = std::env::temp_dir().join(format!("lazyjira_epics_cache_{project}.json"));
        let _remove = Remove::new(vec![old.clone(), cache_path(EPICS_CACHE_PREFIX, &project)]);
        std::fs::write(&old, serde_json::to_string(&epics).unwrap()).unwrap();

        assert!(load_epics_cache(&project).is_empty());

        save_epics_cache(&project, &epics).unwrap();
        assert_eq!(load_epics_cache(&project)[0].key, "OLDTMP-1");
    }

    #[test]
    fn a_refresh_hydrates_from_details_recorded_in_memory() {
        let details = DetailCache::in_memory();
        let mut detail = test_ticket("DEMO-1", "In Progress");
        detail.description = Some("Body".into());
        detail.reporter = Some("Sam Doe".into());
        detail.detail_loaded = true;
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

    fn saved_keys(path: &Path) -> Vec<String> {
        let mut keys: Vec<String> = read_cache_file::<HashMap<String, Ticket>>(path)
            .unwrap_or_default()
            .into_keys()
            .collect();
        keys.sort();
        keys
    }

    #[tokio::test(start_paused = true)]
    async fn details_are_saved_once_none_has_changed_for_the_quiet_period() {
        let path = cache_path(DETAILS_CACHE_PREFIX, &project("QUIET"));
        let _remove = Remove::new(vec![path.clone()]);
        let (details, changed) = DetailCache::new(HashMap::new(), None);
        let writer = tokio::spawn(write_details(
            path.clone(),
            details.by_key.clone(),
            changed,
            FLUSH_AFTER_IDLE,
        ));
        let ms = Duration::from_millis;
        assert_eq!(FLUSH_AFTER_IDLE, ms(750));

        // Details keep arriving 1 ms inside the quiet period, for four times the period in all:
        // each one starts it over, so nothing is saved.
        for n in 1..=4 {
            details.record(test_ticket(&format!("DEMO-{n}"), "To Do"));
            tokio::time::sleep(FLUSH_AFTER_IDLE - ms(1)).await;
            assert!(saved_keys(&path).is_empty(), "saved after {n} details");
        }
        // 750 ms after the last one it is saved, with all four.
        tokio::time::sleep(ms(2)).await;
        assert_eq!(saved_keys(&path), ["DEMO-1", "DEMO-2", "DEMO-3", "DEMO-4"]);
        writer.abort();
    }

    #[tokio::test]
    async fn details_are_saved_when_the_channel_closes_even_before_they_go_quiet() {
        let path = cache_path(DETAILS_CACHE_PREFIX, &project("CLOSING"));
        let _remove = Remove::new(vec![path.clone()]);
        let (details, changed) = DetailCache::new(HashMap::new(), None);
        // A quiet period far longer than the test: only the channel closing can trigger this save.
        let writer = tokio::spawn(write_details(
            path.clone(),
            details.by_key.clone(),
            changed,
            Duration::from_secs(3600),
        ));

        details.record(test_ticket("DEMO-1", "To Do"));
        drop(details);
        tokio::time::timeout(Duration::from_secs(5), writer)
            .await
            .expect("the writer stops when the channel closes")
            .unwrap();

        assert_eq!(saved_keys(&path), ["DEMO-1"]);
    }

    #[test]
    fn closing_saves_what_is_recorded_without_the_writer() {
        let path = cache_path(DETAILS_CACHE_PREFIX, &project("CLOSE"));
        let _remove = Remove::new(vec![path.clone()]);
        // No writer is running, as when a refresh holds the channel open or the writer is stuck
        // behind a slow disk: the caller does the save itself.
        let (details, _changed) = DetailCache::new(HashMap::new(), Some(path.clone()));

        details.record(test_ticket("DEMO-1", "To Do"));
        details.close();

        assert_eq!(saved_keys(&path), ["DEMO-1"]);
    }

    #[test]
    fn cache_files_and_their_directory_are_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;

        // A file an earlier build made readable by everyone is tightened when it's rewritten.
        let existing = cache_path(MY_EMAIL_PREFIX, &project("MODE"));
        let _remove = Remove::new(vec![existing.clone()]);
        std::fs::create_dir_all(existing.parent().unwrap()).unwrap();
        std::fs::write(&existing, "\"old\"").unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_cache_file(&existing, &"new").unwrap();
        assert_eq!(mode(&existing), 0o600);

        // A directory made for the caches is closed to others too.
        let dir = cache_dir().join(format!("private-{}", std::process::id()));
        let made = dir.join("x.json");
        write_cache_text(&made, "{}").unwrap();
        assert_eq!((mode(&dir), mode(&made)), (0o700, 0o600));
        std::fs::remove_file(&made).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
