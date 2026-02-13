use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use tirc_config::{Keymap, Theme};
use tirc_core::{AppViewModel, UiAction, UiEvent};

use crate::{input::{InputEditor, InputHistory}, palette::PaletteEngine};

/// Intent emitted by the UI layer into the app reducer.
#[derive(Debug, Clone)]
pub enum UiIntent {
    Event(UiEvent),
    SubmitLine(String),
    ApplyTheme(String),
    ApplyKeymap(String),
}

#[derive(Debug, Clone, Default)]
struct SelectionEditor {
    open: bool,
    options: Vec<String>,
    selected: usize,
    active: String,
}

impl SelectionEditor {
    fn with_options(options: Vec<String>, active: String) -> Self {
        let selected = options
            .iter()
            .position(|option| option == &active)
            .unwrap_or(0);

        Self {
            open: false,
            options,
            selected,
            active,
        }
    }

    fn set_options(&mut self, options: Vec<String>, active: String) {
        self.options = options;
        self.active = active;
        self.selected = self
            .options
            .iter()
            .position(|option| option == &self.active)
            .unwrap_or(0);
    }

    fn move_up(&mut self) {
        if self.options.is_empty() {
            self.selected = 0;
            return;
        }

        if self.selected == 0 {
            self.selected = self.options.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.options.is_empty() {
            self.selected = 0;
            return;
        }

        self.selected = (self.selected + 1) % self.options.len();
    }

    fn selected_value(&self) -> Option<String> {
        self.options.get(self.selected).cloned()
    }
}

enum EditorKind {
    Theme,
    Keymap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PaletteMode {
    Normal,
    JoinProfile,
    JoinChannel { profile: String },
    MsgProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ComposerMode {
    JoinChannel { profile: String },
    MsgTarget { profile: String },
    MsgBody { profile: String, target: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Servers,
    Buffers,
    Chat,
    Nicklist,
    Input,
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
    server_selected: usize,
    buffer_selected: usize,
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
            server_selected: 0,
            buffer_selected: 0,
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
        
        if network_count != self.last_network_count || connected_count != self.last_connected_count {
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
                self.buffer_refs.push((buffer.id.network.clone(), buffer.id.name.clone()));
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
        let left_area = if self.show_left_sidebar { Some(top[0]) } else { None };
        let chat_idx = if self.show_left_sidebar { 1 } else { 0 };
        let chat_area = top[chat_idx];
        let right_area = if self.show_right_sidebar { Some(top[top.len() - 1]) } else { None };

        // Only create left layout if sidebar is visible
        let left = left_area.map(|area| {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(7), Constraint::Min(4)])
                .split(area)
        });

        // Clamp selection indices after potential cache updates
        if self.server_names.is_empty() {
            self.server_selected = 0;
        } else {
            self.server_selected = self
                .server_selected
                .min(self.server_names.len().saturating_sub(1));
        }

        if self.buffer_refs.is_empty() {
            self.buffer_selected = 0;
        } else {
            self.buffer_selected = self
                .buffer_selected
                .min(self.buffer_refs.len().saturating_sub(1));
        }

        let active_focus = vm
            .active_buffer
            .as_ref()
            .map(|buffer| (buffer.id.network.clone(), buffer.id.name.clone()));
        if let Some((network, name)) = active_focus.clone() {
            if let Some(index) =
                self.buffer_refs
                    .iter()
                    .position(|(candidate_network, candidate_name)| {
                        candidate_network == &network && candidate_name == &name
                    })
            {
                self.buffer_selected = index;
            }

            if let Some(index) = self
                .server_names
                .iter()
                .position(|candidate| candidate == &network)
            {
                self.server_selected = index;
            }
        }

        let network_items = vm
            .networks
            .iter()
            .map(|network| {
                let icon = if network.connected { "*" } else { "o" };
                ListItem::new(format!("{icon} {}", network.name))
            })
            .collect::<Vec<_>>();

        let mut server_state = ListState::default();
        if !network_items.is_empty() {
            server_state.select(Some(self.server_selected));
        }

        // Render left sidebar if visible
        if let Some(ref left) = left {
            frame.render_stateful_widget(
                List::new(network_items)
                    .highlight_style(focus_style(
                        &self.theme,
                        self.focus_pane == FocusPane::Servers,
                    ))
                    .block(
                        Block::default()
                            .title("Servers")
                            .borders(Borders::ALL)
                            .border_style(border_style(
                                &self.theme,
                                self.focus_pane == FocusPane::Servers,
                            )),
                    ),
                left[0],
                &mut server_state,
            );
        }

        let buffer_items = vm
            .buffers
            .iter()
            .map(|buffer| {
                let unread = if buffer.unread > 0 {
                    format!(" ({})", buffer.unread)
                } else {
                    String::new()
                };
                ListItem::new(format!(
                    "{}/{} [{}]{}",
                    buffer.id.network, buffer.id.name, buffer.nick_count, unread
                ))
            })
            .collect::<Vec<_>>();

        let mut buffer_state = ListState::default();
        if !buffer_items.is_empty() {
            buffer_state.select(Some(self.buffer_selected));
        }

        if let Some(ref left) = left {
            frame.render_stateful_widget(
                List::new(buffer_items)
                    .highlight_style(focus_style(
                        &self.theme,
                        self.focus_pane == FocusPane::Buffers,
                    ))
                    .block(
                        Block::default()
                            .title("Buffers")
                            .borders(Borders::ALL)
                            .border_style(border_style(
                                &self.theme,
                                self.focus_pane == FocusPane::Buffers,
                            )),
                    ),
                left[1],
                &mut buffer_state,
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

        let nick_items = vm
            .active_buffer
            .as_ref()
            .map(|buffer| {
                self.nick_refs = buffer.nicklist.clone();
                buffer
                    .nicklist
                    .iter()
                    .map(|nick| ListItem::new(nick.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                self.nick_refs.clear();
                Vec::new()
            });
        if self.nick_refs.is_empty() {
            self.nick_selected = 0;
        } else {
            self.nick_selected = self
                .nick_selected
                .min(self.nick_refs.len().saturating_sub(1));
        }

        let nick_count = self.nick_refs.len();
        let mut nick_state = ListState::default();
        if !nick_items.is_empty() {
            nick_state.select(Some(self.nick_selected));
        }

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
                            .title(format!("Nicklist [{nick_count}]"))
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
            self.render_help_overlay(frame, area);
        }

        if self.palette_open {
            self.render_palette(frame, area);
        }

        if self.theme_editor.open {
            self.render_selection_editor(frame, area, "Theme Editor", &self.theme_editor);
        }

        if self.keymap_editor.open {
            self.render_selection_editor(frame, area, "Keymap Editor", &self.keymap_editor);
        }

        if self.close_confirm_open {
            self.render_close_confirmation(frame, area);
        }
    }

    pub fn handle_terminal_event(&mut self, event: Event) -> Vec<UiIntent> {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.scroll_up_lines(3);
                    vec![UiIntent::Event(UiEvent::Tick)]
                }
                MouseEventKind::ScrollDown => {
                    self.scroll_down_lines(3);
                    vec![UiIntent::Event(UiEvent::Tick)]
                }
                _ => Vec::new(),
            },
            Event::Resize(width, height) => {
                vec![UiIntent::Event(UiEvent::Resize { width, height })]
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

        match key.code {
            KeyCode::Tab => {
                self.cycle_focus_pane(1);
                return vec![UiIntent::Event(UiEvent::Tick)];
            }
            KeyCode::BackTab => {
                self.cycle_focus_pane(-1);
                return vec![UiIntent::Event(UiEvent::Tick)];
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
            panes.push(FocusPane::Servers);
            panes.push(FocusPane::Buffers);
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
            FocusPane::Servers => {
                if self.server_names.is_empty() {
                    return Vec::new();
                }
                if direction < 0 {
                    self.server_selected = self
                        .server_selected
                        .checked_sub(1)
                        .unwrap_or(self.server_names.len().saturating_sub(1));
                } else {
                    self.server_selected = (self.server_selected + 1) % self.server_names.len();
                }

                let Some(profile) = self.server_names.get(self.server_selected).cloned() else {
                    return Vec::new();
                };
                vec![UiIntent::SubmitLine(format!("/buffer {profile} status"))]
            }
            FocusPane::Buffers => {
                if self.buffer_refs.is_empty() {
                    return Vec::new();
                }
                if direction < 0 {
                    self.buffer_selected = self
                        .buffer_selected
                        .checked_sub(1)
                        .unwrap_or(self.buffer_refs.len().saturating_sub(1));
                } else {
                    self.buffer_selected = (self.buffer_selected + 1) % self.buffer_refs.len();
                }
                let Some((profile, buffer)) = self.buffer_refs.get(self.buffer_selected).cloned()
                else {
                    return Vec::new();
                };
                vec![UiIntent::SubmitLine(format!("/buffer {profile} {buffer}"))]
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

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(80, 70, area);
        frame.render_widget(Clear, popup);

        let lines = vec![
            Line::from("TIRC Help"),
            Line::from(""),
            Line::from("Dismiss: Esc, q, or Ctrl-H"),
            Line::from("Ctrl-P: Open command palette"),
            Line::from("Ctrl-H: Toggle help"),
            Line::from("Ctrl-T: Open theme editor"),
            Line::from("Ctrl-Y: Open keymap editor"),
            Line::from("Ctrl-X: Close focused buffer (with confirmation)"),
            Line::from("Ctrl-B: Toggle left sidebar (servers/buffers)"),
            Line::from("Ctrl-R: Toggle right sidebar (nicklist)"),
            Line::from("Alt-Left / Alt-Right: Cycle buffers"),
            Line::from("Ctrl-A / Ctrl-E: Start/end of input"),
            Line::from("Ctrl-K / Ctrl-U / Ctrl-W: Kill operations"),
            Line::from("/join /part /query /msg /connect /disconnect /theme /keymap /quit"),
        ];

        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().title("Help").borders(Borders::ALL))
                .wrap(Wrap { trim: true }),
            popup,
        );
    }

    fn render_palette(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(70, 55, area);
        frame.render_widget(Clear, popup);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(popup);

        let palette_title = match self.palette_mode {
            PaletteMode::Normal => "Command Palette",
            PaletteMode::JoinProfile => "Join: Choose Profile",
            PaletteMode::JoinChannel { .. } => "Join: Choose Channel",
            PaletteMode::MsgProfile => "Message: Choose Profile",
        };

        frame.render_widget(
            Paragraph::new(self.palette.query.clone())
                .block(Block::default().title(palette_title).borders(Borders::ALL)),
            layout[0],
        );

        let items = self
            .palette
            .results
            .iter()
            .enumerate()
            .take(12)
            .map(|(index, item)| {
                if index == self.palette.selected {
                    ListItem::new(Line::from(Span::styled(
                        item.clone(),
                        Style::default()
                            .bg(parse_color(&self.theme.palette.accent))
                            .fg(parse_color(&self.theme.palette.background))
                            .add_modifier(Modifier::BOLD),
                    )))
                } else {
                    ListItem::new(item.clone())
                }
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            List::new(items).block(Block::default().title("Actions").borders(Borders::ALL)),
            layout[1],
        );
    }

    fn render_selection_editor(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        editor: &SelectionEditor,
    ) {
        let popup = centered_rect(60, 60, area);
        frame.render_widget(Clear, popup);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(4),
                Constraint::Length(1),
            ])
            .split(popup);

        frame.render_widget(
            Paragraph::new(format!(
                "Current: {} | Up/Down select | Enter apply | Esc close",
                editor.active
            ))
            .block(Block::default().title(title).borders(Borders::ALL)),
            layout[0],
        );

        let items = editor
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let mut label = option.clone();
                if option == &editor.active {
                    label.push_str(" (active)");
                }

                if index == editor.selected {
                    ListItem::new(Line::from(Span::styled(
                        label,
                        Style::default()
                            .bg(parse_color(&self.theme.palette.accent))
                            .fg(parse_color(&self.theme.palette.background))
                            .add_modifier(Modifier::BOLD),
                    )))
                } else {
                    ListItem::new(label)
                }
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            List::new(items).block(Block::default().title("Available").borders(Borders::ALL)),
            layout[1],
        );

        frame.render_widget(
            Paragraph::new("Changes apply immediately and are persisted to config.toml"),
            layout[2],
        );
    }

    fn render_close_confirmation(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(46, 20, area);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new("Close the currently focused buffer? (y/n)")
                .block(
                    Block::default()
                        .title("Confirm Close")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: true }),
            popup,
        );
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

fn parse_color(value: &str) -> Color {
    if let Some(hex) = value.strip_prefix('#')
        && hex.len() == 6
        && let Ok(raw) = u32::from_str_radix(hex, 16)
    {
        let r = ((raw >> 16) & 0xff) as u8;
        let g = ((raw >> 8) & 0xff) as u8;
        let b = (raw & 0xff) as u8;
        return Color::Rgb(r, g, b);
    }

    match value.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" => Color::Gray,
        "darkgray" => Color::DarkGray,
        "white" => Color::White,
        _ => Color::Reset,
    }
}

fn border_style(theme: &Theme, focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(parse_color(&theme.palette.accent))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn focus_style(theme: &Theme, focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(parse_color(&theme.palette.accent))
            .fg(parse_color(&theme.palette.background))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn canonical_key(key: KeyEvent) -> String {
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift");
    }

    let code = match key.code {
        KeyCode::Char(c) => c.to_ascii_lowercase().to_string(),
        KeyCode::Enter => "enter".to_owned(),
        KeyCode::Esc => "esc".to_owned(),
        KeyCode::Backspace => "backspace".to_owned(),
        KeyCode::Delete => "delete".to_owned(),
        KeyCode::Left => "left".to_owned(),
        KeyCode::Right => "right".to_owned(),
        KeyCode::Up => "up".to_owned(),
        KeyCode::Down => "down".to_owned(),
        KeyCode::PageUp => "pageup".to_owned(),
        KeyCode::PageDown => "pagedown".to_owned(),
        KeyCode::Home => "home".to_owned(),
        KeyCode::End => "end".to_owned(),
        _ => "unknown".to_owned(),
    };

    if parts.is_empty() {
        code
    } else {
        format!("{}-{code}", parts.join("-"))
    }
}
