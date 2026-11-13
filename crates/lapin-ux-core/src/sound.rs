// SPDX-License-Identifier: GPL-3.0-only
//! System sounds (freedesktop sound theme names), played with PipeWire's
//! `pw-play` (or `paplay`). Lapin UX ships the Borealis theme.

use std::path::{Path, PathBuf};

pub const THEME: &str = "borealis";
/// The theme Settings writes per-event choices into (#58). It inherits the
/// chosen scheme and only holds the events that were changed.
pub const CUSTOM: &str = "lapin-ux-custom";

/// Events the Settings page lists: (freedesktop name, label, group).
pub const EVENTS: &[(&str, &str, &str)] = &[
    ("desktop-login", "Sign in", "Desktop"),
    ("desktop-logout", "Sign out", "Desktop"),
    ("message-new-instant", "Notification", "Notifications"),
    ("dialog-warning", "Warning", "Notifications"),
    ("dialog-error", "Error", "System"),
    ("dialog-question", "Question", "System"),
    ("battery-low", "Low battery", "System"),
    ("battery-caution", "Critical battery", "System"),
    ("device-added", "Device connected", "System"),
    ("device-removed", "Device disconnected", "System"),
    ("trash-empty", "Empty Recycle Bin", "Actions"),
    ("camera-shutter", "Screenshot", "Actions"),
    ("audio-volume-change", "Volume change", "Actions"),
];

/// The scheme to play from: `dev.lapinux.Shell` key `sound_theme` (a RON
/// string; empty = no sounds), read each time so a change in Settings counts
/// at once. Missing or unreadable = Borealis.
pub fn theme() -> Option<String> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")));
    let raw = base.and_then(|b| std::fs::read_to_string(b.join("cosmic/dev.lapinux.Shell/v1/sound_theme")).ok());
    parse_theme(raw.as_deref())
}

fn parse_theme(raw: Option<&str>) -> Option<String> {
    let Some(raw) = raw else { return Some(THEME.into()) };
    let t = raw.trim();
    let t = t.strip_prefix('"').and_then(|t| t.strip_suffix('"'));
    match t {
        Some("") => None,
        Some(name) if !name.contains('/') && !name.contains('\\') => Some(name.to_string()),
        _ => Some(THEME.into()),
    }
}

/// Sound themes found under `dirs`: (folder name, display name), sorted.
/// A theme is a folder under `sounds/` with an `index.theme`.
pub fn themes(dirs: &[PathBuf]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d.join("sounds")) else { continue };
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            let Ok(index) = std::fs::read_to_string(e.path().join("index.theme")) else { continue };
            if id == CUSTOM || id == "freedesktop" || out.iter().any(|(i, _)| *i == id) {
                continue;
            }
            let name = index.lines().find_map(|l| l.strip_prefix("Name=")).map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
            out.push((id, name));
        }
    }
    out.sort_by_key(|(_, n)| n.to_lowercase());
    out
}

/// What a theme says about an event.
#[derive(Debug, PartialEq)]
pub enum Lookup {
    Found(PathBuf),
    /// A theme has an `<event>.disabled` marker: no sound, no fallback.
    Disabled,
    Missing,
}

/// `theme` then the themes it `Inherits` (in order, each once, so a loop can't
/// run away), then freedesktop's fallback theme.
fn theme_chain(theme: &str, data_dirs: &[PathBuf]) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut queue = vec![theme.to_string()];
    while let Some(t) = queue.first().cloned() {
        queue.remove(0);
        if t.is_empty() || t.contains('/') || chain.contains(&t) || chain.len() >= 16 {
            continue;
        }
        let mut parents = Vec::new();
        for d in data_dirs {
            if let Ok(index) = std::fs::read_to_string(d.join("sounds").join(&t).join("index.theme")) {
                for line in index.lines() {
                    // Desktop-entry format: whitespace is allowed around "=".
                    if let Some((k, v)) = line.split_once('=') {
                        if k.trim() == "Inherits" {
                            parents.extend(v.split(',').map(|p| p.trim().to_string()));
                        }
                    }
                }
                break;
            }
        }
        chain.push(t);
        queue.splice(0..0, parents);
    }
    if !chain.iter().any(|t| t == "freedesktop") {
        chain.push("freedesktop".into());
    }
    chain
}

/// Look `event` up in `theme`, its parents and then freedesktop's fallback
/// theme, under the data dirs: `<dir>/sounds/<theme>/stereo/<event>.{oga,ogg,wav}`.
pub fn lookup(theme: &str, event: &str, data_dirs: &[PathBuf]) -> Lookup {
    for t in theme_chain(theme, data_dirs) {
        for d in data_dirs {
            for sub in ["stereo", ""] {
                let base = d.join("sounds").join(&t).join(sub);
                if base.join(format!("{event}.disabled")).exists() {
                    return Lookup::Disabled;
                }
                for ext in ["oga", "ogg", "wav"] {
                    let p = base.join(format!("{event}.{ext}"));
                    if p.is_file() {
                        return Lookup::Found(p);
                    }
                }
            }
        }
    }
    Lookup::Missing
}

pub fn find(theme: &str, event: &str, data_dirs: &[PathBuf]) -> Option<PathBuf> {
    match lookup(theme, event, data_dirs) {
        Lookup::Found(p) => Some(p),
        _ => None,
    }
}

/// XDG data dirs, user's first.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share"))) {
        v.push(h);
    }
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    v.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    v
}

pub enum Choice {
    /// Nothing chosen for this event: the scheme decides.
    None,
    /// The event is turned off.
    Silent,
    File(PathBuf),
}

