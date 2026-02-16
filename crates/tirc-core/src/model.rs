use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::event::{AppEvent, NetworkEvent, UiAction, UiEvent};

/// Check whether a buffer/target name is an IRC channel.
///
/// Per RFC 2812: channels start with `#`, `&`, `+`, or `!`.
/// Channel names cannot contain NUL, BELL, CR, LF, SPACE, COMMA, or COLON.
pub fn is_channel_name(name: &str) -> bool {
    matches!(name.as_bytes().first(), Some(b'#' | b'&' | b'+' | b'!'))
}

/// Unique buffer identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BufferId {
    network: String,
    name: String,
}

impl BufferId {
    pub fn new(network: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            network: network.into(),
            name: name.into(),
        }
    }

    pub fn network(&self) -> &str {
        &self.network
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Chat message stored in local state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub ts: DateTime<Utc>,
    pub sender: Option<String>,
    pub body: String,
    pub mention: bool,
    pub tags: BTreeMap<String, String>,
}

/// Per-buffer state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferState {
    pub id: BufferId,
    pub topic: Option<String>,
    pub nicklist: Vec<String>,
    pub messages: VecDeque<Message>,
    pub unread: u32,
}

impl BufferState {
    pub fn new(id: BufferId) -> Self {
        Self {
            id,
            topic: None,
            nicklist: Vec::new(),
            messages: VecDeque::new(),
            unread: 0,
        }
    }
}

/// Per-network state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkState {
    pub name: String,
    pub server: String,
    pub connected: bool,
    pub channels: BTreeSet<String>,
    pub reconnect_attempts: u32,
    pub last_error: Option<String>,
}

impl NetworkState {
    pub fn new(name: impl Into<String>, server: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            server: server.into(),
            connected: false,
            channels: BTreeSet::new(),
            reconnect_attempts: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelListEntry {
    pub channel: String,
    pub users: u32,
    pub topic: String,
}

impl ChannelListEntry {
    pub fn new(channel: String, users: u32, topic: String) -> Self {
        Self {
            channel,
            users,
            topic,
        }
    }
}

/// Command palette state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub results: Vec<String>,
}

/// In-memory app state reduced from events.
///
/// Not serializable — use purpose-built DTOs (e.g. `SessionState`) for persistence.
#[derive(Debug, Clone)]
pub struct AppState {
    pub networks: BTreeMap<String, NetworkState>,
    pub buffers: BTreeMap<BufferId, BufferState>,
    pub focus: Option<BufferId>,
    pub input: String,
    pub status: String,
    pub warnings: Vec<String>,
    pub should_quit: bool,
    pub help_open: bool,
    pub palette: PaletteState,
    pub max_scrollback_lines: usize,
    pub max_channel_list_entries: usize,
    pub channel_list_cache: BTreeMap<String, Vec<ChannelListEntry>>,
    /// Index from (network, channel_name) -> position in channel_list_cache vec.
    channel_list_index: HashMap<(String, String), usize>,
    channel_list_refresh_pending: BTreeSet<String>,
    status_locked_until: Option<DateTime<Utc>>,
}

impl AppState {
    pub fn new(max_scrollback_lines: usize, max_channel_list_entries: usize) -> Self {
        Self {
            networks: BTreeMap::new(),
            buffers: BTreeMap::new(),
            focus: None,
            input: String::new(),
            status: "Ready".to_owned(),
            warnings: Vec::new(),
            should_quit: false,
            help_open: false,
            palette: PaletteState::default(),
            max_scrollback_lines,
            max_channel_list_entries,
            channel_list_cache: BTreeMap::new(),
            channel_list_index: HashMap::new(),
            channel_list_refresh_pending: BTreeSet::new(),
            status_locked_until: None,
        }
    }

    /// Set status text unless a temporary status lock is active.
    pub fn set_status(&mut self, value: impl Into<String>) {
        if self.is_status_locked() {
            return;
        }
        self.status = value.into();
    }

    /// Set status text and prevent overwrite for at least `duration`.
    pub fn set_status_for(&mut self, value: impl Into<String>, duration: Duration) {
        self.status = value.into();
        if let Ok(delta) = chrono::Duration::from_std(duration) {
            self.status_locked_until = Some(Utc::now() + delta);
        }
    }

    pub fn ensure_buffer(&mut self, network: &str, name: &str) -> BufferId {
        let id = BufferId::new(network.to_owned(), name.to_owned());
        self.buffers
            .entry(id.clone())
            .or_insert_with(|| BufferState::new(id.clone()));
        id
    }

