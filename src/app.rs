use crate::cache::Cache;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

const UNASSIGNED_TEAM_NAME: &str = "Unassigned";
const UNASSIGNED_TEAM_EMAIL: &str = "__unassigned__";
const NO_EPIC_KEY: &str = "NO-EPIC";
const NO_EPIC_SUMMARY: &str = "No Epic";

pub const ISSUE_TYPES: &[&str] = &["Task", "Bug", "Story"];

#[derive(Debug, Clone)]
pub struct CommentState {
    pub ticket_key: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct AssignState {
    pub ticket_key: String,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct EditFieldsState {
    pub ticket_key: String,
    pub focused_field: usize, // 0=summary, 1=labels
    pub summary: String,
    pub labels: String, // comma-separated
}

#[derive(Debug, Clone)]
pub struct CreateTicketState {
    pub focused_field: usize, // 0=type, 1=summary, 2=assignee, 3=epic
    pub issue_type_idx: usize,
    pub summary: String,
    pub assignee_idx: usize, // 0 = "None", then 1..N = team members
    pub epic_idx: usize,     // 0 = "None", then 1..N = cached epics
}

/// Which tab is currently active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    MyWork,
    Team,
    Epics,
    Unassigned,
    Filters,
}

impl Tab {
    pub fn next(self) -> Self {
        match self {
            Tab::MyWork => Tab::Team,
            Tab::Team => Tab::Epics,
            Tab::Epics => Tab::Unassigned,
            Tab::Unassigned => Tab::Filters,
            Tab::Filters => Tab::MyWork,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Tab::MyWork => "My Work",
            Tab::Team => "Team",
            Tab::Epics => "Epics",
            Tab::Unassigned => "Unassigned",
            Tab::Filters => "Filters",
        }
    }

    pub fn all() -> &'static [Tab] {
        &[Tab::MyWork, Tab::Team, Tab::Epics, Tab::Unassigned, Tab::Filters]
    }
}

/// Which pane is focused in the Filters tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterFocus {
    Sidebar,
    Results,
}

/// State for the filter create/edit modal.
#[derive(Debug, Clone)]
pub struct FilterEditState {
    pub focused_field: usize, // 0=name, 1=jql
    pub name: String,
    pub jql: String,
    /// None = creating new, Some(idx) = editing existing filter at index.
    pub editing_idx: Option<usize>,
}

