// SPDX-License-Identifier: GPL-3.0-only
//! What Task Manager shows, read from /proc: processes (CPU, memory, disk),
//! CPU and memory use, disk and network throughput, and the apps that start
//! when you sign in (XDG autostart).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One process, as sampled.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub uid: u32,
    /// Short name (comm), e.g. "firefox".
    pub name: String,
    /// Program name from the command line (argv[0] without its path).
    pub program: String,
    pub cmdline: String,
    /// R, S, D, Z, T…
    pub state: char,
    /// CPU time used so far, in clock ticks.
    pub cpu_ticks: u64,
    pub rss: u64,
    /// Bytes read from / written to storage so far (own processes only).
    pub read_bytes: u64,
    pub write_bytes: u64,
    /// It talks to the display (has WAYLAND_DISPLAY or DISPLAY): an app.
    pub gui: bool,
    pub threads: u32,
}

/// Parse /proc/<pid>/stat. The name is in parentheses and may itself hold
/// spaces or parentheses, so split at the last ')'.
pub fn parse_stat(s: &str) -> Option<(String, char, u32, u64, u32)> {
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    let name = s[open + 1..close].to_string();
    let rest: Vec<&str> = s[close + 1..].split_whitespace().collect();
    // rest[0] = state (field 3), rest[1] = ppid, utime = field 14, stime = 15, threads = 20.
    let state = rest.first()?.chars().next()?;
    let ppid = rest.get(1)?.parse().ok()?;
    let utime: u64 = rest.get(11)?.parse().ok()?;
    let stime: u64 = rest.get(12)?.parse().ok()?;
    let threads = rest.get(17).and_then(|t| t.parse().ok()).unwrap_or(1);
    Some((name, state, ppid, utime + stime, threads))
}

fn page_size() -> u64 {
    4096
}

/// Read one process; None if it vanished or can't be read.
pub fn read_proc(proc: &Path, pid: u32) -> Option<Proc> {
    use std::os::unix::fs::MetadataExt;
    let dir = proc.join(pid.to_string());
    let stat = std::fs::read_to_string(dir.join("stat")).ok()?;
    let (name, state, ppid, cpu_ticks, threads) = parse_stat(&stat)?;
    let uid = std::fs::metadata(&dir).ok()?.uid();
    let rss = std::fs::read_to_string(dir.join("statm"))
        .ok()
        .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()))
        .unwrap_or(0)
        * page_size();
    let cmd = std::fs::read(dir.join("cmdline")).unwrap_or_default();
    let args: Vec<String> = cmd.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    let program = args.first().map(|a| a.rsplit('/').next().unwrap_or(a).to_string()).unwrap_or_default();
    let (mut read_bytes, mut write_bytes) = (0, 0);
    if let Ok(io) = std::fs::read_to_string(dir.join("io")) {
        for l in io.lines() {
            if let Some(v) = l.strip_prefix("read_bytes: ") {
                read_bytes = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = l.strip_prefix("write_bytes: ") {
                write_bytes = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let gui = std::fs::read(dir.join("environ"))
        .map(|e| e.split(|b| *b == 0).any(|v| v.starts_with(b"WAYLAND_DISPLAY=") || v.starts_with(b"DISPLAY=")))
        .unwrap_or(false);
    Some(Proc { pid, ppid, uid, name, program, cmdline: args.join(" "), state, cpu_ticks, rss, read_bytes, write_bytes, gui, threads })
}

/// Every process under `proc`.
pub fn read_all(proc: &Path) -> Vec<Proc> {
    let Ok(rd) = std::fs::read_dir(proc) else { return Vec::new() };
    rd.flatten().filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok()).filter_map(|pid| read_proc(proc, pid)).collect()
}

/// Aggregate CPU time from the first line of /proc/stat: (busy, total) ticks.
pub fn parse_cpu(stat: &str) -> Option<(u64, u64)> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    if v.len() < 4 {
        return None;
    }
    // user nice system idle iowait irq softirq steal (guest is inside user).
    let total: u64 = v.iter().take(8).sum();
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    Some((total - idle, total))
}

/// Number of logical processors in /proc/stat.
pub fn cpu_count(stat: &str) -> usize {
    stat.lines().filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(u8::is_ascii_digit)).count().max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

impl Memory {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
}

pub fn parse_meminfo(s: &str) -> Memory {
    let mut m = Memory::default();
    for l in s.lines() {
        let mut it = l.split_whitespace();
        let (Some(k), Some(v)) = (it.next(), it.next().and_then(|v| v.parse::<u64>().ok())) else { continue };
        let v = v * 1024;
        match k {
            "MemTotal:" => m.total = v,
            "MemAvailable:" => m.available = v,
            "Cached:" => m.cached = v,
            "SwapTotal:" => m.swap_total = v,
            "SwapFree:" => m.swap_free = v,
            _ => {}
        }
    }
    m
}

/// Bytes received and sent on all interfaces but loopback (/proc/net/dev).
pub fn parse_net(s: &str) -> (u64, u64) {
    let (mut rx, mut tx) = (0, 0);
    for l in s.lines().skip(2) {
        let Some((iface, rest)) = l.split_once(':') else { continue };
        if iface.trim() == "lo" {
            continue;
        }
        let v: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        rx += v.first().copied().unwrap_or(0);
        tx += v.get(8).copied().unwrap_or(0);
    }
    (rx, tx)
}

/// Whole disks (not partitions) in /proc/diskstats.
fn whole_disk(name: &str) -> bool {
    let digits_tail = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        return !name.contains('p');
    }
    (name.starts_with("sd") || name.starts_with("vd") || name.starts_with("hd") || name.starts_with("xvd")) && digits_tail.len() == name.len()
}

