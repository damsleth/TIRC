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

/// UI action identifiers - compile-time safe replacement for magic strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UiAction {
    AppQuit,
    PaletteToggle,
    PaletteClose,
    PaletteExecute,
    HelpToggle,
    HelpClose,
    BufferNext,
    BufferPrev,
    BufferCloseConfirm,
    BufferCloseCurrent,
    ThemeEditorToggle,
    KeymapEditorToggle,
    ConfigReload,
    InputHome,
    InputEnd,
    InputKillToEnd,
    InputKillToStart,
    InputKillWord,
    ScrollUpPage,
    ScrollDownPage,
    ScrollUpLine,
    ScrollDownLine,
}

impl UiAction {
    /// Parse action from keymap string representation.
    pub fn from_keymap_str(s: &str) -> Option<Self> {
        match s {
            "app.quit" => Some(Self::AppQuit),
            "palette.toggle" => Some(Self::PaletteToggle),
            "palette.close" => Some(Self::PaletteClose),
            "palette.execute" => Some(Self::PaletteExecute),
            "help.toggle" => Some(Self::HelpToggle),
            "help.close" => Some(Self::HelpClose),
            "buffer.next" => Some(Self::BufferNext),
            "buffer.prev" => Some(Self::BufferPrev),
            "buffer.close.confirm" => Some(Self::BufferCloseConfirm),
            "buffer.close.current" => Some(Self::BufferCloseCurrent),
            "theme.editor.toggle" => Some(Self::ThemeEditorToggle),
            "keymap.editor.toggle" => Some(Self::KeymapEditorToggle),
            "config.reload" => Some(Self::ConfigReload),
            "input.home" => Some(Self::InputHome),
            "input.end" => Some(Self::InputEnd),
            "input.kill_to_end" => Some(Self::InputKillToEnd),
            "input.kill_to_start" => Some(Self::InputKillToStart),
            "input.kill_word" => Some(Self::InputKillWord),
            "scroll.up_page" => Some(Self::ScrollUpPage),
            "scroll.down_page" => Some(Self::ScrollDownPage),
            "scroll.up_line" => Some(Self::ScrollUpLine),
            "scroll.down_line" => Some(Self::ScrollDownLine),
            _ => None,
        }
    }
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
    /// A user quit the network
    UserQuit {
        network: String,
        nick: String,
        reason: Option<String>,
        channels: Vec<String>,
    },
    /// A user was kicked from a channel
    Kicked {
        network: String,
        channel: String,
        kicked_nick: String,
        by_nick: Option<String>,
        reason: Option<String>,
        /// True if we were the one kicked
        was_self: bool,
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
    /// CTCP ACTION (/me command)
    Action {
        network: String,
        buffer: String,
        sender: Option<String>,
        body: String,
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
    Action(UiAction),
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