/// What `lapin-ux-custom` says about `event` (only the user's data dir counts).
pub fn custom_choice(event: &str, dirs: &[PathBuf]) -> Choice {
    let Some(user) = dirs.first() else { return Choice::None };
    let dir = user.join("sounds").join(CUSTOM).join("stereo");
    if dir.join(format!("{event}.silent")).exists() {
        return Choice::Silent;
    }
    for ext in ["oga", "ogg", "wav", "flac", "mp3"] {
        let p = dir.join(format!("{event}.{ext}"));
        if p.is_file() {
            return Choice::File(p);
        }
    }
    Choice::None
}

/// Play a sound event in the background; silently does nothing when the
/// sound or a player is missing.
pub fn play(event: &str) {
    if std::env::var_os("LAPIN_UX_NO_SOUNDS").is_some() {
        return;
    }
    let Some(theme) = theme() else { return };
    let dirs = data_dirs();
    // The user's own per-event choices come first (a link to nothing = "no sound").
    let file = match custom_choice(event, &dirs) {
        Choice::Silent => return,
        Choice::File(f) => f,
        Choice::None => match lookup(&theme, event, &dirs) {
            Lookup::Found(f) => f,
            Lookup::Disabled | Lookup::Missing => return,
        },
    };
    play_file(&file);
}

/// Play one file in the background (also what Settings' Test button does).
pub fn play_file(file: &Path) {
    for player in ["pw-play", "paplay"] {
        let spawned = std::process::Command::new(player)
            .arg(file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Ok(mut child) = spawned {
            // Reap it when done so it doesn't linger as a zombie.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_through_links_and_fallback() {
        let d = std::env::temp_dir().join(format!("lapin-sound-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sounds/borealis/stereo")).unwrap();
        std::fs::create_dir_all(d.join("sounds/freedesktop/stereo")).unwrap();
        std::fs::write(d.join("sounds/borealis/stereo/trash-empty.oga"), b"x").unwrap();
        std::fs::write(d.join("sounds/freedesktop/stereo/bell.oga"), b"x").unwrap();
        let dirs = vec![d.clone()];
        assert!(find("borealis", "trash-empty", &dirs).unwrap().ends_with("trash-empty.oga"));
        assert!(find("borealis", "bell", &dirs).unwrap().to_string_lossy().contains("freedesktop"));
        assert!(find("borealis", "nope", &dirs).is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn inherited_themes_come_before_the_fallback() {
        let d = std::env::temp_dir().join(format!("lapin-sound-i-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for t in ["ocean", "borealis", "freedesktop"] {
            std::fs::create_dir_all(d.join("sounds").join(t).join("stereo")).unwrap();
        }
        std::fs::write(d.join("sounds/ocean/index.theme"), "[Sound Theme]\nInherits = borealis\n").unwrap();
        // A loop must not hang the lookup.
        std::fs::write(d.join("sounds/borealis/index.theme"), "[Sound Theme]\nInherits=ocean\n").unwrap();
        std::fs::write(d.join("sounds/borealis/stereo/desktop-login.oga"), b"x").unwrap();
        std::fs::write(d.join("sounds/freedesktop/stereo/desktop-login.oga"), b"x").unwrap();
        std::fs::write(d.join("sounds/freedesktop/stereo/bell.oga"), b"x").unwrap();
        let dirs = vec![d.clone()];
        let found = find("ocean", "desktop-login", &dirs).unwrap();
        assert!(found.to_string_lossy().contains("borealis"), "{found:?}");
        assert!(find("ocean", "bell", &dirs).unwrap().to_string_lossy().contains("freedesktop"));
        assert_eq!(lookup("ocean", "nope", &dirs), Lookup::Missing);
        // A .disabled marker in the theme stops the fallback.
        std::fs::write(d.join("sounds/ocean/stereo/bell.disabled"), b"").unwrap();
        assert_eq!(lookup("ocean", "bell", &dirs), Lookup::Disabled);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn theme_setting() {
        assert_eq!(parse_theme(None).as_deref(), Some("borealis"));
        assert_eq!(parse_theme(Some("\"ocean\"")).as_deref(), Some("ocean"));
        assert_eq!(parse_theme(Some("\"\"")), None);
        assert_eq!(parse_theme(Some("\"../x\"")).as_deref(), Some("borealis"));
        assert_eq!(parse_theme(Some("garbage")).as_deref(), Some("borealis"));
    }

    #[test]
    fn lists_themes_and_custom_choices() {
        let d = std::env::temp_dir().join(format!("lapin-sound-t-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sounds/ocean")).unwrap();
        std::fs::write(d.join("sounds/ocean/index.theme"), "[Sound Theme]\nName=Ocean waves\n").unwrap();
        std::fs::create_dir_all(d.join("sounds/noindex")).unwrap();
        std::fs::create_dir_all(d.join("sounds/lapin-ux-custom/stereo")).unwrap();
        std::fs::write(d.join("sounds/lapin-ux-custom/index.theme"), "[Sound Theme]\nName=Mine\n").unwrap();
        let dirs = vec![d.clone()];
        assert_eq!(themes(&dirs), [("ocean".to_string(), "Ocean waves".to_string())]);
        std::fs::write(d.join("sounds/lapin-ux-custom/stereo/trash-empty.silent"), b"").unwrap();
        std::fs::write(d.join("sounds/lapin-ux-custom/stereo/desktop-login.wav"), b"x").unwrap();
        assert!(matches!(custom_choice("trash-empty", &dirs), Choice::Silent));
        assert!(matches!(custom_choice("desktop-login", &dirs), Choice::File(_)));
        assert!(matches!(custom_choice("bell", &dirs), Choice::None));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