/// Bytes read and written on whole disks so far.
pub fn parse_diskstats(s: &str) -> (u64, u64) {
    let (mut r, mut w) = (0, 0);
    for l in s.lines() {
        let v: Vec<&str> = l.split_whitespace().collect();
        if v.len() < 10 || !whole_disk(v[2]) {
            continue;
        }
        r += v[5].parse::<u64>().unwrap_or(0) * 512;
        w += v[9].parse::<u64>().unwrap_or(0) * 512;
    }
    (r, w)
}

/// "12.3 MB", "980 KB", Task Manager style (decimal-ish binary units).
pub fn bytes_text(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{b} B") } else if v >= 100.0 { format!("{v:.0} {}", units[u]) } else { format!("{v:.1} {}", units[u]) }
}

/// A rate: "0 MB/s", "1.2 MB/s" (Task Manager shows disk use per second).
pub fn rate_text(bytes_per_sec: f64) -> String {
    let mb = bytes_per_sec / (1024.0 * 1024.0);
    if mb < 0.05 { "0 MB/s".into() } else { format!("{mb:.1} MB/s") }
}

/// Group processes into apps: each GUI process whose parent isn't a GUI
/// process of the same program heads a group holding it and everything
/// below it. Returns (head pid -> member pids).
pub fn app_groups(procs: &[Proc]) -> HashMap<u32, Vec<u32>> {
    let by_pid: HashMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let is_head = |p: &Proc| {
        p.gui && by_pid.get(&p.ppid).is_none_or(|parent| !(parent.gui && parent.program == p.program))
    };
    let mut groups: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        // Walk up to the nearest head (or give up at a non-GUI ancestor
        // chain's end).
        let mut cur = p;
        let mut hops = 0;
        loop {
            if is_head(cur) {
                groups.entry(cur.pid).or_default().push(p.pid);
                break;
            }
            match by_pid.get(&cur.ppid) {
                Some(parent) if hops < 64 && (parent.gui || cur.gui) => {
                    cur = parent;
                    hops += 1;
                }
                _ => break,
            }
        }
    }
    groups
}

/// An app that starts when you sign in.
#[derive(Debug, Clone, PartialEq)]
pub struct Startup {
    /// File name, e.g. "org.gnome.Software.desktop".
    pub file: String,
    pub name: String,
    pub icon: String,
    pub exec: String,
    pub enabled: bool,
}

fn desktop_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut in_main = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_main = l == "[Desktop Entry]";
            continue;
        }
        if in_main && let Some(v) = l.strip_prefix(key).and_then(|r| r.trim_start().strip_prefix('=')) {
            return Some(v.trim());
        }
    }
    None
}

fn entry_enabled(text: &str) -> bool {
    desktop_value(text, "Hidden") != Some("true") && desktop_value(text, "X-GNOME-Autostart-enabled") != Some("false")
}

