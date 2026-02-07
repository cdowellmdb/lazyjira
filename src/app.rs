use crate::cache::Cache;

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
            should_quit: false,
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab = self.active_tab.next();
        self.selected_index = 0;
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

    /// Get the currently selected ticket key based on the active tab and selected index.
    pub fn selected_ticket_key(&self) -> Option<String> {
        match self.active_tab {
            Tab::MyWork => {
                let grouped = self.cache.my_tickets_by_status();
                let mut flat: Vec<&str> = Vec::new();
                for (_, tickets) in &grouped {
                    for t in tickets {
                        flat.push(&t.key);
                    }
                }
                flat.get(self.selected_index).map(|k| k.to_string())
            }
            Tab::Team => {
                let mut flat: Vec<&str> = Vec::new();
                for member in self.sorted_team_members() {
                    for t in self.cache.active_tickets_for(&member.email) {
                        flat.push(&t.key);
                    }
                }
                flat.get(self.selected_index).map(|k| k.to_string())
            }
            Tab::Epics => {
                let mut flat: Vec<&str> = Vec::new();
                for epic in &self.cache.epics {
                    for t in &epic.children {
                        flat.push(&t.key);
                    }
                }
                flat.get(self.selected_index).map(|k| k.to_string())
            }
        }
    }

    /// Total number of selectable items in the current tab.
    pub fn item_count(&self) -> usize {
        match self.active_tab {
            Tab::MyWork => {
                self.cache.my_tickets_by_status()
                    .iter()
                    .map(|(_, tickets)| tickets.len())
                    .sum()
            }
            Tab::Team => {
                self.cache.team_members
                    .iter()
                    .map(|m| self.cache.active_tickets_for(&m.email).len())
                    .sum()
            }
            Tab::Epics => {
                self.cache.epics.len()
            }
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
        self.cache.my_tickets.iter().find(|t| t.key == key)
            .or_else(|| self.cache.team_tickets.iter().find(|t| t.key == key))
            .or_else(|| {
                self.cache.epics.iter()
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
            if detail.epic_key.is_some() {
                ticket.epic_key = detail.epic_key.clone();
            }
        };
        for ticket in &mut self.cache.my_tickets {
            if ticket.key == key { update(ticket); }
        }
        for ticket in &mut self.cache.team_tickets {
            if ticket.key == key { update(ticket); }
        }
        for epic in &mut self.cache.epics {
            for ticket in &mut epic.children {
                if ticket.key == key { update(ticket); }
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
