use crate::bulk_actions::BulkState;
use crate::cache::Cache;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

const UNASSIGNED_TEAM_NAME: &str = "Unassigned";
const UNASSIGNED_TEAM_EMAIL: &str = "__unassigned__";
const NO_EPIC_KEY: &str = "NO-EPIC";
const NO_EPIC_SUMMARY: &str = "No Epic";

pub const ISSUE_TYPES: &[&str] = &["Task", "Bug", "Story"];

/// An item in the visible selection list — either a group header or a ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibleItem {
    GroupHeader(String),
    Ticket(String),
}

/// One displayed group. Row indices count occurrences, including repeated ticket keys.
/// Header metadata and totals survive collapse; only expanded groups have ticket rows.
pub(crate) struct VisibleGroup<'a, H> {
    pub id: String,
    pub header: H,
    pub index: usize,
    pub total: usize,
    pub tickets: Option<Vec<(usize, &'a crate::cache::Ticket)>>,
}

#[derive(Debug, Clone)]
pub struct CommentState {
    pub ticket_key: String,
    pub body: tui_textarea::TextArea<'static>,
}

#[derive(Debug, Clone)]
pub struct AssignState {
    pub ticket_key: String,
    pub selected: usize,
    pub search: String,
}

#[derive(Debug, Clone)]
pub struct EditFieldsState {
    pub ticket_key: String,
    pub focused_field: usize, // 0=summary, 1=labels, 2=description
    pub summary: tui_textarea::TextArea<'static>,
    pub labels: tui_textarea::TextArea<'static>,
    pub description: tui_textarea::TextArea<'static>,
}

