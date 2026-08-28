//! User-editable settings, persisted as JSON in `%APPDATA%\Rnote`.
//!
//! The notes themselves never live here — only the small amount of app
//! configuration that has nowhere else to go (hotkey, expiry threshold, the
//! notes folder location itself).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Loopback port for the read-only "R経済圏" API (see `api.rs`). The scheme
/// across the suite is `&lt;vite dev port&gt; + 40000`: Rfinder 41420, Rclip 41430,
/// Rnote 41440, Rhub 41450.
pub const DEFAULT_API_PORT: u16 = 41440;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Keep,
    Scratch,
}

impl Status {
    pub fn dirname(self) -> &'static str {
        match self {
            Status::Keep => "keep",
            Status::Scratch => "scratch",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Where `keep/` and `scratch/` live. Explorer-visible on purpose — the
    /// whole point is that this folder is useful with the app not running.
    pub notes_dir: PathBuf,
    /// Global hotkey, in Tauri accelerator syntax.
    pub hotkey: String,
    pub hide_on_blur: bool,
    /// Days a scratch note sits before the list marks it as expiring. Never
    /// triggers a deletion by itself — only the badge.
    pub expiry_days: u32,
    /// Which side of the Keep/Scratch switch the compose box remembers.
    pub last_status: Status,
    /// Serve the read-only local HTTP API other R-suite apps read from.
    pub api_enabled: bool,
    /// Loopback port for that API. `0` also disables it.
    pub api_port: u16,
    /// Spaces inserted/removed per Ctrl+]/Ctrl+[ level in the compose box.
    pub indent_width: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notes_dir: default_notes_dir(),
            // Ctrl+Alt+N: unclaimed by Windows and distinct from Rclip's
            // Ctrl+Shift+V / Rfinder's Ctrl+Shift+Space.
            hotkey: "Ctrl+Alt+N".into(),
            hide_on_blur: true,
            expiry_days: 14,
            last_status: Status::Scratch,
            api_enabled: true,
            api_port: DEFAULT_API_PORT,
            indent_width: 4,
        }
    }
}

/// `%USERPROFILE%\Documents\Rnote` — not `%APPDATA%`, so the folder is where
/// a user browsing in Explorer would actually look for their notes.
pub fn default_notes_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join("Documents").join("Rnote")
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Rnote")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn load() -> Settings {
    match std::fs::read_to_string(config_path()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("[config] malformed, falling back to defaults: {e}");
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    let text = serde_json::to_string_pretty(settings)?;
    std::fs::write(config_path(), text)
}
