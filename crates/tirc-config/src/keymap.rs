use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

/// Keymap file schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keymap {
    pub global: Vec<KeyBinding>,
    pub input: Vec<KeyBinding>,
    pub buffer: Vec<KeyBinding>,
    pub palette: Vec<KeyBinding>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            global: vec![
                KeyBinding::new("ctrl-p", "palette.toggle"),
                KeyBinding::new("ctrl-h", "help.toggle"),
                KeyBinding::new("ctrl-c", "app.quit"),
                KeyBinding::new("ctrl-t", "theme.editor.toggle"),
                KeyBinding::new("ctrl-y", "keymap.editor.toggle"),
                KeyBinding::new("alt-left", "buffer.prev"),
                KeyBinding::new("alt-right", "buffer.next"),
            ],
            input: vec![
                KeyBinding::new("ctrl-a", "input.home"),
                KeyBinding::new("ctrl-e", "input.end"),
                KeyBinding::new("ctrl-k", "input.kill_to_end"),
                KeyBinding::new("ctrl-u", "input.kill_to_start"),
                KeyBinding::new("ctrl-w", "input.kill_word"),
            ],
            buffer: vec![
                KeyBinding::new("pageup", "scroll.up_page"),
                KeyBinding::new("pagedown", "scroll.down_page"),
            ],
            palette: vec![
                KeyBinding::new("esc", "palette.close"),
                KeyBinding::new("enter", "palette.execute"),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBinding {
    pub key: String,
    pub action: String,
}

impl KeyBinding {
    pub fn new(key: &str, action: &str) -> Self {
        Self {
            key: key.to_owned(),
            action: action.to_owned(),
        }
    }
}

/// Load keymap by name from disk or return built-in defaults.
pub fn load_keymap(keymaps_dir: &Path, name: &str) -> Result<Keymap, ConfigError> {
    if name == "default" {
        return Ok(Keymap::default());
    }

    let path = keymaps_dir.join(format!("{name}.toml"));
    if !path.exists() {
        return Ok(Keymap::default());
    }

    let raw = fs::read_to_string(path)?;
    let keymap = toml::from_str(&raw)?;
    Ok(keymap)
}

/// Save keymap to disk.
pub fn save_keymap(
    keymaps_dir: &Path,
    name: &str,
    keymap: &Keymap,
) -> Result<PathBuf, ConfigError> {
    fs::create_dir_all(keymaps_dir)?;
    let path = keymaps_dir.join(format!("{name}.toml"));
    let mut file = fs::File::create(&path)?;
    let serialized = toml::to_string_pretty(keymap)?;
    file.write_all(serialized.as_bytes())?;
    Ok(path)
}

/// Returns available keymap names from built-ins and local files.
pub fn available_keymaps(keymaps_dir: &Path) -> Result<Vec<String>, ConfigError> {
    let mut names = BTreeSet::new();
    names.insert("default".to_owned());

    if keymaps_dir.exists() {
        for entry in fs::read_dir(keymaps_dir)? {
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
