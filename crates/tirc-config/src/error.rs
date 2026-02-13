use std::path::PathBuf;

use thiserror::Error;

/// Errors raised by TIRC configuration and path handling.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("home directory is not available")]
    HomeDirectoryUnavailable,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("config parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("config serialization error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("insecure permissions on {path}: expected 0600 or stricter")]
    InsecurePermissions { path: PathBuf },
}
