use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::event::{AppEvent, NetworkEvent, UiEvent};

/// Unique buffer identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BufferId {
    pub network: String,
    pub name: String,
}

impl BufferId {
    pub fn new(network: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            network: network.into(),
            name: name.into(),
        }
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

/// Command palette state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub results: Vec<String>,
}

impl Default for PaletteState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected: 0,
            results: Vec::new(),
        }
    }
}

/// In-memory app state reduced from events.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl AppState {
    pub fn new(max_scrollback_lines: usize) -> Self {
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
            AppEvent::Command(command) => self.status = format!("Command: {command:?}"),
            AppEvent::Timer(timer) => self.status = format!("Timer: {timer:?}"),
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
                self.status = format!("Connected to {network}");
            }
            NetworkEvent::Disconnected { network, reason } => {
                if let Some(state) = self.networks.get_mut(&network) {
                    state.connected = false;
                    state.reconnect_attempts = state.reconnect_attempts.saturating_add(1);
                    state.last_error = Some(reason.clone());
                }
                self.status = format!("Disconnected from {network}: {reason}");
            }
            NetworkEvent::Joined { network, channel } => {
                let id = self.ensure_buffer(&network, &channel);
                if let Some(n) = self.networks.get_mut(&network) {
                    n.channels.insert(channel.clone());
                }
                self.focus = Some(id);
                self.status = format!("Joined {channel}");
            }
            NetworkEvent::Parted {
                network,
                channel,
                reason,
            } => {
                if let Some(n) = self.networks.get_mut(&network) {
                    n.channels.remove(&channel);
                }
                self.status = format!(
                    "Parted {channel}: {}",
                    reason.unwrap_or_else(|| "-".to_owned())
                );
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
            UiEvent::InputSubmitted => {
                self.status = "Input submitted".to_owned();
            }
            UiEvent::Action(action) => match action.as_str() {
                "app.quit" => self.should_quit = true,
                "palette.toggle" => self.palette.open = !self.palette.open,
                "help.toggle" => self.help_open = !self.help_open,
                "buffer.next" => self.cycle_focus(1),
                "buffer.prev" => self.cycle_focus(-1),
                _ => self.status = format!("Unknown action: {action}"),
            },
            UiEvent::PaletteQuery(query) => self.palette.query = query,
            UiEvent::PaletteExecute(id) => self.status = format!("Palette selected: {id}"),
            UiEvent::Resize { width, height } => {
                self.status = format!("Resized to {width}x{height}");
            }
            UiEvent::Tick => {}
            UiEvent::QuitRequested => self.should_quit = true,
        }
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
                lines: buffer.messages.iter().cloned().collect(),
            });

        AppViewModel {
            networks: self.networks.values().cloned().collect(),
            buffers,
            active_buffer,
            status: self.status.clone(),
            input: self.input.clone(),
            warnings: self.warnings.clone(),
            palette: self.palette.clone(),
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
    pub lines: Vec<Message>,
}
