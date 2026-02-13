use std::{
    env, fs,
    path::{Path, PathBuf},
};

use directories::BaseDirs;

use crate::error::ConfigError;

/// Runtime paths used by TIRC.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub config_file: PathBuf,
    pub themes_dir: PathBuf,
    pub keymaps_dir: PathBuf,
    pub session_file: PathBuf,
    pub logs_dir: PathBuf,
}

impl AppPaths {
    /// Builds an XDG-first path layout suitable for macOS and Linux.
    pub fn discover() -> Result<Self, ConfigError> {
        let base = BaseDirs::new().ok_or(ConfigError::HomeDirectoryUnavailable)?;
        let home = base.home_dir();

        let config_dir = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("tirc");

        let state_dir = env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("state"))
            .join("tirc");

        let cache_dir = env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"))
            .join("tirc");

        Ok(Self {
            config_file: config_dir.join("config.toml"),
            themes_dir: config_dir.join("themes"),
            keymaps_dir: config_dir.join("keymaps"),
            session_file: state_dir.join("session.json"),
            logs_dir: state_dir.join("logs"),
            config_dir,
            state_dir,
            cache_dir,
        })
    }

    /// Ensures all configured directories exist.
    pub fn ensure_dirs(&self) -> Result<(), ConfigError> {
        fs::create_dir_all(&self.config_dir)?;
        fs::create_dir_all(&self.state_dir)?;
        fs::create_dir_all(&self.cache_dir)?;
        fs::create_dir_all(&self.themes_dir)?;
        fs::create_dir_all(&self.keymaps_dir)?;
        fs::create_dir_all(&self.logs_dir)?;
        Ok(())
    }
}

/// Validate secure file permissions for files containing secrets.
pub fn enforce_private_file(path: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if path.exists() {
            let mode = fs::metadata(path)?.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(ConfigError::InsecurePermissions {
                    path: path.to_path_buf(),
                });
            }
        }
    }

    Ok(())
}
