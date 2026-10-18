// SPDX-License-Identifier: GPL-3.0-only
//! "End task": find the processes behind an app. Wayland doesn't tell the
//! shell which process owns a window, so match the user's own processes by
//! Flatpak id or by program name, the way a task manager groups them.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMatch {
    /// A Flatpak app: its processes carry FLATPAK_ID in their environment.
    Flatpak(String),
    /// A native app: processes whose program (executable or argv[0]) has this name.
    Program(String),
}

/// Programs too generic to end by name (an app started through an
/// interpreter or wrapper would take every other script down with it).
const TOO_GENERIC: &[&str] = &[
    "sh", "bash", "zsh", "fish", "dash", "env", "python", "python3", "perl", "ruby", "node", "java", "sudo", "pkexec",
    "systemd", "dbus-daemon", "gjs", "electron", "wine", "mono", "dotnet", "steam",
];

/// How to find an app's processes from its id and desktop-file command
/// line, or None when it can't be done safely.
pub fn app_match(app_id: &str, exec: Option<&str>) -> Option<AppMatch> {
    let program = exec.map(crate::recent::program_of).unwrap_or_default();
    if program == "flatpak" || exec.is_some_and(|e| e.contains("--command=") && e.contains("flatpak")) {
        return Some(AppMatch::Flatpak(app_id.to_string()));
    }
    if program.is_empty() || TOO_GENERIC.contains(&program.as_str()) || program.starts_with("lapin-ux") || program.starts_with("cosmic-") {
        return None;
    }
    Some(AppMatch::Program(program))
}

/// PIDs under `proc` (normally "/proc") owned by `uid` that belong to the app.
pub fn find(proc: &Path, uid: u32, m: &AppMatch) -> Vec<u32> {
    use std::os::unix::fs::MetadataExt;
    let Ok(rd) = std::fs::read_dir(proc) else { return Vec::new() };
    let mut pids: Vec<u32> = rd
        .flatten()
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let dir = e.path();
            if std::fs::metadata(&dir).ok()?.uid() != uid {
                return None;
            }
            matches(&dir, m).then_some(pid)
        })
        .collect();
    pids.sort_unstable();
    pids
}

fn matches(dir: &Path, m: &AppMatch) -> bool {
    match m {
        AppMatch::Flatpak(id) => {
            let env = std::fs::read(dir.join("environ")).unwrap_or_default();
            let want = format!("FLATPAK_ID={id}");
            env.split(|b| *b == 0).any(|kv| kv == want.as_bytes())
        }
        AppMatch::Program(prog) => {
            let exe = std::fs::read_link(dir.join("exe")).ok();
            let exe_name = exe.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
            let cmd = std::fs::read(dir.join("cmdline")).unwrap_or_default();
            let argv0 = cmd.split(|b| *b == 0).next().map(|a| String::from_utf8_lossy(a).into_owned()).unwrap_or_default();
            let argv0_name = argv0.rsplit('/').next().unwrap_or("").to_string();
            exe_name.as_deref() == Some(prog.as_str()) || argv0_name == *prog
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_match() {
        assert_eq!(app_match("org.gnome.Loupe", Some("flatpak run --branch=stable org.gnome.Loupe %U")), Some(AppMatch::Flatpak("org.gnome.Loupe".into())));
        assert_eq!(app_match("firefox", Some("/usr/lib/firefox/firefox %u")), Some(AppMatch::Program("firefox".into())));
        assert_eq!(app_match("tool", Some("python3 /opt/tool.py")), None);
        assert_eq!(app_match("x", Some("env FOO=1 sh -c thing")), None);
        assert_eq!(app_match("x", None), None);
    }
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn finds_by_program_and_flatpak() {
        let root = std::env::temp_dir().join(format!("lapin-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mk = |pid: u32, cmd: &[u8], env: &[u8]| {
            let d = root.join(pid.to_string());
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("cmdline"), cmd).unwrap();
            std::fs::write(d.join("environ"), env).unwrap();
        };
        mk(10, b"/usr/lib/firefox/firefox\0-new-window\0", b"HOME=/h\0");
        mk(11, b"firefox\0-contentproc\0", b"");
        mk(12, b"/usr/bin/bash\0", b"");
        mk(13, b"/app/bin/spotify\0", b"A=1\0FLATPAK_ID=com.spotify.Client\0");
        std::fs::create_dir_all(root.join("self")).unwrap();
        let uid = std::fs::metadata(&root).unwrap().uid();
        assert_eq!(find(&root, uid, &AppMatch::Program("firefox".into())), vec![10, 11]);
        assert_eq!(find(&root, uid, &AppMatch::Flatpak("com.spotify.Client".into())), vec![13]);
        assert!(find(&root, uid + 1, &AppMatch::Program("firefox".into())).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
