// SPDX-License-Identifier: GPL-3.0-only
//! System integration that needs no GUI: Settings pages searchable from
//! Start, and how to uninstall an app given its .desktop file.

use std::path::Path;

use crate::search::{Searchable, score};

/// A page of the system Settings app, findable from Start search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsPage {
    pub title: &'static str,
    /// The Settings page: a COSMIC page name (`cosmic-settings <page>`) or
    /// a Lapin page id (`lapin-…`, opened with `cosmic-settings lapin <id>`).
    pub page: &'static str,
    pub icon: &'static str,
    pub keywords: &'static [&'static str],
}

const fn page(title: &'static str, page: &'static str, icon: &'static str, keywords: &'static [&'static str]) -> SettingsPage {
    SettingsPage { title, page, icon, keywords }
}

pub const SETTINGS_PAGES: &[SettingsPage] = &[
    page("Wi-Fi", "wireless", "network-wireless-symbolic", &["wifi", "wireless", "network", "internet"]),
    page("Ethernet", "wired", "network-wired-symbolic", &["wired", "lan", "cable", "network"]),
    page("VPN", "vpn", "network-vpn-symbolic", &["vpn", "wireguard", "openvpn", "proxy"]),
    page("Network & internet", "lapin-network", "preferences-system-network-symbolic", &["network", "internet", "hotspot", "connections"]),
    page("Bluetooth & devices", "bluetooth", "bluetooth-active-symbolic", &["bluetooth", "pair", "headphones", "devices"]),
    page("Display", "displays", "preferences-desktop-display-symbolic", &["display", "monitor", "screen", "resolution", "scale", "refresh rate", "brightness"]),
    page("Night light", "lapin-night-light", "night-light-symbolic", &["night light", "blue light", "warm", "night"]),
    page("Sound", "sound", "audio-speakers-symbolic", &["sound", "audio", "volume", "speakers", "microphone", "output", "input"]),
    page("Notifications", "lapin-notifications", "preferences-system-notifications-symbolic", &["notifications", "do not disturb", "focus", "alerts", "banners"]),
    page("Power & battery", "power", "battery-symbolic", &["power", "battery", "sleep", "energy", "screen off", "lid", "power mode"]),
    page("Storage", "lapin-storage", "drive-harddisk-symbolic", &["storage", "disk space", "free up space", "cleanup", "temporary files", "drive"]),
    page("Multitasking", "lapin-multitasking", "view-grid-symbolic", &["multitasking", "snap", "snap layouts", "virtual desktops", "desktops", "workspaces", "alt tab"]),
    page("Clipboard", "lapin-clipboard", "edit-paste-symbolic", &["clipboard", "clipboard history", "copy", "paste"]),
    page("About", "about", "help-about-symbolic", &["about", "system info", "computer name", "rename this pc", "hostname", "version", "specs"]),
    page("Personalization", "lapin-personalization", "preferences-desktop-appearance-symbolic", &["personalize", "personalization", "look"]),
    page("Colors", "appearance", "preferences-color-symbolic", &["theme", "dark mode", "light mode", "accent", "colors", "appearance", "transparency"]),
    page("Background", "wallpaper", "preferences-desktop-wallpaper-symbolic", &["wallpaper", "background", "desktop picture"]),
    page("Lock screen", "lapin-lock-screen", "system-lock-screen-symbolic", &["lock screen", "lock picture", "sign-in screen"]),
    page("Start", "lapin-start", "view-app-grid-symbolic", &["start menu", "recent", "recommended", "web results", "search engine"]),
    page("Taskbar", "lapin-taskbar", "user-desktop-symbolic", &["taskbar", "alignment", "auto-hide", "hide taskbar", "combine", "system tray"]),
    page("Mouse", "mouse", "input-mouse-symbolic", &["mouse", "pointer", "cursor speed", "scroll", "primary button"]),
    page("Touchpad", "touchpad", "input-touchpad-symbolic", &["touchpad", "trackpad", "tap to click", "gestures"]),
    page("Keyboard", "keyboard", "input-keyboard-symbolic", &["keyboard", "layout", "input method", "shortcuts"]),
    page("Installed apps", "lapin-installed-apps", "view-app-grid-symbolic", &["installed apps", "uninstall", "remove app", "programs", "apps & features"]),
    page("Default apps", "default-apps", "preferences-desktop-default-applications-symbolic", &["default apps", "browser", "email", "open with"]),
    page("Startup apps", "startup-apps", "system-run-symbolic", &["startup", "autostart", "login"]),
    page("Accounts", "users", "system-users-symbolic", &["users", "accounts", "your info", "other users", "family"]),
    page("Sign-in options", "lapin-signin", "dialog-password-symbolic", &["sign-in", "password", "change password", "fingerprint", "face", "facial recognition", "howdy"]),
    page("Date & time", "date-time", "preferences-system-time-symbolic", &["date", "time", "clock", "time zone", "timezone"]),
    page("Language & region", "region-language", "preferences-desktop-locale-symbolic", &["language", "region", "locale", "formats"]),
    page("Accessibility", "accessibility", "preferences-desktop-accessibility-symbolic", &["accessibility", "magnifier", "high contrast", "screen reader", "narrator", "color filters"]),
    page("Privacy & security", "lapin-privacy", "security-high-symbolic", &["privacy", "security", "permissions", "camera", "microphone", "location", "encryption"]),
    page("Update", "lapin-update", "software-update-available-symbolic", &["update", "updates", "upgrade", "firmware", "check for updates", "windows update"]),
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

/// The command that opens Settings on `page` ("" = Home). Lapin pages
/// (`lapin-…`) need Lapin's Settings; with COSMIC's own Settings they fall
/// back to the nearest COSMIC page.
pub fn settings_command(page: &str, lapin_settings: bool) -> Vec<String> {
    let mut cmd = vec!["cosmic-settings".to_string()];
    if page.is_empty() {
        return cmd;
    }
    if page.starts_with("lapin-") {
        if lapin_settings {
            cmd.extend(["lapin".to_string(), page.to_string()]);
        } else if let Some(p) = cosmic_fallback(page) {
            cmd.push(p.to_string());
        }
    } else {
        cmd.push(page.to_string());
    }
    cmd
}

/// The closest COSMIC Settings page for a Lapin page.
pub fn cosmic_fallback(page: &str) -> Option<&'static str> {
    Some(match page {
        "lapin-network" => "network",
        "lapin-night-light" | "lapin-system" => "displays",
        "lapin-notifications" | "lapin-storage" | "lapin-clipboard" => "system",
        "lapin-multitasking" => "window-management",
        "lapin-personalization" | "lapin-lock-screen" => "appearance",
        "lapin-start" | "lapin-taskbar" => "panel",
        "lapin-installed-apps" | "lapin-apps" => "applications",
        "lapin-signin" | "lapin-accounts" => "users",
        "lapin-time" => "time",
        "lapin-devices" => "input",
        "lapin-accessibility" => "accessibility",
        _ => return None,
    })
}

/// Whether the installed Settings is Lapin's (it knows `cosmic-settings
/// lapin <page>`). Checked once.
pub fn lapin_settings_installed() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::process::Command::new("cosmic-settings")
            .args(["help", "lapin"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
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
        assert_eq!(search_settings("uninstall")[0].page, "lapin-installed-apps");
        assert_eq!(search_settings("taskbar")[0].page, "lapin-taskbar");
        assert_eq!(settings_command("lapin-update", true), ["cosmic-settings", "lapin", "lapin-update"]);
        assert_eq!(settings_command("lapin-taskbar", false), ["cosmic-settings", "panel"]);
        assert_eq!(settings_command("sound", true), ["cosmic-settings", "sound"]);
        assert_eq!(settings_command("", true), ["cosmic-settings"]);
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

/// The installed OS as `/etc/os-release` names it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OsInfo {
    /// `NAME`, e.g. "Arch Linux".
    pub name: String,
    /// `PRETTY_NAME`, falling back to `NAME`.
    pub pretty_name: String,
    /// `LOGO`: an icon name such as "archlinux-logo", when the OS ships one.
    pub logo: Option<String>,
}

pub fn parse_os_release(text: &str) -> OsInfo {
    let mut info = OsInfo::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
        match k.trim() {
            "NAME" => info.name = v,
            "PRETTY_NAME" => info.pretty_name = v,
            "LOGO" if !v.is_empty() => info.logo = Some(v),
            _ => {}
        }
    }
    if info.name.is_empty() {
        info.name = "Linux".into();
    }
    if info.pretty_name.is_empty() {
        info.pretty_name = info.name.clone();
    }
    info
}

/// Reads `/etc/os-release` (or `/usr/lib/os-release`).
pub fn os_info() -> OsInfo {
    let text = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .unwrap_or_default();
    parse_os_release(&text)
}

#[cfg(test)]
mod os_release_tests {
    use super::*;

    #[test]
    fn arch() {
        let i = parse_os_release("NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\nLOGO=archlinux-logo\n");
        assert_eq!(i.name, "Arch Linux");
        assert_eq!(i.logo.as_deref(), Some("archlinux-logo"));
    }

    #[test]
    fn missing() {
        let i = parse_os_release("");
        assert_eq!((i.name.as_str(), i.pretty_name.as_str(), i.logo), ("Linux", "Linux", None));
    }
}