/// What the detail overlay is showing.
#[derive(Debug, Clone)]
pub enum DetailMode {
    /// Showing ticket info.
    View,
    /// Showing the status move picker, with selected index and optional pending confirmation.
    MovePicker {
        selected: usize,
        confirm_target: Option<crate::cache::Status>,
    },
    /// Showing the activity/history timeline with scroll offset.
    History { scroll: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketSyncStage {
    ActiveOnly,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisibleKeysState {
    active_tab: Tab,
    search: Option<String>,
    show_done: bool,
    status_focus: Option<crate::cache::Status>,
    view_generation: u64,
}

#[derive(Default)]
struct VisibleKeysCache {
    state: Option<VisibleKeysState>,
    keys: Vec<String>,
}

/// Full application state.
pub struct App {
    pub cache: Cache,
    pub active_tab: Tab,
    /// Index of the selected item in the current tab's list.
    pub selected_index: usize,
    /// If Some, the detail overlay is open for this ticket key.
    pub detail_ticket_key: Option<String>,
    pub detail_mode: DetailMode,
    /// Vertical scroll offset for the ticket detail body.
    pub detail_scroll: u16,
    /// True while data is being fetched.
    pub loading: bool,
    /// Flash message (error or success), cleared on next keypress.
    pub flash: Option<String>,
    /// Search/filter string when `/` is active.
    pub search: Option<String>,
    /// Whether Done tickets are visible in My Work and Team tabs.
    pub show_done: bool,
    /// Optional focused active status filter for My Work and Team.
    pub status_focus: Option<crate::cache::Status>,
    /// True while full epic relationships are being refreshed in background.
    pub epics_refreshing: bool,
    /// Ticket sync stage for background cache refresh.
    pub ticket_sync_stage: Option<TicketSyncStage>,
    /// Age of the cache snapshot loaded at startup, in seconds.
    pub cache_stale_age_secs: Option<u64>,
    /// Whether the keybindings overlay is visible.
    pub show_keybindings: bool,
    /// Ticket keys currently being fetched for rich detail.
    detail_fetching: HashSet<String>,
    /// Monotonic generation used to invalidate derived visibility caches.
    view_generation: u64,
    /// Cached visible ticket keys for selection/counting in the active tab.
    visible_keys_cache: RefCell<VisibleKeysCache>,
    pub should_quit: bool,
    /// State for the create ticket modal overlay.
    pub create_ticket: Option<CreateTicketState>,
    /// State for the comment modal overlay.
    pub comment_state: Option<CommentState>,
    /// State for the assign/reassign modal overlay.
    pub assign_state: Option<AssignState>,
    /// State for the edit fields modal overlay.
    pub edit_state: Option<EditFieldsState>,
    /// Which pane is focused in the Filters tab.
    pub filter_focus: FilterFocus,
    /// Index of the selected filter in the sidebar.
    pub filter_sidebar_idx: usize,
    /// Results of the currently running/active filter.
    pub filter_results: Vec<crate::cache::Ticket>,
    /// Whether a filter query is currently loading.
    pub filter_loading: bool,
    /// State for filter create/edit modal.
    pub filter_edit: Option<FilterEditState>,
}

impl App {
    pub fn new() -> Self {
        Self {
            cache: Cache::empty(),
            active_tab: Tab::MyWork,
            selected_index: 0,
            detail_ticket_key: None,
            detail_mode: DetailMode::View,
            detail_scroll: 0,
            loading: true,
            flash: None,
            search: None,
            show_done: true,
            status_focus: None,
            epics_refreshing: false,
            ticket_sync_stage: None,
            cache_stale_age_secs: None,
            show_keybindings: false,
            detail_fetching: HashSet::new(),
            view_generation: 0,
            visible_keys_cache: RefCell::new(VisibleKeysCache::default()),
            should_quit: false,
            create_ticket: None,
            comment_state: None,
            assign_state: None,
            edit_state: None,
            filter_focus: FilterFocus::Sidebar,
            filter_sidebar_idx: 0,
            filter_results: Vec::new(),
            filter_loading: false,
            filter_edit: None,
        }
    }

    pub fn replace_cache(&mut self, cache: Cache) {
        self.cache = cache;
        self.mark_cache_changed();
    }

    pub fn mark_cache_changed(&mut self) {
        self.view_generation = self.view_generation.wrapping_add(1);
        let cache = self.visible_keys_cache.get_mut();
        cache.state = None;
        cache.keys.clear();
    }

    pub fn next_tab(&mut self) {
        self.active_tab = self.active_tab.next();
        self.selected_index = 0;
        self.clamp_selection();
    }

    pub fn open_detail(&mut self, key: String) {
        self.detail_ticket_key = Some(key);
        self.detail_mode = DetailMode::View;
        self.detail_scroll = 0;
    }

    pub fn close_detail(&mut self) {
        self.detail_ticket_key = None;
        self.detail_mode = DetailMode::View;
        self.detail_scroll = 0;
    }

    pub fn is_detail_open(&self) -> bool {
        self.detail_ticket_key.is_some()
    }

    pub fn is_ticket_detail_loaded(&self, key: &str) -> bool {
        self.find_ticket(key)
            .map(|t| t.detail_loaded)
            .unwrap_or(false)
    }

    /// Team members sorted by active ticket count (most active first).
    /// Must match the order used in views/team.rs.
    pub fn sorted_team_members(&self) -> Vec<&crate::cache::TeamMember> {
        let mut active_counts_by_email: HashMap<&str, usize> = HashMap::new();
        for ticket in &self.cache.team_tickets {
            if ticket.status == crate::cache::Status::Done {
                continue;
            }
            if let Some(email) = ticket.assignee_email.as_deref() {
                *active_counts_by_email.entry(email).or_insert(0) += 1;
            }
        }

        let mut members: Vec<_> = self.cache.team_members.iter().collect();
        members.sort_by(|a, b| {
            let ac = active_counts_by_email
                .get(a.email.as_str())
                .copied()
                .unwrap_or(0);
            let bc = active_counts_by_email
                .get(b.email.as_str())
                .copied()
                .unwrap_or(0);
            bc.cmp(&ac)
        });
        members
    }

    fn visible_keys_state(&self) -> VisibleKeysState {
        VisibleKeysState {
            active_tab: self.active_tab,
            search: self.search.clone().filter(|s| !s.is_empty()),
            show_done: self.show_done,
            status_focus: self.status_focus.clone(),
            view_generation: self.view_generation,
        }
    }

    fn compute_visible_ticket_keys_for_tab(&self, tab: Tab) -> Vec<String> {
        match tab {
            Tab::MyWork => self.my_work_visible_ticket_keys(),
            Tab::Team => self.team_visible_ticket_keys(),
            Tab::Epics => self.epics_visible_ticket_keys(),
            Tab::Unassigned => self.unassigned_visible_ticket_keys(),
            Tab::Filters => self.filter_results.iter().map(|t| t.key.clone()).collect(),
        }
    }

    fn ensure_visible_keys_cache(&self) {
        let state = self.visible_keys_state();
        {
            let cache = self.visible_keys_cache.borrow();
            if cache.state.as_ref() == Some(&state) {
                return;
            }
        }

        let keys = self.compute_visible_ticket_keys_for_tab(state.active_tab);
        let mut cache = self.visible_keys_cache.borrow_mut();
        cache.state = Some(state);
        cache.keys = keys;
    }

    fn normalized_search(&self) -> Option<String> {
        self.search
            .as_ref()
            .map(|s| s.to_lowercase())
            .filter(|s| !s.is_empty())
    }

    fn contains_case_insensitive_ascii(haystack: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() {
            return true;
        }
        if needle.len() > haystack.len() {
            return false;
        }
        haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
    }

    fn contains_case_insensitive(haystack: &str, needle_lower: &str) -> bool {
        if haystack.is_ascii() && needle_lower.is_ascii() {
            return Self::contains_case_insensitive_ascii(
                haystack.as_bytes(),
                needle_lower.as_bytes(),
            );
        }
        haystack.to_lowercase().contains(needle_lower)
    }

    fn ticket_matches_search(ticket: &crate::cache::Ticket, search: &str) -> bool {
        Self::contains_case_insensitive(&ticket.key, search)
            || Self::contains_case_insensitive(&ticket.summary, search)
            || ticket
                .assignee
                .as_ref()
                .map(|a| Self::contains_case_insensitive(a, search))
                .unwrap_or(false)
            || ticket
                .labels
                .iter()
                .any(|label| Self::contains_case_insensitive(label, search))
    }

    fn is_unassigned_team_ticket(ticket: &crate::cache::Ticket) -> bool {
        ticket.assignee_email.as_deref() == Some(UNASSIGNED_TEAM_EMAIL)
            || ticket.assignee.as_deref() == Some(UNASSIGNED_TEAM_NAME)
    }

    fn epic_status_rank(status: &crate::cache::Status) -> usize {
        match status {
            crate::cache::Status::InProgress => 0,
            crate::cache::Status::ReadyForWork => 1,
            crate::cache::Status::NeedsTriage => 2,
            crate::cache::Status::ToDo => 3,
            crate::cache::Status::InReview => 4,
            crate::cache::Status::Other(_) => 5,
            crate::cache::Status::Blocked => 6,
            crate::cache::Status::Done => 7,
        }
    }

    fn sort_epic_children<'a>(tickets: &mut Vec<&'a crate::cache::Ticket>) {
        tickets.sort_by(|a, b| {
            Self::epic_status_rank(&a.status)
                .cmp(&Self::epic_status_rank(&b.status))
                .then_with(|| a.key.cmp(&b.key))
        });
    }

    /// Epics and visible child rows in the exact order used by the Epics tab.
    pub(crate) fn epics_visible_epics<'a>(
        &'a self,
    ) -> Vec<(&'a crate::cache::Epic, Vec<&'a crate::cache::Ticket>)> {
        let search = self.normalized_search();
        let mut visible = Vec::new();

        for epic in &self.cache.epics {
            match &search {
                Some(s) => {
                    let epic_matches = Self::contains_case_insensitive(&epic.key, s)
                        || Self::contains_case_insensitive(&epic.summary, s);
                    if epic_matches {
                        let mut children: Vec<_> = epic.children.iter().collect();
                        Self::sort_epic_children(&mut children);
                        visible.push((epic, children));
                        continue;
                    }

                    let mut matching_children: Vec<_> = epic
                        .children
                        .iter()
                        .filter(|t| Self::ticket_matches_search(t, s))
                        .collect();
                    Self::sort_epic_children(&mut matching_children);

                    if !matching_children.is_empty() {
                        visible.push((epic, matching_children));
                    }
                }
                None => {
                    let mut children: Vec<_> = epic.children.iter().collect();
                    Self::sort_epic_children(&mut children);
                    visible.push((epic, children));
                }
            }
        }

        visible
    }

    pub(crate) fn epics_visible_ticket_keys(&self) -> Vec<String> {
        self.epics_visible_epics()
            .into_iter()
            .flat_map(|(_, tickets)| tickets.into_iter().map(|t| t.key.clone()))
            .collect()
    }

    /// Unassigned tickets grouped by epic.
    pub(crate) fn unassigned_visible_by_epic<'a>(
        &'a self,
    ) -> Vec<(String, String, Vec<&'a crate::cache::Ticket>)> {
        let search = self.normalized_search();
        let mut grouped: HashMap<(String, String), Vec<&crate::cache::Ticket>> = HashMap::new();

        for ticket in &self.cache.team_tickets {
            if !Self::is_unassigned_team_ticket(ticket) {
                continue;
            }

            let epic_key = ticket
                .epic_key
                .clone()
                .unwrap_or_else(|| NO_EPIC_KEY.to_string());
            let epic_summary = ticket
                .epic_name
                .clone()
                .unwrap_or_else(|| NO_EPIC_SUMMARY.to_string());
            grouped.entry((epic_key, epic_summary)).or_default().push(ticket);
        }

        let mut groups: Vec<_> = grouped
            .into_iter()
            .map(|((epic_key, epic_summary), mut tickets)| {
                Self::sort_epic_children(&mut tickets);
                (epic_key, epic_summary, tickets)
            })
            .collect();

        groups.sort_by(|a, b| {
            b.2.len()
                .cmp(&a.2.len())
                .then_with(|| a.0.cmp(&b.0))
                .then_with(|| a.1.cmp(&b.1))
        });

        let mut visible = Vec::new();
        for (epic_key, epic_summary, tickets) in groups {
            match &search {
                Some(s) => {
                    let epic_matches = Self::contains_case_insensitive(&epic_key, s)
                        || Self::contains_case_insensitive(&epic_summary, s);
                    if epic_matches {
                        visible.push((epic_key, epic_summary, tickets));
                        continue;
                    }

                    let filtered: Vec<_> = tickets
                        .into_iter()
                        .filter(|t| Self::ticket_matches_search(t, s))
                        .collect();
                    if !filtered.is_empty() {
                        visible.push((epic_key, epic_summary, filtered));
                    }
                }
                None => visible.push((epic_key, epic_summary, tickets)),
            }
        }

        visible
    }

