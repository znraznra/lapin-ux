// SPDX-License-Identifier: GPL-3.0-only
//! Low disk space warning (#174): reading free space, deciding when it is
//! low, and warning once per crossing.
//!
//! A drive counts as low when less than 5 percent of it is free, but never
//! warn above 10 GB (5 percent of a big drive is plenty) and always warn
//! under 1 GB. The warning is shown once; it is armed again only after the
//! free space has recovered with some room to spare, so a drive hovering
//! around the limit does not nag.

use std::collections::HashSet;

const MB: u64 = 1_000_000;
const GB: u64 = 1_000 * MB;

/// Below this much free space a drive is always low.
pub const FLOOR: u64 = GB;
/// Above this much free space a drive is never low.
pub const CEILING: u64 = 10 * GB;

/// One mounted drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    pub mount: String,
    pub total: u64,
    pub free: u64,
}

impl Drive {
    /// The free space under which this drive is low.
    pub fn limit(&self) -> u64 {
        (self.total / 20).clamp(FLOOR, CEILING)
    }

    pub fn is_low(&self) -> bool {
        self.free < self.limit()
    }

    /// Free again by a margin (a fifth over the limit), so the warning can
    /// be shown again the next time it runs low.
    pub fn has_recovered(&self) -> bool {
        self.free >= self.limit() + self.limit() / 5
    }
}

/// `df -P -B1 PATH...` output: "Filesystem 1-blocks Used Available Capacity
/// Mounted on", then one line per path. Drives that appear twice (home and
/// root on the same filesystem) are listed once.
pub fn parse_df(out: &str) -> Vec<Drive> {
    let mut seen = HashSet::new();
    out.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 6 {
                return None;
            }
            let (total, free) = (f[1].parse().ok()?, f[3].parse().ok()?);
            let mount = f[5..].join(" ");
            seen.insert(mount.clone()).then_some(Drive { mount, total, free })
        })
        .collect()
}

/// Remembers which drives were already warned about.
#[derive(Debug, Default)]
pub struct Watch {
    warned: HashSet<String>,
}

impl Watch {
    /// Feed it the current state of the drives; it returns the ones that
    /// just ran low and have not been warned about yet.
    pub fn check(&mut self, drives: &[Drive]) -> Vec<Drive> {
        self.warned.retain(|m| drives.iter().any(|d| &d.mount == m && !d.has_recovered()));
        drives.iter().filter(|d| d.is_low() && self.warned.insert(d.mount.clone())).cloned().collect()
    }
}

/// "850 MB", "1.5 GB".
pub fn format_size(bytes: u64) -> String {
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{} MB", bytes / MB)
    }
}

/// The notification text for drives that just ran low.
pub fn notice(low: &[Drive], home_mount: &str) -> (String, String) {
    let name = |d: &Drive| match (d.mount == home_mount, d.mount == "/") {
        (true, true) => "your drive".to_string(),
        (true, false) => "the drive with your files".to_string(),
        (false, true) => "the system drive".to_string(),
        (false, false) => format!("the drive at {}", d.mount),
    };
    let lines: Vec<String> = low.iter().map(|d| format!("Only {} is left on {}.", format_size(d.free), name(d))).collect();
    (
        "Disk space is running low".into(),
        format!("{} Free up some space so you can keep saving your work and signing in.", lines.join(" ")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(mount: &str, total: u64, free: u64) -> Drive {
        Drive { mount: mount.into(), total, free }
    }

    #[test]
    fn limit_scales_with_the_drive() {
        assert_eq!(drive("/", 8 * GB, 0).limit(), GB);
        assert_eq!(drive("/", 100 * GB, 0).limit(), 5 * GB);
        assert_eq!(drive("/", 4_000 * GB, 0).limit(), 10 * GB);
    }

    #[test]
    fn low_when_under_the_limit() {
        assert!(drive("/", 100 * GB, 4 * GB).is_low());
        assert!(!drive("/", 100 * GB, 6 * GB).is_low());
        assert!(!drive("/", 4_000 * GB, 150 * GB).is_low());
        assert!(drive("/", 4_000 * GB, 800 * MB).is_low());
    }

    #[test]
    fn parses_df_and_lists_a_shared_drive_once() {
        let out = "Filesystem 1-blocks Used Available Capacity Mounted on\n\
                   /dev/nvme0n1p2 100000000000 98000000000 2000000000 98% /\n\
                   /dev/nvme0n1p2 100000000000 98000000000 2000000000 98% /\n";
        assert_eq!(parse_df(out), vec![drive("/", 100 * GB, 2 * GB)]);
        assert!(parse_df("garbage").is_empty());
    }

    #[test]
    fn warns_once_per_crossing() {
        let mut w = Watch::default();
        let low = [drive("/", 100 * GB, 2 * GB)];
        assert_eq!(w.check(&low).len(), 1);
        // Still low: no repeat.
        assert!(w.check(&low).is_empty());
        // Barely above the limit: not recovered, no repeat when it dips again.
        assert!(w.check(&[drive("/", 100 * GB, 5_100 * MB)]).is_empty());
        assert!(w.check(&low).is_empty());
        // Properly freed, then low again: warned again.
        assert!(w.check(&[drive("/", 100 * GB, 20 * GB)]).is_empty());
        assert_eq!(w.check(&low).len(), 1);
    }

    #[test]
    fn each_drive_has_its_own_warning() {
        let mut w = Watch::default();
        let a = drive("/", 100 * GB, 2 * GB);
        let b = drive("/home", 100 * GB, 2 * GB);
        assert_eq!(w.check(std::slice::from_ref(&a)).len(), 1);
        assert_eq!(w.check(&[a, b]).len(), 1);
    }

    #[test]
    fn text_names_the_drive() {
        let (t, b) = notice(&[drive("/", 100 * GB, 850 * MB)], "/");
        assert_eq!(t, "Disk space is running low");
        assert!(b.starts_with("Only 850 MB is left on your drive."));
        assert!(notice(&[drive("/", GB, 1_500 * MB)], "/home").1.contains("1.5 GB is left on the system drive"));
    }
}
