//! Configuration loading and persistence for TIRC.

pub mod error;
pub mod keymap;
pub mod paths;
pub mod schema;
pub mod theme;

pub use error::ConfigError;
pub use keymap::{KeyBinding, Keymap, load_keymap, save_keymap};
pub use paths::{AppPaths, enforce_private_file};
pub use schema::{
    AppConfig, KeymapConfig, LoadedConfig, LoggingConfig, NotificationConfig, PerformanceConfig,
    SaslConfig, SaslMechanism, SecretRef, ServerProfile, UiConfig, load_config, save_config,
};
pub use theme::{Theme, ThemePalette, ThemeStyles, load_theme, save_theme};