    pub fn push_message(&mut self, network: &str, buffer: &str, message: Message) {
        let id = self.ensure_buffer(network, buffer);
        if let Some(state) = self.buffers.get_mut(&id) {
            if state.messages.len() >= self.max_scrollback_lines {
                state.messages.pop_front();
            }
            state.messages.push_back(message);
            if self.focus.as_ref() != Some(&id) {
                state.unread = state.unread.saturating_add(1);
            }
        }
    }

    pub fn apply(&mut self, event: AppEvent) {
        match event {
            AppEvent::Network(network) => self.apply_network_event(network),
            AppEvent::Ui(ui) => self.apply_ui_event(ui),
            AppEvent::Command(_) => {}
            AppEvent::Timer(_) => {}
        }
    }

    fn apply_network_event(&mut self, event: NetworkEvent) {
        match event {
            NetworkEvent::Connected { network, server } => {
                let status_buffer = self.ensure_buffer(&network, "status");
                self.networks
                    .entry(network.clone())
                    .and_modify(|entry| {
                        entry.connected = true;
                        entry.server = server.clone();
                        entry.reconnect_attempts = 0;
                    })
                    .or_insert_with(|| {
                        let mut s = NetworkState::new(network.clone(), server.clone());
                        s.connected = true;
                        s
                    });
                self.focus.get_or_insert(status_buffer);
                self.set_status(format!("Connected to {network}"));
                self.push_message(
                    &network,
                    "status",
                    Message {
                        ts: Utc::now(),
                        sender: None,
                        body: format!("Connected to {server}"),
                        mention: false,
                        tags: BTreeMap::new(),
                    },
                );
            }
            NetworkEvent::Disconnected { network, reason } => {
                if let Some(state) = self.networks.get_mut(&network) {
                    state.connected = false;
                    state.reconnect_attempts = state.reconnect_attempts.saturating_add(1);
                    state.last_error = Some(reason.clone());
                }
                self.set_status(format!("Disconnected from {network}: {reason}"));
                self.push_message(
                    &network,
                    "status",
                    Message {
                        ts: Utc::now(),
                        sender: None,
                        body: format!("Disconnected: {reason}"),
                        mention: false,
                        tags: BTreeMap::new(),
                    },
                );
            }
            NetworkEvent::Joined { network, channel } => {
                let id = self.ensure_buffer(&network, &channel);
                if let Some(n) = self.networks.get_mut(&network) {
                    n.channels.insert(channel.clone());
                }
                self.focus = Some(id);
                self.set_status(format!("Joined {channel}"));
                self.push_message(
                    &network,
                    "status",
                    Message {
                        ts: Utc::now(),
                        sender: None,
                        body: format!("Joined {channel}"),
                        mention: false,
                        tags: BTreeMap::new(),
                    },
                );
            }
            NetworkEvent::Parted {
                network,
                channel,
                reason,
            } => {
                if let Some(n) = self.networks.get_mut(&network) {
                    n.channels.remove(&channel);
                }
                // Remove the buffer so it disappears from sidebar
                let buf_id = BufferId::new(&network, &channel);
                self.buffers.remove(&buf_id);
                // If parted buffer was focused, move focus to another buffer
                if self.focus.as_ref() == Some(&buf_id) {
                    self.focus = self.buffers.keys().next().cloned();
                }
                self.set_status(format!(
                    "Parted {channel}: {}",
                    reason.unwrap_or_else(|| "-".to_owned())
                ));
                self.push_message(
                    &network,
                    "status",
                    Message {
                        ts: Utc::now(),
                        sender: None,
                        body: format!("Parted {channel}"),
                        mention: false,
                        tags: BTreeMap::new(),
                    },
                );
            }
            NetworkEvent::UserQuit {
                network,
                nick,
                reason,
                channels,
            } => {
                let quit_msg = if let Some(r) = reason {
                    format!("{nick} has quit ({r})")
                } else {
                    format!("{nick} has quit")
                };

                // Remove the user from all channel nick lists and post quit message only to affected channels
                for channel in &channels {
                    let id = BufferId {
                        network: network.clone(),
                        name: channel.clone(),
                    };
                    if let Some(buffer) = self.buffers.get_mut(&id) {
                        // Remove user from nicklist (accounting for mode prefixes like @, +)
                        let len_before = buffer.nicklist.len();
                        buffer.nicklist.retain(|n| {
                            let plain_nick = n.trim_start_matches(&['@', '+', '%', '&', '~'][..]);
                            plain_nick != nick
                        });
                        let was_present = buffer.nicklist.len() < len_before;

                        // Only post quit message if the user was actually in this channel
                        if was_present {
                            buffer.messages.push_back(Message {
                                ts: Utc::now(),
                                sender: None,
                                body: quit_msg.clone(),
                                mention: false,
                                tags: BTreeMap::new(),
                            });
                            // Enforce scrollback limit
                            while buffer.messages.len() > self.max_scrollback_lines {
                                buffer.messages.pop_front();
                            }
                        }
                    }
                }
            }
            NetworkEvent::NickList {
                network,
                channel,
                nicks,
            } => {
                let id = self.ensure_buffer(&network, &channel);
                if let Some(buffer) = self.buffers.get_mut(&id) {
                    buffer.nicklist = nicks;
                }
            }
            NetworkEvent::Kicked {
                network,
                channel,
                kicked_nick,
                by_nick,
                reason,
                was_self,
            } => {
                let kick_msg = match (&by_nick, &reason) {
                    (Some(by), Some(r)) => format!("{kicked_nick} was kicked by {by} ({r})"),
                    (Some(by), None) => format!("{kicked_nick} was kicked by {by}"),
                    (None, Some(r)) => format!("{kicked_nick} was kicked ({r})"),
                    (None, None) => format!("{kicked_nick} was kicked"),
                };

                let id = BufferId {
                    network: network.clone(),
                    name: channel.clone(),
                };

                if let Some(buffer) = self.buffers.get_mut(&id) {
                    // Remove kicked user from nicklist
                    buffer.nicklist.retain(|n| {
                        let plain_nick = n.trim_start_matches(&['@', '+', '%', '&', '~'][..]);
                        plain_nick != kicked_nick
                    });

                    // Post kick message
                    buffer.messages.push_back(Message {
                        ts: Utc::now(),
                        sender: None,
                        body: kick_msg,
                        mention: false,
                        tags: BTreeMap::new(),
                    });

                    // Enforce scrollback limit
                    while buffer.messages.len() > self.max_scrollback_lines {
                        buffer.messages.pop_front();
                    }
                }

                // If we were kicked, remove channel and set status
                if was_self {
                    if let Some(net_state) = self.networks.get_mut(&network) {
                        net_state.channels.remove(&channel);
                    }
                    self.set_status(format!("Kicked from {channel}"));
                }
            }
            NetworkEvent::ModeChange {
                network,
                target,
                modes,
                args,
                set_by,
            } => {
                // Check if this is a channel mode change (affects nick prefixes)
                if is_channel_name(&target) {
                    let id = BufferId {
                        network: network.clone(),
                        name: target.clone(),
                    };

                    if let Some(buffer) = self.buffers.get_mut(&id) {
                        // Parse mode changes and update nick prefixes
                        // Format: +ooo nick1 nick2 nick3 or +o-v nick1 nick2
                        let mut adding = true;
                        let mut arg_idx = 0;

                        for ch in modes.chars() {
                            match ch {
                                '+' => adding = true,
                                '-' => adding = false,
                                'o' | 'v' | 'h' | 'a' | 'q' => {
                                    // These modes take nick arguments
                                    if let Some(nick) = args.get(arg_idx) {
                                        let prefix = match ch {
                                            'q' => '~', // owner
                                            'a' => '&', // admin
                                            'o' => '@', // op
                                            'h' => '%', // halfop
                                            'v' => '+', // voice
                                            _ => continue,
                                        };

                                        // Find and update the nick in nicklist
                                        for entry in &mut buffer.nicklist {
                                            let plain = entry
                                                .trim_start_matches(&['~', '&', '@', '%', '+'][..]);
                                            if plain == nick {
                                                if adding {
                                                    // Add prefix if not already present
                                                    if !entry.starts_with(prefix) {
                                                        *entry = format!("{prefix}{plain}");
                                                    }
                                                } else {
                                                    // Remove prefix
                                                    *entry = plain.to_owned();
                                                }
                                                break;
                                            }
                                        }
                                        arg_idx += 1;
                                    }
                                }
                                'b' | 'e' | 'I' | 'k' => {
                                    // These modes also consume an argument
                                    arg_idx += 1;
                                }
                                'l' => {
                                    // +l takes argument, -l doesn't
                                    if adding {
                                        arg_idx += 1;
                                    }
                                }
                                _ => {} // Other modes don't consume args
                            }
                        }

                        // Display mode change in channel
                        let mode_msg = if let Some(setter) = set_by {
                            format!("{setter} sets mode: {modes} {}", args.join(" "))
                        } else {
                            format!("Mode: {modes} {}", args.join(" "))
                        };

                        buffer.messages.push_back(Message {
                            ts: Utc::now(),
                            sender: None,
                            body: mode_msg,
                            mention: false,
                            tags: BTreeMap::new(),
                        });

                        while buffer.messages.len() > self.max_scrollback_lines {
                            buffer.messages.pop_front();
                        }
                    }
                } else {
                    // User mode change (affects us)
                    self.push_message(
                        &network,
                        "status",
                        Message {
                            ts: Utc::now(),
                            sender: None,
                            body: format!("Mode {modes} set for {target}"),
                            mention: false,
                            tags: BTreeMap::new(),
                        },
                    );
                }
            }
            NetworkEvent::ChannelListStart { network } => {
                // Keep the previous cached list available until the first new item arrives.
                self.channel_list_refresh_pending.insert(network.clone());
                self.channel_list_cache.entry(network).or_default();
            }
            NetworkEvent::ChannelListItem {
                network,
                channel,
                users,
                topic,
            } => {
                let should_reset = self.channel_list_refresh_pending.remove(&network);
                let entries = self.channel_list_cache.entry(network.clone()).or_default();
                if should_reset {
                    entries.clear();
                    // Clear index entries for this network
                    self.channel_list_index
                        .retain(|(net, _), _| net != &network);
                }
                let key = (network.clone(), channel.clone());
                if let Some(&idx) = self.channel_list_index.get(&key) {
                    if let Some(existing) = entries.get_mut(idx) {
                        existing.users = users;
                        existing.topic = topic;
                    }
                } else if entries.len() < self.max_channel_list_entries {
                    let idx = entries.len();
                    entries.push(ChannelListEntry::new(channel.clone(), users, topic));
                    self.channel_list_index.insert(key, idx);
                }
                // Silently drop entries beyond max_channel_list_entries to prevent memory exhaustion
            }
            NetworkEvent::ChannelListEnd { network } => {
                // If the refresh completed without items, clear stale cached entries.
                if self.channel_list_refresh_pending.remove(&network) {
                    self.channel_list_cache.insert(network.clone(), Vec::new());
                    self.channel_list_index
                        .retain(|(net, _), _| net != &network);
                } else {
                    self.channel_list_cache.entry(network).or_default();
                }
            }
            NetworkEvent::Message {
                network,
                buffer,
                sender,
                body,
                tags,
                is_private: _,
                is_mention,
                ts,
            } => {
                self.push_message(
                    &network,
                    &buffer,
                    Message {
                        ts,
                        sender,
                        body,
                        mention: is_mention,
                        tags,
                    },
                );
            }
            NetworkEvent::Action {
                network,
                buffer,
                sender,
                body,
                is_private: _,
                is_mention,
                ts,
            } => {
                // Format CTCP ACTION as "* nick action text"
                let action_body = if let Some(nick) = &sender {
                    format!("* {nick} {body}")
                } else {
                    format!("* {body}")
                };
                self.push_message(
                    &network,
                    &buffer,
                    Message {
                        ts,
                        sender,
                        body: action_body,
                        mention: is_mention,
                        tags: BTreeMap::new(),
                    },
                );
            }
            NetworkEvent::System { network, body, ts } => {
                self.push_message(
                    &network,
                    "status",
                    Message {
                        ts,
                        sender: None,
                        body,
                        mention: false,
                        tags: BTreeMap::new(),
                    },
                );
            }
        }
    }

