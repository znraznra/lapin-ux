// SPDX-License-Identifier: GPL-3.0-only
//! System integration that needs no GUI: Settings pages searchable from
//! Start, and how to uninstall an app given its .desktop file.

use std::path::Path;

use crate::search::{Searchable, score};

/// A page of the system Settings app, findable from Start search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsPage {
    pub title: &'static str,
    /// Argument for `cosmic-settings <page>` (its clap subcommand names).
    pub page: &'static str,
    pub icon: &'static str,
    pub keywords: &'static [&'static str],
}

const fn page(title: &'static str, page: &'static str, icon: &'static str, keywords: &'static [&'static str]) -> SettingsPage {
    SettingsPage { title, page, icon, keywords }
}

pub const SETTINGS_PAGES: &[SettingsPage] = &[
    page("Wi-Fi", "wireless", "network-wireless-symbolic", &["wifi", "wireless", "network", "internet", "hotspot"]),
    page("Ethernet", "wired", "network-wired-symbolic", &["wired", "lan", "cable", "network"]),
    page("VPN", "vpn", "network-vpn-symbolic", &["vpn", "wireguard", "openvpn", "proxy"]),
    page("Bluetooth", "bluetooth", "bluetooth-active-symbolic", &["bluetooth", "pair", "headphones", "devices"]),
    page("Display", "displays", "preferences-desktop-display-symbolic", &["display", "monitor", "screen", "resolution", "scale", "refresh rate", "night light"]),
    page("Sound", "sound", "audio-speakers-symbolic", &["sound", "audio", "volume", "speakers", "microphone", "output", "input"]),
    page("Power & battery", "power", "battery-symbolic", &["power", "battery", "sleep", "energy", "screen off", "lid"]),
    page("Personalization", "appearance", "preferences-desktop-appearance-symbolic", &["theme", "dark mode", "light mode", "accent", "colors", "appearance", "font"]),
    page("Background", "wallpaper", "preferences-desktop-wallpaper-symbolic", &["wallpaper", "background", "desktop picture"]),
    page("Mouse", "mouse", "input-mouse-symbolic", &["mouse", "pointer", "cursor speed", "scroll"]),
    page("Touchpad", "touchpad", "input-touchpad-symbolic", &["touchpad", "trackpad", "tap to click", "gestures"]),
    page("Keyboard", "keyboard", "input-keyboard-symbolic", &["keyboard", "layout", "input method", "shortcuts", "language"]),
    page("Date & time", "date-time", "preferences-system-time-symbolic", &["date", "time", "clock", "time zone", "timezone"]),
    page("Language & region", "region-language", "preferences-desktop-locale-symbolic", &["language", "region", "locale", "formats"]),
    page("Accounts", "users", "system-users-symbolic", &["users", "accounts", "password", "sign-in", "family"]),
    page("Default apps", "default-apps", "preferences-desktop-default-applications-symbolic", &["default apps", "browser", "email", "open with"]),
    page("Startup apps", "startup-apps", "system-run-symbolic", &["startup", "autostart", "login"]),
    page("Accessibility", "accessibility", "preferences-desktop-accessibility-symbolic", &["accessibility", "magnifier", "high contrast", "screen reader"]),
    page("Window management", "window-management", "preferences-system-windows-symbolic", &["windows", "snap", "focus", "title bar", "super key"]),
    page("Workspaces", "workspaces", "view-paged-symbolic", &["workspaces", "virtual desktops", "desktops"]),
    page("About", "about", "help-about-symbolic", &["about", "system info", "computer name", "hostname", "version", "specs"]),
];

/// Settings pages matching `query`, best first. Keywords only count for
/// queries of 3+ letters so short queries stay app-focused.
pub fn search_settings(query: &str) -> Vec<&'static SettingsPage> {
    let mut hits: Vec<(u32, &SettingsPage)> = SETTINGS_PAGES
        .iter()
        .filter_map(|p| {
            let keywords: Vec<String> = if query.trim().chars().count() >= 3 {
                p.keywords.iter().map(|k| k.to_string()).collect()
            } else {
                Vec::new()
            };
            score(query, &Searchable { name: p.title, keywords: &keywords, id: p.page, ..Default::default() }).map(|s| (s, p))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.title.cmp(b.1.title)));
    hits.into_iter().map(|h| h.1).collect()
}