    pub(crate) fn unassigned_visible_ticket_keys(&self) -> Vec<String> {
        self.unassigned_visible_by_epic()
            .into_iter()
            .flat_map(|(_, _, tickets)| tickets.into_iter().map(|t| t.key.clone()))
            .collect()
    }

    /// Status groups and visible tickets in the exact order used by the My Work tab.
    pub(crate) fn my_work_visible_by_status<'a>(
        &'a self,
    ) -> Vec<(&'static crate::cache::Status, Vec<&'a crate::cache::Ticket>)> {
        let search = self.normalized_search();

        crate::cache::Status::all()
            .iter()
            .filter_map(|status| {
                if *status == crate::cache::Status::Done {
                    if !self.show_done {
                        return None;
                    }
                } else if let Some(focus) = &self.status_focus {
                    if *status != *focus {
                        return None;
                    }
                }

                let tickets: Vec<_> = self
                    .cache
                    .my_tickets
                    .iter()
                    .filter(|t| &t.status == status)
                    .filter(|t| {
                        if let Some(s) = &search {
                            Self::ticket_matches_search(t, s)
                        } else {
                            true
                        }
                    })
                    .collect();

                if tickets.is_empty() {
                    return None;
                }
                Some((status, tickets))
            })
            .collect()
    }

    pub(crate) fn my_work_visible_ticket_keys(&self) -> Vec<String> {
        self.my_work_visible_by_status()
            .into_iter()
            .flat_map(|(_, tickets)| tickets.into_iter().map(|t| t.key.clone()))
            .collect()
    }

    /// Team members and visible tickets in the exact order used by the Team tab.
    /// Returns active tickets first, then Done tickets as a secondary group.
    pub(crate) fn team_visible_tickets_by_member<'a>(
        &'a self,
    ) -> Vec<(
        &'a crate::cache::TeamMember,
        Vec<&'a crate::cache::Ticket>,
        Vec<&'a crate::cache::Ticket>,
    )> {
        let search = self.normalized_search();
        let mut visible = Vec::new();
        let mut tickets_by_email: HashMap<&str, Vec<&crate::cache::Ticket>> = HashMap::new();
        for ticket in &self.cache.team_tickets {
            if let Some(email) = ticket.assignee_email.as_deref() {
                tickets_by_email.entry(email).or_default().push(ticket);
            }
        }

        for member in self.sorted_team_members() {
            let member_tickets = tickets_by_email
                .get(member.email.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let filtered: Vec<_> = match &search {
                Some(s) => {
                    let member_match = Self::contains_case_insensitive(&member.name, s)
                        || Self::contains_case_insensitive(&member.email, s);
                    if member_match {
                        member_tickets.to_vec()
                    } else {
                        member_tickets
                            .into_iter()
                            .copied()
                            .filter(|t| Self::ticket_matches_search(t, s))
                            .collect()
                    }
                }
                None => member_tickets.to_vec(),
            };

            if search.is_some() && filtered.is_empty() {
                continue;
            }

            let mut active = Vec::new();
            let mut done = Vec::new();
            for ticket in filtered {
                if ticket.status == crate::cache::Status::Done {
                    if self.show_done {
                        done.push(ticket);
                    }
                } else if let Some(focus) = &self.status_focus {
                    if &ticket.status == focus {
                        active.push(ticket);
                    }
                } else {
                    active.push(ticket);
                }
            }

            visible.push((member, active, done));
        }

        visible
    }

    pub(crate) fn team_visible_ticket_keys(&self) -> Vec<String> {
        self.team_visible_tickets_by_member()
            .into_iter()
            .flat_map(|(_, active, done)| {
                active
                    .into_iter()
                    .chain(done)
                    .map(|t| t.key.clone())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn toggle_show_done(&mut self) {
        self.show_done = !self.show_done;
        self.clamp_selection();
    }

    pub fn toggle_keybindings(&mut self) {
        self.show_keybindings = !self.show_keybindings;
    }

    pub fn close_keybindings(&mut self) {
        self.show_keybindings = false;
    }

    pub fn is_create_ticket_open(&self) -> bool {
        self.create_ticket.is_some()
    }

    pub fn is_comment_open(&self) -> bool {
        self.comment_state.is_some()
    }

    pub fn is_assign_open(&self) -> bool {
        self.assign_state.is_some()
    }

    pub fn is_edit_open(&self) -> bool {
        self.edit_state.is_some()
    }

    pub fn is_filter_edit_open(&self) -> bool {
        self.filter_edit.is_some()
    }

    pub fn begin_detail_fetch(&mut self, key: &str) -> bool {
        self.detail_fetching.insert(key.to_string())
    }

    pub fn end_detail_fetch(&mut self, key: &str) {
        self.detail_fetching.remove(key);
    }

    pub fn missing_detail_ticket_keys(&self) -> Vec<String> {
        let mut keys = HashSet::new();
        for ticket in &self.cache.my_tickets {
            if !ticket.detail_loaded && !self.detail_fetching.contains(&ticket.key) {
                keys.insert(ticket.key.clone());
            }
        }
        for ticket in &self.cache.team_tickets {
            if !ticket.detail_loaded && !self.detail_fetching.contains(&ticket.key) {
                keys.insert(ticket.key.clone());
            }
        }
        let mut keys: Vec<String> = keys.into_iter().collect();
        keys.sort();
        keys
    }

    pub fn toggle_status_focus(&mut self, status: crate::cache::Status) {
        if self.status_focus.as_ref() == Some(&status) {
            self.status_focus = None;
        } else {
            self.status_focus = Some(status);
        }
        self.clamp_selection();
    }

    /// Get the currently selected ticket key based on the active tab and selected index.
    pub fn selected_ticket_key(&self) -> Option<String> {
        self.ensure_visible_keys_cache();
        self.visible_keys_cache
            .borrow()
            .keys
            .get(self.selected_index)
            .cloned()
    }

    /// Total number of selectable items in the current tab.
    pub fn item_count(&self) -> usize {
        self.ensure_visible_keys_cache();
        self.visible_keys_cache.borrow().keys.len()
    }

    pub fn clamp_selection(&mut self) {
        let count = self.item_count();
        if count == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= count {
            self.selected_index = count - 1;
        }
    }

    pub fn move_selection_down(&mut self) {
        let count = self.item_count();
        if count > 0 && self.selected_index < count - 1 {
            self.selected_index += 1;
        }
    }

    pub fn move_selection_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn scroll_detail_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    /// Find a ticket by key across all cached data.
    pub fn find_ticket(&self, key: &str) -> Option<&crate::cache::Ticket> {
        self.cache
            .my_tickets
            .iter()
            .find(|t| t.key == key)
            .or_else(|| self.cache.team_tickets.iter().find(|t| t.key == key))
            .or_else(|| {
                self.cache
                    .epics
                    .iter()
                    .flat_map(|e| e.children.iter())
                    .find(|t| t.key == key)
            })
            .or_else(|| self.filter_results.iter().find(|t| t.key == key))
    }

    /// Enrich a cached ticket with full detail from JSON (description, accurate status/assignee).
    pub fn enrich_ticket(&mut self, key: &str, detail: &crate::cache::Ticket) {
        let mut changed = false;
        let update = |ticket: &mut crate::cache::Ticket| {
            ticket.status = detail.status.clone();
            if detail.assignee.is_some() {
                ticket.assignee = detail.assignee.clone();
            }
            if detail.assignee_email.is_some() {
                ticket.assignee_email = detail.assignee_email.clone();
            }
            ticket.description = detail.description.clone();
            ticket.labels = detail.labels.clone();
            if detail.epic_key.is_some() {
                ticket.epic_key = detail.epic_key.clone();
            }
            if detail.epic_name.is_some() {
                ticket.epic_name = detail.epic_name.clone();
            }
            if !detail.activity.is_empty() {
                ticket.activity = detail.activity.clone();
            }
            ticket.detail_loaded = true;
        };
        for ticket in &mut self.cache.my_tickets {
            if ticket.key == key {
                update(ticket);
                changed = true;
            }
        }
        for ticket in &mut self.cache.team_tickets {
            if ticket.key == key {
                update(ticket);
                changed = true;
            }
        }
        for epic in &mut self.cache.epics {
            for ticket in &mut epic.children {
                if ticket.key == key {
                    update(ticket);
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_cache_changed();
        }
    }

    /// Update a ticket's status in the cache (optimistic update).
    pub fn update_ticket_status(&mut self, key: &str, new_status: crate::cache::Status) {
        let mut changed = false;
        for ticket in &mut self.cache.my_tickets {
            if ticket.key == key {
                ticket.status = new_status.clone();
                changed = true;
            }
        }
        for ticket in &mut self.cache.team_tickets {
            if ticket.key == key {
                ticket.status = new_status.clone();
                changed = true;
            }
        }
        for epic in &mut self.cache.epics {
            for ticket in &mut epic.children {
                if ticket.key == key {
                    ticket.status = new_status.clone();
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_cache_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Tab};
    use crate::cache::{Epic, Status, Ticket};

    fn ticket(key: &str, summary: &str) -> Ticket {
        Ticket {
            key: key.to_string(),
            summary: summary.to_string(),
            status: Status::ToDo,
            assignee: None,
            assignee_email: None,
            description: None,
            labels: Vec::new(),
            epic_key: None,
            epic_name: None,
            detail_loaded: false,
            url: format!("https://jira.mongodb.org/browse/{}", key),
            activity: Vec::new(),
        }
    }

    fn epics_app(epics: Vec<Epic>) -> App {
        let mut app = App::new();
        app.active_tab = Tab::Epics;
        app.loading = false;
        app.cache.epics = epics;
        app
    }

    #[test]
    fn epics_item_count_matches_visible_child_rows() {
        let app = epics_app(vec![
            Epic {
                key: "AMP-100".to_string(),
                summary: "Auth".to_string(),
                children: vec![ticket("AMP-1", "Session"), ticket("AMP-2", "Password")],
            },
            Epic {
                key: "AMP-200".to_string(),
                summary: "Perf".to_string(),
                children: vec![ticket("AMP-3", "Cache")],
            },
        ]);

        assert_eq!(app.item_count(), 3);
    }

    #[test]
    fn epics_selected_ticket_key_uses_cross_epic_row_order() {
        let mut app = epics_app(vec![
            Epic {
                key: "AMP-100".to_string(),
                summary: "Auth".to_string(),
                children: vec![ticket("AMP-1", "Session"), ticket("AMP-2", "Password")],
            },
            Epic {
                key: "AMP-200".to_string(),
                summary: "Perf".to_string(),
                children: vec![ticket("AMP-3", "Cache")],
            },
        ]);

        app.selected_index = 2;
        assert_eq!(app.selected_ticket_key(), Some("AMP-3".to_string()));
    }

    #[test]
    fn epics_filtered_search_mapping_is_deterministic() {
        let mut app = epics_app(vec![
            Epic {
                key: "AMP-100".to_string(),
                summary: "Auth Platform".to_string(),
                children: vec![
                    ticket("AMP-1", "Session resume"),
                    ticket("AMP-2", "Passwords"),
                ],
            },
            Epic {
                key: "AMP-200".to_string(),
                summary: "Performance".to_string(),
                children: vec![
                    ticket("AMP-3", "Session cache"),
                    ticket("AMP-4", "Load test"),
                ],
            },
        ]);

        app.search = Some("session".to_string());
        assert_eq!(
            app.epics_visible_ticket_keys(),
            vec!["AMP-1".to_string(), "AMP-3".to_string()]
        );

        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("AMP-3".to_string()));

        app.search = Some("auth".to_string());
        assert_eq!(
            app.epics_visible_ticket_keys(),
            vec!["AMP-1".to_string(), "AMP-2".to_string()]
        );
    }

    #[test]
    fn epics_with_zero_children_contribute_no_selectable_rows() {
        let mut app = epics_app(vec![
            Epic {
                key: "AMP-100".to_string(),
                summary: "Empty Epic".to_string(),
                children: vec![],
            },
            Epic {
                key: "AMP-200".to_string(),
                summary: "Auth".to_string(),
                children: vec![ticket("AMP-1", "Session")],
            },
        ]);

        assert_eq!(app.item_count(), 1);

        app.search = Some("empty".to_string());
        assert_eq!(app.item_count(), 0);
        assert_eq!(app.selected_ticket_key(), None);
    }

    #[test]
    fn my_work_search_matches_labels() {
        let mut app = App::new();
        app.active_tab = Tab::MyWork;
        app.loading = false;

        let mut t = ticket("AMP-1", "Refactor parser");
        t.status = Status::InProgress;
        t.labels = vec!["metis".to_string(), "backend".to_string()];
        app.cache.my_tickets = vec![t];

        app.search = Some("metis".to_string());
        assert_eq!(app.item_count(), 1);
        assert_eq!(app.selected_ticket_key(), Some("AMP-1".to_string()));
    }

    #[test]
    fn team_search_matches_labels() {
        let mut app = App::new();
        app.active_tab = Tab::Team;
        app.loading = false;
        app.cache.team_members = vec![crate::cache::TeamMember {
            name: "Dev".to_string(),
            email: "dev@example.com".to_string(),
        }];

        let mut t = ticket("AMP-2", "Triage regression");
        t.status = Status::NeedsTriage;
        t.labels = vec!["infra".to_string()];
        t.assignee_email = Some("dev@example.com".to_string());
        app.cache.team_tickets = vec![t];

        app.search = Some("infra".to_string());
        assert_eq!(app.item_count(), 1);
        assert_eq!(app.selected_ticket_key(), Some("AMP-2".to_string()));
    }

    #[test]
    fn epics_search_matches_child_labels() {
        let mut app = epics_app(vec![Epic {
            key: "AMP-500".to_string(),
            summary: "Platform".to_string(),
            children: {
                let mut t = ticket("AMP-55", "Improve cache");
                t.labels = vec!["perf".to_string()];
                vec![t]
            },
        }]);

        app.search = Some("perf".to_string());
        assert_eq!(app.item_count(), 1);
        assert_eq!(app.selected_ticket_key(), Some("AMP-55".to_string()));
    }

    #[test]
    fn unassigned_item_count_matches_visible_rows() {
        let mut app = App::new();
        app.active_tab = Tab::Unassigned;
        app.loading = false;

        let mut t1 = ticket("AMP-91", "Missing owner in epic one");
        t1.assignee = Some("Unassigned".to_string());
        t1.assignee_email = Some("__unassigned__".to_string());
        t1.epic_key = Some("AMP-100".to_string());
        t1.epic_name = Some("Epic One".to_string());

        let mut t2 = ticket("AMP-92", "Another owner gap in epic one");
        t2.assignee = Some("Unassigned".to_string());
        t2.assignee_email = Some("__unassigned__".to_string());
        t2.epic_key = Some("AMP-100".to_string());
        t2.epic_name = Some("Epic One".to_string());

        let mut t3 = ticket("AMP-93", "Unassigned without epic");
        t3.assignee = Some("Unassigned".to_string());
        t3.assignee_email = Some("__unassigned__".to_string());

        let mut assigned = ticket("AMP-94", "Assigned ticket");
        assigned.assignee = Some("Dev".to_string());
        assigned.assignee_email = Some("dev@example.com".to_string());

        app.cache.team_tickets = vec![t3, assigned, t2, t1];

        assert_eq!(app.item_count(), 3);
        assert_eq!(
            app.unassigned_visible_ticket_keys(),
            vec![
                "AMP-91".to_string(),
                "AMP-92".to_string(),
                "AMP-93".to_string()
            ]
        );
    }

    #[test]
    fn unassigned_search_matches_epic_and_ticket_fields() {
        let mut app = App::new();
        app.active_tab = Tab::Unassigned;
        app.loading = false;

        let mut t1 = ticket("AMP-101", "Upgrade parser error handling");
        t1.assignee = Some("Unassigned".to_string());
        t1.assignee_email = Some("__unassigned__".to_string());
        t1.epic_key = Some("AMP-501".to_string());
        t1.epic_name = Some("Parser Platform".to_string());
        t1.labels = vec!["infra".to_string()];

        let mut t2 = ticket("AMP-102", "Refactor retries");
        t2.assignee = Some("Unassigned".to_string());
        t2.assignee_email = Some("__unassigned__".to_string());
        t2.epic_key = Some("AMP-502".to_string());
        t2.epic_name = Some("Runner".to_string());
        t2.labels = vec!["perf".to_string()];

        app.cache.team_tickets = vec![t1, t2];

        app.search = Some("parser".to_string());
        assert_eq!(app.item_count(), 1);
        assert_eq!(app.selected_ticket_key(), Some("AMP-101".to_string()));

        app.search = Some("perf".to_string());
        assert_eq!(app.item_count(), 1);
        assert_eq!(app.selected_ticket_key(), Some("AMP-102".to_string()));
    }
}
