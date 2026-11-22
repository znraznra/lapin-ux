// SPDX-License-Identifier: GPL-3.0-only
//! Removable drives for the taskbar's "Safely Remove Hardware" icon (#102).
//!
//! UI-free: the shell reads UDisks2 and fills these types, this module decides
//! which drives are listed, how they are named and which programs keep a
//! volume busy. Tested without a bus.

use std::path::{Path, PathBuf};

/// What UDisks2 says about a drive (`org.freedesktop.UDisks2.Drive`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DriveProps {
    pub vendor: String,
    pub model: String,
    /// `ConnectionBus`: "usb", "sdio", "ata"...
    pub bus: String,
    pub removable: bool,
    pub ejectable: bool,
    pub can_power_off: bool,
    pub media_removable: bool,
    /// CD/DVD drives (`MediaCompatibility` has an `optical_*` entry): not listed.
    /// Phones (MTP) and network shares never appear in UDisks2 drives at all.
    pub optical: bool,
}

/// A partition of a drive (`Block` + `Filesystem` + `Encrypted`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VolumeInfo {
    /// Object path of the block device.
    pub path: String,
    /// `IdLabel`.
    pub label: String,
    pub size: u64,
    pub mount_points: Vec<PathBuf>,
    /// A LUKS container, locked or open.
    pub encrypted: bool,
    /// For an open LUKS container: the object path of the cleartext device.
    pub cleartext: Option<String>,
}

impl VolumeInfo {
    pub fn mounted(&self) -> bool {
        !self.mount_points.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drive {
    /// Object path of the drive.
    pub path: String,
    pub props: DriveProps,
    pub volumes: Vec<VolumeInfo>,
}

/// How a drive is taken away once its volumes are unmounted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remove {
    /// USB sticks and disks: power the port off.
    PowerOff,
    /// SD card readers and optical drives: eject the card, keep the reader.
    Eject,
}

/// Whether the tray icon lists this drive: USB and SD readers that can be
/// taken away. Internal disks (sata, nvme), optical drives and network shares
/// are left out.
pub fn is_listed(p: &DriveProps) -> bool {
    let bus_ok = matches!(p.bus.as_str(), "usb" | "sdio");
    bus_ok && !p.optical && (p.removable || p.ejectable || p.can_power_off || p.media_removable)
}

/// `PowerOff` for a USB drive that supports it, `Eject` for a reader whose
/// media is removable (the reader stays), `PowerOff` otherwise.
pub fn how_to_remove(p: &DriveProps) -> Remove {
    if p.media_removable && p.ejectable {
        Remove::Eject
    } else if p.can_power_off {
        Remove::PowerOff
    } else {
        Remove::Eject
    }
}

/// "Vendor Model", tidied (UDisks2 pads vendors with spaces and repeats them).
pub fn drive_name(p: &DriveProps) -> String {
    let vendor = p.vendor.split_whitespace().collect::<Vec<_>>().join(" ");
    let model = p.model.split_whitespace().collect::<Vec<_>>().join(" ");
    match (vendor.is_empty(), model.is_empty()) {
        (true, true) => "Removable drive".to_string(),
        (true, false) => model,
        (false, true) => vendor,
        (false, false) if model.to_lowercase().starts_with(&vendor.to_lowercase()) => model,
        (false, false) => format!("{vendor} {model}"),
    }
}

/// The volume's label, or its size when it has none ("Removable disk (16 GB)").
pub fn volume_name(v: &VolumeInfo) -> String {
    let label = v.label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    if v.size > 0 { format!("Removable disk ({})", size_text(v.size)) } else { "Removable disk".to_string() }
}

fn size_text(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    const MB: f64 = 1_000_000.0;
    let b = bytes as f64;
    if b >= GB {
        let g = b / GB;
        if g >= 10.0 { format!("{g:.0} GB") } else { format!("{g:.1} GB") }
    } else {
        format!("{:.0} MB", (b / MB).max(1.0))
    }
}

/// The menu entry: "Eject Vendor Model".
pub fn eject_label(d: &Drive) -> String {
    format!("Eject {}", drive_name(&d.props))
}

/// The tray tooltip for the whole icon.
pub fn tooltip(drives: &[Drive]) -> String {
    match drives {
        [] => "Safely Remove Hardware and Eject Media".to_string(),
        [d] => format!("Safely Remove Hardware and Eject Media: {}", drive_name(&d.props)),
        n => format!("Safely Remove Hardware and Eject Media: {} drives", n.len()),
    }
}

/// The volumes the menu lists under a drive: an open LUKS container is
/// represented by its cleartext volume, so the container itself is left out.
pub fn shown_volumes(d: &Drive) -> impl Iterator<Item = &VolumeInfo> {
    d.volumes.iter().filter(|v| !(v.encrypted && v.cleartext.is_some()))
}

/// Mount points of every mounted volume of a drive, also the cleartext of an
/// open LUKS container (given as separate volumes by the shell).
pub fn mounted_paths(d: &Drive) -> Vec<&Path> {
    d.volumes.iter().flat_map(|v| v.mount_points.iter().map(|m| m.as_path())).collect()
}

/// A running program and the places it holds open (its working directory and
/// open files), read from `/proc` by the shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    /// `/proc/PID/comm`, or a friendlier name when known.
    pub name: String,
    pub open: Vec<PathBuf>,
}