/// How an installed app can be removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uninstall {
    Flatpak { app_id: String, user: bool },
    /// A native package that owns this .desktop file (looked up with pacman).
    Package { desktop_file: String },
    /// Installed by hand under ~/.local: nothing safe to do automatically.
    Unsupported,
}

/// Decide from the .desktop file's location.
pub fn uninstall_method(desktop_file: &Path, app_id: &str, home: &Path) -> Uninstall {
    let p = desktop_file.to_string_lossy();
    if p.contains("/flatpak/exports/share/applications/") {
        let user = desktop_file.starts_with(home.join(".local/share/flatpak"));
        let app_id = desktop_file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| app_id.to_string());
        return Uninstall::Flatpak { app_id, user };
    }
    if desktop_file.starts_with(home) {
        return Uninstall::Unsupported;
    }
    Uninstall::Package { desktop_file: p.into_owned() }
}

/// The command that removes `package` with an admin prompt (polkit).
/// `-Rs` also removes dependencies nothing else needs, like the
/// benchmark's "uninstall" does for an app's own components.
pub fn pacman_remove_command(package: &str) -> Vec<String> {
    ["pkexec", "pacman", "-Rs", "--noconfirm", package].iter().map(|s| s.to_string()).collect()
}

pub fn flatpak_remove_command(app_id: &str, user: bool) -> Vec<String> {
    let scope = if user { "--user" } else { "--system" };
    ["flatpak", "uninstall", scope, "--noninteractive", "-y", app_id].iter().map(|s| s.to_string()).collect()
}

/// Packages that must never be removed from the Start menu (they'd take the
/// desktop down with them).
pub fn is_protected_package(pkg: &str) -> bool {
    pkg.starts_with("cosmic-")
        || pkg.starts_with("lapin-ux")
        || matches!(pkg, "networkmanager" | "systemd" | "pacman" | "glibc" | "linux" | "bash" | "sudo" | "polkit" | "dbus" | "wireplumber" | "pipewire")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_search() {
        assert_eq!(search_settings("wi")[0].page, "wireless");
        assert_eq!(search_settings("bluetooth")[0].page, "bluetooth");
        let dark: Vec<_> = search_settings("dark mode").iter().map(|p| p.page).collect();
        assert_eq!(dark, vec!["appearance"]);
        assert!(search_settings("resolution").iter().any(|p| p.page == "displays"));
        assert!(search_settings("xyzzy").is_empty());
    }

    #[test]
    fn uninstall_methods() {
        let home = Path::new("/home/ana");
        assert_eq!(
            uninstall_method(Path::new("/var/lib/flatpak/exports/share/applications/org.gimp.GIMP.desktop"), "x", home),
            Uninstall::Flatpak { app_id: "org.gimp.GIMP".into(), user: false }
        );
        assert_eq!(
            uninstall_method(Path::new("/home/ana/.local/share/flatpak/exports/share/applications/com.spotify.Client.desktop"), "x", home),
            Uninstall::Flatpak { app_id: "com.spotify.Client".into(), user: true }
        );
        assert_eq!(
            uninstall_method(Path::new("/usr/share/applications/firefox.desktop"), "firefox", home),
            Uninstall::Package { desktop_file: "/usr/share/applications/firefox.desktop".into() }
        );
        assert_eq!(uninstall_method(Path::new("/home/ana/.local/share/applications/my.desktop"), "my", home), Uninstall::Unsupported);
        assert_eq!(pacman_remove_command("gimp"), ["pkexec", "pacman", "-Rs", "--noconfirm", "gimp"]);
        assert!(is_protected_package("cosmic-files"));
        assert!(!is_protected_package("firefox"));
    }
}
