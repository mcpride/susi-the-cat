//! User settings stored in `%APPDATA%\susi\settings.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, read_file, write_file};

/// Configuration directory (`%APPDATA%\susi`, `$XDG_CONFIG_HOME/susi` or `~/.config/susi`).
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SUSI_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os("APPDATA") {
        return PathBuf::from(d).join("susi");
    }
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(d).join("susi");
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config").join("susi")).unwrap_or_else(|| PathBuf::from(".susi"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Recently opened projects (most recent first).
    pub recent: Vec<PathBuf>,
    /// Translation memory file (TMX). Empty = default location.
    pub tm_path: Option<PathBuf>,
    /// Minimum score for fuzzy TM matches.
    pub fuzzy_threshold: u8,
    /// Automatically add translations marked Translated/Complete to the TM on save.
    pub auto_save_to_tm: bool,
    /// Ask before overwriting existing translations on import.
    pub confirm_overwrite: bool,
    /// Show the row id column in the grid.
    pub show_id_column: bool,
    /// Show the comment column in the grid.
    pub show_comment_column: bool,
    /// Show the translation date column in the grid.
    pub show_date_column: bool,
    /// Languages always shown in the single-language view in addition to the selected one.
    pub extra_langs: Vec<String>,
    /// Pseudo translation options.
    pub pseudo: crate::pseudo::PseudoOptions,
    /// Dark UI theme.
    pub dark_mode: bool,
    /// User interface language: `auto` (system language) or a code such as `en`, `de`.
    pub ui_language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            tm_path: None,
            fuzzy_threshold: 75,
            auto_save_to_tm: false,
            confirm_overwrite: true,
            show_id_column: true,
            show_comment_column: true,
            show_date_column: true,
            extra_langs: Vec::new(),
            pseudo: crate::pseudo::PseudoOptions::default(),
            dark_mode: false,
            ui_language: "auto".into(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        config_dir().join("settings.json")
    }

    /// Loads the settings; missing or unreadable files yield the defaults.
    pub fn load() -> Self {
        let path = Self::path();
        read_file(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let data = serde_json::to_vec_pretty(self).map_err(|e| Error::Json(e.to_string()))?;
        write_file(&Self::path(), &data)
    }

    pub fn tm_path(&self) -> PathBuf {
        self.tm_path.clone().unwrap_or_else(crate::tm::TranslationMemory::default_path)
    }

    /// Moves `path` to the front of the recent file list (max. 10 entries).
    pub fn add_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(10);
    }
}
