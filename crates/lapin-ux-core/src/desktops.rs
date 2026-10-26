// SPDX-License-Identifier: GPL-3.0-only
//! Which desktop entries belong to *other* desktops' plumbing (settings
//! apps, panels, launchers, daemons…), so a Lapin UX session can leave them
//! out: Start / search / Run / Open with hide them, and the session doesn't
//! autostart them. Ordinary apps stay, even when a desktop shipped them.
//!
//! `XDG_CURRENT_DESKTOP` is `LapinUX:COSMIC` (COSMIC is there so COSMIC's own
//! apps and toolkit behave), so plain `OnlyShowIn=COSMIC` matching would let
//! COSMIC's components in. Here `COSMIC` only counts for an allow-list.
//!
//! Nothing here edits a .desktop file: other desktops see what they always saw.

/// What we know about one desktop entry.
#[derive(Debug, Default, Clone)]
pub struct Entry<'a> {
    /// Desktop-file id without `.desktop` (e.g. `org.gnome.Settings`).
    pub id: &'a str,
    pub only_show_in: Option<Vec<&'a str>>,
    pub not_show_in: Option<Vec<&'a str>>,
    pub categories: Vec<&'a str>,
    /// `X-GNOME-Autostart-Phase` names a session phase other than
    /// "Applications" (session-manager plumbing).
    pub autostart_phase: bool,
}

/// COSMIC entries restricted to COSMIC that Lapin UX does use.
const COSMIC_ALLOW: &[&str] = &["com.system76.CosmicSettings"];

/// Other desktops' parts Lapin UX replaces or that only make sense inside
/// their own desktop. Matched as an id prefix (case-insensitive).
const COMPONENT_PREFIXES: &[&str] = &[
    // Settings / control centres (Lapin has Settings + Control Panel).
    "org.gnome.settings",
    "gnome-control-center",
    "gnome-",
    "org.gnome.settingsdaemon",
    "systemsettings",
    "org.kde.systemsettings",
    "kcm_",
    "org.kde.kinfocenter",
    "org.kde.plasma",
    "org.kde.plasmashell",
    "plasma-",
    "org.kde.krunner",
    "org.kde.spectacle",
    "org.kde.ksysguard",
    "org.kde.kmenuedit",
    "org.kde.kdeconnect.nonplasma",
    "io.elementary.settings",
    "io.elementary.switchboard",
    "io.elementary.wingpanel",
    "io.elementary.dock",
    "io.elementary.greeter",
    "io.elementary.screenshot",
    "io.elementary.desktop.",
    "io.elementary.settings-daemon",
    "io.elementary.capnet-assist",
    "io.elementary.onboarding",
    "io.elementary.initial-setup",
    "io.elementary.notifications",
    "io.elementary.gala",
    "gala-",
    "xfce4-",
    "xfce-settings",
    "xfsettingsd",
    "xfdesktop",
    "lxqt-",
    "lxsession",
    "mate-",
    "cinnamon-",
    "nemo-autostart",
    "budgie-",
    "org.buddiesofbudgie.",
    "polkit-gnome-authentication-agent",
    "polkit-kde-authentication-agent",
    "polkit-mate-authentication-agent",
    "lxpolkit",
    // COSMIC's own components (Lapin has its own taskbar, Start, Settings…).
    "com.system76.cosmicapplet",
    "com.system76.cosmiclauncher",
    "com.system76.cosmicapplibrary",
    "com.system76.cosmicpanel",
    "com.system76.cosmicsettings.",
    "com.system76.cosmicscreenshot",
    "com.system76.cosmicinitialsetup",
    "cosmic-",
];

/// Everyday GNOME / KDE apps that start with a component prefix above but
/// are ordinary apps people use (kept in Start).
const APP_EXCEPTIONS: &[&str] = &[
    "gnome-terminal",
    "gnome-system-monitor",
    "gnome-disks",
    "gnome-calculator",
    "gnome-text-editor",
    "gnome-boxes",
    "gnome-maps",
    "gnome-weather",
    "gnome-clocks",
    "gnome-calendar",
    "gnome-contacts",
    "gnome-characters",
    "gnome-font-viewer",
    "gnome-logs",
    "gnome-software",
    "mate-terminal",
    "mate-calc",
    "xfce4-terminal",
    "xfce4-taskmanager",
    "xfce4-screenshooter",
    "xfce4-dict",
    "lxqt-archiver",
];

