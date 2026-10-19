// SPDX-License-Identifier: GPL-3.0-only
//! System sounds (freedesktop sound theme names), played with PipeWire's
//! `pw-play` (or `paplay`). Lapin UX ships the Borealis theme.

use std::path::{Path, PathBuf};

pub const THEME: &str = "borealis";

/// Find `event` in `theme` (then freedesktop's fallback theme) under the
/// data dirs: `<dir>/sounds/<theme>/stereo/<event>.{oga,ogg,wav}`.
pub fn find(theme: &str, event: &str, data_dirs: &[PathBuf]) -> Option<PathBuf> {
    for t in [theme, "freedesktop"] {
        for d in data_dirs {
            for sub in ["stereo", ""] {
                for ext in ["oga", "ogg", "wav"] {
                    let p = d.join("sounds").join(t).join(sub).join(format!("{event}.{ext}"));
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
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

/// Play a sound event in the background; silently does nothing when the
/// sound or a player is missing.
pub fn play(event: &str) {
    if std::env::var_os("LAPIN_UX_NO_SOUNDS").is_some() {
        return;
    }
    let Some(file) = find(THEME, event, &data_dirs()) else { return };
    for player in ["pw-play", "paplay"] {
        let spawned = std::process::Command::new(player)
            .arg(&file)
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
}
