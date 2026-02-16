use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
    time::{Duration, Instant},
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use tirc_config::{Keymap, Theme};
use tirc_core::{AppViewModel, UiAction, UiEvent};

use crate::{
    input::{InputEditor, InputHistory},
    palette::PaletteEngine,
    render::{
        PaletteMode, SelectionEditor, border_style, canonical_key, focus_style, parse_color,
        render_close_confirmation, render_help_overlay, render_palette, render_selection_editor,
    },
};

/// Intent emitted by the UI layer into the app reducer.
#[derive(Debug, Clone)]
pub enum UiIntent {
    Event(UiEvent),
    SubmitLine(String),
    ApplyTheme(String),
    ApplyKeymap(String),
}

enum EditorKind {
    Theme,
    Keymap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ComposerMode {
    JoinChannel { profile: String },
    MsgTarget { profile: String },
    MsgBody { profile: String, target: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusPane {
    Sidebar,
    Chat,
    Nicklist,
    Input,
}

/// Sidebar section type for collapse tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SidebarSection {
    Channels,
    Chats,
}

/// Represents a single item in the unified sidebar tree.
#[derive(Debug, Clone)]
enum SidebarItem {
    /// Profile header
    Profile {
        name: Rc<str>,
    },
    /// Status buffer under a profile
    Status {
        profile: Rc<str>,
    },
    ChannelsHeader {
        profile: Rc<str>,
    },
    Channel {
        profile: Rc<str>,
        name: String,
    },
    ChatsHeader {
        profile: Rc<str>,
    },
    Chat {
        profile: Rc<str>,
        name: String,
    },
}

impl SidebarItem {
    fn profile_section(&self) -> Option<(String, SidebarSection)> {
        match self {
            SidebarItem::ChannelsHeader { profile } => {
                Some((profile.to_string(), SidebarSection::Channels))
            }
            SidebarItem::ChatsHeader { profile } => {
                Some((profile.to_string(), SidebarSection::Chats))
            }
            _ => None,
        }
    }

    fn buffer_id(&self) -> Option<(String, String)> {
        match self {
            SidebarItem::Status { profile } => Some((profile.to_string(), "status".to_owned())),
            SidebarItem::Channel { profile, name } => {
                Some((profile.to_string(), name.clone()))
            }
            SidebarItem::Chat { profile, name } => Some((profile.to_string(), name.clone())),
            _ => None,
        }
    }
}

/// Encapsulates sidebar items and their pre-computed buffer indices.
///
/// Ensures the two data structures always stay in sync — `buffer_indices`
/// is recomputed whenever `items` is rebuilt.
#[derive(Debug, Clone, Default)]
struct SidebarTree {
    items: Vec<SidebarItem>,
    buffer_indices: Vec<usize>,
}

impl SidebarTree {
    fn rebuild(&mut self, items: Vec<SidebarItem>) {
        self.buffer_indices = items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| item.buffer_id().is_some().then_some(idx))
            .collect();
        self.items = items;
    }
}

/// Stateful ratatui engine for rendering and keyboard handling.
pub struct UiEngine {
    theme: Theme,
    keymap_actions: HashMap<String, UiAction>,
    input: InputEditor,
    pub palette_open: bool,
    palette: PaletteEngine,
    palette_items: Vec<String>,
    palette_mode: PaletteMode,
    theme_editor: SelectionEditor,
    keymap_editor: SelectionEditor,
    close_confirm_open: bool,
    connected_profiles: Vec<String>,
    channel_list_cache: BTreeMap<String, Vec<tirc_core::ChannelListEntry>>,
    server_names: Vec<String>,
    buffer_refs: Vec<(String, String)>,
    nick_refs: Vec<String>,
    active_profile: Option<String>,
    focus_pane: FocusPane,
    sidebar_selected: usize,
    sidebar: SidebarTree,
    sidebar_collapse: HashMap<(String, SidebarSection), bool>,
    nick_selected: usize,
    composer: Option<ComposerMode>,
    input_history: InputHistory,
    chat_scroll: u16,
    chat_max_scroll: u16,
    chat_page_step: u16,
    chat_follow_tail: bool,
    last_active_buffer: Option<String>,
    help_open: bool,
    last_draw: Instant,
    idle_hz: u8,
    // Dirty tracking for performance
    last_network_count: usize,
    last_connected_count: usize,
    last_buffer_count: usize,
    last_channel_list_len: usize,
    // Sidebar visibility
    show_left_sidebar: bool,
    show_right_sidebar: bool,
    // Nicklist scroll and filter
    nicklist_scroll: usize,
    nicklist_filter: String,
    nicklist_filtering: bool,
    // Layout regions for mouse hit testing
    region_sidebar: Option<Rect>,
    region_chat: Option<Rect>,
    region_nicklist: Option<Rect>,
    region_input: Option<Rect>,
}

impl UiEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: Theme,
        keymap: Keymap,
        palette_items: Vec<String>,
        idle_hz: u8,
        theme_options: Vec<String>,
        active_theme: String,
        keymap_options: Vec<String>,
        active_keymap: String,
    ) -> Self {
        let keymap_actions = flatten_keymap(&keymap);

        Self {
            theme,
            keymap_actions,
            input: InputEditor::default(),
            palette_open: false,
            palette: PaletteEngine::new(palette_items.clone()),
            palette_items,
            palette_mode: PaletteMode::Normal,
            theme_editor: SelectionEditor::with_options(theme_options, active_theme),
            keymap_editor: SelectionEditor::with_options(keymap_options, active_keymap),
            close_confirm_open: false,
            connected_profiles: Vec::new(),
            channel_list_cache: BTreeMap::new(),
            server_names: Vec::new(),
            buffer_refs: Vec::new(),
            nick_refs: Vec::new(),
            active_profile: None,
            focus_pane: FocusPane::Input,
            sidebar_selected: 0,
            sidebar: SidebarTree::default(),
            sidebar_collapse: HashMap::new(),
            nick_selected: 0,
            composer: None,
            input_history: InputHistory::new(500),
            chat_scroll: 0,
            chat_max_scroll: 0,
            chat_page_step: 8,
            chat_follow_tail: true,
            last_active_buffer: None,
            help_open: false,
            last_draw: Instant::now(),
            idle_hz,
            last_network_count: 0,
            last_connected_count: 0,
            last_buffer_count: 0,
            last_channel_list_len: 0,
            show_left_sidebar: true,
            show_right_sidebar: true,
            nicklist_scroll: 0,
            nicklist_filter: String::new(),
            nicklist_filtering: false,
            region_sidebar: None,
            region_chat: None,
            region_nicklist: None,
            region_input: None,
        }
    }

    pub fn set_theme(&mut self, theme: Theme, name: String) {
        self.theme = theme;
        self.theme_editor.active = name.clone();
        if let Some(pos) = self
            .theme_editor
            .options
            .iter()
            .position(|option| option == &name)
        {
            self.theme_editor.selected = pos;
        }
    }

    pub fn set_keymap(&mut self, keymap: &Keymap, name: String) {
        self.keymap_actions = flatten_keymap(keymap);
        self.keymap_editor.active = name.clone();
        if let Some(pos) = self
            .keymap_editor
            .options
            .iter()
            .position(|option| option == &name)
        {
            self.keymap_editor.selected = pos;
        }
    }

    pub fn set_theme_options(&mut self, options: Vec<String>, active: String) {
        self.theme_editor.set_options(options, active);
    }

    pub fn set_keymap_options(&mut self, options: Vec<String>, active: String) {
        self.keymap_editor.set_options(options, active);
    }

    pub fn set_palette_items(&mut self, items: Vec<String>) {
        self.palette_items = items;
        if self.palette_mode == PaletteMode::Normal {
            self.palette.set_items(self.palette_items.clone());
        }
    }

    pub fn sync_input(&mut self, value: &str) {
        if self.input.text() != value {
            self.input.set_text(value.to_owned());
        }
    }

    pub fn begin_join_interactive(&mut self) -> Vec<UiIntent> {
        self.start_join_composer()
    }

    pub fn open_close_confirmation(&mut self) {
        self.close_confirm_open = true;
    }

    pub fn max_idle_interval(&self) -> Duration {
        Duration::from_millis((1000 / u64::from(self.idle_hz.max(1))).max(16))
    }

    pub fn should_draw_heartbeat(&self) -> bool {
        self.last_draw.elapsed() >= self.max_idle_interval()
    }

    pub fn draw(&mut self, frame: &mut Frame, vm: &AppViewModel) {
        self.last_draw = Instant::now();
        self.help_open = vm.help_open;

        // Only rebuild network-related caches when network state changes
        let connected_count = vm.networks.iter().filter(|n| n.connected).count();
        let network_count = vm.networks.len();
        let buffer_count = vm.buffers.len();
        let channel_list_len = vm.channel_list_cache.values().map(|v| v.len()).sum();

        if network_count != self.last_network_count || connected_count != self.last_connected_count
        {
            self.last_network_count = network_count;
            self.last_connected_count = connected_count;

            // Pre-allocate with capacity
            self.connected_profiles = Vec::with_capacity(connected_count);
            for network in vm.networks.iter().filter(|n| n.connected) {
                self.connected_profiles.push(network.name.clone());
            }
            self.connected_profiles.sort();
            self.connected_profiles.dedup();

            self.server_names = Vec::with_capacity(network_count);
            for network in &vm.networks {
                self.server_names.push(network.name.clone());
            }
        }

        // Only clone channel list cache when it changes
        if channel_list_len != self.last_channel_list_len {
            self.last_channel_list_len = channel_list_len;
            self.channel_list_cache = vm.channel_list_cache.clone();
        }

        // Only rebuild buffer refs when buffer count changes
        if buffer_count != self.last_buffer_count {
            self.last_buffer_count = buffer_count;
            self.buffer_refs = Vec::with_capacity(buffer_count);
            for buffer in &vm.buffers {
                self.buffer_refs
                    .push((buffer.id.network.clone(), buffer.id.name.clone()));
            }
        }

        let area = frame.area();
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(area);

        // Build horizontal constraints based on sidebar visibility
        let mut h_constraints = Vec::new();
        if self.show_left_sidebar {
            h_constraints.push(Constraint::Length(28));
        }
        h_constraints.push(Constraint::Min(30));
        if self.show_right_sidebar {
            h_constraints.push(Constraint::Length(24));
        }

        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(h_constraints)
            .split(root[0]);

        // Calculate indices based on which sidebars are visible
        let left_area = if self.show_left_sidebar {
            Some(top[0])
        } else {
            None
        };
        let chat_idx = if self.show_left_sidebar { 1 } else { 0 };
        let chat_area = top[chat_idx];
        let right_area = if self.show_right_sidebar {
            Some(top[top.len() - 1])
        } else {
            None
        };

        // Store layout regions for mouse hit testing
        self.region_chat = Some(chat_area);
        self.region_nicklist = right_area;
        self.region_sidebar = left_area;
        self.region_input = Some(root[2]);

        // Build unified sidebar items
        let items = self.build_sidebar_items(vm);
        self.sidebar.rebuild(items);

        // Clamp sidebar selection
        if self.sidebar.items.is_empty() {
            self.sidebar_selected = 0;
        } else {
            self.sidebar_selected = self
                .sidebar_selected
                .min(self.sidebar.items.len().saturating_sub(1));
        }

        // Sync selection with active buffer
        if let Some(ref buffer) = vm.active_buffer {
            let target = (buffer.id.network.clone(), buffer.id.name.clone());
            if let Some(idx) = self
                .sidebar
                .items
                .iter()
                .position(|item| item.buffer_id() == Some(target.clone()))
            {
                self.sidebar_selected = idx;
            }
        }

        // Render unified sidebar
        if let Some(sidebar_area) = left_area {
            let sidebar_list_items: Vec<ListItem> = self
                .sidebar
                .items
                .iter()
                .map(|item| match item {
                    SidebarItem::Profile { name } => {
                        let connected =
                            vm.networks.iter().any(|n| n.name.as_str() == &**name && n.connected);
                        let icon = if connected { "●" } else { "○" };
                        ListItem::new(Line::from(Span::styled(
                            format!("{icon} {name}"),
                            Style::default().add_modifier(Modifier::BOLD),
                        )))
                    }
                    SidebarItem::Status { .. } => ListItem::new("    status"),
                    SidebarItem::ChannelsHeader { profile } => {
                        let collapsed = self
                            .sidebar_collapse
                            .get(&(profile.to_string(), SidebarSection::Channels))
                            .copied()
                            .unwrap_or(false);
                        let arrow = if collapsed { "▸" } else { "▾" };
                        ListItem::new(Line::from(Span::styled(
                            format!("  {arrow} channels"),
                            Style::default().fg(Color::DarkGray),
                        )))
                    }
                    SidebarItem::Channel { name, .. } => ListItem::new(format!("    {name}")),
                    SidebarItem::ChatsHeader { profile } => {
                        let collapsed = self
                            .sidebar_collapse
                            .get(&(profile.to_string(), SidebarSection::Chats))
                            .copied()
                            .unwrap_or(false);
                        let arrow = if collapsed { "▸" } else { "▾" };
                        ListItem::new(Line::from(Span::styled(
                            format!("  {arrow} chats"),
                            Style::default().fg(Color::DarkGray),
                        )))
                    }
                    SidebarItem::Chat { name, .. } => ListItem::new(format!("    {name}")),
                })
                .collect();

            let mut sidebar_state = ListState::default();
            if !sidebar_list_items.is_empty() {
                sidebar_state.select(Some(self.sidebar_selected));
            }

            frame.render_stateful_widget(
                List::new(sidebar_list_items)
                    .highlight_style(focus_style(
                        &self.theme,
                        self.focus_pane == FocusPane::Sidebar,
                    ))
                    .block(
                        Block::default()
                            .title("Buffers")
                            .borders(Borders::ALL)
                            .border_style(border_style(
                                &self.theme,
                                self.focus_pane == FocusPane::Sidebar,
                            )),
                    ),
                sidebar_area,
                &mut sidebar_state,
            );
        }

        let chat_title = vm
            .active_buffer
            .as_ref()
            .map(|buffer| format!("{} / {}", buffer.id.network, buffer.id.name))
            .unwrap_or_else(|| "No active buffer".to_owned());
        self.active_profile = vm
            .active_buffer
            .as_ref()
            .map(|buffer| buffer.id.network.clone());

        let chat_lines = vm
            .active_buffer
            .as_ref()
            .map(|buffer| {
                buffer
                    .lines
                    .iter()
                    .map(|line| {
                        let ts = line.ts.format("%H:%M:%S").to_string();
                        let sender = line.sender.clone().unwrap_or_else(|| "*".to_owned());
                        let style = if line.mention {
                            Style::default().fg(parse_color(&self.theme.palette.mention))
                        } else {
                            Style::default().fg(parse_color(&self.theme.palette.foreground))
                        };
                        Line::from(vec![
                            Span::styled(format!("[{ts}] "), Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("<{sender}> "), Style::default().fg(Color::Cyan)),
                            Span::styled(line.body.clone(), style),
                        ])
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let active_buffer_key = vm
            .active_buffer
            .as_ref()
            .map(|buffer| format!("{}/{}", buffer.id.network, buffer.id.name));
        if self.last_active_buffer != active_buffer_key {
            self.last_active_buffer = active_buffer_key;
            self.chat_follow_tail = true;
            // Clear nicklist filter when switching buffers
            self.nicklist_filter.clear();
            self.nicklist_filtering = false;
            self.nick_selected = 0;
        }

        let viewport_lines = chat_area.height.saturating_sub(2).max(1);
        self.chat_page_step = viewport_lines.saturating_sub(1).max(1);
        self.chat_max_scroll = chat_lines
            .len()
            .saturating_sub(usize::from(viewport_lines))
            .try_into()
            .unwrap_or(u16::MAX);
        if self.chat_follow_tail {
            self.chat_scroll = self.chat_max_scroll;
        } else {
            self.chat_scroll = self.chat_scroll.min(self.chat_max_scroll);
        }

        frame.render_widget(
            Paragraph::new(chat_lines)
                .block(
                    Block::default()
                        .title(chat_title)
                        .borders(Borders::ALL)
                        .border_style(border_style(
                            &self.theme,
                            self.focus_pane == FocusPane::Chat,
                        )),
                )
                .scroll((self.chat_scroll, 0))
                .wrap(Wrap { trim: false }),
            chat_area,
        );

        // Build filtered nicklist
        let raw_nicks: Vec<String> = vm
            .active_buffer
            .as_ref()
            .map(|buffer| buffer.nicklist.clone())
            .unwrap_or_default();

        // Apply filter if active
        let filtered_nicks: Vec<String> =
            if self.nicklist_filtering && !self.nicklist_filter.is_empty() {
                let filter_lower = self.nicklist_filter.to_lowercase();
                raw_nicks
                    .iter()
                    .filter(|nick| nick.to_lowercase().contains(&filter_lower))
                    .cloned()
                    .collect()
            } else {
                raw_nicks.clone()
            };

        self.nick_refs = filtered_nicks.clone();

        let nick_items: Vec<ListItem> = filtered_nicks
            .iter()
            .map(|nick| ListItem::new(nick.clone()))
            .collect();

        if self.nick_refs.is_empty() {
            self.nick_selected = 0;
            self.nicklist_scroll = 0;
        } else {
            self.nick_selected = self
                .nick_selected
                .min(self.nick_refs.len().saturating_sub(1));
        }

        let nick_count = raw_nicks.len();
        let filtered_count = filtered_nicks.len();
        let mut nick_state = ListState::default();
        if !nick_items.is_empty() {
            nick_state.select(Some(self.nick_selected));
        }

        // Build nicklist title with filter indicator
        let nicklist_title = if self.nicklist_filtering {
            format!("Nicklist [{}] /{}", filtered_count, self.nicklist_filter)
        } else if filtered_count != nick_count {
            format!("Nicklist [{}/{}]", filtered_count, nick_count)
        } else {
            format!("Nicklist [{}]", nick_count)
        };

        // Render right sidebar (nicklist) if visible
        if let Some(right) = right_area {
            frame.render_stateful_widget(
                List::new(nick_items)
                    .highlight_style(focus_style(
                        &self.theme,
                        self.focus_pane == FocusPane::Nicklist,
                    ))
                    .block(
                        Block::default()
                            .title(nicklist_title)
                            .borders(Borders::ALL)
                            .border_style(border_style(
                                &self.theme,
                                self.focus_pane == FocusPane::Nicklist,
                            )),
                    ),
                right,
                &mut nick_state,
            );
        }

        let status_style = Style::default()
            .fg(parse_color(&self.theme.palette.background))
            .bg(parse_color(&self.theme.palette.accent))
            .add_modifier(Modifier::BOLD);
        frame.render_widget(
            Paragraph::new(vm.status.clone()).style(status_style),
            root[1],
        );

        let input_title = match &self.composer {
            Some(ComposerMode::JoinChannel { profile }) => {
                format!("Input (join {profile}: enter channel name)")
            }
            Some(ComposerMode::MsgTarget { profile }) => {
                format!("Input (msg {profile}: enter target)")
            }
            Some(ComposerMode::MsgBody { profile, target }) => {
                format!("Input (msg {profile} -> {target}: enter message)")
            }
            None => "Input".to_owned(),
        };

        let input_block = Block::default()
            .title(input_title)
            .borders(Borders::ALL)
            .border_style(border_style(
                &self.theme,
                self.focus_pane == FocusPane::Input,
            ));
        frame.render_widget(
            Paragraph::new(self.input.text().to_owned())
                .style(Style::default().fg(parse_color(&self.theme.palette.foreground)))
                .block(input_block),
            root[2],
        );

        if vm.help_open {
            render_help_overlay(frame, area);
        }

        if self.palette_open {
            render_palette(frame, area, &self.theme, &self.palette, &self.palette_mode);
        }

        if self.theme_editor.open {
            render_selection_editor(frame, area, "Theme Editor", &self.theme_editor, &self.theme);
        }

        if self.keymap_editor.open {
            render_selection_editor(
                frame,
                area,
                "Keymap Editor",
                &self.keymap_editor,
                &self.theme,
            );
        }

        if self.close_confirm_open {
            render_close_confirmation(frame, area);
        }
    }

    pub fn handle_terminal_event(&mut self, event: Event) -> Vec<UiIntent> {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize(width, height) => {
                vec![UiIntent::Event(UiEvent::Resize { width, height })]
            }
            _ => Vec::new(),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Vec<UiIntent> {
        let pos = (mouse.column, mouse.row);

        // Helper to check if position is within a rect
        let in_rect = |rect: Option<Rect>| -> bool {
            rect.map(|r| {
                pos.0 >= r.x && pos.0 < r.x + r.width && pos.1 >= r.y && pos.1 < r.y + r.height
            })
            .unwrap_or(false)
        };

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if in_rect(self.region_nicklist) {
                    // Scroll nicklist up
                    if self.nick_selected > 0 {
                        self.nick_selected = self.nick_selected.saturating_sub(3);
                    }
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else if in_rect(self.region_chat) {
                    self.scroll_up_lines(3);
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else if in_rect(self.region_sidebar) {
                    if self.sidebar_selected > 0 {
                        self.sidebar_selected = self.sidebar_selected.saturating_sub(1);
                    }
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else {
                    self.scroll_up_lines(3);
                    vec![UiIntent::Event(UiEvent::Tick)]
                }
            }
            MouseEventKind::ScrollDown => {
                if in_rect(self.region_nicklist) {
                    // Scroll nicklist down
                    if !self.nick_refs.is_empty() {
                        self.nick_selected =
                            (self.nick_selected + 3).min(self.nick_refs.len().saturating_sub(1));
                    }
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else if in_rect(self.region_chat) {
                    self.scroll_down_lines(3);
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else if in_rect(self.region_sidebar) {
                    if !self.sidebar.items.is_empty() {
                        self.sidebar_selected = (self.sidebar_selected + 1)
                            .min(self.sidebar.items.len().saturating_sub(1));
                    }
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else {
                    self.scroll_down_lines(3);
                    vec![UiIntent::Event(UiEvent::Tick)]
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if in_rect(self.region_sidebar) {
                    // Click on sidebar - select and possibly toggle collapse or switch buffer
                    if let Some(rect) = self.region_sidebar {
                        let rel_y = (pos.1 - rect.y).saturating_sub(1) as usize; // -1 for border
                        if rel_y < self.sidebar.items.len() {
                            self.sidebar_selected = rel_y;
                            self.focus_pane = FocusPane::Sidebar;

                            // Handle click action based on item type
                            if let Some(item) = self.sidebar.items.get(rel_y).cloned() {
                                if let Some((profile, section)) = item.profile_section() {
                                    // Toggle collapse for headers
                                    let key = (profile, section);
                                    let collapsed =
                                        self.sidebar_collapse.get(&key).copied().unwrap_or(false);
                                    self.sidebar_collapse.insert(key, !collapsed);
                                } else if let Some((network, name)) = item.buffer_id() {
                                    // Switch to buffer
                                    return vec![
                                        UiIntent::Event(UiEvent::Tick),
                                        UiIntent::SubmitLine(format!(
                                            "/buffer {} {}",
                                            network, name
                                        )),
                                    ];
                                }
                            }
                        }
                    }
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else if in_rect(self.region_nicklist) {
                    // Click on nicklist - select nick and focus
                    self.focus_pane = FocusPane::Nicklist;
                    if let Some(rect) = self.region_nicklist {
                        let rel_y = (pos.1 - rect.y).saturating_sub(1) as usize;
                        if rel_y < self.nick_refs.len() {
                            self.nick_selected = rel_y;
                        }
                    }
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else if in_rect(self.region_chat) {
                    self.focus_pane = FocusPane::Chat;
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else if in_rect(self.region_input) {
                    self.focus_pane = FocusPane::Input;
                    vec![UiIntent::Event(UiEvent::Tick)]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Vec<UiIntent> {
        if key.kind != KeyEventKind::Press {
            return Vec::new();
        }

        if self.help_open {
            match key.code {
                KeyCode::Esc => {
                    return vec![UiIntent::Event(UiEvent::Action(UiAction::HelpClose))];
                }
                KeyCode::Char('q') if key.modifiers == KeyModifiers::NONE => {
                    return vec![UiIntent::Event(UiEvent::Action(UiAction::HelpClose))];
                }
                _ => {
                    if let Some(action) = self.lookup_action(key)
                        && action == UiAction::HelpToggle
                    {
                        return self.handle_action(action);
                    }
                    return Vec::new();
                }
            }
        }

        if self.close_confirm_open {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.close_confirm_open = false;
                    return vec![UiIntent::Event(UiEvent::Action(
                        UiAction::BufferCloseCurrent,
                    ))];
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.close_confirm_open = false;
                    return Vec::new();
                }
                _ => return Vec::new(),
            }
        }

        if self.palette_open {
            return self.handle_palette_key(key);
        }

        if self.theme_editor.open {
            return self.handle_selection_editor_key(EditorKind::Theme, key);
        }

        if self.keymap_editor.open {
            return self.handle_selection_editor_key(EditorKind::Keymap, key);
        }

        // Handle nicklist filter mode
        if self.nicklist_filtering {
            match key.code {
                KeyCode::Esc => {
                    self.nicklist_filtering = false;
                    self.nicklist_filter.clear();
                    return vec![UiIntent::Event(UiEvent::Tick)];
                }
                KeyCode::Enter => {
                    // Keep filter active but exit filter mode
                    self.nicklist_filtering = false;
                    return vec![UiIntent::Event(UiEvent::Tick)];
                }
                KeyCode::Backspace => {
                    self.nicklist_filter.pop();
                    return vec![UiIntent::Event(UiEvent::Tick)];
                }
                KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE => {
                    self.nicklist_filter.push(c);
                    return vec![UiIntent::Event(UiEvent::Tick)];
                }
                _ => return Vec::new(),
            }
        }

        match key.code {
            KeyCode::Tab => {
                self.cycle_focus_pane(1);
                return vec![UiIntent::Event(UiEvent::Tick)];
            }
            KeyCode::BackTab => {
                self.cycle_focus_pane(-1);
                return vec![UiIntent::Event(UiEvent::Tick)];
            }
            // Left/Right collapse/expand sidebar sections when sidebar is focused
            KeyCode::Left if self.focus_pane == FocusPane::Sidebar => {
                return self.collapse_sidebar_section();
            }
            KeyCode::Right if self.focus_pane == FocusPane::Sidebar => {
                return self.expand_sidebar_section();
            }
            KeyCode::Up if self.focus_pane != FocusPane::Input => {
                return self.navigate_focused_pane(-1);
            }
            KeyCode::Down if self.focus_pane != FocusPane::Input => {
                return self.navigate_focused_pane(1);
            }
            KeyCode::Enter
                if self.focus_pane == FocusPane::Nicklist
                    && self.composer.is_none()
                    && !self.nick_refs.is_empty() =>
            {
                return self.open_pm_from_nicklist();
            }
            // Enter toggles collapse on sidebar headers, or switches to buffer
            KeyCode::Enter if self.focus_pane == FocusPane::Sidebar && self.composer.is_none() => {
                if let Some(item) = self.sidebar.items.get(self.sidebar_selected).cloned() {
                    if let Some((profile, section)) = item.profile_section() {
                        // Toggle collapse for headers
                        let key = (profile, section);
                        let collapsed = self.sidebar_collapse.get(&key).copied().unwrap_or(false);
                        self.sidebar_collapse.insert(key, !collapsed);
                        return vec![UiIntent::Event(UiEvent::Tick)];
                    } else if let Some((network, name)) = item.buffer_id() {
                        // Switch to buffer
                        return vec![UiIntent::SubmitLine(format!("/buffer {network} {name}"))];
                    }
                }
                return vec![UiIntent::Event(UiEvent::Tick)];
            }
            // "/" starts nicklist filter mode when nicklist is focused
            KeyCode::Char('/')
                if self.focus_pane == FocusPane::Nicklist && !self.nicklist_filtering =>
            {
                self.nicklist_filtering = true;
                self.nicklist_filter.clear();
                return vec![UiIntent::Event(UiEvent::Tick)];
            }
            _ => {}
        }

        if let Some(action) = self.lookup_action(key) {
            return self.handle_action(action);
        }

        match key.code {
            KeyCode::Esc if self.composer.is_some() => {
                self.composer = None;
                self.input.clear();
                self.clear_history_navigation();
                vec![UiIntent::Event(UiEvent::InputChanged(String::new()))]
            }
            KeyCode::Enter => {
                if self.composer.is_some() {
                    return self.handle_composer_submit();
                }
                if self.focus_pane != FocusPane::Input {
                    return Vec::new();
                }
                let line = self.input.text().to_owned();
                self.push_history(line.clone());
                self.input.clear();
                self.clear_history_navigation();
                vec![
                    UiIntent::Event(UiEvent::InputChanged(String::new())),
                    UiIntent::SubmitLine(line),
                ]
            }
            KeyCode::Up if self.focus_pane == FocusPane::Input => {
                if self.history_prev() {
                    vec![UiIntent::Event(UiEvent::InputChanged(
                        self.input.text().to_owned(),
                    ))]
                } else {
                    Vec::new()
                }
            }
            KeyCode::Down if self.focus_pane == FocusPane::Input => {
                if self.history_next() {
                    vec![UiIntent::Event(UiEvent::InputChanged(
                        self.input.text().to_owned(),
                    ))]
                } else {
                    Vec::new()
                }
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE
                    && (self.focus_pane == FocusPane::Input || self.composer.is_some()) =>
            {
                self.clear_history_navigation();
                self.input.insert_char(c);
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            KeyCode::Backspace
                if self.focus_pane == FocusPane::Input || self.composer.is_some() =>
            {
                self.clear_history_navigation();
                self.input.backspace();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            KeyCode::Delete if self.focus_pane == FocusPane::Input || self.composer.is_some() => {
                self.clear_history_navigation();
                self.input.delete();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            KeyCode::Left if self.focus_pane == FocusPane::Input || self.composer.is_some() => {
                self.input.move_left();
                Vec::new()
            }
            KeyCode::Right if self.focus_pane == FocusPane::Input || self.composer.is_some() => {
                self.input.move_right();
                Vec::new()
            }
            KeyCode::Home if self.focus_pane == FocusPane::Input || self.composer.is_some() => {
                self.input.move_home();
                Vec::new()
            }
            KeyCode::End if self.focus_pane == FocusPane::Input || self.composer.is_some() => {
                self.input.move_end();
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_palette_key(&mut self, key: KeyEvent) -> Vec<UiIntent> {
        match key.code {
            KeyCode::Esc => {
                self.palette_open = false;
                self.palette.clear_query();
                self.reset_palette_mode();
                vec![UiIntent::Event(UiEvent::Action(UiAction::PaletteToggle))]
            }
            KeyCode::Up => {
                self.palette.move_selection_up();
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            KeyCode::Down => {
                self.palette.move_selection_down();
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            KeyCode::Enter => {
                if let Some(selected) = self.palette.selected_item().map(ToOwned::to_owned) {
                    match self.palette_mode.clone() {
                        PaletteMode::Normal => self.handle_palette_selection(selected),
                        PaletteMode::JoinProfile => self.start_join_for_profile(selected),
                        PaletteMode::JoinChannel { profile } => {
                            let channel = selected
                                .split_whitespace()
                                .next()
                                .map(ToOwned::to_owned)
                                .unwrap_or(selected);
                            self.reset_palette_mode();
                            self.palette_open = false;
                            self.submit_join_line(profile, channel)
                        }
                        PaletteMode::MsgProfile => {
                            self.reset_palette_mode();
                            self.palette_open = false;
                            self.begin_msg_target(selected)
                        }
                    }
                } else {
                    Vec::new()
                }
            }
            KeyCode::Backspace => {
                self.palette.query_backspace();
                vec![UiIntent::Event(UiEvent::PaletteQuery(
                    self.palette.query.clone(),
                ))]
            }
            KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE => {
                self.palette.query_push(c);
                vec![UiIntent::Event(UiEvent::PaletteQuery(
                    self.palette.query.clone(),
                ))]
            }
            _ => Vec::new(),
        }
    }

    fn handle_selection_editor_key(&mut self, kind: EditorKind, key: KeyEvent) -> Vec<UiIntent> {
        let editor = match kind {
            EditorKind::Theme => &mut self.theme_editor,
            EditorKind::Keymap => &mut self.keymap_editor,
        };

        match key.code {
            KeyCode::Esc => {
                editor.open = false;
                Vec::new()
            }
            KeyCode::Up => {
                editor.move_up();
                Vec::new()
            }
            KeyCode::Down => {
                editor.move_down();
                Vec::new()
            }
            KeyCode::Enter => {
                editor.open = false;
                if let Some(selected) = editor.selected_value() {
                    match kind {
                        EditorKind::Theme => vec![UiIntent::ApplyTheme(selected)],
                        EditorKind::Keymap => vec![UiIntent::ApplyKeymap(selected)],
                    }
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn handle_action(&mut self, action: UiAction) -> Vec<UiIntent> {
        match action {
            UiAction::PaletteToggle => {
                self.palette_open = !self.palette_open;
                if self.palette_open {
                    self.reset_palette_mode();
                    self.palette.clear_query();
                    self.palette.set_items(self.palette_items.clone());
                }
                self.theme_editor.open = false;
                self.keymap_editor.open = false;
                vec![UiIntent::Event(UiEvent::Action(action))]
            }
            UiAction::HelpToggle => vec![UiIntent::Event(UiEvent::Action(action))],
            UiAction::ThemeEditorToggle => {
                self.theme_editor.open = !self.theme_editor.open;
                self.keymap_editor.open = false;
                self.palette_open = false;
                vec![UiIntent::Event(UiEvent::Action(action))]
            }
            UiAction::KeymapEditorToggle => {
                self.keymap_editor.open = !self.keymap_editor.open;
                self.theme_editor.open = false;
                self.palette_open = false;
                vec![UiIntent::Event(UiEvent::Action(action))]
            }
            UiAction::BufferCloseConfirm => {
                self.close_confirm_open = true;
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            UiAction::InputHome => {
                self.input.move_home();
                Vec::new()
            }
            UiAction::InputEnd => {
                self.input.move_end();
                Vec::new()
            }
            UiAction::InputKillToEnd => {
                self.clear_history_navigation();
                self.input.kill_to_end();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            UiAction::InputKillToStart => {
                self.clear_history_navigation();
                self.input.kill_to_start();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            UiAction::InputKillWord => {
                self.clear_history_navigation();
                self.input.kill_word();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            UiAction::ScrollUpPage => {
                self.scroll_up_lines(self.chat_page_step);
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            UiAction::ScrollDownPage => {
                self.scroll_down_lines(self.chat_page_step);
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            UiAction::ScrollUpLine => {
                self.scroll_up_lines(1);
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            UiAction::ScrollDownLine => {
                self.scroll_down_lines(1);
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            UiAction::SidebarLeftToggle => {
                self.show_left_sidebar = !self.show_left_sidebar;
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            UiAction::SidebarRightToggle => {
                self.show_right_sidebar = !self.show_right_sidebar;
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            // Forward other actions to the app layer
            _ => vec![UiIntent::Event(UiEvent::Action(action))],
        }
    }

    fn handle_palette_selection(&mut self, selected: String) -> Vec<UiIntent> {
        match selected.as_str() {
            "join" => self.start_join_composer(),
            "msg" => self.start_msg_composer(),
            _ => {
                self.palette_open = false;
                vec![UiIntent::Event(UiEvent::PaletteExecute(selected))]
            }
        }
    }

    fn start_join_composer(&mut self) -> Vec<UiIntent> {
        if self.connected_profiles.len() == 1 {
            return self.start_join_for_profile(self.connected_profiles[0].clone());
        }
        if self.connected_profiles.len() > 1 {
            self.palette_mode = PaletteMode::JoinProfile;
            self.palette.clear_query();
            self.palette.set_items(self.connected_profiles.clone());
            self.palette_open = true;
            return Vec::new();
        }

        self.palette_open = false;
        vec![UiIntent::Event(UiEvent::PaletteExecute("join".to_owned()))]
    }

    fn start_join_for_profile(&mut self, profile: String) -> Vec<UiIntent> {
        let cached = self
            .channel_list_cache
            .get(&profile)
            .cloned()
            .unwrap_or_default();
        if !cached.is_empty() {
            let mut items = cached
                .iter()
                .map(|entry| format!("{} ({}) {}", entry.channel, entry.users, entry.topic))
                .collect::<Vec<_>>();
            items.sort();
            items.dedup();
            self.palette_mode = PaletteMode::JoinChannel {
                profile: profile.clone(),
            };
            self.palette.clear_query();
            self.palette.set_items(items);
            self.palette_open = true;
            return Vec::new();
        }

        self.begin_join_channel(profile)
    }

    fn start_msg_composer(&mut self) -> Vec<UiIntent> {
        if self.connected_profiles.len() == 1 {
            return self.begin_msg_target(self.connected_profiles[0].clone());
        }
        if self.connected_profiles.len() > 1 {
            self.palette_mode = PaletteMode::MsgProfile;
            self.palette.clear_query();
            self.palette.set_items(self.connected_profiles.clone());
            return Vec::new();
        }

        self.palette_open = false;
        vec![UiIntent::Event(UiEvent::PaletteExecute("msg".to_owned()))]
    }

    fn begin_join_channel(&mut self, profile: String) -> Vec<UiIntent> {
        self.palette_open = false;
        self.palette.clear_query();
        self.composer = Some(ComposerMode::JoinChannel { profile });
        self.focus_pane = FocusPane::Input;
        self.input.set_text("#".to_owned());
        vec![UiIntent::Event(UiEvent::InputChanged("#".to_owned()))]
    }

    fn begin_msg_target(&mut self, profile: String) -> Vec<UiIntent> {
        self.palette_open = false;
        self.palette.clear_query();
        self.composer = Some(ComposerMode::MsgTarget { profile });
        self.focus_pane = FocusPane::Input;
        self.input.clear();
        vec![UiIntent::Event(UiEvent::InputChanged(String::new()))]
    }

    fn submit_join_line(&mut self, profile: String, channel: String) -> Vec<UiIntent> {
        let normalized = if channel.starts_with('#') {
            channel
        } else {
            format!("#{channel}")
        };
        let line = format!("/join {profile} {normalized}");
        self.push_history(line.clone());
        self.clear_history_navigation();
        self.focus_pane = FocusPane::Input;
        vec![
            UiIntent::Event(UiEvent::InputChanged(String::new())),
            UiIntent::SubmitLine(line),
        ]
    }

    fn handle_composer_submit(&mut self) -> Vec<UiIntent> {
        match self.composer.clone() {
            Some(ComposerMode::JoinChannel { profile }) => {
                let raw = self.input.text().trim().trim_start_matches('#').to_owned();
                if raw.is_empty() {
                    return Vec::new();
                }
                self.composer = None;
                self.input.clear();
                self.submit_join_line(profile, raw)
            }
            Some(ComposerMode::MsgTarget { profile }) => {
                let target = self.input.text().trim().to_owned();
                if target.is_empty() {
                    return Vec::new();
                }
                self.composer = Some(ComposerMode::MsgBody { profile, target });
                self.input.clear();
                vec![UiIntent::Event(UiEvent::InputChanged(String::new()))]
            }
            Some(ComposerMode::MsgBody { profile, target }) => {
                let body = self.input.text().trim().to_owned();
                if body.is_empty() {
                    return Vec::new();
                }
                let line = format!("/msg {profile} {target} {body}");
                self.push_history(line.clone());
                self.composer = None;
                self.input.clear();
                self.clear_history_navigation();
                vec![
                    UiIntent::Event(UiEvent::InputChanged(String::new())),
                    UiIntent::SubmitLine(line),
                ]
            }
            None => Vec::new(),
        }
    }

    fn reset_palette_mode(&mut self) {
        self.palette_mode = PaletteMode::Normal;
        self.palette.set_items(self.palette_items.clone());
    }

    fn clear_history_navigation(&mut self) {
        self.input_history.reset();
    }

    fn push_history(&mut self, line: String) {
        self.input_history.push(line);
    }

    fn history_prev(&mut self) -> bool {
        let current = self.input.text();
        if let Some(entry) = self.input_history.prev(current) {
            self.input.set_text(entry.to_owned());
            true
        } else {
            false
        }
    }

    fn history_next(&mut self) -> bool {
        if let Some(entry) = self.input_history.next() {
            self.input.set_text(entry.to_owned());
            true
        } else {
            false
        }
    }

    fn scroll_up_lines(&mut self, lines: u16) {
        self.chat_follow_tail = false;
        self.chat_scroll = self.chat_scroll.saturating_sub(lines);
    }

    fn scroll_down_lines(&mut self, lines: u16) {
        self.chat_scroll = self
            .chat_scroll
            .saturating_add(lines)
            .min(self.chat_max_scroll);
        if self.chat_scroll >= self.chat_max_scroll {
            self.chat_follow_tail = true;
        }
    }

    fn cycle_focus_pane(&mut self, direction: i8) {
        // Build available panes based on sidebar visibility
        let mut panes = Vec::new();
        if self.show_left_sidebar {
            panes.push(FocusPane::Sidebar);
        }
        panes.push(FocusPane::Chat);
        if self.show_right_sidebar {
            panes.push(FocusPane::Nicklist);
        }
        panes.push(FocusPane::Input);

        let current = panes
            .iter()
            .position(|pane| pane == &self.focus_pane)
            .unwrap_or(0);
        let next = if direction < 0 {
            current
                .checked_sub(1)
                .unwrap_or(panes.len().saturating_sub(1))
        } else {
            (current + 1) % panes.len()
        };
        self.focus_pane = panes[next];
    }

    fn navigate_focused_pane(&mut self, direction: i8) -> Vec<UiIntent> {
        match self.focus_pane {
            FocusPane::Sidebar => {
                if self.sidebar.items.is_empty() {
                    return Vec::new();
                }

                if self.sidebar.buffer_indices.is_empty() {
                    return vec![UiIntent::Event(UiEvent::Tick)];
                }

                let idx_len = self.sidebar.buffer_indices.len();
                let current_pos = self
                    .sidebar
                    .buffer_indices
                    .iter()
                    .position(|idx| *idx == self.sidebar_selected);
                let next_pos = match current_pos {
                    Some(pos) if direction < 0 => {
                        pos.checked_sub(1).unwrap_or(idx_len.saturating_sub(1))
                    }
                    Some(pos) => (pos + 1) % idx_len,
                    None if direction < 0 => idx_len.saturating_sub(1),
                    None => 0,
                };
                self.sidebar_selected = self.sidebar.buffer_indices[next_pos];

                if let Some(item) = self.sidebar.items.get(self.sidebar_selected) {
                    if let Some((network, name)) = item.buffer_id() {
                        return vec![UiIntent::SubmitLine(format!("/buffer {network} {name}"))];
                    }
                }
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            FocusPane::Chat => {
                if direction < 0 {
                    self.scroll_up_lines(1);
                } else {
                    self.scroll_down_lines(1);
                }
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            FocusPane::Nicklist => {
                if self.nick_refs.is_empty() {
                    return Vec::new();
                }
                if direction < 0 {
                    self.nick_selected = self
                        .nick_selected
                        .checked_sub(1)
                        .unwrap_or(self.nick_refs.len().saturating_sub(1));
                } else {
                    self.nick_selected = (self.nick_selected + 1) % self.nick_refs.len();
                }
                vec![UiIntent::Event(UiEvent::Tick)]
            }
            FocusPane::Input => Vec::new(),
        }
    }

    /// Collapse the sidebar section containing the currently selected item.
    fn collapse_sidebar_section(&mut self) -> Vec<UiIntent> {
        if let Some(section_key) = self.find_section_for_selected() {
            self.sidebar_collapse.insert(section_key, true);
        }
        vec![UiIntent::Event(UiEvent::Tick)]
    }

    /// Expand the sidebar section containing the currently selected item.
    fn expand_sidebar_section(&mut self) -> Vec<UiIntent> {
        if let Some(section_key) = self.find_section_for_selected() {
            self.sidebar_collapse.insert(section_key, false);
        }
        vec![UiIntent::Event(UiEvent::Tick)]
    }

    /// Find the section key for the currently selected sidebar item.
    /// Returns the (profile, section) key if the item belongs to a collapsible section.
    fn find_section_for_selected(&self) -> Option<(String, SidebarSection)> {
        let item = self.sidebar.items.get(self.sidebar_selected)?;
        match item {
            SidebarItem::ChannelsHeader { profile } => {
                Some((profile.to_string(), SidebarSection::Channels))
            }
            SidebarItem::Channel { profile, .. } => {
                Some((profile.to_string(), SidebarSection::Channels))
            }
            SidebarItem::ChatsHeader { profile } => {
                Some((profile.to_string(), SidebarSection::Chats))
            }
            SidebarItem::Chat { profile, .. } => {
                Some((profile.to_string(), SidebarSection::Chats))
            }
            _ => None,
        }
    }

    /// Build the sidebar items tree from the view model.
    fn build_sidebar_items(&self, vm: &AppViewModel) -> Vec<SidebarItem> {
        let mut items = Vec::new();

        for network in &vm.networks {
            let profile: Rc<str> = Rc::from(network.name.as_str());

            // Profile header and status buffer entry.
            items.push(SidebarItem::Profile {
                name: Rc::clone(&profile),
            });
            items.push(SidebarItem::Status {
                profile: Rc::clone(&profile),
            });

            // Collect channels and chats for this profile
            let mut channels: Vec<String> = Vec::new();
            let mut chats: Vec<String> = Vec::new();

            for buf in &vm.buffers {
                if buf.id.network.as_str() != &*profile || buf.id.name == "status" {
                    continue;
                }
                if buf.id.name.starts_with('#') || buf.id.name.starts_with('&') {
                    channels.push(buf.id.name.clone());
                } else {
                    chats.push(buf.id.name.clone());
                }
            }

            // Channels section (if any)
            if !channels.is_empty() {
                let collapsed = self
                    .sidebar_collapse
                    .get(&(profile.to_string(), SidebarSection::Channels))
                    .copied()
                    .unwrap_or(false);

                items.push(SidebarItem::ChannelsHeader {
                    profile: Rc::clone(&profile),
                });

                if !collapsed {
                    for chan in channels {
                        items.push(SidebarItem::Channel {
                            profile: Rc::clone(&profile),
                            name: chan,
                        });
                    }
                }
            }

            // Chats section (if any)
            if !chats.is_empty() {
                let collapsed = self
                    .sidebar_collapse
                    .get(&(profile.to_string(), SidebarSection::Chats))
                    .copied()
                    .unwrap_or(false);

                items.push(SidebarItem::ChatsHeader {
                    profile: Rc::clone(&profile),
                });

                if !collapsed {
                    for chat in chats {
                        items.push(SidebarItem::Chat {
                            profile: Rc::clone(&profile),
                            name: chat,
                        });
                    }
                }
            }
        }

        items
    }

    fn open_pm_from_nicklist(&mut self) -> Vec<UiIntent> {
        let Some(profile) = self.active_profile.clone() else {
            return Vec::new();
        };
        let Some(target) = self.nick_refs.get(self.nick_selected).cloned() else {
            return Vec::new();
        };
        self.focus_pane = FocusPane::Input;
        vec![UiIntent::SubmitLine(format!("/query {profile} {target}"))]
    }

    fn lookup_action(&self, key: KeyEvent) -> Option<UiAction> {
        let canonical = canonical_key(key);
        self.keymap_actions.get(&canonical).copied()
    }
}

fn flatten_keymap(keymap: &Keymap) -> HashMap<String, UiAction> {
    let mut map = HashMap::new();
    for binding in keymap
        .global
        .iter()
        .chain(keymap.input.iter())
        .chain(keymap.buffer.iter())
    {
        if let Some(action) = UiAction::from_keymap_str(&binding.action) {
            map.insert(binding.key.to_ascii_lowercase(), action);
        }
    }
    map
}