/// Categories other desktops use for their settings dialogs.
const SETTINGS_CATEGORIES: &[&str] = &[
    "X-GNOME-Settings-Panel",
    "X-GNOME-SystemSettings",
    "X-KDE-settings-hardware",
    "X-KDE-settings-system",
    "X-KDE-settings-looknfeel",
    "X-KDE-settings-personalization",
    "X-KDE-settings-workspace",
    "X-XFCE-SettingsDialog",
    "X-XFCE-SettingsManager",
    "X-XFCE-PersonalSettings",
    "X-XFCE-HardwareSettings",
    "X-XFCE-SystemSettings",
    "X-LXQt-Config",
    "X-Cinnamon-Settings-Panel",
    "X-MATE-Control-Center",
    "X-MATE-PersonalSettings",
    "X-MATE-HardwareSettings",
    "X-MATE-SystemSettings",
    "X-Budgie-Settings",
    "X-Pantheon-Settings",
];

fn is_component_id(id: &str) -> bool {
    let id = id.to_lowercase();
    if APP_EXCEPTIONS.iter().any(|a| id == *a || id.starts_with(&format!("{a}."))) {
        return false;
    }
    // gnome-* is broad: only the plumbing ones count.
    if id.starts_with("gnome-") {
        return ["gnome-control-center", "gnome-initial-setup", "gnome-shell", "gnome-session", "gnome-screenshot", "gnome-keyring", "gnome-settings", "gnome-tweaks", "gnome-printers-panel", "gnome-network-panel", "gnome-wifi-panel", "gnome-background-panel", "gnome-power-panel", "gnome-extensions"]
            .iter()
            .any(|p| id.starts_with(p));
    }
    COMPONENT_PREFIXES.iter().filter(|p| **p != "gnome-").any(|p| id.starts_with(p))
}

/// Is this another desktop's own part, rather than an app? (Ignores the
/// user's "show apps from other desktops" choice; see [`show_in_start`].)
pub fn belongs_to_other_desktop(e: &Entry, desktop_name: &str) -> bool {
    if let Some(only) = &e.only_show_in {
        if only.iter().any(|d| *d == desktop_name) {
            return false;
        }
        // Restricted to some other desktop(s): theirs. COSMIC only counts for
        // the allow-list.
        return !(only.contains(&"COSMIC") && COSMIC_ALLOW.iter().any(|a| e.id.eq_ignore_ascii_case(a)));
    }
    if COSMIC_ALLOW.iter().any(|a| e.id.eq_ignore_ascii_case(a)) {
        return false;
    }
    if is_component_id(e.id) {
        return true;
    }
    e.categories.iter().any(|c| SETTINGS_CATEGORIES.contains(c))
}

/// Should Start, search, Run and Open with list this app in a Lapin session?
/// `show_others` is the user's "Show apps from other desktops" switch.
pub fn show_in_start(e: &Entry, desktop_name: &str, show_others: bool) -> bool {
    if e.not_show_in.as_ref().is_some_and(|n| n.contains(&desktop_name)) {
        return false;
    }
    show_others || !belongs_to_other_desktop(e, desktop_name)
}

/// Should the Lapin session start this *system* autostart entry? (The
/// user's own entries in ~/.config/autostart are always theirs to keep.)
pub fn autostart_in_session(e: &Entry, desktop_name: &str) -> bool {
    if e.not_show_in.as_ref().is_some_and(|n| n.contains(&desktop_name)) {
        return false;
    }
    // Session-manager plumbing (keyrings, settings daemons) of other desktops;
    // Lapin starts what it needs itself.
    if e.autostart_phase && e.only_show_in.as_ref().is_none_or(|o| !o.contains(&desktop_name)) {
        return false;
    }
    !belongs_to_other_desktop(e, desktop_name)
}

/// Split a `;`-separated desktop-entry list.
pub fn list(v: &str) -> Vec<&str> {
    v.split(';').map(str::trim).filter(|s| !s.is_empty()).collect()
}

