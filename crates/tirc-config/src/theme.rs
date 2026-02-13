use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

/// Runtime theme description loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub palette: ThemePalette,
    pub styles: ThemeStyles,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "tirc-default".to_owned(),
            palette: ThemePalette::default(),
            styles: ThemeStyles::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemePalette {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub mention: String,
    pub warning: String,
    pub error: String,
    pub success: String,
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self {
            background: "#0f1419".to_owned(),
            foreground: "#d1d5db".to_owned(),
            accent: "#2ea8ff".to_owned(),
            mention: "#f59e0b".to_owned(),
            warning: "#f97316".to_owned(),
            error: "#ef4444".to_owned(),
            success: "#22c55e".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeStyles {
    pub border: String,
    pub status: String,
    pub input: String,
    pub server_list: String,
    pub buffer_list: String,
    pub message_meta: String,
}

impl Default for ThemeStyles {
    fn default() -> Self {
        Self {
            border: "fg=accent".to_owned(),
            status: "fg=background,bg=accent,bold".to_owned(),
            input: "fg=foreground,bg=background".to_owned(),
            server_list: "fg=foreground".to_owned(),
            buffer_list: "fg=foreground".to_owned(),
            message_meta: "fg=#9ca3af".to_owned(),
        }
    }
}

/// Load a theme by name from disk, with fallback to built-in defaults.
pub fn load_theme(themes_dir: &Path, name: &str) -> Result<Theme, ConfigError> {
    let mut builtins = built_in_themes();
    if let Some(theme) = builtins.remove(name) {
        return Ok(theme);
    }

    let path = themes_dir.join(format!("{name}.toml"));
    if path.exists() {
        let raw = fs::read_to_string(path)?;
        let theme = toml::from_str(&raw)?;
        return Ok(theme);
    }

    Ok(Theme::default())
}

/// Saves a theme as TOML under the provided directory.
pub fn save_theme(themes_dir: &Path, theme: &Theme) -> Result<PathBuf, ConfigError> {
    fs::create_dir_all(themes_dir)?;
    let path = themes_dir.join(format!("{}.toml", theme.name));
    let serialized = toml::to_string_pretty(theme)?;
    let mut file = fs::File::create(&path)?;
    file.write_all(serialized.as_bytes())?;
    Ok(path)
}

/// Returns available theme names from built-ins and local files.
pub fn available_themes(themes_dir: &Path) -> Result<Vec<String>, ConfigError> {
    let mut names = built_in_themes().into_keys().collect::<BTreeSet<_>>();
    if themes_dir.exists() {
        for entry in fs::read_dir(themes_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }

            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                names.insert(stem.to_owned());
            }
        }
    }

    Ok(names.into_iter().collect())
}

fn built_in_themes() -> BTreeMap<String, Theme> {
    let mut themes = BTreeMap::new();

    let default = Theme::default();
    themes.insert(default.name.clone(), default);

    themes.insert(
        "sunset-console".to_owned(),
        Theme {
            name: "sunset-console".to_owned(),
            palette: ThemePalette {
                background: "#1a120b".to_owned(),
                foreground: "#ffd9b8".to_owned(),
                accent: "#ff7b00".to_owned(),
                mention: "#ffb703".to_owned(),
                warning: "#fb8500".to_owned(),
                error: "#d62828".to_owned(),
                success: "#2a9d8f".to_owned(),
            },
            styles: ThemeStyles::default(),
        },
    );

    themes
}