#[derive(Debug, Clone)]
pub struct CreateTicketState {
    pub focused_field: usize, // 0=type, 1=summary, 2=assignee, 3=epic, 4=labels, 5=description
    pub issue_type_idx: usize,
    pub summary: tui_textarea::TextArea<'static>,
    pub labels: tui_textarea::TextArea<'static>,
    pub description: tui_textarea::TextArea<'static>,
    pub assignee_search: String,
    pub epic_search: String,
    pub assignee_idx: usize, // 0 = "None", then 1..N = team members
    pub epic_idx: usize,     // 0 = "None", then 1..N = cached epics
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkUploadRow {
    pub row_number: usize,
    pub issue_type: String,
    pub summary: String,
    pub assignee_email: Option<String>,
    pub epic_key: Option<String>,
    pub labels: Vec<String>,
    pub description: Option<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkUploadPreview {
    pub source_path: String,
    pub rows: Vec<BulkUploadRow>,
    pub total_rows: usize,
    pub valid_rows: usize,
    pub invalid_rows: usize,
    pub warning_count: usize,
}

impl BulkUploadPreview {
    pub fn can_submit(&self) -> bool {
        self.invalid_rows == 0 && self.valid_rows > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkUploadSummary {
    pub source_path: String,
    pub total_rows: usize,
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub created_keys: Vec<String>,
    pub failed_details: Vec<(usize, String, String)>,
}

#[derive(Debug, Clone)]
pub enum BulkUploadState {
    PathInput {
        path: Box<tui_textarea::TextArea<'static>>,
        loading: bool,
    },
    Preview {
        preview: BulkUploadPreview,
        selected: usize,
    },
    Running {
        preview: BulkUploadPreview,
    },
    Result {
        summary: BulkUploadSummary,
    },
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
    pub fn index(self) -> usize {
        self as usize
    }

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
        &[
            Tab::MyWork,
            Tab::Team,
            Tab::Epics,
            Tab::Unassigned,
            Tab::Filters,
        ]
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
    pub name: tui_textarea::TextArea<'static>,
    pub jql: tui_textarea::TextArea<'static>,
    /// None = creating new, Some(idx) = editing existing filter at index.
    pub editing_idx: Option<usize>,
}

/// What the detail overlay is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMode {
    /// Showing ticket info.
    View,
    /// Waiting for Jira to list the ticket's transitions. Only the answer to `request` is shown.
    MoveLoading { request: u64 },
    /// Choosing one of the ticket's transitions.
    MovePicker(crate::move_picker::MovePicker),
    /// Choosing one of `picker.resolution_choices()` to send with the picker's selected transition.
    ResolutionPicker {
        picker: crate::move_picker::MovePicker,
        selected: usize,
    },
    /// Showing the activity/history timeline with scroll offset.
    History { scroll: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketSyncStage {
    ActiveOnly,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupSelectionState {
    None,
    Partial,
    All,
}

/// A Team tab row group: the member, their active tickets, then their Done tickets.
pub(crate) type TeamMemberTickets<'a> = (
    &'a crate::cache::TeamMember,
    Vec<&'a crate::cache::Ticket>,
    Vec<&'a crate::cache::Ticket>,
);

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisibleKeysState {
    active_tab: Tab,
    search: Option<String>,
    show_done: bool,
    status_focus: Option<String>,
    view_generation: u64,
}

#[derive(Default)]
struct VisibleKeysCache {
    state: Option<VisibleKeysState>,
    items: Vec<VisibleItem>,
    group_ticket_keys: HashMap<String, Vec<String>>,
}

#[derive(Default, Clone)]
struct ViewPosition {
    selected: Option<VisibleItem>,
    group: Option<String>,
    index: usize,
    search: Option<String>,
    status_focus: Option<String>,
}

/// Full application state.
pub struct App {
    pub cache: Cache,
    pub active_tab: Tab,
    /// Index of the selected item in the current tab's list.
    pub selected_index: usize,
    tab_positions: [ViewPosition; 5],
    /// If Some, the detail overlay is open for this ticket key.
    pub detail_ticket_key: Option<String>,
    /// If Some, the detail overlay is open for this epic key.
    pub detail_epic_key: Option<String>,
    pub detail_mode: DetailMode,
    /// Vertical scroll offset for the ticket detail body.
    pub detail_scroll: u16,
    /// Largest useful `detail_scroll` and the body's height, as last rendered.
    pub detail_scroll_max: Cell<u16>,
    pub detail_page_height: Cell<u16>,
    /// Whether the detail overlay fills the screen (`z`).
    pub detail_fullscreen: bool,
    /// True while data is being fetched.
    pub loading: bool,
    /// Flash message (error or success), cleared on next keypress.
    pub flash: Option<String>,
    /// Search/filter string when `/` is active.
    pub search: Option<String>,
    /// Whether Done tickets are visible in My Work and Team tabs.
    pub show_done: bool,
    /// Optional focused active status name for My Work and Team.
    pub status_focus: Option<String>,
    /// True while full epic relationships are being refreshed in background.
    pub epics_refreshing: bool,
    pub epic_refresh_request: u64,
    pub cache_refresh_request: u64,
    /// Ticket sync stage for background cache refresh.
    pub ticket_sync_stage: Option<TicketSyncStage>,
    /// Age of the cache snapshot loaded at startup, in seconds.
    pub cache_stale_age_secs: Option<u64>,
    /// Whether the keybindings overlay is visible.
    pub show_keybindings: bool,
    pub keybindings_scroll: u16,
    pub keybindings_scroll_max: Cell<u16>,
    pub keybindings_page_height: Cell<u16>,
    /// Ticket keys currently being fetched for rich detail.
    detail_fetching: HashSet<String>,
    /// Why the last detail fetch failed, by ticket key.
    detail_fetch_errors: HashMap<String, String>,
    /// Single-ticket moves waiting on Jira, confirmed, or rejected.
    pub moves: crate::moves::MoveTracker,
    /// Last id handed out by `next_request_id`.
    last_request_id: u64,
    /// Monotonic generation used to invalidate derived visibility caches.
    view_generation: u64,
    /// Cached visible ticket keys for selection/counting in the active tab.
    visible_keys_cache: RefCell<VisibleKeysCache>,
    pub should_quit: bool,
    pub settings: Option<crate::settings::Settings>,
    pub external_editor_requested: bool,
    pub mouse_targets: RefCell<Vec<(ratatui::layout::Rect, crate::mouse::Target)>>,
    pub text_selection: RefCell<crate::mouse::TextSelection>,
    /// State for the create ticket modal overlay.
    pub create_ticket: Option<CreateTicketState>,
    /// Selected ticket keys in the current visible list context.
    pub selected_ticket_keys: HashSet<String>,
    /// State for the bulk actions modal flow.
    pub bulk_state: Option<BulkState>,
    /// State for bulk CSV upload and ticket creation flow.
    pub bulk_upload_state: Option<BulkUploadState>,
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
    /// Collapsed groups per tab (group identifiers).
    pub collapsed_my_work: HashSet<String>,
    pub collapsed_team: HashSet<String>,
    pub collapsed_epics: HashSet<String>,
    pub collapsed_unassigned: HashSet<String>,
    pub collapsed_filters: HashSet<String>,
    /// Optional epic focus order used by the Epics tab; empty means show all epics.
    epics_i_care_about_rank: HashMap<String, usize>,
    /// Status order and done/active, from the `[statuses]` config.
    status_rules: crate::cache::StatusRules,
}

impl App {
    pub fn new() -> Self {
        Self {
            cache: Cache::empty(),
            active_tab: Tab::MyWork,
            selected_index: 0,
            tab_positions: std::array::from_fn(|_| ViewPosition::default()),
            detail_ticket_key: None,
            detail_epic_key: None,
            detail_mode: DetailMode::View,
            detail_scroll: 0,
            detail_scroll_max: Cell::new(0),
            detail_page_height: Cell::new(1),
            detail_fullscreen: false,
            loading: true,
            flash: None,
            search: None,
            show_done: true,
            status_focus: None,
            epics_refreshing: false,
            epic_refresh_request: 0,
            cache_refresh_request: 0,
            ticket_sync_stage: None,
            cache_stale_age_secs: None,
            show_keybindings: false,
            keybindings_scroll: 0,
            keybindings_scroll_max: Cell::new(0),
            keybindings_page_height: Cell::new(1),
            detail_fetching: HashSet::new(),
            detail_fetch_errors: HashMap::new(),
            moves: crate::moves::MoveTracker::default(),
            last_request_id: 0,
            view_generation: 0,
            visible_keys_cache: RefCell::new(VisibleKeysCache::default()),
            should_quit: false,
            settings: None,
            external_editor_requested: false,
            mouse_targets: RefCell::new(Vec::new()),
            text_selection: RefCell::new(crate::mouse::TextSelection::default()),
            create_ticket: None,
            selected_ticket_keys: HashSet::new(),
            bulk_state: None,
            bulk_upload_state: None,
            comment_state: None,
            assign_state: None,
            edit_state: None,
            filter_focus: FilterFocus::Sidebar,
            filter_sidebar_idx: 0,
            filter_results: Vec::new(),
            filter_loading: false,
            filter_edit: None,
            collapsed_my_work: HashSet::new(),
            collapsed_team: HashSet::new(),
            collapsed_epics: HashSet::new(),
            collapsed_unassigned: HashSet::new(),
            collapsed_filters: HashSet::new(),
            epics_i_care_about_rank: HashMap::new(),
            status_rules: crate::cache::StatusRules::default(),
        }
    }

    pub fn focus_field(&mut self, field: usize) {
        if let Some(state) = &mut self.settings {
            state.focused_field = field.min(3);
        } else if let Some(state) = &mut self.filter_edit {
            state.focused_field = field.min(1);
        } else if let Some(state) = &mut self.create_ticket {
            state.focused_field = field.min(5);
        } else if let Some(state) = &mut self.edit_state {
            state.focused_field = field.min(2);
        }
    }

    pub fn current_editor(&mut self) -> Option<(&mut tui_textarea::TextArea<'static>, bool)> {
        if !self.moves.failures().is_empty() || self.show_keybindings || self.bulk_state.is_some() {
            return None;
        }
        if let Some(state) = &mut self.bulk_upload_state {
            return match state {
                BulkUploadState::PathInput {
                    path,
                    loading: false,
                } => Some((path, false)),
                _ => None,
            };
        }
        if let Some(state) = &mut self.settings {
            return match state.focused_field {
                0 => Some((&mut state.team, true)),
                1 => Some((&mut state.epics, false)),
                _ => None,
            };
        }
        if let Some(state) = &mut self.filter_edit {
            return Some((
                if state.focused_field == 0 {
                    &mut state.name
                } else {
                    &mut state.jql
                },
                false,
            ));
        }
        if let Some(state) = &mut self.create_ticket {
            return match state.focused_field {
                1 => Some((&mut state.summary, false)),
                4 => Some((&mut state.labels, false)),
                5 => Some((&mut state.description, true)),
                _ => None,
            };
        }
        if let Some(state) = &mut self.comment_state {
            return Some((&mut state.body, true));
        }
        if let Some(state) = &mut self.edit_state {
            return match state.focused_field {
                0 => Some((&mut state.summary, false)),
                1 => Some((&mut state.labels, false)),
                _ => Some((&mut state.description, true)),
            };
        }
        None
    }

    pub fn focused_field(&self) -> Option<usize> {
        if let Some(state) = &self.settings {
            Some(state.focused_field)
        } else if let Some(state) = &self.filter_edit {
            Some(state.focused_field)
        } else if let Some(state) = &self.create_ticket {
            Some(state.focused_field)
        } else if let Some(state) = &self.edit_state {
            Some(state.focused_field)
        } else if self.assign_state.is_some()
            || self.comment_state.is_some()
            || self.bulk_upload_state.is_some()
        {
            Some(0)
        } else {
            None
        }
    }

    /// Orders statuses and decides which are done from the configured `active` and `done` lists.
    pub fn set_status_rules(&mut self, statuses: &crate::config::StatusConfig) {
        let rules = crate::cache::StatusRules::new(&statuses.active, &statuses.done);
        if self.status_rules != rules {
            self.status_rules = rules;
            self.mark_cache_changed();
            self.clamp_selection();
        }
    }

    pub fn status_rules(&self) -> &crate::cache::StatusRules {
        &self.status_rules
    }

    pub fn set_epics_i_care_about(&mut self, epics: Vec<String>) {
        self.ensure_visible_keys_cache();
        let mut rank = HashMap::new();
        for key in epics {
            let normalized = key.trim().to_ascii_uppercase();
            if normalized.is_empty() {
                continue;
            }
            let idx = rank.len();
            rank.entry(normalized).or_insert(idx);
        }

        if self.epics_i_care_about_rank != rank {
            self.epics_i_care_about_rank = rank;
            self.mark_cache_changed();
            self.clamp_selection();
        }
    }

    /// Replaces the cache with a Jira read requested at `requested_at` (see `MoveTracker::now`).
    pub fn replace_cache(&mut self, cache: Cache, requested_at: u64) {
        self.ensure_visible_keys_cache();
        self.cache = cache;
        self.reapply_moves_since(requested_at);
        self.mark_cache_changed();
    }

    pub fn replace_epics(&mut self, epics: Vec<crate::cache::Epic>, requested_at: u64) {
        self.ensure_visible_keys_cache();
        crate::jira_client::attach_epics_to_tickets(
            &mut self.cache.my_tickets,
            &mut self.cache.team_tickets,
            &epics,
        );
        self.cache.epics = epics;
        self.reapply_moves_since(requested_at);
        self.mark_cache_changed();
        self.clamp_selection();
    }

    /// A new id for a background request, so its answer can be matched to what's on screen.
    pub fn next_request_id(&mut self) -> u64 {
        self.last_request_id += 1;
        self.last_request_id
    }

    pub fn mark_cache_changed(&mut self) {
        // ponytail: O(n) row scan per update; batch restoration if large-board hydration lags.
        // Keep the old visible rows long enough to identify the selected occurrence.
        let position = {
            let cache = self.visible_keys_cache.get_mut();
            cache
                .state
                .as_ref()
                .filter(|state| state.active_tab == self.active_tab)
                .and_then(|_| cache.items.get(self.selected_index).cloned())
                .map(|selected| (selected, Self::group_at(&cache.items, self.selected_index)))
        };
        self.view_generation = self.view_generation.wrapping_add(1);
        let cache = self.visible_keys_cache.get_mut();
        cache.state = None;
        cache.items.clear();
        cache.group_ticket_keys.clear();
        if let Some((selected, group)) = position {
            self.restore_position(Some(&selected), group.as_deref(), self.selected_index);
        }
    }

    pub fn next_tab(&mut self) {
        self.switch_tab(self.active_tab.next());
    }

    fn group_at(items: &[VisibleItem], index: usize) -> Option<String> {
        items
            .iter()
            .take(index.saturating_add(1))
            .rev()
            .find_map(|item| match item {
                VisibleItem::GroupHeader(id) => Some(id.clone()),
                _ => None,
            })
    }

    fn restore_position(
        &mut self,
        selected: Option<&VisibleItem>,
        group: Option<&str>,
        index: usize,
    ) {
        self.ensure_visible_keys_cache();
        let cache = self.visible_keys_cache.borrow();
        let mut current_group = None;
        let mut found = None;
        for (i, item) in cache.items.iter().enumerate() {
            if let VisibleItem::GroupHeader(id) = item {
                current_group = Some(id.as_str());
            }
            if Some(item) == selected && current_group == group {
                found = Some(i);
                break;
            }
        }
        self.selected_index = found
            .or_else(|| cache.items.iter().position(|item| Some(item) == selected))
            .unwrap_or(index.min(cache.items.len().saturating_sub(1)));
    }

    pub fn switch_tab(&mut self, tab: Tab) {
        if tab == self.active_tab {
            return;
        }
        self.ensure_visible_keys_cache();
        let cache = self.visible_keys_cache.borrow();
        self.tab_positions[self.active_tab.index()] = ViewPosition {
            selected: cache.items.get(self.selected_index).cloned(),
            group: Self::group_at(&cache.items, self.selected_index),
            index: self.selected_index,
            search: self.search.clone(),
            status_focus: self.status_focus.clone(),
        };
        drop(cache);
        self.active_tab = tab;
        let position = self.tab_positions[tab.index()].clone();
        self.search = position.search;
        self.status_focus = position.status_focus;
        self.restore_position(
            position.selected.as_ref(),
            position.group.as_deref(),
            position.index,
        );
        self.clamp_selection();
    }

    pub fn open_detail(&mut self, key: String) {
        self.detail_ticket_key = Some(key);
        self.detail_epic_key = None;
        self.detail_mode = DetailMode::View;
        self.detail_scroll = 0;
    }

    pub fn open_epic_detail(&mut self, key: String) {
        self.detail_epic_key = Some(key);
        self.detail_ticket_key = None;
        self.detail_mode = DetailMode::View;
        self.detail_scroll = 0;
    }

    pub fn close_detail(&mut self) {
        self.detail_ticket_key = None;
        self.detail_epic_key = None;
        self.detail_mode = DetailMode::View;
        self.detail_scroll = 0;
    }

    pub fn is_detail_open(&self) -> bool {
        self.detail_ticket_key.is_some() || self.detail_epic_key.is_some()
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
            if self.status_rules.is_done(&ticket.status) {
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

    fn compute_visible_items_for_tab(&self, tab: Tab) -> Vec<VisibleItem> {
        match tab {
            Tab::MyWork => Self::group_items(self.my_work_visible_by_status()),
            Tab::Team => Self::group_items(self.team_visible_tickets_by_member()),
            Tab::Epics => Self::group_items(self.epics_visible_epics()),
            Tab::Unassigned => Self::group_items(self.unassigned_visible_by_epic()),
            Tab::Filters => Self::group_items(self.filters_visible_by_status()),
        }
    }

    fn index_groups<'a, H>(
        &self,
        tab: Tab,
        groups: impl IntoIterator<Item = (String, H, Vec<&'a crate::cache::Ticket>)>,
    ) -> Vec<VisibleGroup<'a, H>> {
        let mut next_index = 0;
        groups
            .into_iter()
            .map(|(id, header, tickets)| {
                let index = next_index;
                next_index += 1;
                let total = tickets.len();
                let tickets = (!self.is_collapsed(tab, &id)).then(|| {
                    tickets
                        .into_iter()
                        .map(|ticket| {
                            let index = next_index;
                            next_index += 1;
                            (index, ticket)
                        })
                        .collect()
                });
                VisibleGroup {
                    id,
                    header,
                    index,
                    total,
                    tickets,
                }
            })
            .collect()
    }

    fn group_items<H>(groups: Vec<VisibleGroup<'_, H>>) -> Vec<VisibleItem> {
        groups
            .into_iter()
            .flat_map(|group| {
                std::iter::once(VisibleItem::GroupHeader(group.id)).chain(
                    group
                        .tickets
                        .into_iter()
                        .flatten()
                        .map(|(_, ticket)| VisibleItem::Ticket(ticket.key.clone())),
                )
            })
            .collect()
    }

    fn ensure_visible_keys_cache(&self) {
        let state = self.visible_keys_state();
        {
            let cache = self.visible_keys_cache.borrow();
            if cache.state.as_ref() == Some(&state) {
                return;
            }
        }

        let items = self.compute_visible_items_for_tab(state.active_tab);
        let group_ticket_keys = Self::build_group_ticket_keys(&items);
        let mut cache = self.visible_keys_cache.borrow_mut();
        cache.state = Some(state);
        cache.items = items;
        cache.group_ticket_keys = group_ticket_keys;
    }

    fn build_group_ticket_keys(items: &[VisibleItem]) -> HashMap<String, Vec<String>> {
        let mut group_ticket_keys: HashMap<String, Vec<String>> = HashMap::new();
        let mut current_group: Option<String> = None;

        for item in items {
            match item {
                VisibleItem::GroupHeader(group_id) => {
                    current_group = Some(group_id.clone());
                    group_ticket_keys.entry(group_id.clone()).or_default();
                }
                VisibleItem::Ticket(key) => {
                    if let Some(group_id) = current_group.as_ref() {
                        if let Some(keys) = group_ticket_keys.get_mut(group_id) {
                            keys.push(key.clone());
                        }
                    }
                }
            }
        }

        group_ticket_keys
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

    fn epic_is_in_focus(&self, epic_key: &str) -> bool {
        if self.epics_i_care_about_rank.is_empty() {
            return true;
        }
        self.epics_i_care_about_rank
            .contains_key(&epic_key.trim().to_ascii_uppercase())
    }

    fn epic_focus_rank(&self, epic_key: &str) -> usize {
        self.epics_i_care_about_rank
            .get(&epic_key.trim().to_ascii_uppercase())
            .copied()
            .unwrap_or(usize::MAX)
    }

    /// Epics and visible child rows in the exact order used by the Epics tab.
    pub(crate) fn epics_visible_epics(&self) -> Vec<VisibleGroup<'_, &crate::cache::Epic>> {
        let search = self.normalized_search();
        let mut visible = Vec::new();
        let mut epics: Vec<_> = self
            .cache
            .epics
            .iter()
            .filter(|epic| self.epic_is_in_focus(&epic.key))
            .collect();

        if !self.epics_i_care_about_rank.is_empty() {
            epics.sort_by(|a, b| {
                self.epic_focus_rank(&a.key)
                    .cmp(&self.epic_focus_rank(&b.key))
                    .then_with(|| a.key.cmp(&b.key))
            });
        }

        for epic in epics {
            match &search {
                Some(s) => {
                    let epic_matches = Self::contains_case_insensitive(&epic.key, s)
                        || Self::contains_case_insensitive(&epic.summary, s);
                    if epic_matches {
                        let mut children: Vec<_> = epic.children.iter().collect();
                        self.status_rules.sort_tickets(&mut children);
                        visible.push((epic, children));
                        continue;
                    }

                    let mut matching_children: Vec<_> = epic
                        .children
                        .iter()
                        .filter(|t| Self::ticket_matches_search(t, s))
                        .collect();
                    self.status_rules.sort_tickets(&mut matching_children);

                    if !matching_children.is_empty() {
                        visible.push((epic, matching_children));
                    }
                }
                None => {
                    let mut children: Vec<_> = epic.children.iter().collect();
                    self.status_rules.sort_tickets(&mut children);
                    visible.push((epic, children));
                }
            }
        }

        self.index_groups(
            Tab::Epics,
            visible
                .into_iter()
                .map(|(epic, tickets)| (epic.key.clone(), epic, tickets)),
        )
    }

    /// Unassigned tickets grouped by epic.
    pub(crate) fn unassigned_visible_by_epic(&self) -> Vec<VisibleGroup<'_, (String, String)>> {
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
            grouped
                .entry((epic_key, epic_summary))
                .or_default()
                .push(ticket);
        }

        let mut groups: Vec<_> = grouped
            .into_iter()
            .map(|((epic_key, epic_summary), mut tickets)| {
                self.status_rules.sort_tickets(&mut tickets);
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

        self.index_groups(
            Tab::Unassigned,
            visible
                .into_iter()
                .map(|(key, summary, tickets)| (key.clone(), (key, summary), tickets)),
        )
    }

    pub(crate) fn filters_visible_by_status(&self) -> Vec<VisibleGroup<'_, String>> {
        self.index_groups(
            Tab::Filters,
            self.status_rules
                .group(&self.filter_results)
                .into_iter()
                .map(|(status, tickets)| (status.clone(), status, tickets)),
        )
    }

    /// Whether a status passes the My Work/Team toggles: `show_done` governs done statuses,
    /// and `status_focus` (when set) governs every other status.
    fn status_visible(&self, status: &str) -> bool {
        if self.status_rules.is_done(status) {
            self.show_done
        } else {
            self.status_focus
                .as_deref()
                .is_none_or(|focus| focus == status)
        }
    }

    /// Status groups and visible tickets in the exact order used by the My Work tab.
    pub(crate) fn my_work_visible_by_status(&self) -> Vec<VisibleGroup<'_, String>> {
        self.index_groups(
            Tab::MyWork,
            self.my_work_by_status(|status| self.status_visible(status))
                .into_iter()
                .map(|(status, tickets)| (status.clone(), status, tickets)),
        )
    }

    /// My Work's status groups for the current search, keeping the statuses `shown` accepts.
    fn my_work_by_status(
        &self,
        shown: impl Fn(&str) -> bool,
    ) -> Vec<(String, Vec<&crate::cache::Ticket>)> {
        let search = self.normalized_search();
        self.status_rules
            .group(self.cache.my_tickets.iter().filter(|ticket| {
                shown(&ticket.status)
                    && search
                        .as_deref()
                        .is_none_or(|s| Self::ticket_matches_search(ticket, s))
            }))
    }

    /// Team members and visible tickets in the exact order used by the Team tab.
    /// Returns active tickets first, then Done tickets as a secondary group.
    pub(crate) fn team_visible_tickets_by_member(
        &self,
    ) -> Vec<VisibleGroup<'_, (&crate::cache::TeamMember, usize)>> {
        self.index_groups(
            Tab::Team,
            self.team_tickets_by_member(|status| self.status_visible(status))
                .into_iter()
                .map(|(member, active, done)| {
                    let active_count = active.len();
                    (
                        member.email.clone(),
                        (member, active_count),
                        active.into_iter().chain(done).collect(),
                    )
                }),
        )
    }

    /// The Team tab's members and tickets for the current search, keeping the statuses
    /// `shown` accepts.
    fn team_tickets_by_member(&self, shown: impl Fn(&str) -> bool) -> Vec<TeamMemberTickets<'_>> {
        let search = self.normalized_search();
        let search = search.as_deref();
        let has_search = search.is_some();
        let mut visible = Vec::new();
        let mut tickets_by_email: HashMap<&str, Vec<&crate::cache::Ticket>> = HashMap::new();
        for ticket in &self.cache.team_tickets {
            if let Some(email) = ticket.assignee_email.as_deref() {
                tickets_by_email.entry(email).or_default().push(ticket);
            }
        }

        for member in self.sorted_team_members() {
            if member.email == UNASSIGNED_TEAM_EMAIL {
                continue;
            }
            let member_tickets = tickets_by_email
                .get(member.email.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            let member_match = search.is_some_and(|s| {
                Self::contains_case_insensitive(&member.name, s)
                    || Self::contains_case_insensitive(&member.email, s)
            });
            let mut any_match = !has_search;
            let mut active = Vec::new();
            let mut done = Vec::new();
            for ticket in member_tickets.iter().copied() {
                let matches_search = match search {
                    Some(s) => member_match || Self::ticket_matches_search(ticket, s),
                    None => true,
                };
                if !matches_search {
                    continue;
                }
                any_match = true;
                if !shown(&ticket.status) {
                    continue;
                }
                if self.status_rules.is_done(&ticket.status) {
                    done.push(ticket);
                } else {
                    active.push(ticket);
                }
            }

            if has_search && !any_match {
                continue;
            }

            visible.push((member, active, done));
        }

        visible
    }

    pub fn toggle_show_done(&mut self) {
        self.show_done = !self.show_done;
        self.clamp_selection();
    }

    pub fn toggle_keybindings(&mut self) {
        self.show_keybindings = !self.show_keybindings;
        self.keybindings_scroll = 0;
    }

    pub fn close_keybindings(&mut self) {
        self.show_keybindings = false;
    }

    pub fn is_create_ticket_open(&self) -> bool {
        self.create_ticket.is_some()
    }

    pub fn is_bulk_open(&self) -> bool {
        self.bulk_state.is_some()
    }

    pub fn is_bulk_upload_open(&self) -> bool {
        self.bulk_upload_state.is_some()
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

    pub fn is_collapsed(&self, tab: Tab, group_id: &str) -> bool {
        match tab {
            Tab::MyWork => self.collapsed_my_work.contains(group_id),
            Tab::Team => self.collapsed_team.contains(group_id),
            Tab::Epics => self.collapsed_epics.contains(group_id),
            Tab::Unassigned => self.collapsed_unassigned.contains(group_id),
            Tab::Filters => self.collapsed_filters.contains(group_id),
        }
    }

    /// Get the group ID for the currently selected item (whether header or ticket).
    pub fn selected_group_id(&self) -> Option<String> {
        self.ensure_visible_keys_cache();
        let cache = self.visible_keys_cache.borrow();
        let items = &cache.items;
        if items.is_empty() || self.selected_index >= items.len() {
            return None;
        }
        // If on a header, return its group ID directly.
        if let VisibleItem::GroupHeader(ref id) = items[self.selected_index] {
            return Some(id.clone());
        }
        // Walk backwards to find the nearest header.
        for i in (0..self.selected_index).rev() {
            if let VisibleItem::GroupHeader(ref id) = items[i] {
                return Some(id.clone());
            }
        }
        None
    }

    fn visible_ticket_keys_in_group(&self, group_id: &str) -> Vec<String> {
        self.ensure_visible_keys_cache();
        let cache = self.visible_keys_cache.borrow();
        cache
            .group_ticket_keys
            .get(group_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn toggle_selection_at_cursor(&mut self) {
        match self.selected_item() {
            Some(VisibleItem::Ticket(key)) => {
                if !self.selected_ticket_keys.remove(&key) {
                    self.selected_ticket_keys.insert(key);
                }
            }
            Some(VisibleItem::GroupHeader(group_id)) => self.toggle_group_selection(&group_id),
            None => {}
        }
    }

    pub fn toggle_group_selection(&mut self, group_id: &str) {
        let keys = self.visible_ticket_keys_in_group(group_id);
        if keys.is_empty() {
            return;
        }
        let all_selected = keys.iter().all(|k| self.selected_ticket_keys.contains(k));
        if all_selected {
            for key in keys {
                self.selected_ticket_keys.remove(&key);
            }
        } else {
            for key in keys {
                self.selected_ticket_keys.insert(key);
            }
        }
    }

    pub fn select_all_visible_tickets(&mut self) {
        self.ensure_visible_keys_cache();
        let cache = self.visible_keys_cache.borrow();
        for item in &cache.items {
            if let VisibleItem::Ticket(key) = item {
                self.selected_ticket_keys.insert(key.clone());
            }
        }
    }

    pub fn clear_selected_tickets(&mut self) {
        self.selected_ticket_keys.clear();
    }

    pub fn selected_visible_ticket_keys_in_order(&self) -> Vec<String> {
        self.ensure_visible_keys_cache();
        let cache = self.visible_keys_cache.borrow();
        cache
            .items
            .iter()
            .filter_map(|item| match item {
                VisibleItem::Ticket(key) if self.selected_ticket_keys.contains(key) => {
                    Some(key.clone())
                }
                _ => None,
            })
            .collect()
    }

    pub fn selected_ticket_count(&self) -> usize {
        self.selected_ticket_keys.len()
    }

    pub fn is_ticket_selected(&self, key: &str) -> bool {
        self.selected_ticket_keys.contains(key)
    }

    pub fn group_selection_state(&self, group_id: &str) -> GroupSelectionState {
        self.ensure_visible_keys_cache();
        let cache = self.visible_keys_cache.borrow();
        let Some(keys) = cache.group_ticket_keys.get(group_id) else {
            return GroupSelectionState::None;
        };
        let selected = keys
            .iter()
            .filter(|k| self.selected_ticket_keys.contains(*k))
            .count();
        if selected == 0 {
            GroupSelectionState::None
        } else if selected == keys.len() {
            GroupSelectionState::All
        } else {
            GroupSelectionState::Partial
        }
    }

    pub fn toggle_group_collapse(&mut self, group_id: &str) {
        let set = match self.active_tab {
            Tab::MyWork => &mut self.collapsed_my_work,
            Tab::Team => &mut self.collapsed_team,
            Tab::Epics => &mut self.collapsed_epics,
            Tab::Unassigned => &mut self.collapsed_unassigned,
            Tab::Filters => &mut self.collapsed_filters,
        };
        let collapsing = !set.remove(group_id);
        if collapsing {
            set.insert(group_id.to_string());
        }
        self.mark_cache_changed();
        if collapsing {
            // Move selection to the group header
            self.ensure_visible_keys_cache();
            let cache = self.visible_keys_cache.borrow();
            if let Some(pos) = cache
                .items
                .iter()
                .position(|item| matches!(item, VisibleItem::GroupHeader(ref id) if id == group_id))
            {
                drop(cache);
                self.selected_index = pos;
            }
        }
        self.clamp_selection();
    }

    pub fn toggle_all_groups_collapse(&mut self) {
        let current_group = self.selected_group_id();
        let (set, all_ids) = match self.active_tab {
            Tab::MyWork => {
                let ids: Vec<String> = self
                    .my_work_visible_by_status()
                    .iter()
                    .map(|group| group.id.clone())
                    .collect();
                (&mut self.collapsed_my_work, ids)
            }
            Tab::Team => {
                let ids: Vec<String> = self
                    .sorted_team_members()
                    .iter()
                    .filter(|m| m.email != "__unassigned__")
                    .map(|m| m.email.clone())
                    .collect();
                (&mut self.collapsed_team, ids)
            }
            Tab::Epics => {
                let ids: Vec<String> = self.cache.epics.iter().map(|e| e.key.clone()).collect();
                (&mut self.collapsed_epics, ids)
            }
            Tab::Unassigned => {
                let ids: Vec<String> = self
                    .unassigned_visible_by_epic()
                    .iter()
                    .map(|group| group.id.clone())
                    .collect();
                (&mut self.collapsed_unassigned, ids)
            }
            Tab::Filters => {
                let ids: Vec<String> = self
                    .filters_visible_by_status()
                    .iter()
                    .map(|group| group.header.clone())
                    .collect();
                (&mut self.collapsed_filters, ids)
            }
        };
        if set.is_empty() {
            // Collapse all except the current group
            for id in &all_ids {
                if current_group.as_deref() != Some(id) {
                    set.insert(id.clone());
                }
            }
        } else {
            set.clear();
        }
        self.mark_cache_changed();
        self.clamp_selection();
    }

    pub fn detail_fetch_error(&self, key: &str) -> Option<&str> {
        self.detail_fetch_errors.get(key).map(String::as_str)
    }

    pub fn fail_detail_fetch(&mut self, key: &str, error: String) {
        self.detail_fetch_errors.insert(key.to_string(), error);
    }

    pub fn begin_detail_fetch(&mut self, key: &str) -> bool {
        self.detail_fetch_errors.remove(key);
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

    /// The statuses `cycle_status_focus` steps through: every status with tickets in the
    /// My Work or Team tab for the current search, whatever the focus, in display order.
    /// Done statuses are left out because `d` shows and hides them. Other tabs have none.
    pub fn focusable_statuses(&self) -> Vec<String> {
        let groups = match self.active_tab {
            Tab::MyWork => self.my_work_by_status(|_| true),
            Tab::Team => {
                let members = self.team_tickets_by_member(|_| true);
                self.status_rules.group(
                    members
                        .iter()
                        .flat_map(|(_, active, done)| active.iter().chain(done).copied()),
                )
            }
            _ => Vec::new(),
        };
        groups
            .into_iter()
            .map(|(status, _)| status)
            .filter(|status| !self.status_rules.is_done(status))
            .collect()
    }

    /// Focuses the next focusable status (`forward`) or the previous one. Stepping past
    /// either end clears the focus, so the cycle includes "all".
    pub fn cycle_status_focus(&mut self, forward: bool) {
        let statuses = self.focusable_statuses();
        let current = self
            .status_focus
            .as_ref()
            .and_then(|focus| statuses.iter().position(|status| status == focus));
        let next = match (current, forward) {
            (None, true) => statuses.first(),
            (None, false) => statuses.last(),
            (Some(i), true) => statuses.get(i + 1),
            (Some(i), false) => i.checked_sub(1).and_then(|i| statuses.get(i)),
        };
        self.status_focus = next.cloned();
        self.clamp_selection();
    }

    /// Describes the focus for the status bar flash, e.g. "Focus: On Deck (2 of 5)".
    pub fn status_focus_message(&self) -> String {
        let statuses = self.focusable_statuses();
        match &self.status_focus {
            Some(focus) => match statuses.iter().position(|status| status == focus) {
                Some(i) => format!(
                    "Focus: {} ({} of {})",
                    focus.as_str(),
                    i + 1,
                    statuses.len()
                ),
                None => format!("Focus: {}", focus.as_str()),
            },
            None if statuses.is_empty() => "Focus: all (no statuses to focus)".to_string(),
            None => "Focus: all".to_string(),
        }
    }

    /// Get the currently selected item (header or ticket).
    pub fn selected_item(&self) -> Option<VisibleItem> {
        self.ensure_visible_keys_cache();
        self.visible_keys_cache
            .borrow()
            .items
            .get(self.selected_index)
            .cloned()
    }

    /// Get the currently selected ticket key, or None if a header is selected.
    pub fn selected_ticket_key(&self) -> Option<String> {
        match self.selected_item() {
            Some(VisibleItem::Ticket(key)) => Some(key),
            _ => None,
        }
    }

    /// Get the group ID if a header is currently selected.
    pub fn selected_header_group_id(&self) -> Option<String> {
        match self.selected_item() {
            Some(VisibleItem::GroupHeader(id)) => Some(id),
            _ => None,
        }
    }

    pub fn prune_selection_to_visible(&mut self) {
        self.ensure_visible_keys_cache();
        let visible: HashSet<String> = self
            .visible_keys_cache
            .borrow()
            .items
            .iter()
            .filter_map(|item| match item {
                VisibleItem::Ticket(key) => Some(key.clone()),
                _ => None,
            })
            .collect();
        self.selected_ticket_keys.retain(|k| visible.contains(k));
    }

    /// Total number of selectable items (headers + tickets) in the current tab.
    pub fn item_count(&self) -> usize {
        self.ensure_visible_keys_cache();
        self.visible_keys_cache.borrow().items.len()
    }

    pub fn clamp_selection(&mut self) {
        self.prune_selection_to_visible();
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

    /// Scrolls the detail body by `lines`, keeping the last line at the bottom.
    pub fn scroll_detail_by(&mut self, lines: i32) {
        let max = i32::from(self.detail_scroll_max.get());
        self.detail_scroll = (i32::from(self.detail_scroll) + lines).clamp(0, max) as u16;
    }

    pub fn scroll_detail_down(&mut self) {
        self.scroll_detail_by(1);
    }

    pub fn scroll_detail_up(&mut self) {
        self.scroll_detail_by(-1);
    }

    /// Scrolls by a page, keeping two lines of context.
    pub fn scroll_detail_page(&mut self, down: bool) {
        let page = i32::from(self.detail_page_height.get())
            .saturating_sub(2)
            .max(1);
        self.scroll_detail_by(if down { page } else { -page });
    }

    pub fn scroll_detail_to(&mut self, bottom: bool) {
        self.detail_scroll = if bottom {
            self.detail_scroll_max.get()
        } else {
            0
        };
    }

    /// The detail overlay's position among the items `step_detail` moves
    /// through, as (1-based index, count).
    pub fn detail_position(&self) -> Option<(usize, usize)> {
        let keys = self.detail_step_keys();
        let current = self
            .detail_ticket_key
            .as_ref()
            .or(self.detail_epic_key.as_ref())?;
        let index = keys.iter().position(|(_, k)| k == current)?;
        Some((index + 1, keys.len()))
    }

    /// The list rows the detail overlay steps through, with their indexes:
    /// tickets for a ticket detail, epic headers for an epic detail.
    fn detail_step_keys(&self) -> Vec<(usize, String)> {
        let epics = self.detail_epic_key.is_some();
        if epics && self.active_tab != Tab::Epics {
            return Vec::new();
        }
        self.ensure_visible_keys_cache();
        self.visible_keys_cache
            .borrow()
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| match item {
                VisibleItem::Ticket(key) if !epics => Some((i, key.clone())),
                VisibleItem::GroupHeader(id) if epics => Some((i, id.clone())),
                _ => None,
            })
            .collect()
    }

    /// Shows the next (or previous) ticket's detail, or epic's when an epic
    /// is shown, and selects its row. Returns the ticket's key when its detail
    /// still needs fetching.
    pub fn step_detail(&mut self, forward: bool) -> Option<String> {
        let keys = self.detail_step_keys();
        let target = if forward {
            keys.iter().find(|(i, _)| *i > self.selected_index)
        } else {
            keys.iter().rev().find(|(i, _)| *i < self.selected_index)
        };
        let (index, key) = target?.clone();
        self.selected_index = index;
        if self.detail_epic_key.is_some() {
            self.open_epic_detail(key);
            None
        } else {
            self.open_detail(key.clone());
            let needs_fetch = !self.is_ticket_detail_loaded(&key) && self.begin_detail_fetch(&key);
            needs_fetch.then_some(key)
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
            .or_else(|| self.filter_results.iter().find(|t| t.key == key))
    }

    /// Applies `update` to every cached copy of `key`: My Work, Team, epic children and
    /// filter results.
    fn update_ticket(&mut self, key: &str, mut update: impl FnMut(&mut crate::cache::Ticket)) {
        self.ensure_visible_keys_cache();
        let copies = self
            .cache
            .my_tickets
            .iter_mut()
            .chain(self.cache.team_tickets.iter_mut())
            .chain(
                self.cache
                    .epics
                    .iter_mut()
                    .flat_map(|e| e.children.iter_mut()),
            )
            .chain(self.filter_results.iter_mut())
            .filter(|ticket| ticket.key == key);
        let mut changed = false;
        for ticket in copies {
            update(ticket);
            changed = true;
        }
        if changed {
            self.mark_cache_changed();
        }
    }

    /// Enrich a cached ticket with a detail read (description, accurate status/assignee)
    /// requested at `requested_at`. Returns false, changing nothing, if the read predates the
    /// ticket's latest confirmed move.
    pub fn enrich_ticket(
        &mut self,
        key: &str,
        requested_at: u64,
        detail: &crate::cache::Ticket,
    ) -> bool {
        if self.moves.is_stale(key, requested_at) {
            return false;
        }
        self.update_ticket(key, |ticket| {
            ticket.status = detail.status.clone();
            if detail.assignee.is_some() {
                ticket.assignee = detail.assignee.clone();
            }
            if detail.assignee_email.is_some() {
                ticket.assignee_email = detail.assignee_email.clone();
            }
            if detail.reporter.is_some() {
                ticket.reporter = detail.reporter.clone();
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
        });
        true
    }

    /// Set a ticket's status in the cache from Jira's name for it.
    pub fn update_ticket_status(&mut self, key: &str, status_name: &str) {
        self.update_ticket(key, |ticket| ticket.status = status_name.to_string());
    }

    /// Update a ticket's assignee in the cache.
    pub fn update_ticket_assignee(&mut self, key: &str, name: &str, email: &str) {
        self.update_ticket(key, |ticket| {
            ticket.assignee = Some(name.to_string());
            ticket.assignee_email = Some(email.to_string());
        });
    }

    /// Update the editable fields in every cached copy of a ticket.
    pub fn update_ticket_fields(&mut self, key: &str, summary: &str, labels: &[String]) {
        self.update_ticket(key, |ticket| {
            ticket.summary = summary.to_string();
            ticket.labels = labels.to_vec();
        });
    }

    pub fn update_ticket_description(&mut self, key: &str, description: &str) {
        self.update_ticket(key, |ticket| {
            ticket.description = Some(description.to_string())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{App, GroupSelectionState, Tab};
    use crate::cache::{Epic, Ticket};

    #[test]
    fn refresh_and_tab_changes_keep_ticket_identity_and_folds() {
        let mut app = App::new();
        app.cache.my_tickets = vec![
            Ticket::for_test("DEMO-2", "In Progress"),
            Ticket::for_test("DEMO-3", "In Progress"),
            Ticket::for_test("DEMO-9", "Blocked"),
        ];
        app.selected_index = 2;
        let mut refreshed = app.cache.clone();
        refreshed
            .my_tickets
            .insert(0, Ticket::for_test("DEMO-1", "In Progress"));
        app.replace_cache(refreshed, app.moves.now());
        assert_eq!(app.selected_ticket_key().as_deref(), Some("DEMO-3"));
        assert_eq!(app.selected_index, 3);
        app.toggle_group_collapse("Blocked");
        app.selected_index = 3;
        app.switch_tab(Tab::Epics);
        app.switch_tab(Tab::MyWork);
        assert_eq!(app.selected_ticket_key().as_deref(), Some("DEMO-3"));
        assert!(app.is_collapsed(Tab::MyWork, "Blocked"));
        app.cache.my_tickets.remove(0);
        app.mark_cache_changed();
        assert_eq!(app.selected_ticket_key().as_deref(), Some("DEMO-3"));
        app.search = Some("DEMO-3".into());
        app.status_focus = Some("In Progress".into());
        app.selected_index = 1;
        app.switch_tab(Tab::Team);
        assert!(app.search.is_none());
        app.switch_tab(Tab::MyWork);
        assert_eq!(app.search.as_deref(), Some("DEMO-3"));
        assert_eq!(app.status_focus.as_deref(), Some("In Progress"));
        assert_eq!(app.selected_ticket_key().as_deref(), Some("DEMO-3"));
    }

    #[test]
    fn refresh_keeps_the_same_occurrence_of_a_ticket_shared_by_epics() {
        let mut app = App::new();
        app.active_tab = Tab::Epics;
        app.cache.epics = ["DEMO-100", "DEMO-200"]
            .into_iter()
            .map(|key| Epic {
                key: key.into(),
                summary: key.into(),
                children: vec![Ticket::for_test("DEMO-1", "In Progress")],
            })
            .collect();
        app.selected_index = 3;
        let mut refreshed = app.cache.clone();
        refreshed.epics[0]
            .children
            .push(Ticket::for_test("DEMO-2", "In Progress"));
        app.replace_cache(refreshed, app.moves.now());
        assert_eq!(app.selected_ticket_key().as_deref(), Some("DEMO-1"));
        assert_eq!(app.selected_group_id().as_deref(), Some("DEMO-200"));
        assert_eq!(app.selected_index, 4);
    }

    #[test]
    fn ticket_changes_reach_every_copy_and_refresh_visibility() {
        for filter_only in [false, true] {
            let mut app = App::new();
            let ticket = Ticket::for_test("DEMO-1", "To Do");
            app.filter_results = vec![ticket.clone()];
            if !filter_only {
                app.cache.my_tickets = vec![ticket.clone()];
                app.cache.team_tickets = vec![ticket.clone()];
                app.cache.epics = vec![Epic {
                    key: "DEMO-100".into(),
                    summary: "Epic".into(),
                    children: vec![ticket],
                }];
            }
            app.search = Some("updated".into());
            assert_eq!(app.item_count(), 0);

            app.update_ticket_assignee("DEMO-1", "Alex", "alex@example.com");
            app.update_ticket_fields("DEMO-1", "Updated summary", &["updated-label".into()]);

            assert_eq!(app.item_count(), if filter_only { 0 } else { 2 });
            for ticket in app
                .cache
                .my_tickets
                .iter()
                .chain(&app.cache.team_tickets)
                .chain(app.cache.epics.iter().flat_map(|epic| &epic.children))
                .chain(&app.filter_results)
            {
                assert_eq!(ticket.assignee.as_deref(), Some("Alex"));
                assert_eq!(ticket.assignee_email.as_deref(), Some("alex@example.com"));
                assert_eq!(ticket.summary, "Updated summary");
                assert_eq!(ticket.labels, ["updated-label"]);
            }
        }
    }

    fn ticket(key: &str, summary: &str) -> Ticket {
        Ticket {
            key: key.to_string(),
            summary: summary.to_string(),
            status: "To Do".to_string(),
            assignee: None,
            assignee_email: None,
            reporter: None,
            description: None,
            labels: Vec::new(),
            epic_key: None,
            epic_name: None,
            detail_loaded: false,
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

    /// Tickets from `(key, Jira status name)` pairs, parsed like real fetches.
    fn tickets_with_statuses(tickets: &[(&str, &str)]) -> Vec<Ticket> {
        tickets
            .iter()
            .map(|(key, status)| Ticket {
                status: status.to_string(),
                ..ticket(key, key)
            })
            .collect()
    }

    fn my_work_app(tickets: &[(&str, &str)]) -> App {
        let mut app = App::new();
        app.active_tab = Tab::MyWork;
        app.loading = false;
        app.cache.my_tickets = tickets_with_statuses(tickets);
        app
    }

    #[test]
    fn step_detail_moves_between_tickets_skipping_headers() {
        let mut app = my_work_app(&[("DSCI-1", "In Progress"), ("DSCI-2", "Backlog")]);
        fn row_of(app: &mut App, key: &str) -> usize {
            (0..app.item_count())
                .find(|&i| {
                    app.selected_index = i;
                    app.selected_ticket_key().as_deref() == Some(key)
                })
                .unwrap()
        }
        let first = row_of(&mut app, "DSCI-1");
        let second = row_of(&mut app, "DSCI-2");
        assert!(second > first + 1, "a status header sits between them");

        app.selected_index = first;
        app.open_detail("DSCI-1".to_string());
        assert_eq!(app.detail_position(), Some((1, 2)));

        // DSCI-2's detail isn't loaded, so it needs fetching.
        assert_eq!(app.step_detail(true), Some("DSCI-2".to_string()));
        assert_eq!(app.detail_ticket_key.as_deref(), Some("DSCI-2"));
        assert_eq!(app.selected_index, second);
        assert_eq!(app.detail_position(), Some((2, 2)));

        // Nothing after the last ticket.
        assert_eq!(app.step_detail(true), None);
        assert_eq!(app.detail_ticket_key.as_deref(), Some("DSCI-2"));

        assert_eq!(app.step_detail(false), Some("DSCI-1".to_string()));
        assert_eq!(app.selected_index, first);
        // Its fetch is already running.
        app.step_detail(true);
        assert_eq!(app.step_detail(false), None);
    }

    #[test]
    fn detail_scroll_stops_at_the_last_line() {
        let mut app = App::new();
        app.detail_scroll_max.set(5);
        app.detail_page_height.set(4);

        app.scroll_detail_page(true);
        assert_eq!(app.detail_scroll, 2);
        for _ in 0..10 {
            app.scroll_detail_down();
        }
        assert_eq!(app.detail_scroll, 5);
        app.scroll_detail_to(false);
        app.scroll_detail_up();
        assert_eq!(app.detail_scroll, 0);
        app.scroll_detail_to(true);
        assert_eq!(app.detail_scroll, 5);
    }

    #[test]
    fn detail_fetch_error_clears_on_retry() {
        let mut app = App::new();
        assert!(app.begin_detail_fetch("DSCI-1"));
        app.end_detail_fetch("DSCI-1");
        app.fail_detail_fetch("DSCI-1", "timed out".to_string());
        assert_eq!(app.detail_fetch_error("DSCI-1"), Some("timed out"));
        assert!(app.begin_detail_fetch("DSCI-1"));
        assert_eq!(app.detail_fetch_error("DSCI-1"), None);
    }

    fn my_work_group_names(app: &App) -> Vec<String> {
        app.my_work_visible_by_status()
            .iter()
            .map(|group| group.header.clone())
            .collect()
    }

    fn filters_app(tickets: Vec<Ticket>) -> App {
        let mut app = App::new();
        app.active_tab = Tab::Filters;
        app.loading = false;
        app.filter_results = tickets;
        app.mark_cache_changed();
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

        // 2 epic headers + 3 tickets
        assert_eq!(app.item_count(), 5);
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

        // Items: H(AMP-100), T(AMP-1), T(AMP-2), H(AMP-200), T(AMP-3)
        app.selected_index = 4;
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
        // Items: H(AMP-100), T(AMP-1), H(AMP-200), T(AMP-3)
        assert_eq!(app.item_count(), 4);

        app.selected_index = 3;
        assert_eq!(app.selected_ticket_key(), Some("AMP-3".to_string()));

        app.search = Some("auth".to_string());
        // Items: H(AMP-100), T(AMP-1), T(AMP-2)
        assert_eq!(app.item_count(), 3);
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

        // H(AMP-100), H(AMP-200), T(AMP-1)
        assert_eq!(app.item_count(), 3);

        app.search = Some("empty".to_string());
        // H(AMP-100) only, no children
        assert_eq!(app.item_count(), 1);
        assert_eq!(app.selected_ticket_key(), None);
    }

    #[test]
    fn my_work_search_matches_labels() {
        let mut app = App::new();
        app.active_tab = Tab::MyWork;
        app.loading = false;

        let mut t = ticket("AMP-1", "Refactor parser");
        t.status = "In Progress".to_string();
        t.labels = vec!["metis".to_string(), "backend".to_string()];
        app.cache.my_tickets = vec![t];

        app.search = Some("metis".to_string());
        // H(In Progress) + T(AMP-1)
        assert_eq!(app.item_count(), 2);
        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("AMP-1".to_string()));
    }

    #[test]
    fn my_work_groups_workflow_statuses_before_done() {
        let mut app = my_work_app(&[
            ("DSCI-2000", "Stalled"),
            ("DSCI-2478", "Backlog"),
            ("DSCI-3100", "In Progress"),
            ("DSCI-3240", "On Deck"),
            ("DSCI-3241", "On Deck"),
            ("DSCI-3300", "Done"),
        ]);

        // Configured statuses first, then unlisted ones in first-seen (key) order, then done.
        assert_eq!(
            my_work_group_names(&app),
            ["In Progress", "Stalled", "Backlog", "On Deck", "Done"]
        );
        // 5 headers + 6 tickets; the last workflow-status row sits just above Done.
        assert_eq!(app.item_count(), 11);
        app.selected_index = 8;
        assert_eq!(app.selected_ticket_key(), Some("DSCI-3241".to_string()));
        app.selected_index = 10;
        assert_eq!(app.selected_ticket_key(), Some("DSCI-3300".to_string()));
    }

    fn dsci_statuses() -> crate::config::StatusConfig {
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
        crate::config::StatusConfig {
            active: names(&[
                "Backlog",
                "On Deck",
                "In Progress",
                "In Team Review",
                "Stalled",
            ]),
            done: names(&["Done", "Closed", "Resolved"]),
        }
    }

    #[test]
    fn my_work_filters_and_epics_follow_the_configured_status_order() {
        let statuses = [
            ("DSCI-1", "Resolved"),
            ("DSCI-2", "Stalled"),
            ("DSCI-3", "In Team Review"),
            ("DSCI-4", "Backlog"),
            ("DSCI-5", "Blocked"),
            ("DSCI-6", "On Deck"),
        ];
        let expected = [
            "Backlog",
            "On Deck",
            "In Team Review",
            "Stalled",
            "Blocked",
            "Resolved",
        ];

        let mut app = my_work_app(&statuses);
        app.set_status_rules(&dsci_statuses());
        assert_eq!(my_work_group_names(&app), expected);

        let mut app = filters_app(tickets_with_statuses(&statuses));
        app.set_status_rules(&dsci_statuses());
        let filter_groups: Vec<_> = app
            .filters_visible_by_status()
            .iter()
            .map(|group| group.header.clone())
            .collect();
        assert_eq!(filter_groups, expected);

        let mut app = epics_app(vec![Epic {
            key: "DSCI-100".to_string(),
            summary: "Epic".to_string(),
            children: tickets_with_statuses(&statuses),
        }]);
        app.set_status_rules(&dsci_statuses());
        let groups = app.epics_visible_epics();
        let children = groups[0].tickets.as_ref().unwrap();
        let child_statuses: Vec<_> = children.iter().map(|(_, t)| t.status.as_str()).collect();
        assert_eq!(child_statuses, expected);
    }

    #[test]
    fn my_work_collapses_workflow_status_groups() {
        let mut app = my_work_app(&[
            ("DSCI-1", "In Progress"),
            ("DSCI-2", "On Deck"),
            ("DSCI-3", "On Deck"),
        ]);

        app.selected_index = 4; // T(DSCI-3)
        app.toggle_group_collapse("On Deck");
        assert_eq!(app.item_count(), 3);
        assert_eq!(app.selected_header_group_id(), Some("On Deck".to_string()));

        app.toggle_group_collapse("On Deck");
        app.selected_index = 0;
        app.toggle_all_groups_collapse();
        assert!(app.collapsed_my_work.contains("On Deck"));
        assert!(!app.collapsed_my_work.contains("In Progress"));
    }

    #[test]
    fn my_work_focus_done_and_search_apply_to_workflow_statuses() {
        let mut app = my_work_app(&[
            ("DSCI-1", "In Progress"),
            ("DSCI-2", "On Deck"),
            ("DSCI-3", "Done"),
        ]);

        app.cycle_status_focus(true);
        assert_eq!(app.status_focus, Some("In Progress".to_string()));
        assert_eq!(my_work_group_names(&app), ["In Progress", "Done"]);
        app.toggle_show_done();
        assert_eq!(my_work_group_names(&app), ["In Progress"]);
        app.cycle_status_focus(false);
        assert_eq!(app.status_focus, None);
        assert_eq!(my_work_group_names(&app), ["In Progress", "On Deck"]);
        app.search = Some("dsci-2".to_string());
        assert_eq!(my_work_group_names(&app), ["On Deck"]);
    }

    #[test]
    fn focus_cycles_through_the_workflow_statuses_shown_then_back_to_all() {
        let mut app = my_work_app(&[
            ("DSCI-1", "In Team Review"),
            ("DSCI-2", "On Deck"),
            ("DSCI-3", "Done"),
            ("DSCI-4", "Backlog"),
            ("DSCI-5", "On Deck"),
        ]);
        app.set_status_rules(&dsci_statuses());

        // Only statuses the tickets are in, in display order, and never a done one.
        assert_eq!(
            app.focusable_statuses(),
            ["Backlog", "On Deck", "In Team Review",]
        );

        app.cycle_status_focus(true);
        app.cycle_status_focus(true);
        assert_eq!(app.status_focus_message(), "Focus: On Deck (2 of 3)");
        assert_eq!(my_work_group_names(&app), ["On Deck", "Done"]);
        app.selected_index = 2;
        assert_eq!(app.selected_ticket_key(), Some("DSCI-5".to_string()));

        app.cycle_status_focus(true);
        assert_eq!(app.status_focus, Some("In Team Review".to_string()));
        app.cycle_status_focus(true);
        assert_eq!(app.status_focus, None);
        assert_eq!(app.status_focus_message(), "Focus: all");

        app.cycle_status_focus(false);
        assert_eq!(app.status_focus, Some("In Team Review".to_string()));
    }

    #[test]
    fn focus_skips_statuses_hidden_by_search_and_restarts_from_a_stale_focus() {
        let mut app = my_work_app(&[("DSCI-1", "In Progress"), ("DSCI-2", "On Deck")]);
        app.search = Some("dsci-2".to_string());
        assert_eq!(app.focusable_statuses(), ["On Deck"]);

        // A focused status that is no longer shown (moved away, say) restarts the cycle.
        app.status_focus = Some("Ready for Work".to_string());
        app.cycle_status_focus(true);
        assert_eq!(app.status_focus, Some("On Deck".to_string()));

        app.search = Some("nothing matches".to_string());
        app.cycle_status_focus(true);
        assert_eq!(app.status_focus, None);
        assert_eq!(
            app.status_focus_message(),
            "Focus: all (no statuses to focus)"
        );
    }

    #[test]
    fn team_focus_offers_every_member_status_and_other_tabs_offer_none() {
        let mut app = App::new();
        app.active_tab = Tab::Team;
        app.loading = false;
        app.cache.team_members = vec![
            crate::cache::TeamMember {
                name: "Dev".to_string(),
                email: "dev@example.com".to_string(),
            },
            crate::cache::TeamMember {
                name: "Ops".to_string(),
                email: "ops@example.com".to_string(),
            },
        ];
        app.cache.team_tickets = tickets_with_statuses(&[
            ("DSCI-1", "Stalled"),
            ("DSCI-2", "On Deck"),
            ("DSCI-3", "Closed"),
        ]);
        app.cache.team_tickets[0].assignee_email = Some("dev@example.com".to_string());
        app.cache.team_tickets[1].assignee_email = Some("ops@example.com".to_string());
        app.cache.team_tickets[2].assignee_email = Some("ops@example.com".to_string());
        app.set_status_rules(&dsci_statuses());

        assert_eq!(app.focusable_statuses(), ["On Deck", "Stalled"]);
        app.cycle_status_focus(true);
        // H(dev) + H(ops) + On Deck + the done Closed ticket
        assert_eq!(app.item_count(), 4);
        assert_eq!(app.selected_ticket_key(), None);
        app.selected_index = 2;
        assert_eq!(app.selected_ticket_key(), Some("DSCI-2".to_string()));

        app.active_tab = Tab::Epics;
        assert!(app.focusable_statuses().is_empty());
    }

    #[test]
    fn configured_done_statuses_are_done_everywhere() {
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
        let statuses = crate::config::StatusConfig {
            active: names(&["On Deck", "In Progress"]),
            done: names(&["Done", "Cancelled", "Won't Do", "Denied"]),
        };
        let tickets = [
            ("DSCI-1", "In Progress"),
            ("DSCI-2", "Cancelled"),
            ("DSCI-3", "Won't Do"),
            ("DSCI-4", "Denied"),
            ("DSCI-5", "On Deck"),
        ];

        // My Work: done groups come last, `d` hides them, and focus skips them.
        let mut app = my_work_app(&tickets);
        app.set_status_rules(&statuses);
        assert_eq!(
            my_work_group_names(&app),
            ["On Deck", "In Progress", "Cancelled", "Won't Do", "Denied"]
        );
        assert_eq!(app.focusable_statuses(), ["On Deck", "In Progress"]);
        app.toggle_show_done();
        assert_eq!(my_work_group_names(&app), ["On Deck", "In Progress"]);

        // Team: they're in the done split and don't count as active work.
        let mut app = App::new();
        app.active_tab = Tab::Team;
        app.loading = false;
        app.set_status_rules(&statuses);
        app.cache.team_members = vec![
            crate::cache::TeamMember {
                name: "Busy".to_string(),
                email: "busy@example.com".to_string(),
            },
            crate::cache::TeamMember {
                name: "Wrapped Up".to_string(),
                email: "done@example.com".to_string(),
            },
        ];
        app.cache.team_tickets = tickets_with_statuses(&tickets);
        for (i, t) in app.cache.team_tickets.iter_mut().enumerate() {
            let email = if i == 0 {
                "busy@example.com"
            } else {
                "done@example.com"
            };
            t.assignee_email = Some(email.to_string());
        }
        app.cache.team_tickets[4].assignee_email = Some("busy@example.com".to_string());
        let members: Vec<_> = app
            .sorted_team_members()
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(members, ["Busy", "Wrapped Up"]);
        let by_member = app.team_visible_tickets_by_member();
        assert_eq!(by_member[1].header.1, 0);
        assert_eq!(by_member[1].total, 3);

        // Epics: they count toward progress.
        let epic = Epic {
            key: "DSCI-100".to_string(),
            summary: "Epic".to_string(),
            children: tickets_with_statuses(&tickets),
        };
        assert_eq!(epic.done_count(app.status_rules()), 3);
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
        t.status = "Needs Triage".to_string();
        t.labels = vec!["infra".to_string()];
        t.assignee_email = Some("dev@example.com".to_string());
        app.cache.team_tickets = vec![t];

        app.search = Some("infra".to_string());
        // H(dev@example.com) + T(AMP-2)
        assert_eq!(app.item_count(), 2);
        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("AMP-2".to_string()));
    }

    #[test]
    fn team_focus_and_done_toggles_treat_workflow_statuses_as_active() {
        let mut app = App::new();
        app.active_tab = Tab::Team;
        app.loading = false;
        app.cache.team_members = vec![crate::cache::TeamMember {
            name: "Dev".to_string(),
            email: "dev@example.com".to_string(),
        }];
        app.cache.team_tickets = tickets_with_statuses(&[
            ("DSCI-1", "In Progress"),
            ("DSCI-2", "On Deck"),
            ("DSCI-3", "Done"),
        ]);
        for t in &mut app.cache.team_tickets {
            t.assignee_email = Some("dev@example.com".to_string());
        }

        // H(dev) + 3 tickets
        assert_eq!(app.item_count(), 4);
        app.cycle_status_focus(true);
        assert_eq!(app.status_focus, Some("In Progress".to_string()));
        // Focus hides On Deck but not Done.
        assert_eq!(app.item_count(), 3);
        app.toggle_show_done();
        assert_eq!(app.item_count(), 2);
        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("DSCI-1".to_string()));
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
        // H(AMP-500) + T(AMP-55)
        assert_eq!(app.item_count(), 2);
        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("AMP-55".to_string()));
    }

    #[test]
    fn epics_focus_filter_limits_epics_view() {
        let mut app = epics_app(vec![
            Epic {
                key: "AMP-100".to_string(),
                summary: "Auth".to_string(),
                children: vec![ticket("AMP-1", "Session")],
            },
            Epic {
                key: "AMP-200".to_string(),
                summary: "Perf".to_string(),
                children: vec![ticket("AMP-2", "Cache")],
            },
        ]);

        app.set_epics_i_care_about(vec!["amp-200".to_string()]);
        assert_eq!(app.item_count(), 2);
        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("AMP-2".to_string()));
    }

    #[test]
    fn epics_focus_filter_honors_config_order() {
        let app = {
            let mut app = epics_app(vec![
                Epic {
                    key: "AMP-100".to_string(),
                    summary: "Auth".to_string(),
                    children: vec![ticket("AMP-1", "Session")],
                },
                Epic {
                    key: "AMP-300".to_string(),
                    summary: "Perf".to_string(),
                    children: vec![ticket("AMP-3", "Cache")],
                },
                Epic {
                    key: "AMP-200".to_string(),
                    summary: "Runner".to_string(),
                    children: vec![ticket("AMP-2", "Task")],
                },
            ]);
            app.set_epics_i_care_about(vec![
                "AMP-300".to_string(),
                "AMP-100".to_string(),
                "AMP-200".to_string(),
            ]);
            app
        };

        let ordered_keys: Vec<_> = app
            .epics_visible_epics()
            .into_iter()
            .map(|group| group.header.key.as_str())
            .collect();
        assert_eq!(ordered_keys, vec!["AMP-300", "AMP-100", "AMP-200"]);
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

        // 2 epic headers + 3 tickets
        assert_eq!(app.item_count(), 5);
        // Verify ticket keys are reachable by selection
        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("AMP-91".to_string()));
        app.selected_index = 2;
        assert_eq!(app.selected_ticket_key(), Some("AMP-92".to_string()));
        app.selected_index = 4;
        assert_eq!(app.selected_ticket_key(), Some("AMP-93".to_string()));
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
        // H(AMP-501) + T(AMP-101)
        assert_eq!(app.item_count(), 2);
        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("AMP-101".to_string()));

        app.search = Some("perf".to_string());
        // H(AMP-502) + T(AMP-102)
        assert_eq!(app.item_count(), 2);
        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("AMP-102".to_string()));
    }

    #[test]
    fn toggle_selection_at_cursor_toggles_ticket_membership() {
        let mut app = App::new();
        app.active_tab = Tab::MyWork;
        app.loading = false;
        let mut t = ticket("AMP-10", "Parser migration");
        t.status = "In Progress".to_string();
        app.cache.my_tickets = vec![t];

        // H(In Progress), T(AMP-10)
        app.selected_index = 1;
        app.toggle_selection_at_cursor();
        assert!(app.is_ticket_selected("AMP-10"));
        app.toggle_selection_at_cursor();
        assert!(!app.is_ticket_selected("AMP-10"));
    }

    #[test]
    fn header_toggle_selects_and_clears_group_tickets() {
        let mut app = App::new();
        app.active_tab = Tab::MyWork;
        app.loading = false;

        let mut t1 = ticket("AMP-11", "A");
        t1.status = "In Progress".to_string();
        let mut t2 = ticket("AMP-12", "B");
        t2.status = "In Progress".to_string();
        app.cache.my_tickets = vec![t1, t2];

        app.selected_index = 0; // In Progress header
        app.toggle_selection_at_cursor();
        assert!(app.is_ticket_selected("AMP-11"));
        assert!(app.is_ticket_selected("AMP-12"));
        assert_eq!(
            app.group_selection_state("In Progress"),
            GroupSelectionState::All
        );

        app.toggle_selection_at_cursor();
        assert!(!app.is_ticket_selected("AMP-11"));
        assert!(!app.is_ticket_selected("AMP-12"));
        assert_eq!(
            app.group_selection_state("In Progress"),
            GroupSelectionState::None
        );
    }

    #[test]
    fn group_selection_state_reports_partial_when_some_selected() {
        let mut app = App::new();
        app.active_tab = Tab::MyWork;
        app.loading = false;
        let mut t1 = ticket("AMP-13", "A");
        t1.status = "In Progress".to_string();
        let mut t2 = ticket("AMP-14", "B");
        t2.status = "In Progress".to_string();
        app.cache.my_tickets = vec![t1, t2];

        app.selected_ticket_keys.insert("AMP-13".to_string());
        assert_eq!(
            app.group_selection_state("In Progress"),
            GroupSelectionState::Partial
        );
    }

    #[test]
    fn filters_item_count_includes_status_headers() {
        let mut in_progress = ticket("AMP-40", "Grouped");
        in_progress.status = "In Progress".to_string();
        let mut ready = ticket("AMP-41", "Queued");
        ready.status = "Ready for Work".to_string();

        let app = filters_app(vec![in_progress, ready]);

        assert_eq!(app.item_count(), 4);
    }

    #[test]
    fn filters_selected_ticket_key_skips_status_headers() {
        let mut in_progress = ticket("AMP-42", "Grouped");
        in_progress.status = "In Progress".to_string();
        let mut ready = ticket("AMP-43", "Queued");
        ready.status = "Ready for Work".to_string();

        let mut app = filters_app(vec![in_progress, ready]);

        app.selected_index = 0;
        assert_eq!(app.selected_ticket_key(), None);

        app.selected_index = 1;
        assert_eq!(app.selected_ticket_key(), Some("AMP-42".to_string()));
    }

    #[test]
    fn filters_header_toggle_selects_and_clears_group_tickets() {
        let mut first = ticket("AMP-44", "First");
        first.status = "In Progress".to_string();
        let mut second = ticket("AMP-45", "Second");
        second.status = "In Progress".to_string();

        let mut app = filters_app(vec![first, second]);

        app.selected_index = 0;
        app.toggle_selection_at_cursor();
        assert!(app.is_ticket_selected("AMP-44"));
        assert!(app.is_ticket_selected("AMP-45"));
        assert_eq!(
            app.group_selection_state("In Progress"),
            GroupSelectionState::All
        );

        app.toggle_selection_at_cursor();
        assert!(!app.is_ticket_selected("AMP-44"));
        assert!(!app.is_ticket_selected("AMP-45"));
        assert_eq!(
            app.group_selection_state("In Progress"),
            GroupSelectionState::None
        );
    }

    #[test]
    fn select_all_and_clear_selected_tickets() {
        let mut app = App::new();
        app.active_tab = Tab::MyWork;
        app.loading = false;
        let mut t1 = ticket("AMP-21", "A");
        t1.status = "In Progress".to_string();
        let mut t2 = ticket("AMP-22", "B");
        t2.status = "Ready for Work".to_string();
        app.cache.my_tickets = vec![t1, t2];

        app.select_all_visible_tickets();
        assert_eq!(app.selected_visible_ticket_keys_in_order().len(), 2);
        app.clear_selected_tickets();
        assert!(app.selected_visible_ticket_keys_in_order().is_empty());
    }

    #[test]
    fn selected_visible_ticket_keys_preserve_row_order() {
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

        app.selected_ticket_keys.insert("AMP-3".to_string());
        app.selected_ticket_keys.insert("AMP-1".to_string());
        assert_eq!(
            app.selected_visible_ticket_keys_in_order(),
            vec!["AMP-1".to_string(), "AMP-3".to_string()]
        );
    }

    #[test]
    fn prune_selection_removes_hidden_tickets_after_visibility_change() {
        let mut app = App::new();
        app.active_tab = Tab::MyWork;
        app.loading = false;
        let mut active = ticket("AMP-31", "Active");
        active.status = "In Progress".to_string();
        let mut done = ticket("AMP-32", "Done");
        done.status = "Closed".to_string();
        app.cache.my_tickets = vec![active, done];

        app.selected_ticket_keys.insert("AMP-31".to_string());
        app.selected_ticket_keys.insert("AMP-32".to_string());
        app.toggle_show_done(); // hides closed tickets
        assert!(app.is_ticket_selected("AMP-31"));
        assert!(!app.is_ticket_selected("AMP-32"));
    }
}