    fn apply_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::InputChanged(input) => self.input = input,
            UiEvent::InputSubmitted => {}
            UiEvent::Action(action) => match action {
                UiAction::AppQuit => self.should_quit = true,
                UiAction::PaletteToggle => self.palette.open = !self.palette.open,
                UiAction::HelpToggle => self.help_open = !self.help_open,
                UiAction::HelpClose => self.help_open = false,
                UiAction::BufferNext => self.cycle_focus(1),
                UiAction::BufferPrev => self.cycle_focus(-1),
                UiAction::PaletteClose => self.palette.open = false,
                UiAction::PaletteExecute => {}
                // These actions are handled by UI layer, not state
                UiAction::BufferCloseConfirm
                | UiAction::BufferCloseCurrent
                | UiAction::ThemeEditorToggle
                | UiAction::KeymapEditorToggle
                | UiAction::ConfigReload
                | UiAction::InputHome
                | UiAction::InputEnd
                | UiAction::InputKillToEnd
                | UiAction::InputKillToStart
                | UiAction::InputKillWord
                | UiAction::ScrollUpPage
                | UiAction::ScrollDownPage
                | UiAction::ScrollUpLine
                | UiAction::ScrollDownLine
                | UiAction::SidebarLeftToggle
                | UiAction::SidebarRightToggle => {}
            },
            UiEvent::PaletteQuery(query) => self.palette.query = query,
            UiEvent::PaletteExecute(_) => {}
            UiEvent::Resize { width, height } => {
                self.set_status(format!("Resized to {width}x{height}"))
            }
            UiEvent::Tick => {}
            UiEvent::QuitRequested => self.should_quit = true,
        }
    }

    fn is_status_locked(&self) -> bool {
        self.status_locked_until
            .as_ref()
            .is_some_and(|deadline| Utc::now() < *deadline)
    }

    fn cycle_focus(&mut self, direction: i8) {
        if self.buffers.is_empty() {
            return;
        }

        let keys: Vec<BufferId> = self.buffers.keys().cloned().collect();
        if self.focus.is_none() {
            self.focus = keys.first().cloned();
            return;
        }

        let pos = keys
            .iter()
            .position(|id| self.focus.as_ref().is_some_and(|focus| focus == id))
            .unwrap_or(0);
        let next = if direction < 0 {
            pos.checked_sub(1).unwrap_or(keys.len().saturating_sub(1))
        } else {
            (pos + 1) % keys.len()
        };

        self.focus = Some(keys[next].clone());
        if let Some(buffer) = self.buffers.get_mut(&keys[next]) {
            buffer.unread = 0;
        }
    }

    pub fn view_model(&self) -> AppViewModel {
        let buffers = self
            .buffers
            .values()
            .map(|buffer| BufferView {
                id: buffer.id.clone(),
                unread: buffer.unread,
                nick_count: buffer.nicklist.len(),
            })
            .collect::<Vec<_>>();

        let active_buffer = self
            .focus
            .as_ref()
            .and_then(|focus| self.buffers.get(focus))
            .map(|buffer| ActiveBufferView {
                id: buffer.id.clone(),
                topic: buffer.topic.clone(),
                nicklist: buffer.nicklist.clone(),
                lines: Arc::from(buffer.messages.iter().cloned().collect::<Vec<_>>()),
            });

        AppViewModel {
            networks: self.networks.values().cloned().collect(),
            buffers,
            active_buffer,
            status: self.status.clone(),
            input: self.input.clone(),
            warnings: self.warnings.clone(),
            palette: self.palette.clone(),
            channel_list_cache: self.channel_list_cache.clone(),
            help_open: self.help_open,
        }
    }
}