/// Autostart entries: the user's folder overrides the system ones.
pub fn startup_apps(user_dir: &Path, system_dirs: &[PathBuf], desktops: &[String]) -> Vec<Startup> {
    let mut files: HashMap<String, PathBuf> = HashMap::new();
    for d in system_dirs.iter().rev().chain(std::iter::once(&user_dir.to_path_buf())) {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".desktop") {
                files.insert(name, e.path());
            }
        }
    }
    let mut out: Vec<Startup> = files
        .into_iter()
        .filter_map(|(file, path)| {
            let text = std::fs::read_to_string(&path).ok()?;
            // Entries meant for other desktops aren't ours to show.
            if let Some(only) = desktop_value(&text, "OnlyShowIn")
                && !only.split(';').any(|d| desktops.iter().any(|x| x == d))
            {
                return None;
            }
            if let Some(not) = desktop_value(&text, "NotShowIn")
                && not.split(';').any(|d| desktops.iter().any(|x| x == d))
            {
                return None;
            }
            Some(Startup {
                name: desktop_value(&text, "Name").unwrap_or(&file).to_string(),
                icon: desktop_value(&text, "Icon").unwrap_or("application-x-executable").to_string(),
                exec: desktop_value(&text, "Exec").unwrap_or_default().to_string(),
                enabled: entry_enabled(&text),
                file,
            })
        })
        .collect();
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// Turn an autostart entry on or off with a copy in the user's folder.
pub fn set_startup_enabled(user_dir: &Path, system_dirs: &[PathBuf], file: &str, enabled: bool) -> std::io::Result<()> {
    let user = user_dir.join(file);
    let source = if user.exists() {
        user.clone()
    } else {
        system_dirs.iter().map(|d| d.join(file)).find(|p| p.exists()).ok_or(std::io::ErrorKind::NotFound)?
    };
    let text = std::fs::read_to_string(&source)?;
    let mut out = String::new();
    let mut in_main = false;
    let mut wrote = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            if in_main && !wrote {
                out.push_str(&format!("Hidden={}\n", !enabled));
                wrote = true;
            }
            in_main = t == "[Desktop Entry]";
        } else if in_main && (t.starts_with("Hidden=") || t.starts_with("X-GNOME-Autostart-enabled=")) {
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    if !wrote {
        out.push_str(&format!("Hidden={}\n", !enabled));
    }
    std::fs::create_dir_all(user_dir)?;
    std::fs::write(user, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_with_odd_names() {
        let s = "1234 (Web (Content)) S 1000 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 7 0 100 0 0";
        let (name, state, ppid, ticks, threads) = parse_stat(s).unwrap();
        assert_eq!((name.as_str(), state, ppid, ticks, threads), ("Web (Content)", 'S', 1000, 300, 7));
    }

    #[test]
    fn cpu_mem_net_disk() {
        let stat = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 50 0 25 400 25 0 0 0 0 0\ncpu1 50 0 25 400 25 0 0 0 0 0\n";
        assert_eq!(parse_cpu(stat), Some((150, 1000)));
        assert_eq!(cpu_count(stat), 2);
        let m = parse_meminfo("MemTotal: 1000 kB\nMemAvailable: 400 kB\nCached: 100 kB\n");
        assert_eq!((m.total, m.used()), (1_024_000, 614_400));
        let net = "Inter-|\n face |\n    lo: 10 0 0 0 0 0 0 0 10 0\n  eth0: 100 1 0 0 0 0 0 0 200 2\n";
        assert_eq!(parse_net(net), (100, 200));
        let disks = "   8       0 sda 10 0 4 0 20 0 8 0 0 0 0\n   8       1 sda1 10 0 4 0 20 0 8 0 0 0 0\n 259 0 nvme0n1 1 0 2 0 1 0 2 0 0 0 0\n 259 1 nvme0n1p1 1 0 2 0 1 0 2 0 0 0 0\n";
        assert_eq!(parse_diskstats(disks), (6 * 512, 10 * 512));
        assert_eq!(bytes_text(1536), "1.5 KB");
    }

    #[test]
    fn groups_gui_processes() {
        let p = |pid, ppid, program: &str, gui| Proc { pid, ppid, program: program.into(), gui, ..Default::default() };
        let procs = vec![
            p(1, 0, "systemd", false),
            p(10, 1, "firefox", true),
            p(11, 10, "firefox", true),
            p(12, 10, "firefox", true),
            p(20, 1, "foot", true),
            p(21, 20, "bash", true),
            p(30, 1, "pipewire", false),
        ];
        let g = app_groups(&procs);
        let mut ff = g[&10].clone();
        ff.sort();
        assert_eq!(ff, vec![10, 11, 12]);
        assert!(g[&20].contains(&20));
        assert!(!g.contains_key(&30));
    }

    #[test]
    fn startup_toggle() {
        let d = std::env::temp_dir().join(format!("lapin-startup-{}", std::process::id()));
        let (sys, user) = (d.join("sys"), d.join("user"));
        std::fs::create_dir_all(&sys).unwrap();
        std::fs::write(sys.join("a.desktop"), "[Desktop Entry]\nName=Alpha\nExec=alpha\n").unwrap();
        std::fs::write(sys.join("k.desktop"), "[Desktop Entry]\nName=Kde\nExec=k\nOnlyShowIn=KDE;\n").unwrap();
        let desk = vec!["LapinUX".to_string(), "COSMIC".into()];
        let list = startup_apps(&user, std::slice::from_ref(&sys), &desk);
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);
        set_startup_enabled(&user, std::slice::from_ref(&sys), "a.desktop", false).unwrap();
        assert!(!startup_apps(&user, std::slice::from_ref(&sys), &desk)[0].enabled);
        set_startup_enabled(&user, std::slice::from_ref(&sys), "a.desktop", true).unwrap();
        assert!(startup_apps(&user, std::slice::from_ref(&sys), &desk)[0].enabled);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
