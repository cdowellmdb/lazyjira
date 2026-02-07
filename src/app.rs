use crate::cache::Cache;
use std::collections::HashSet;

/// Which tab is currently active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    MyWork,
    Team,
    Epics,
}

impl Tab {
    pub fn next(self) -> Self {
        match self {
            Tab::MyWork => Tab::Team,
            Tab::Team => Tab::Epics,
            Tab::Epics => Tab::MyWork,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Tab::MyWork => "My Work",
            Tab::Team => "Team",
            Tab::Epics => "Epics",
        }
    }

    pub fn all() -> &'static [Tab] {
        &[Tab::MyWork, Tab::Team, Tab::Epics]
    }
}

/// What the detail overlay is showing.
#[derive(Debug, Clone)]
pub enum DetailMode {
    /// Showing ticket info.
    View,
    /// Showing the status move picker, with selected index.
    MovePicker { selected: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketSyncStage {
    ActiveOnly,
    Full,
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
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            cache: Cache::empty(),
            active_tab: Tab::MyWork,
            selected_index: 0,
            detail_ticket_key: None,
            detail_mode: DetailMode::View,
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
            should_quit: false,
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab = self.active_tab.next();
        self.selected_index = 0;
        self.clamp_selection();
    }

    pub fn open_detail(&mut self, key: String) {
        self.detail_ticket_key = Some(key);
        self.detail_mode = DetailMode::View;
    }

    pub fn close_detail(&mut self) {
        self.detail_ticket_key = None;
        self.detail_mode = DetailMode::View;
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
        let mut members: Vec<_> = self.cache.team_members.iter().collect();
        members.sort_by(|a, b| {
            let ac = self.cache.active_tickets_for(&a.email).len();
            let bc = self.cache.active_tickets_for(&b.email).len();
            bc.cmp(&ac)
        });
        members
    }

    fn normalized_search(&self) -> Option<String> {
        self.search
            .as_ref()
            .map(|s| s.to_lowercase())
            .filter(|s| !s.is_empty())
    }

    fn ticket_matches_search(ticket: &crate::cache::Ticket, search: &str) -> bool {
        ticket.key.to_lowercase().contains(search)
            || ticket.summary.to_lowercase().contains(search)
            || ticket
                .assignee
                .as_ref()
                .map(|a| a.to_lowercase().contains(search))
                .unwrap_or(false)
            || ticket
                .labels
                .iter()
                .any(|label| label.to_lowercase().contains(search))
    }

    fn epic_status_rank(status: &crate::cache::Status) -> usize {
        match status {
            crate::cache::Status::InProgress => 0,
            crate::cache::Status::ReadyForWork => 1,
            crate::cache::Status::NeedsTriage => 2,
            crate::cache::Status::ToDo => 3,
            crate::cache::Status::InReview => 4,
            crate::cache::Status::Blocked => 5,
            crate::cache::Status::Done => 6,
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
                    let epic_matches = epic.key.to_lowercase().contains(s.as_str())
                        || epic.summary.to_lowercase().contains(s.as_str());
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

        for member in self.sorted_team_members() {
            let tickets = self.cache.tickets_for(&member.email);
            let member_name = member.name.to_lowercase();
            let member_email = member.email.to_lowercase();
            let filtered: Vec<_> = match &search {
                Some(s) => {
                    let member_match =
                        member_name.contains(s.as_str()) || member_email.contains(s.as_str());
                    if member_match {
                        tickets
                    } else {
                        tickets
                            .into_iter()
                            .filter(|t| Self::ticket_matches_search(t, s))
                            .collect()
                    }
                }
                None => tickets,
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
        match self.active_tab {
            Tab::MyWork => self
                .my_work_visible_ticket_keys()
                .get(self.selected_index)
                .cloned(),
            Tab::Team => self
                .team_visible_ticket_keys()
                .get(self.selected_index)
                .cloned(),
            Tab::Epics => self
                .epics_visible_ticket_keys()
                .get(self.selected_index)
                .cloned(),
        }
    }

    /// Total number of selectable items in the current tab.
    pub fn item_count(&self) -> usize {
        match self.active_tab {
            Tab::MyWork => self.my_work_visible_ticket_keys().len(),
            Tab::Team => self.team_visible_ticket_keys().len(),
            Tab::Epics => self.epics_visible_ticket_keys().len(),
        }
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
    }

    /// Enrich a cached ticket with full detail from JSON (description, accurate status/assignee).
    pub fn enrich_ticket(&mut self, key: &str, detail: &crate::cache::Ticket) {
        let update = |ticket: &mut crate::cache::Ticket| {
            ticket.status = detail.status.clone();
            ticket.assignee = detail.assignee.clone();
            ticket.assignee_email = detail.assignee_email.clone();
            ticket.description = detail.description.clone();
            ticket.labels = detail.labels.clone();
            if detail.epic_key.is_some() {
                ticket.epic_key = detail.epic_key.clone();
            }
            if detail.epic_name.is_some() {
                ticket.epic_name = detail.epic_name.clone();
            }
            ticket.detail_loaded = true;
        };
        for ticket in &mut self.cache.my_tickets {
            if ticket.key == key {
                update(ticket);
            }
        }
        for ticket in &mut self.cache.team_tickets {
            if ticket.key == key {
                update(ticket);
            }
        }
        for epic in &mut self.cache.epics {
            for ticket in &mut epic.children {
                if ticket.key == key {
                    update(ticket);
                }
            }
        }
    }

    /// Update a ticket's status in the cache (optimistic update).
    pub fn update_ticket_status(&mut self, key: &str, new_status: crate::cache::Status) {
        for ticket in &mut self.cache.my_tickets {
            if ticket.key == key {
                ticket.status = new_status.clone();
            }
        }
        for ticket in &mut self.cache.team_tickets {
            if ticket.key == key {
                ticket.status = new_status.clone();
            }
        }
        for epic in &mut self.cache.epics {
            for ticket in &mut epic.children {
                if ticket.key == key {
                    ticket.status = new_status.clone();
                }
            }
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
}
