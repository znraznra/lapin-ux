// SPDX-License-Identifier: GPL-3.0-only
//! Graphics adapters for Task Manager (#98), read without root or helpers.
//!
//! The adapters come from `/sys/class/drm`, their names from `pci.ids`, and
//! the load from DRM "fdinfo" (`/proc/<pid>/fdinfo/<fd>` of every open
//! `/dev/dri` file), the same source nvtop uses. That works the same for Intel
//! (`i915`, `xe`), AMD (`amdgpu`) and open NVIDIA drivers. AMD also offers
//! `gpu_busy_percent` and VRAM totals in sysfs, which are preferred when there.
//! Only the user's own processes can be read, so work done by other users'
//! processes is not counted (sysfs-only figures are).

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// One graphics adapter.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Adapter {
    /// `card0`.
    pub card: String,
    /// PCI address, `0000:03:00.0` (what fdinfo calls `drm-pdev`).
    pub pci: String,
    /// Kernel driver, `xe`, `i915`, `amdgpu`…
    pub driver: String,
    /// "Intel Arc B580", or the vendor and ids when `pci.ids` doesn't know it.
    pub name: String,
    /// Dedicated memory in bytes, when the driver says.
    pub vram_total: Option<u64>,
}

/// Cumulative counters of one adapter at one moment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reading {
    /// Busy time per engine in ns, summed over the processes.
    pub busy_ns: HashMap<String, u64>,
    /// Busy cycles and clock cycles per engine (Intel `xe`).
    pub cycles: HashMap<String, (u64, u64)>,
    /// Dedicated and shared memory used by the processes, bytes.
    pub dedicated: u64,
    pub shared: u64,
    /// sysfs figures (AMD): busy 0..=1, VRAM used.
    pub sysfs_busy: Option<f64>,
    pub vram_used: Option<u64>,
    pub temp_c: Option<f64>,
}

/// What Task Manager shows for one adapter.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Usage {
    /// 0..=1, the busiest engine.
    pub overall: f64,
    /// Friendly engine name and 0..=1, busiest first.
    pub engines: Vec<(String, f64)>,
}

/// `Intel Arc B580` from the text of `pci.ids`.
pub fn pci_name(pci_ids: &str, vendor: u16, device: u16) -> Option<(String, String)> {
    let v = format!("{vendor:04x}");
    let d = format!("{device:04x}");
    let mut vendor_name = None;
    for line in pci_ids.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            if vendor_name.is_some() {
                return None;
            }
            if let Some(rest) = line.strip_prefix(v.as_str()) {
                vendor_name = Some(rest.trim().to_string());
            }
        } else if vendor_name.is_some() && !line.starts_with("\t\t") {
            let l = &line[1..];
            if let Some(rest) = l.strip_prefix(d.as_str()) {
                return Some((vendor_name?, rest.trim().to_string()));
            }
        }
    }
    None
}

fn short_vendor(v: &str) -> &str {
    let v = v.split(" Corporation").next().unwrap_or(v);
    v.split(" Technologies").next().unwrap_or(v).split(" [").next().unwrap_or(v).trim()
}

fn hex(path: &Path) -> Option<u16> {
    u16::from_str_radix(std::fs::read_to_string(path).ok()?.trim().trim_start_matches("0x"), 16).ok()
}

fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// "Intel Arc B580" from "Intel Corporation" + "BMG [Arc B580]".
fn friendly(vendor: &str, device: &str) -> String {
    let model = match (device.find('['), device.rfind(']')) {
        (Some(a), Some(b)) if b > a => device[a + 1..b].to_string(),
        _ => device.to_string(),
    };
    let vendor = short_vendor(vendor);
    if model.to_lowercase().starts_with(&vendor.to_lowercase()) { model } else { format!("{vendor} {model}") }
}

