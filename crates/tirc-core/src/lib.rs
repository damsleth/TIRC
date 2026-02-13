//! Core domain state, events, commands, and persistence primitives for TIRC.

pub mod command;
pub mod event;
pub mod log;
pub mod model;
pub mod session;

pub use command::{CommandOutput, CommandRegistry, CommandSpec, OutboundCommand};
pub use event::{AppEvent, CommandEvent, CommandSource, NetworkEvent, TimerEvent, UiEvent};
pub use log::{LogRecord, LogWriter};
pub use model::{
    ActiveBufferView, AppState, AppViewModel, BufferId, BufferState, BufferView, ChannelListEntry,
    Message, NetworkState, PaletteState,
};
pub use session::{
    ReconnectState, SessionState, build_session_snapshot, load_session, save_session,
};
