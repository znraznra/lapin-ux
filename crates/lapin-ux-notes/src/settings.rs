// SPDX-License-Identifier: GPL-3.0-only
//! Notes' own settings, recent files and session restore, kept under
//! `~/.config/dev.lapinux.Notes/`. Nothing here is shared with other apps or
//! desktops. The session file holds the user's own text: uninstalling never
//! deletes it.

use crate::encoding::Enc;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "dev.lapinux.Notes";
pub const MAX_RECENT: usize = 12;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub font_size: f32,
    pub wrap: bool,
    pub show_ws: bool,
    pub show_eol: bool,
    pub line_numbers: bool,
    pub hl_line: bool,
    pub use_spaces: bool,
    pub tab_width: usize,
    pub auto_close: bool,
    pub auto_indent: bool,
    pub recent: Vec<String>,
    /// Thumbnail of the whole file next to the text.
    pub minimap: bool,
    /// Offer word completions while typing.
    pub autocomplete: bool,
    /// Saved macros, by name.
    pub macros: Vec<SavedMacro>,
    /// Commands run from the Run panel, newest first.
    pub commands: Vec<String>,
    /// Colour overrides by token class (hex like #aabbcc).
    pub styles: std::collections::BTreeMap<String, String>,
}

/// One recorded step of a macro.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MacroStep {
    Text(String),
    /// A named key with its modifiers (ctrl, shift, alt).
    Key(String, bool, bool, bool),
    /// A menu command by its stable name.
    Cmd(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SavedMacro {
    pub name: String,
    pub steps: Vec<MacroStep>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            wrap: false,
            show_ws: false,
            show_eol: false,
            line_numbers: true,
            hl_line: true,
            use_spaces: true,
            tab_width: 4,
            auto_close: true,
            auto_indent: true,
            recent: Vec::new(),
            minimap: false,
            autocomplete: false,
            macros: Vec::new(),
            commands: Vec::new(),
            styles: Default::default(),
        }
    }
}

impl Settings {
    pub fn clamp(&mut self) {
        self.font_size = self.font_size.clamp(8.0, 48.0);
        self.tab_width = self.tab_width.clamp(1, 16);
    }
    pub fn add_recent(&mut self, p: &Path) {
        let s = p.to_string_lossy().into_owned();
        self.recent.retain(|r| *r != s);
        self.recent.insert(0, s);
        self.recent.truncate(MAX_RECENT);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct SessTab {
    pub path: Option<String>,
    /// Kept only for tabs with unsaved changes and for untitled ones.
    pub text: Option<String>,
    pub enc: String,
    pub eol: String,
    pub lang: String,
    pub line: usize,
    pub col: usize,
    pub scroll_y: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Session {
    pub tabs: Vec<SessTab>,
    pub active: usize,
}

pub fn dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_ID))
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension("tmp");
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

pub fn load_settings() -> Settings {
    let mut s: Settings = dir()
        .and_then(|d| std::fs::read(d.join("settings.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    s.clamp();
    s
}

/// A failed save is logged and never ends the program.
pub fn save_settings(s: &Settings) {
    let Some(d) = dir() else { return };
    match serde_json::to_vec_pretty(s) {
        Ok(b) => {
            if let Err(e) = write_atomic(&d.join("settings.json"), &b) {
                tracing::warn!("could not save settings: {e}");
            }
        }
        Err(e) => tracing::warn!("could not encode settings: {e}"),
    }
}

pub fn load_session() -> Session {
    dir()
        .and_then(|d| std::fs::read(d.join("session.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_session(s: &Session) -> Result<(), String> {
    let d = dir().ok_or_else(|| "no settings folder".to_string())?;
    let b = serde_json::to_vec(s).map_err(|e| format!("could not encode the session: {e}"))?;
    write_atomic(&d.join("session.json"), &b).map_err(|e| {
        tracing::warn!("could not save the session: {e}");
        e.to_string()
    })
}

/// Only one window owns the session (restores it and keeps it up to date), so
/// several windows never overwrite each other's tabs. The lock goes away with
/// the process.
pub fn lock_session() -> Option<File> {
    let d = dir()?;
    std::fs::create_dir_all(&d).ok()?;
    let f = File::options().create(true).write(true).truncate(false).open(d.join("session.lock")).ok()?;
    f.try_lock().ok()?;
    Some(f)
}

pub fn enc_key(e: Enc) -> String {
    match e {
        Enc::Utf8 => "utf-8".into(),
        Enc::Utf8Bom => "utf-8-bom".into(),
        Enc::Utf16Le => "utf-16le".into(),
        Enc::Utf16Be => "utf-16be".into(),
        Enc::Legacy(x) => x.name().to_string(),
    }
}

pub fn enc_from_key(k: &str) -> Enc {
    match k {
        "utf-8-bom" => Enc::Utf8Bom,
        "utf-16le" => Enc::Utf16Le,
        "utf-16be" => Enc::Utf16Be,
        "" | "utf-8" => Enc::Utf8,
        other => encoding_rs::Encoding::for_label(other.as_bytes()).map(Enc::Legacy).unwrap_or(Enc::Utf8),
    }
}