/// The adapters under `drm_root` (`/sys/class/drm`), first card first.
pub fn adapters(drm_root: &Path, pci_ids: &str) -> Vec<Adapter> {
    let mut cards: Vec<String> = std::fs::read_dir(drm_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.strip_prefix("card").is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit())))
        .collect();
    cards.sort_by_key(|c| c[4..].parse::<u32>().unwrap_or(0));
    let mut out = Vec::new();
    for card in cards {
        let dev = drm_root.join(&card).join("device");
        let (Some(vendor), Some(device)) = (hex(&dev.join("vendor")), hex(&dev.join("device"))) else { continue };
        let Some(driver) = std::fs::read_link(dev.join("driver")).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())) else {
            continue;
        };
        let pci = std::fs::read_link(&dev).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        let name = match pci_name(pci_ids, vendor, device) {
            Some((v, d)) => friendly(&v, &d),
            None => format!("Graphics {vendor:04x}:{device:04x}"),
        };
        out.push(Adapter { card, pci, driver, name, vram_total: read_u64(&dev.join("mem_info_vram_total")) });
    }
    out
}

/// What one fdinfo file says (empty when it isn't a DRM one).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fdinfo {
    pub pdev: String,
    pub client: String,
    pub busy_ns: HashMap<String, u64>,
    pub cycles: HashMap<String, u64>,
    pub total_cycles: HashMap<String, u64>,
    pub dedicated: u64,
    pub shared: u64,
}

fn amount(v: &str) -> u64 {
    let mut it = v.split_whitespace();
    let n: u64 = it.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    match it.next() {
        Some("KiB") => n * 1024,
        Some("MiB") => n * 1024 * 1024,
        Some("GiB") => n * 1024 * 1024 * 1024,
        _ => n,
    }
}

fn dedicated_region(r: &str) -> bool {
    r.contains("vram") || r.contains("local")
}

pub fn parse_fdinfo(s: &str) -> Fdinfo {
    let mut f = Fdinfo::default();
    let mut resident = (0u64, 0u64);
    let mut memory = (0u64, 0u64);
    let mut have_resident = false;
    for line in s.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        if k == "drm-pdev" {
            f.pdev = v.to_string();
        } else if k == "drm-client-id" {
            f.client = v.to_string();
        } else if let Some(e) = k.strip_prefix("drm-engine-") {
            if !e.starts_with("capacity-") {
                f.busy_ns.insert(e.to_string(), amount(v));
            }
        } else if let Some(e) = k.strip_prefix("drm-total-cycles-") {
            f.total_cycles.insert(e.to_string(), amount(v));
        } else if let Some(e) = k.strip_prefix("drm-cycles-") {
            f.cycles.insert(e.to_string(), amount(v));
        } else if let Some(r) = k.strip_prefix("drm-resident-") {
            have_resident = true;
            if dedicated_region(r) { resident.0 += amount(v) } else { resident.1 += amount(v) }
        } else if let Some(r) = k.strip_prefix("drm-memory-") {
            if dedicated_region(r) { memory.0 += amount(v) } else { memory.1 += amount(v) }
        }
    }
    let m = if have_resident { resident } else { memory };
    f.dedicated = m.0;
    f.shared = m.1;
    f
}

