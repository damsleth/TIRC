use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Top-level app event envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppEvent {
    Network(NetworkEvent),
    Ui(UiEvent),
    Command(CommandEvent),
    Timer(TimerEvent),
}

/// Normalized network-level events produced by connection tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkEvent {
    Connected {
        network: String,
        server: String,
    },
    Disconnected {
        network: String,
        reason: String,
    },
    Joined {
        network: String,
        channel: String,
    },
    Parted {
        network: String,
        channel: String,
        reason: Option<String>,
    },
    NickList {
        network: String,
        channel: String,
        nicks: Vec<String>,
    },
    ChannelListStart {
        network: String,
    },
    ChannelListItem {
        network: String,
        channel: String,
        users: u32,
        topic: String,
    },
    ChannelListEnd {
        network: String,
    },
    Message {
        network: String,
        buffer: String,
        sender: Option<String>,
        body: String,
        tags: BTreeMap<String, String>,
        is_private: bool,
        is_mention: bool,
        ts: DateTime<Utc>,
    },
    System {
        network: String,
        body: String,
        ts: DateTime<Utc>,
    },
}

/// UI-originated events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UiEvent {
    InputChanged(String),
    InputSubmitted,
    Action(String),
    PaletteQuery(String),
    PaletteExecute(String),
    Resize { width: u16, height: u16 },
    Tick,
    QuitRequested,
}

/// Command events from slash parser or command palette.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandEvent {
    Run { source: CommandSource, line: String },
    Succeeded { command: String, detail: String },
    Failed { command: String, error: String },
}

/// Command invocation source.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CommandSource {
    Slash,
    Palette,
}

/// Internal timer events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimerEvent {
    Heartbeat,
    FlushLogs,
    SaveSession,
    Reconnect { network: String },
}
