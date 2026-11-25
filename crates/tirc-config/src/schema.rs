use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
    error::ConfigError,
    paths::{AppPaths, enforce_private_file},
};

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub ui: UiConfig,
    pub keymap: KeymapConfig,
    pub logging: LoggingConfig,
    pub profiles: Vec<ServerProfile>,
    pub notifications: NotificationConfig,
    pub performance: PerformanceConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ui: UiConfig::default(),
            keymap: KeymapConfig::default(),
            logging: LoggingConfig::default(),
            profiles: vec![sample_profile()],
            notifications: NotificationConfig::default(),
            performance: PerformanceConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: String,
    pub show_nicklist: bool,
    pub show_server_list: bool,
    pub max_scrollback_lines: usize,
    #[serde(default)]
    pub startup: StartupConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "tirc-default".to_owned(),
            show_nicklist: true,
            show_server_list: true,
            max_scrollback_lines: 5_000,
            startup: StartupConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StartupConfig {
    pub hide_buffer_when_empty: bool,
    pub hide_nicklist_when_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeymapConfig {
    pub active_map: String,
}

impl Default for KeymapConfig {
    fn default() -> Self {
        Self {
            active_map: "default".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub enabled: bool,
    pub retention_days: u16,
    pub redact_secrets: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 90,
            redact_secrets: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub bell_on_mention: bool,
    pub bell_on_private_message: bool,
    pub macos_notifications: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            bell_on_mention: true,
            bell_on_private_message: true,
            macos_notifications: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub max_idle_redraw_hz: u8,
    pub max_memory_mb: u16,
    #[serde(default = "default_max_channel_list_entries")]
    pub max_channel_list_entries: usize,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            max_idle_redraw_hz: 4,
            max_memory_mb: 80,
            max_channel_list_entries: default_max_channel_list_entries(),
        }
    }
}

const fn default_max_channel_list_entries() -> usize {
    10_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerProfile {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub nick: String,
    pub user: String,
    pub realname: String,
    pub sasl: Option<SaslConfig>,
    pub autojoin: Vec<String>,
    pub secret: Option<SecretRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaslConfig {
    pub mechanism: SaslMechanism,
    pub account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum SaslMechanism {
    #[default]
    Plain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretRef {
    #[serde(
        default,
        serialize_with = "serialize_zeroizing_option",
        deserialize_with = "deserialize_zeroizing_option"
    )]
    pub password: Option<Zeroizing<String>>,
    #[serde(
        default,
        serialize_with = "serialize_zeroizing_option",
        deserialize_with = "deserialize_zeroizing_option"
    )]
    pub sasl_password: Option<Zeroizing<String>>,
    pub updated_at: Option<DateTime<Utc>>,
}

fn serialize_zeroizing_option<S: serde::Serializer>(
    value: &Option<Zeroizing<String>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(z) => serializer.serialize_some(z.as_str()),
        None => serializer.serialize_none(),
    }
}

fn deserialize_zeroizing_option<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Zeroizing<String>>, D::Error> {
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.map(Zeroizing::new))
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub source_path: PathBuf,
    pub config: AppConfig,
    pub warning_plaintext_secret: bool,
}

/// Load config from an explicit path or default XDG location.
pub fn load_config(paths: &AppPaths, explicit: Option<&Path>) -> Result<LoadedConfig, ConfigError> {
    let source_path = explicit
        .map(Path::to_path_buf)
        .unwrap_or_else(|| paths.config_file.clone());

    if !source_path.exists() {
        write_default_config(&source_path)?;
    }

    enforce_private_file(&source_path)?;

    let raw = fs::read_to_string(&source_path)?;
    let config: AppConfig = toml::from_str(&raw)?;

    let warning_plaintext_secret = config.profiles.iter().any(|profile| {
        profile
            .secret
            .as_ref()
            .map(|s| s.password.is_some() || s.sasl_password.is_some())
            .unwrap_or(false)
    });

    Ok(LoadedConfig {
        source_path,
        config,
        warning_plaintext_secret,
    })
}

/// Persist updated configuration with strict permissions.
pub fn save_config(path: &Path, config: &AppConfig) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let serialized = toml::to_string_pretty(config)?;
    let mut file = fs::File::create(path)?;
    file.write_all(serialized.as_bytes())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

fn write_default_config(path: &Path) -> Result<(), ConfigError> {
    let cfg = AppConfig::default();
    save_config(path, &cfg)
}

fn sample_profile() -> ServerProfile {
    ServerProfile {
        name: "default".to_owned(),
        enabled: true,
        host: "irc.homelien.no".to_owned(),
        port: 6666,
        tls: true,
        nick: "tirc_user".to_owned(),
        user: "tirc".to_owned(),
        realname: "TIRC User".to_owned(),
        sasl: None,
        autojoin: Vec::new(),
        secret: None,
    }
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn writes_and_loads_default_config() {
        let dir = tempdir().expect("tempdir");
        let config_file = dir.path().join("config.toml");
        let paths = AppPaths {
            config_dir: dir.path().to_path_buf(),
            state_dir: dir.path().join("state"),
            cache_dir: dir.path().join("cache"),
            config_file: config_file.clone(),
            themes_dir: dir.path().join("themes"),
            keymaps_dir: dir.path().join("keymaps"),
            session_file: dir.path().join("state").join("session.json"),
            logs_dir: dir.path().join("state").join("logs"),
        };

        paths.ensure_dirs().expect("dirs");
        let loaded = load_config(&paths, Some(&config_file)).expect("load config");
        assert!(config_file.exists());
        assert_eq!(loaded.config.ui.theme, "tirc-default");
        assert_eq!(loaded.config.profiles.len(), 1);
        assert_eq!(loaded.config.profiles[0].name, "default");
        assert!(loaded.config.profiles[0].enabled);
        assert_eq!(loaded.config.profiles[0].host, "irc.homelien.no");
        assert_eq!(loaded.config.profiles[0].port, 6666);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_world_readable_secret_config() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let config_file = dir.path().join("config.toml");
        let cfg = AppConfig::default();
        save_config(&config_file, &cfg).expect("save");
        std::fs::set_permissions(&config_file, std::fs::Permissions::from_mode(0o644))
            .expect("chmod");

        let paths = AppPaths {
            config_dir: dir.path().to_path_buf(),
            state_dir: dir.path().join("state"),
            cache_dir: dir.path().join("cache"),
            config_file: config_file.clone(),
            themes_dir: dir.path().join("themes"),
            keymaps_dir: dir.path().join("keymaps"),
            session_file: dir.path().join("state").join("session.json"),
            logs_dir: dir.path().join("state").join("logs"),
        };

        let err = load_config(&paths, Some(&config_file)).expect_err("should fail permissions");
        assert!(matches!(err, ConfigError::InsecurePermissions { .. }));
    }

    #[test]
    fn startup_config_defaults() {
        let config = AppConfig::default();
        assert!(!config.ui.startup.hide_buffer_when_empty);
        assert!(!config.ui.startup.hide_nicklist_when_empty);
    }

    #[test]
    fn startup_config_serialization() {
        let mut config = AppConfig::default();
        config.ui.startup.hide_buffer_when_empty = true;
        config.ui.startup.hide_nicklist_when_empty = true;

        let serialized = toml::to_string_pretty(&config).expect("serialize");
        let deserialized: AppConfig = toml::from_str(&serialized).expect("deserialize");

        assert!(deserialized.ui.startup.hide_buffer_when_empty);
        assert!(deserialized.ui.startup.hide_nicklist_when_empty);
    }
}
