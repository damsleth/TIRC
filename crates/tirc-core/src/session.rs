use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;

use crate::model::BufferId;

/// Serialized session state used for restore across restarts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub open_buffers: Vec<BufferId>,
    pub focused_buffer: Option<BufferId>,
    pub reconnect: Vec<ReconnectState>,
    pub selected_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectState {
    pub network: String,
    pub attempt: u32,
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Load a session snapshot from disk if present.
pub async fn load_session(path: &Path) -> Result<SessionState, SessionError> {
    if !path.exists() {
        return Ok(SessionState::default());
    }

    let raw = fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&raw)?)
}

/// Save a session snapshot to disk.
pub async fn save_session(path: &Path, state: &SessionState) -> Result<(), SessionError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let serialized = serde_json::to_string_pretty(state)?;
    fs::write(path, serialized).await?;
    Ok(())
}

/// Build session snapshot from app state.
pub fn build_session_snapshot(state: &crate::model::AppState) -> SessionState {
    SessionState {
        open_buffers: state.buffers.keys().cloned().collect(),
        focused_buffer: state.focus.clone(),
        reconnect: state
            .networks
            .values()
            .map(|network| ReconnectState {
                network: network.name.clone(),
                attempt: network.reconnect_attempts,
            })
            .collect(),
        selected_profile: None,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::model::BufferId;

    #[tokio::test]
    async fn session_round_trip() {
        let dir = tempdir().expect("tempdir");
        let file = dir.path().join("session.json");

        let expected = SessionState {
            open_buffers: vec![
                BufferId::new("net-a", "#rust"),
                BufferId::new("net-a", "status"),
            ],
            focused_buffer: Some(BufferId::new("net-a", "#rust")),
            reconnect: vec![ReconnectState {
                network: "net-a".to_owned(),
                attempt: 2,
            }],
            selected_profile: Some("default".to_owned()),
        };

        save_session(&file, &expected).await.expect("save");
        let loaded = load_session(&file).await.expect("load");
        assert_eq!(loaded.open_buffers.len(), 2);
        assert_eq!(loaded.focused_buffer, expected.focused_buffer);
        assert_eq!(loaded.reconnect[0].attempt, 2);
        assert_eq!(loaded.selected_profile.as_deref(), Some("default"));
    }
}
