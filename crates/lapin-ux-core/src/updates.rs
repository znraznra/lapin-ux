// SPDX-License-Identifier: GPL-3.0-only
//! Pending updates, as `checkupdates` (pacman-contrib) and
//! `flatpak remote-ls --updates` report them.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub name: String,
    pub from: String,
    pub to: String,
    /// Flatpak app rather than a system package.
    pub flatpak: bool,
}

/// `checkupdates` lines: "linux 6.11.1-1 -> 6.11.2-1".
pub fn parse_checkupdates(out: &str) -> Vec<Update> {
    out.lines()
        .filter_map(|l| {
            let v: Vec<&str> = l.split_whitespace().collect();
            (v.len() == 4 && v[2] == "->").then(|| Update { name: v[0].into(), from: v[1].into(), to: v[3].into(), flatpak: false })
        })
        .collect()
}

/// `flatpak remote-ls --updates --columns=application,version` lines.
pub fn parse_flatpak(out: &str) -> Vec<Update> {
    out.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("Application"))
        .map(|l| {
            let mut it = l.split('\t');
            let name = it.next().unwrap_or_default().trim().to_string();
            let to = it.next().unwrap_or_default().trim().to_string();
            Update { name, from: String::new(), to, flatpak: true }
        })
        .filter(|u| !u.name.is_empty())
        .collect()
}

/// Updates that only take effect after a restart.
pub fn needs_restart(updates: &[Update]) -> bool {
    const CORE: &[&str] = &["linux", "linux-lts", "linux-zen", "linux-hardened", "systemd", "glibc", "mesa", "nvidia", "amd-ucode", "intel-ucode", "lapin-ux-comp", "cosmic-comp"];
    updates.iter().any(|u| !u.flatpak && (CORE.contains(&u.name.as_str()) || u.name.starts_with("linux-firmware") || u.name.starts_with("nvidia")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let u = parse_checkupdates("linux 6.11.1.arch1-1 -> 6.11.2.arch1-1\nfirefox 131.0-1 -> 131.0.2-1\n\n");
        assert_eq!(u.len(), 2);
        assert_eq!(u[1].to, "131.0.2-1");
        assert!(needs_restart(&u));
        let f = parse_flatpak("org.gimp.GIMP\t2.10.38\ncom.spotify.Client\t1.2\n");
        assert_eq!(f.len(), 2);
        assert!(f[0].flatpak);
        assert!(!needs_restart(&f));
    }
}