/// Names of the programs that keep one of `mounts` busy, sorted, without
/// repeats. A path counts when it is the mount point or below it.
pub fn busy_programs(mounts: &[&Path], procs: &[Proc]) -> Vec<String> {
    let mut names: Vec<String> = procs
        .iter()
        .filter(|p| p.open.iter().any(|f| mounts.iter().any(|m| f.starts_with(m))))
        .map(|p| p.name.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

/// "Windows can't stop..." in our words: which programs to close.
pub fn busy_message(drive: &str, names: &[String]) -> String {
    match names {
        [] => format!("{drive} is in use. Close any programs or windows that use it, then try again."),
        [one] => format!("{drive} is in use by {one}. Close it, then try again."),
        many => {
            let (last, first) = many.split_last().expect("not empty");
            format!("{drive} is in use by {} and {last}. Close them, then try again.", first.join(", "))
        }
    }
}

/// Whether a UDisks2 error name means "something is using it".
pub fn is_busy_error(name: &str) -> bool {
    name.ends_with("DeviceBusy")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb() -> DriveProps {
        DriveProps {
            vendor: "SanDisk ".into(),
            model: "Ultra  Fit".into(),
            bus: "usb".into(),
            removable: true,
            can_power_off: true,
            ..Default::default()
        }
    }

    #[test]
    fn lists_usb_and_sd_only() {
        assert!(is_listed(&usb()));
        let sd = DriveProps { bus: "sdio".into(), removable: true, media_removable: true, ejectable: true, ..Default::default() };
        assert!(is_listed(&sd));
        let nvme = DriveProps { bus: "nvme".into(), ..Default::default() };
        assert!(!is_listed(&nvme));
        let sata = DriveProps { bus: "ata".into(), removable: true, ..Default::default() };
        assert!(!is_listed(&sata));
        let dvd = DriveProps { bus: "usb".into(), optical: true, ejectable: true, ..Default::default() };
        assert!(!is_listed(&dvd));
        let fixed_usb = DriveProps { bus: "usb".into(), ..Default::default() };
        assert!(!is_listed(&fixed_usb));
    }

    #[test]
    fn sticks_power_off_readers_eject() {
        assert_eq!(how_to_remove(&usb()), Remove::PowerOff);
        let reader = DriveProps { bus: "usb".into(), media_removable: true, ejectable: true, can_power_off: true, ..Default::default() };
        assert_eq!(how_to_remove(&reader), Remove::Eject);
    }

    #[test]
    fn names() {
        assert_eq!(drive_name(&usb()), "SanDisk Ultra Fit");
        let same = DriveProps { vendor: "Kingston".into(), model: "Kingston DataTraveler".into(), ..Default::default() };
        assert_eq!(drive_name(&same), "Kingston DataTraveler");
        assert_eq!(drive_name(&DriveProps::default()), "Removable drive");
        let d = Drive { props: usb(), ..Default::default() };
        assert_eq!(eject_label(&d), "Eject SanDisk Ultra Fit");
    }

    #[test]
    fn volume_names() {
        let v = VolumeInfo { label: " PHOTOS ".into(), ..Default::default() };
        assert_eq!(volume_name(&v), "PHOTOS");
        let v = VolumeInfo { size: 16_000_000_000, ..Default::default() };
        assert_eq!(volume_name(&v), "Removable disk (16 GB)");
        let v = VolumeInfo { size: 4_000_000_000, ..Default::default() };
        assert_eq!(volume_name(&v), "Removable disk (4.0 GB)");
        assert_eq!(volume_name(&VolumeInfo::default()), "Removable disk");
    }

    #[test]
    fn tooltip_counts() {
        assert_eq!(tooltip(&[]), "Safely Remove Hardware and Eject Media");
        let d = Drive { props: usb(), ..Default::default() };
        assert!(tooltip(std::slice::from_ref(&d)).ends_with("SanDisk Ultra Fit"));
        assert!(tooltip(&[d.clone(), d]).ends_with("2 drives"));
    }

    #[test]
    fn busy_by_path() {
        let mnt = PathBuf::from("/run/media/tako/PHOTOS");
        let procs = vec![
            Proc { pid: 1, name: "cp".into(), open: vec!["/run/media/tako/PHOTOS/a.iso".into()] },
            Proc { pid: 2, name: "Explorer".into(), open: vec!["/run/media/tako/PHOTOS".into()] },
            Proc { pid: 3, name: "bash".into(), open: vec!["/run/media/tako/PHOTOS2/x".into(), "/home/tako".into()] },
            Proc { pid: 4, name: "cp".into(), open: vec!["/run/media/tako/PHOTOS/b.iso".into()] },
        ];
        // PHOTOS2 is a different folder, not below PHOTOS.
        assert_eq!(busy_programs(&[mnt.as_path()], &procs), vec!["cp", "Explorer"]);
        assert!(busy_programs(&[], &procs).is_empty());
    }

    #[test]
    fn open_luks_shows_cleartext_only() {
        let container = VolumeInfo { path: "/b/1".into(), encrypted: true, cleartext: Some("/b/2".into()), ..Default::default() };
        let clear = VolumeInfo { path: "/b/2".into(), label: "Secret".into(), ..Default::default() };
        let locked = VolumeInfo { path: "/b/3".into(), encrypted: true, ..Default::default() };
        let d = Drive { volumes: vec![container, clear, locked], ..Default::default() };
        let shown: Vec<_> = shown_volumes(&d).map(|v| v.path.as_str()).collect();
        assert_eq!(shown, vec!["/b/2", "/b/3"]);
    }

    #[test]
    fn busy_text() {
        assert!(busy_message("Stick", &[]).contains("in use"));
        assert_eq!(busy_message("Stick", &["cp".into()]), "Stick is in use by cp. Close it, then try again.");
        assert_eq!(
            busy_message("Stick", &["a".into(), "b".into(), "c".into()]),
            "Stick is in use by a, b and c. Close them, then try again."
        );
    }

    #[test]
    fn busy_error_names() {
        assert!(is_busy_error("org.freedesktop.UDisks2.Error.DeviceBusy"));
        assert!(!is_busy_error("org.freedesktop.UDisks2.Error.Failed"));
    }
}
