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
    /// Device firmware from fwupd (BIOS/UEFI, SSDs, docks…).
    pub firmware: bool,
}

/// `checkupdates` lines: "linux 6.11.1-1 -> 6.11.2-1".
pub fn parse_checkupdates(out: &str) -> Vec<Update> {
    out.lines()
        .filter_map(|l| {
            let v: Vec<&str> = l.split_whitespace().collect();
            (v.len() == 4 && v[2] == "->").then(|| Update { name: v[0].into(), from: v[1].into(), to: v[3].into(), flatpak: false, firmware: false })
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
            Update { name, from: String::new(), to, flatpak: true, firmware: false }
        })
        .filter(|u| !u.name.is_empty())
        .collect()
}

/// `fwupdmgr get-updates --json`: one update per device, to its newest
/// release. fwupd's JSON names are stable across versions; anything
/// unexpected is skipped rather than failing the whole check.
pub fn parse_fwupd(json: &str) -> Vec<Update> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    v.get("Devices")
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .filter_map(|dev| {
            let release = dev.get("Releases")?.as_array()?.first()?;
            let to = s(release, "Version");
            (!to.is_empty()).then(|| Update { name: s(dev, "Name"), from: s(dev, "Version"), to, flatpak: false, firmware: true })
        })
        .collect()
}

/// Updates that only take effect after a restart.
pub fn needs_restart(updates: &[Update]) -> bool {
    const CORE: &[&str] = &["linux", "linux-lts", "linux-zen", "linux-hardened", "systemd", "glibc", "mesa", "nvidia", "amd-ucode", "intel-ucode", "lapin-ux-comp", "cosmic-comp"];
    updates.iter().any(|u| u.firmware || !u.flatpak && (CORE.contains(&u.name.as_str()) || u.name.starts_with("linux-firmware") || u.name.starts_with("nvidia")))
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

    #[test]
    fn firmware() {
        let json = r#"{"Devices":[
            {"Name":"System Firmware","DeviceId":"a","Version":"1.20","Releases":[{"Version":"1.22","Summary":"UEFI"},{"Version":"1.21"}]},
            {"Name":"Dock","Version":"3","Releases":[]},
            {"Name":"SSD","Version":"2.0"}]}"#;
        let u = parse_fwupd(json);
        assert_eq!(u.len(), 1);
        assert_eq!((u[0].name.as_str(), u[0].from.as_str(), u[0].to.as_str()), ("System Firmware", "1.20", "1.22"));
        assert!(u[0].firmware && needs_restart(&u));
        assert!(parse_fwupd("No updates").is_empty());
    }
}