/// Parse the keys we need from a .desktop file's text.
pub fn parse<'a>(id: &'a str, text: &'a str) -> Entry<'a> {
    let mut e = Entry { id, ..Default::default() };
    let mut in_main = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_main = l == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        match k.trim() {
            "OnlyShowIn" => e.only_show_in = Some(list(v)),
            "NotShowIn" => e.not_show_in = Some(list(v)),
            "Categories" => e.categories = list(v),
            // Every phase but "Applications" is the session's own plumbing.
            "X-GNOME-Autostart-Phase" => e.autostart_phase = !v.trim().is_empty() && v.trim() != "Applications",
            _ => {}
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    const D: &str = "LapinUX";

    fn e(id: &str, text: &str) -> bool {
        let text = format!("[Desktop Entry]\n{text}");
        show_in_start(&parse(id, &text), D, false)
    }

    #[test]
    fn ordinary_apps_stay() {
        assert!(e("firefox", "Categories=Network;WebBrowser;"));
        assert!(e("org.kde.konsole", "Categories=System;TerminalEmulator;"));
        assert!(e("org.gnome.Nautilus", "Categories=GNOME;GTK;Utility;Core;FileManager;"));
        assert!(e("io.elementary.files", ""));
        assert!(e("gnome-system-monitor", ""));
        assert!(e("com.system76.CosmicFiles", "Categories=COSMIC;Utility;FileManager;"));
        assert!(e("com.system76.CosmicSettings", "OnlyShowIn=COSMIC;"));
    }

    #[test]
    fn other_desktops_parts_go() {
        assert!(!e("org.gnome.Settings", "OnlyShowIn=GNOME;Unity;"));
        assert!(!e("gnome-control-center", ""));
        assert!(!e("systemsettings", "Categories=Qt;KDE;Settings;"));
        assert!(!e("kcm_mouse", ""));
        assert!(!e("io.elementary.settings", "OnlyShowIn=Pantheon;"));
        assert!(!e("io.elementary.switchboard", ""));
        assert!(!e("xfce4-appearance-settings", "Categories=XFCE;GTK;Settings;DesktopSettings;X-XFCE-SettingsDialog;"));
        assert!(!e("com.system76.CosmicAppLibrary", "OnlyShowIn=COSMIC;"));
        assert!(!e("com.system76.CosmicLauncher", ""));
        assert!(!e("com.system76.CosmicSettings.Displays", "OnlyShowIn=COSMIC;"));
        assert!(!e("some-thing", "Categories=Settings;X-GNOME-Settings-Panel;"));
        assert!(!e("x", "NotShowIn=LapinUX;"));
        assert!(e("x", "OnlyShowIn=LapinUX;"));
    }

    #[test]
    fn switch_shows_them_again() {
        let p = parse("org.gnome.Settings", "[Desktop Entry]\nOnlyShowIn=GNOME;\n");
        assert!(show_in_start(&p, D, true));
        let n = parse("x", "[Desktop Entry]\nNotShowIn=LapinUX;\n");
        assert!(!show_in_start(&n, D, true));
    }

    #[test]
    fn autostart() {
        let a = |id: &str, t: &str| autostart_in_session(&parse(id, &format!("[Desktop Entry]\n{t}")), D);
        assert!(a("nextcloud", "Exec=nextcloud"));
        assert!(a("org.kde.kdeconnect.daemon", ""));
        assert!(!a("io.elementary.settings-daemon", "OnlyShowIn=Pantheon;"));
        assert!(!a("io.elementary.desktop.agent-polkit", ""));
        assert!(!a("gnome-keyring-secrets", "X-GNOME-Autostart-Phase=PreDisplayServer\nOnlyShowIn=GNOME;Unity;MATE;Pantheon;"));
        assert!(!a("org.gnome.SettingsDaemon.Power", "X-GNOME-Autostart-Phase=Initialization"));
        assert!(!a("xfsettingsd", ""));
        assert!(!a("cosmic-something", "OnlyShowIn=COSMIC;"));
        assert!(!a("anything", "X-GNOME-Autostart-Phase=Panel"));
        assert!(a("org.gnome.Evolution-alarm-notify", "X-GNOME-Autostart-Phase=Applications"));
    }
}