/// Walks the processes' DRM files and adds the figures up per adapter.
/// `pids`: the processes that may be read (the user's own).
pub fn read(proc: &Path, drm_root: &Path, pids: &[u32], adapters: &[Adapter]) -> HashMap<String, Reading> {
    let mut out: HashMap<String, Reading> = adapters.iter().map(|a| (a.pci.clone(), Reading::default())).collect();
    let mut seen: HashSet<(u32, String, String)> = HashSet::new();
    for &pid in pids {
        let fd_dir = proc.join(pid.to_string()).join("fd");
        let Ok(fds) = std::fs::read_dir(&fd_dir) else { continue };
        for fd in fds.flatten() {
            if !std::fs::read_link(fd.path()).is_ok_and(|l| l.starts_with("/dev/dri/")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(proc.join(pid.to_string()).join("fdinfo").join(fd.file_name())) else { continue };
            let f = parse_fdinfo(&text);
            if f.pdev.is_empty() || !seen.insert((pid, f.pdev.clone(), f.client.clone())) {
                continue;
            }
            let Some(r) = out.get_mut(&f.pdev) else { continue };
            for (e, ns) in f.busy_ns {
                *r.busy_ns.entry(e).or_default() += ns;
            }
            for (e, c) in f.cycles {
                let t = f.total_cycles.get(&e).copied().unwrap_or(0);
                let slot = r.cycles.entry(e).or_default();
                slot.0 += c;
                slot.1 = slot.1.max(t);
            }
            r.dedicated += f.dedicated;
            r.shared += f.shared;
        }
    }
    for a in adapters {
        let dev = drm_root.join(&a.card).join("device");
        let Some(r) = out.get_mut(&a.pci) else { continue };
        r.sysfs_busy = read_u64(&dev.join("gpu_busy_percent")).map(|p| (p as f64 / 100.0).clamp(0.0, 1.0));
        r.vram_used = read_u64(&dev.join("mem_info_vram_used"));
        r.temp_c = std::fs::read_dir(dev.join("hwmon"))
            .into_iter()
            .flatten()
            .flatten()
            .find_map(|h| read_u64(&h.path().join("temp1_input")))
            .map(|m| m as f64 / 1000.0);
    }
    out
}

/// How an engine is called on screen.
pub fn engine_label(e: &str) -> String {
    match e {
        "rcs" | "render" | "gfx" | "gfx0" => "3D",
        "ccs" | "compute" => "Compute",
        "bcs" | "copy" | "dma" => "Copy",
        "vcs" | "video" | "dec" | "vcn_dec" => "Video decode",
        "vecs" | "video-enhance" | "enc" | "vcn_enc" => "Video encode",
        other => return other.to_string(),
    }
    .to_string()
}

/// Load between two readings `dt` seconds apart.
pub fn usage(prev: &Reading, now: &Reading, dt: f64) -> Usage {
    let dt = dt.max(0.05);
    let mut by_label: HashMap<String, f64> = HashMap::new();
    for (e, ns) in &now.busy_ns {
        let before = prev.busy_ns.get(e).copied().unwrap_or(*ns);
        let u = ns.saturating_sub(before) as f64 / (dt * 1e9);
        let slot = by_label.entry(engine_label(e)).or_default();
        *slot = slot.max(u.clamp(0.0, 1.0));
    }
    for (e, (busy, total)) in &now.cycles {
        let (b0, t0) = prev.cycles.get(e).copied().unwrap_or((*busy, *total));
        let dt_total = total.saturating_sub(t0);
        if dt_total > 0 {
            let u = busy.saturating_sub(b0) as f64 / dt_total as f64;
            let slot = by_label.entry(engine_label(e)).or_default();
            *slot = slot.max(u.clamp(0.0, 1.0));
        }
    }
    let mut engines: Vec<(String, f64)> = by_label.into_iter().collect();
    engines.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let overall = now.sysfs_busy.unwrap_or_else(|| engines.first().map_or(0.0, |e| e.1));
    Usage { overall, engines }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: &str = "# comment\n8086  Intel Corporation\n\t56a0  DG2 [Arc A770]\n\te20b  BMG [Arc B580]\n\t\t172f 0000  sub\n10de  NVIDIA Corporation\n\t2204  GA102 [GeForce RTX 3090]\n";

    #[test]
    fn names_from_pci_ids() {
        assert_eq!(pci_name(IDS, 0x8086, 0xe20b), Some(("Intel Corporation".into(), "BMG [Arc B580]".into())));
        assert_eq!(friendly("Intel Corporation", "BMG [Arc B580]"), "Intel Arc B580");
        assert_eq!(friendly("NVIDIA Corporation", "GA102 [GeForce RTX 3090]"), "NVIDIA GeForce RTX 3090");
        assert_eq!(pci_name(IDS, 0x8086, 0x9999), None);
    }

    const XE: &str = "pos:\t0\ndrm-driver:\txe\ndrm-client-id:\t12\ndrm-pdev:\t0000:03:00.0\ndrm-total-vram0:\t2048 KiB\ndrm-resident-vram0:\t1024 KiB\ndrm-resident-gtt:\t512 KiB\ndrm-cycles-rcs:\t500\ndrm-total-cycles-rcs:\t1000\ndrm-cycles-ccs:\t0\ndrm-total-cycles-ccs:\t1000\n";
    const I915: &str = "drm-driver:\ti915\ndrm-pdev:\t0000:00:02.0\ndrm-client-id:\t7\ndrm-engine-render:\t2000000000 ns\ndrm-engine-capacity-render:\t1\ndrm-total-local0:\t4 MiB\ndrm-resident-system0:\t8 MiB\n";

    #[test]
    fn parses_fdinfo() {
        let f = parse_fdinfo(XE);
        assert_eq!((f.pdev.as_str(), f.client.as_str()), ("0000:03:00.0", "12"));
        assert_eq!((f.dedicated, f.shared), (1024 * 1024, 512 * 1024));
        assert_eq!(f.cycles["rcs"], 500);
        assert_eq!(f.total_cycles["rcs"], 1000);
        let g = parse_fdinfo(I915);
        assert_eq!(g.busy_ns.len(), 1);
        assert_eq!(g.busy_ns["render"], 2_000_000_000);
        assert_eq!(g.shared, 8 * 1024 * 1024);
        assert_eq!(parse_fdinfo("pos:\t0\nflags:\t02\n").pdev, "");
    }

    #[test]
    fn usage_from_two_readings() {
        let mut a = Reading::default();
        a.busy_ns.insert("render".into(), 1_000_000_000);
        a.cycles.insert("ccs".into(), (100, 1000));
        let mut b = a.clone();
        b.busy_ns.insert("render".into(), 1_500_000_000);
        b.cycles.insert("ccs".into(), (200, 2000));
        let u = usage(&a, &b, 1.0);
        assert!((u.overall - 0.5).abs() < 1e-9);
        assert_eq!(u.engines[0].0, "3D");
        assert_eq!(u.engines[1].0, "Compute");
        assert!((u.engines[1].1 - 0.1).abs() < 1e-9);
        // sysfs wins when there.
        b.sysfs_busy = Some(0.8);
        assert!((usage(&a, &b, 1.0).overall - 0.8).abs() < 1e-9);
    }

    #[test]
    fn reads_a_mock_tree() {
        let root = std::env::temp_dir().join(format!("lapin-gpu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let drm = root.join("sys/class/drm");
        let pcidev = root.join("sys/devices/pci0000:00/0000:03:00.0");
        std::fs::create_dir_all(&pcidev).unwrap();
        std::fs::create_dir_all(drm.join("card0")).unwrap();
        std::fs::create_dir_all(drm.join("card0-DP-1")).unwrap();
        std::fs::write(pcidev.join("vendor"), "0x8086\n").unwrap();
        std::fs::write(pcidev.join("device"), "0xe20b\n").unwrap();
        std::os::unix::fs::symlink("../../bus/pci/drivers/xe", pcidev.join("driver")).unwrap();
        std::os::unix::fs::symlink(&pcidev, drm.join("card0/device")).unwrap();
        let list = adapters(&drm, IDS);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].card.as_str(), list[0].driver.as_str(), list[0].name.as_str()), ("card0", "xe", "Intel Arc B580"));
        assert_eq!(list[0].pci, "0000:03:00.0");
        // A process with the card open twice (same client) and a render node.
        let proc = root.join("proc");
        let p = proc.join("42");
        std::fs::create_dir_all(p.join("fd")).unwrap();
        std::fs::create_dir_all(p.join("fdinfo")).unwrap();
        for fd in ["5", "6"] {
            std::os::unix::fs::symlink("/dev/dri/renderD128", p.join("fd").join(fd)).unwrap();
            std::fs::write(p.join("fdinfo").join(fd), XE).unwrap();
        }
        let r = read(&proc, &drm, &[42], &list);
        let r = &r["0000:03:00.0"];
        assert_eq!(r.dedicated, 1024 * 1024);
        assert_eq!(r.cycles["rcs"], (500, 1000));
        let _ = std::fs::remove_dir_all(&root);
    }
}