/// Lightweight view model for rendering.
#[derive(Debug, Clone)]
pub struct AppViewModel {
    pub networks: Vec<NetworkState>,
    pub buffers: Vec<BufferView>,
    pub active_buffer: Option<ActiveBufferView>,
    pub status: String,
    pub input: String,
    pub warnings: Vec<String>,
    pub palette: PaletteState,
    pub channel_list_cache: BTreeMap<String, Vec<ChannelListEntry>>,
    pub help_open: bool,
}

#[derive(Debug, Clone)]
pub struct BufferView {
    pub id: BufferId,
    pub unread: u32,
    pub nick_count: usize,
}

#[derive(Debug, Clone)]
pub struct ActiveBufferView {
    pub id: BufferId,
    pub topic: Option<String>,
    pub nicklist: Vec<String>,
    pub lines: Arc<Vec<Message>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        AppState::new(100, 10) // 100 scrollback, max 10 channel list entries
    }

    #[test]
    fn test_channel_list_cache_bounded() {
        let mut state = test_state();

        // Add 15 entries - should only keep 10
        for i in 0..15 {
            state.apply(AppEvent::Network(NetworkEvent::ChannelListItem {
                network: "test".to_string(),
                channel: format!("#channel{}", i),
                users: i as u32,
                topic: String::new(),
            }));
        }

        let entries = state.channel_list_cache.get("test").unwrap();
        assert_eq!(entries.len(), 10, "Should be bounded to max 10 entries");

        // First 10 channels should be present
        assert_eq!(entries[0].channel, "#channel0");
        assert_eq!(entries[9].channel, "#channel9");
    }

    #[test]
    fn test_channel_list_updates_existing() {
        let mut state = test_state();

        // Add a channel
        state.apply(AppEvent::Network(NetworkEvent::ChannelListItem {
            network: "test".to_string(),
            channel: "#rust".to_string(),
            users: 100,
            topic: "Old topic".to_string(),
        }));

        // Update same channel
        state.apply(AppEvent::Network(NetworkEvent::ChannelListItem {
            network: "test".to_string(),
            channel: "#rust".to_string(),
            users: 150,
            topic: "New topic".to_string(),
        }));

        let entries = state.channel_list_cache.get("test").unwrap();
        assert_eq!(entries.len(), 1, "Should not duplicate");
        assert_eq!(entries[0].users, 150);
        assert_eq!(entries[0].topic, "New topic".to_string());
    }

    #[test]
    fn test_channel_list_start_keeps_previous_cache_until_first_item() {
        let mut state = test_state();
        state.apply(AppEvent::Network(NetworkEvent::ChannelListItem {
            network: "test".to_string(),
            channel: "#old".to_string(),
            users: 10,
            topic: "old topic".to_string(),
        }));

        state.apply(AppEvent::Network(NetworkEvent::ChannelListStart {
            network: "test".to_string(),
        }));

        let entries = state.channel_list_cache.get("test").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].channel, "#old");

        state.apply(AppEvent::Network(NetworkEvent::ChannelListItem {
            network: "test".to_string(),
            channel: "#new".to_string(),
            users: 20,
            topic: "new topic".to_string(),
        }));

        let entries = state.channel_list_cache.get("test").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].channel, "#new");
    }

    #[test]
    fn test_channel_list_end_without_items_clears_cache() {
        let mut state = test_state();
        state.apply(AppEvent::Network(NetworkEvent::ChannelListItem {
            network: "test".to_string(),
            channel: "#stale".to_string(),
            users: 10,
            topic: "stale".to_string(),
        }));

        state.apply(AppEvent::Network(NetworkEvent::ChannelListStart {
            network: "test".to_string(),
        }));
        state.apply(AppEvent::Network(NetworkEvent::ChannelListEnd {
            network: "test".to_string(),
        }));

        let entries = state.channel_list_cache.get("test").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_user_quit_removes_from_nicklist() {
        let mut state = test_state();

        // Setup: create network and buffer with users
        state.networks.insert(
            "test".to_string(),
            NetworkState::new("test", "irc.test.net"),
        );
        state.networks.get_mut("test").unwrap().connected = true;

        let buffer_id = BufferId::new("test", "#channel");
        let mut buffer = BufferState::new(buffer_id.clone());
        buffer.nicklist = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ];
        state.buffers.insert(buffer_id.clone(), buffer);

        // User quits
        state.apply(AppEvent::Network(NetworkEvent::UserQuit {
            network: "test".to_string(),
            nick: "bob".to_string(),
            reason: Some("Goodbye".to_string()),
            channels: vec!["#channel".to_string()],
        }));

        let buffer = state.buffers.get(&buffer_id).unwrap();
        assert_eq!(buffer.nicklist.len(), 2);
        assert!(!buffer.nicklist.contains(&"bob".to_string()));
    }

    #[test]
    fn test_kicked_removes_from_nicklist() {
        let mut state = test_state();

        // Setup
        state.networks.insert(
            "test".to_string(),
            NetworkState::new("test", "irc.test.net"),
        );
        state.networks.get_mut("test").unwrap().connected = true;

        let buffer_id = BufferId::new("test", "#channel");
        let mut buffer = BufferState::new(buffer_id.clone());
        buffer.nicklist = vec!["alice".to_string(), "bob".to_string()];
        state.buffers.insert(buffer_id.clone(), buffer);

        // User kicked
        state.apply(AppEvent::Network(NetworkEvent::Kicked {
            network: "test".to_string(),
            channel: "#channel".to_string(),
            kicked_nick: "alice".to_string(),
            by_nick: Some("op".to_string()),
            reason: Some("rule violation".to_string()),
            was_self: false,
        }));

        let buffer = state.buffers.get(&buffer_id).unwrap();
        assert_eq!(buffer.nicklist.len(), 1);
        assert!(!buffer.nicklist.contains(&"alice".to_string()));
    }
}
