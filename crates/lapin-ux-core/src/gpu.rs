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
    /// One entry per DRM client (an open GPU context, shared by processes that
    /// inherited it), by its `drm-client-id`. Load is worked out per client
    /// between two readings and only then added up, so a client that leaves
    /// doesn't take the others' progress with it.
    pub clients: HashMap<String, Client>,
    /// Dedicated and shared memory used by the processes, bytes.
    pub dedicated: u64,
    pub shared: u64,
    /// sysfs figures (AMD): busy 0..=1, VRAM used.
    pub sysfs_busy: Option<f64>,
    pub vram_used: Option<u64>,
    pub temp_c: Option<f64>,
}

/// One DRM client's counters.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Client {
    /// Busy time per engine in ns.
    pub busy_ns: HashMap<String, u64>,
    /// Busy cycles and clock cycles per engine (Intel `xe`).
    pub cycles: HashMap<String, (u64, u64)>,
    /// Engines of a group (`drm-engine-capacity-*`): 4 compute engines run up to 4x.
    pub capacity: HashMap<String, u64>,
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
    pub capacity: HashMap<String, u64>,
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
            if let Some(name) = e.strip_prefix("capacity-") {
                f.capacity.insert(name.to_string(), amount(v).max(1));
            } else {
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
    // A client is counted once even when several processes hold it (a fork keeps the
    // descriptor): by adapter and client id. Without an id, by process.
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for &pid in pids {
        let fd_dir = proc.join(pid.to_string()).join("fd");
        let Ok(fds) = std::fs::read_dir(&fd_dir) else { continue };
        for fd in fds.flatten() {
            if !std::fs::read_link(fd.path()).is_ok_and(|l| l.starts_with("/dev/dri/")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(proc.join(pid.to_string()).join("fdinfo").join(fd.file_name())) else { continue };
            let f = parse_fdinfo(&text);
            let key = if f.client.is_empty() { format!("pid{pid}") } else { f.client.clone() };
            if f.pdev.is_empty() || !seen.insert((f.pdev.clone(), key.clone())) {
                continue;
            }
            let Some(r) = out.get_mut(&f.pdev) else { continue };
            let mut c = Client { busy_ns: f.busy_ns, capacity: f.capacity, ..Client::default() };
            for (e, busy) in f.cycles {
                let t = f.total_cycles.get(&e).copied().unwrap_or(0);
                c.cycles.insert(e, (busy, t));
            }
            r.clients.insert(key, c);
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

impl Reading {
    /// DRM lets a client's busy counter drop for a moment and requires userspace to
    /// keep the highest value it has seen until the counter catches up. Raise every
    /// counter here to at least what `prev` (itself already adjusted) had, so the
    /// readings that follow a dip don't count the same work twice.
    pub fn keep_maximum(&mut self, prev: &Reading) {
        for (id, c) in self.clients.iter_mut() {
            let Some(p) = prev.clients.get(id) else { continue };
            for (e, v) in c.busy_ns.iter_mut() {
                if let Some(old) = p.busy_ns.get(e) {
                    *v = (*v).max(*old);
                }
            }
            for (e, (busy, total)) in c.cycles.iter_mut() {
                if let Some((ob, ot)) = p.cycles.get(e) {
                    *busy = (*busy).max(*ob);
                    *total = (*total).max(*ot);
                }
            }
        }
    }
}

/// Load between two readings `dt` seconds apart. Each client that is in both
/// readings contributes its own change; one that appeared or left in between
/// contributes nothing to that interval (it has no earlier or later figure).
pub fn usage(prev: &Reading, now: &Reading, dt: f64) -> Usage {
    let dt = dt.max(0.05);
    // Per engine: busy ns gained, busy cycles gained, clock cycles passed, capacity.
    let mut ns: HashMap<String, f64> = HashMap::new();
    let mut cyc: HashMap<String, (f64, f64)> = HashMap::new();
    let mut cap: HashMap<String, f64> = HashMap::new();
    for (id, c) in &now.clients {
        let Some(before) = prev.clients.get(id) else { continue };
        for (e, v) in &c.busy_ns {
            let b = before.busy_ns.get(e).copied().unwrap_or(*v);
            *ns.entry(e.clone()).or_default() += v.saturating_sub(b) as f64;
        }
        for (e, (busy, total)) in &c.cycles {
            let (b0, t0) = before.cycles.get(e).copied().unwrap_or((*busy, *total));
            let slot = cyc.entry(e.clone()).or_default();
            slot.0 += busy.saturating_sub(b0) as f64;
            // Every client sees the same clock: take the longest interval.
            slot.1 = slot.1.max(total.saturating_sub(t0) as f64);
        }
        for (e, n) in &c.capacity {
            let slot = cap.entry(e.clone()).or_insert(1.0);
            *slot = slot.max(*n as f64);
        }
    }
    let mut by_label: HashMap<String, f64> = HashMap::new();
    for (e, gained) in &ns {
        let u = gained / (dt * 1e9 * cap.get(e).copied().unwrap_or(1.0));
        let slot = by_label.entry(engine_label(e)).or_default();
        *slot = slot.max(u.clamp(0.0, 1.0));
    }
    for (e, (gained, total)) in &cyc {
        if *total > 0.0 {
            let u = gained / (total * cap.get(e).copied().unwrap_or(1.0));
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
        assert_eq!(g.capacity["render"], 1);
        assert_eq!(g.shared, 8 * 1024 * 1024);
        assert_eq!(parse_fdinfo("pos:\t0\nflags:\t02\n").pdev, "");
    }

    fn client(busy: &[(&str, u64)], cycles: &[(&str, u64, u64)], cap: &[(&str, u64)]) -> Client {
        Client {
            busy_ns: busy.iter().map(|(e, v)| (e.to_string(), *v)).collect(),
            cycles: cycles.iter().map(|(e, b, t)| (e.to_string(), (*b, *t))).collect(),
            capacity: cap.iter().map(|(e, v)| (e.to_string(), *v)).collect(),
        }
    }
    fn reading(clients: &[(&str, Client)]) -> Reading {
        Reading { clients: clients.iter().map(|(k, c)| (k.to_string(), c.clone())).collect(), ..Reading::default() }
    }

    #[test]
    fn usage_from_two_readings() {
        let a = reading(&[("1", client(&[("render", 1_000_000_000)], &[("ccs", 100, 1000)], &[]))]);
        let mut b = reading(&[("1", client(&[("render", 1_500_000_000)], &[("ccs", 200, 2000)], &[]))]);
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
    fn a_counter_that_dips_is_not_counted_twice() {
        let g = |ns: u64| reading(&[("1", client(&[("render", ns * 1_000_000_000)], &[("rcs", ns * 100, ns * 1000)], &[]))]);
        let (s1, mut s2, mut s3) = (g(10), g(9), g(10));
        s2.keep_maximum(&s1);
        assert_eq!(s2.clients["1"].busy_ns["render"], 10_000_000_000);
        assert_eq!(usage(&s1, &s2, 1.0).overall, 0.0);
        s3.keep_maximum(&s2);
        // 10, 9, 10 s: the work was all reported before; nothing new happened.
        let u = usage(&s2, &s3, 1.0);
        assert_eq!(u.overall, 0.0);
        assert!(u.engines.iter().all(|e| e.1 == 0.0));
        // Recovery past the old maximum counts only what is new.
        let mut s4 = g(11);
        s4.keep_maximum(&s3);
        assert!((usage(&s3, &s4, 1.0).overall - 1.0).abs() < 1e-9);
    }

    #[test]
    fn grouped_engines_share_their_capacity() {
        // 4 compute engines, one fully busy for the second: 25%, not 100%.
        let a = reading(&[("1", client(&[("ccs", 0)], &[], &[("ccs", 4)]))]);
        let b = reading(&[("1", client(&[("ccs", 1_000_000_000)], &[], &[("ccs", 4)]))]);
        assert!((usage(&a, &b, 1.0).engines[0].1 - 0.25).abs() < 1e-9);
    }

    #[test]
    fn a_client_leaving_doesnt_hide_the_others_load() {
        // Client 1 (10 s of life-time busy) leaves; client 2 stays and gains 0.5 s.
        let a = reading(&[("1", client(&[("render", 10_000_000_000)], &[], &[])), ("2", client(&[("render", 1_000_000_000)], &[], &[]))]);
        let b = reading(&[("2", client(&[("render", 1_500_000_000)], &[], &[]))]);
        assert!((usage(&a, &b, 1.0).overall - 0.5).abs() < 1e-9);
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
        // Both descriptors are the same client: counted once.
        assert_eq!(r.clients.len(), 1);
        assert_eq!(r.clients["12"].cycles["rcs"], (500, 1000));
        // A forked process with the same client id doesn't count it twice.
        let p2 = proc.join("43");
        std::fs::create_dir_all(p2.join("fd")).unwrap();
        std::fs::create_dir_all(p2.join("fdinfo")).unwrap();
        std::os::unix::fs::symlink("/dev/dri/renderD128", p2.join("fd/5")).unwrap();
        std::fs::write(p2.join("fdinfo/5"), XE).unwrap();
        let r2 = read(&proc, &drm, &[42, 43], &list);
        assert_eq!(r2["0000:03:00.0"].dedicated, 1024 * 1024);
        let _ = std::fs::remove_dir_all(&root);
    }
}
