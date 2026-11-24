// SPDX-License-Identifier: GPL-3.0-only
//! The player's own settings, kept in `dev.lapinux.MediaPlayer/settings.json`.
//! Nothing here is shared with another app or desktop.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Folders to look in. Empty means the usual Music and Videos folders.
    pub folders: Vec<String>,
    /// Read titles, artists and albums from the files themselves.
    pub use_tags: bool,
    /// Show album pictures.
    pub show_covers: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { folders: Vec::new(), use_tags: true, show_covers: true }
    }
}

fn path() -> PathBuf {
    crate::engine::config_dir().join("settings.json")
}

impl Settings {
    /// A missing file means the defaults. A file that cannot be read also gives
    /// the defaults, and is only replaced when the person changes a setting.
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = crate::engine::config_dir();
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, path())
    }

    /// The folders to scan.
    pub fn roots(&self) -> Vec<PathBuf> {
        if self.folders.is_empty() {
            [dirs::audio_dir(), dirs::video_dir()].into_iter().flatten().collect()
        } else {
            self.folders.iter().map(PathBuf::from).collect()
        }
    }

    /// Adds a folder once. `false` when it was already there.
    pub fn add_folder(&mut self, dir: &str) -> bool {
        if self.folders.is_empty() {
            // the first added folder replaces the defaults, so keep them visible first
            self.folders = [dirs::audio_dir(), dirs::video_dir()].into_iter().flatten().map(|p| p.to_string_lossy().into_owned()).collect();
        }
        if self.folders.iter().any(|f| f == dir) {
            return false;
        }
        self.folders.push(dir.to_string());
        true
    }

    pub fn remove_folder(&mut self, i: usize) {
        if i < self.folders.len() {
            self.folders.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_scan_and_read_tags() {
        let s = Settings::default();
        assert!(s.use_tags && s.show_covers && s.folders.is_empty());
    }

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let s: Settings = serde_json::from_str(r#"{"use_tags":false}"#).unwrap();
        assert!(!s.use_tags && s.show_covers);
    }

    #[test]
    fn a_folder_is_added_once() {
        let mut s = Settings { folders: vec!["/a".into()], ..Default::default() };
        assert!(s.add_folder("/b"));
        assert!(!s.add_folder("/b"));
        s.remove_folder(0);
        assert_eq!(s.folders, vec!["/b".to_string()]);
        s.remove_folder(9);
        assert_eq!(s.folders.len(), 1);
    }
}
