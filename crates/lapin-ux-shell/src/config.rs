// SPDX-License-Identifier: GPL-3.0-only
//! Persistent shell settings, stored with cosmic-config under
//! `~/.config/cosmic/dev.lapinux.Shell/v1/` (one small file per key) and
//! watched live, so a future Settings app can change them on the fly.

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;

pub const VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TaskbarAlignment {
    #[default]
    Center,
    Left,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct ShellConfig {
    /// Defaults below are filled from installed apps on first start.
    pub initialized: bool,
    pub alignment: TaskbarAlignment,
    pub pinned_taskbar: Vec<String>,
    pub pinned_start: Vec<String>,
    pub show_search: bool,
    /// None = follow the locale.
    pub clock_24h: Option<bool>,
    pub clock_seconds: bool,
    pub show_desktop_corner: bool,
    /// Tray icons (by their stable tray id) shown on the taskbar; the rest
    /// live in the ^ overflow menu, like the benchmark's default.
    pub tray_inline: Vec<String>,
    /// Bumped when new defaults (e.g. shortcuts) ship, so upgrades get them.
    pub defaults_version: u32,
    /// Web search URL for Start, `{}` = the query. Empty = DuckDuckGo.
    pub web_search: String,
    /// Don't offer "Search the web" in Start.
    pub hide_web_search: bool,
    pub desktop_icon_size: IconSize,
    pub desktop_sort: lapin_ux_core::desktop::SortBy,
    /// View > Show desktop icons (unchecked).
    pub hide_desktop_icons: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum IconSize {
    Small,
    #[default]
    Medium,
    Large,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopSlot {
    pub name: String,
    pub col: u32,
    pub row: u32,
}

/// Small runtime state (not settings): recently launched apps.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct ShellState {
    pub recent_apps: Vec<String>,
    /// Commands typed in the Run dialog, newest first.
    pub run_history: Vec<String>,
    /// Where desktop icons were placed (by file name).
    pub desktop_cells: Vec<DesktopSlot>,
    /// Emoji panel "Most recently used".
    pub recent_emoji: Vec<String>,
    /// Pinned clipboard history texts survive restarts (unpinned ones don't).
    pub pinned_clips: Vec<String>,
}

impl ShellState {
    pub const MAX_RECENT: usize = 6;

    pub fn record_launch(&mut self, app: &str) {
        self.recent_apps.retain(|a| a != app);
        self.recent_apps.insert(0, app.to_string());
        self.recent_apps.truncate(Self::MAX_RECENT);
    }

    pub fn record_run(&mut self, cmd: &str) {
        self.run_history.retain(|c| c != cmd);
        self.run_history.insert(0, cmd.to_string());
        self.run_history.truncate(10);
    }
}

/// First installed id from each group, in group order.
fn first_installed(catalog: &Catalog, groups: &[&[&str]]) -> Vec<String> {
    groups
        .iter()
        .filter_map(|group| group.iter().find(|id| catalog.get(id).is_some()).map(|s| s.to_string()))
        .collect()
}

pub const FILES: &[&str] = &["com.system76.CosmicFiles", "org.gnome.Nautilus", "org.kde.dolphin", "thunar", "nemo", "pcmanfm-qt"];
const BROWSER: &[&str] = &[
    "firefox", "org.mozilla.firefox", "chromium", "org.chromium.Chromium", "brave-browser", "google-chrome",
    "librewolf", "vivaldi-stable", "microsoft-edge",
];
const STORE: &[&str] = &["com.system76.CosmicStore", "org.gnome.Software", "org.kde.discover", "io.github.kolunmi.Bazaar"];
pub const SETTINGS: &[&str] = &["com.system76.CosmicSettings", "org.gnome.Settings", "systemsettings"];
const EDITOR: &[&str] = &["com.system76.CosmicEdit", "org.gnome.TextEditor", "org.kde.kate", "mousepad", "gedit"];
pub const TERMINAL: &[&str] = &["com.system76.CosmicTerm", "org.gnome.Console", "org.kde.konsole", "Alacritty", "kitty", "foot"];
const CALC: &[&str] = &["org.gnome.Calculator", "org.kde.kcalc", "qalculate-gtk"];
const PHOTOS: &[&str] = &["org.gnome.Loupe", "org.gnome.eog", "org.kde.gwenview", "ristretto"];
const MEDIA: &[&str] = &["org.gnome.Showtime", "vlc", "mpv", "org.gnome.Totem", "org.kde.haruna", "io.github.celluloid_player.Celluloid"];
const OFFICE: &[&str] = &["libreoffice-writer", "onlyoffice-desktopeditors"];
const MAIL: &[&str] = &["org.mozilla.Thunderbird", "thunderbird", "org.gnome.Evolution", "org.gnome.Geary"];
const CALENDAR: &[&str] = &["org.gnome.Calendar", "org.kde.merkuro.calendar"];
pub const TASK_MANAGER: &[&str] = &[
    "io.missioncenter.MissionCenter", "net.nokyan.Resources", "org.gnome.SystemMonitor",
    "gnome-system-monitor", "org.kde.plasma-systemmonitor", "com.system76.CosmicMonitor",
];
pub const DISKS: &[&str] = &["org.gnome.DiskUtility", "gnome-disks", "org.kde.partitionmanager", "gparted"];

/// A role the desktop needs an app for (Win+X menu, keyboard shortcuts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Files,
    Settings,
    Terminal,
    TaskManager,
    Disks,
}

/// Best installed app for a role: a known id first, else any app whose
/// desktop-entry categories say it does the job.
pub fn find_app(catalog: &Catalog, role: Role) -> Option<String> {
    let (ids, cats): (&[&str], &[&str]) = match role {
        Role::Files => (FILES, &["FileManager"]),
        Role::Settings => (SETTINGS, &["DesktopSettings"]),
        Role::Terminal => (TERMINAL, &["TerminalEmulator"]),
        Role::TaskManager => (TASK_MANAGER, &["Monitor"]),
        Role::Disks => (DISKS, &["DiskTool"]),
    };
    ids.iter()
        .find(|id| catalog.get(id).is_some())
        .map(|s| s.to_string())
        .or_else(|| {
            catalog
                .apps()
                .iter()
                .find(|a| cats.iter().any(|c| a.data.categories.iter().any(|x| x == c)))
                .map(|a| a.data.id.clone())
        })
}

impl ShellConfig {
    /// Fill pinned lists with sensible apps that are actually installed.
    pub fn with_defaults(mut self, catalog: &Catalog) -> Self {
        self.initialized = true;
        self.show_search = true;
        self.show_desktop_corner = true;
        if self.pinned_taskbar.is_empty() {
            self.pinned_taskbar = first_installed(catalog, &[FILES, BROWSER, STORE]);
        }
        if self.pinned_start.is_empty() {
            self.pinned_start = first_installed(
                catalog,
                &[BROWSER, MAIL, CALENDAR, STORE, PHOTOS, SETTINGS, OFFICE, MEDIA, CALC, EDITOR, FILES, TERMINAL],
            );
        }
        self
    }
}

pub fn shell_config() -> Option<cosmic_config::Config> {
    cosmic_config::Config::new(lapin_ux_core::ids::SHELL_APP_ID, VERSION)
        .inspect_err(|e| tracing::error!("config unavailable: {e}"))
        .ok()
}

pub fn shell_state() -> Option<cosmic_config::Config> {
    cosmic_config::Config::new_state(lapin_ux_core::ids::SHELL_APP_ID, VERSION)
        .inspect_err(|e| tracing::error!("state unavailable: {e}"))
        .ok()
}

pub fn load<T: CosmicConfigEntry + Default>(cfg: Option<&cosmic_config::Config>) -> T {
    match cfg {
        Some(c) => T::get_entry(c).unwrap_or_else(|(errs, partial)| {
            for e in errs.iter().filter(|e| e.is_err()) {
                tracing::warn!("config: {e}");
            }
            partial
        }),
        None => T::default(),
    }
}
