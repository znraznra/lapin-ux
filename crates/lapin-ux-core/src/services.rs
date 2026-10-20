// SPDX-License-Identifier: GPL-3.0-only
//! Task Manager's Users and Services tabs, the parts that need no display:
//! account names from /etc/passwd, per-user totals from the process list,
//! and how a systemd service's state reads in Windows' words.

use std::collections::{BTreeMap, HashMap};

use crate::sysmon::Proc;

/// One account from /etc/passwd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub uid: u32,
    pub name: String,
    /// The GECOS full name, when set ("Alex Lee").
    pub full_name: String,
}

impl Account {
    /// What Windows shows: the full name, or the login name.
    pub fn display(&self) -> &str {
        if self.full_name.is_empty() { &self.name } else { &self.full_name }
    }
}

pub fn parse_passwd(text: &str) -> HashMap<u32, Account> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() < 5 {
            continue;
        }
        let Ok(uid) = f[2].parse::<u32>() else { continue };
        let full_name = f[4].split(',').next().unwrap_or("").trim().to_string();
        out.insert(uid, Account { uid, name: f[0].to_string(), full_name });
    }
    out
}

/// Accounts people sign in to: regular users (uid 1000 up to `nobody`).
pub fn is_person(uid: u32) -> bool {
    (1000..65534).contains(&uid)
}

/// One signed-in user's share of the machine.
#[derive(Debug, Clone, PartialEq)]
pub struct UserUsage {
    pub uid: u32,
    /// CPU % summed over the user's processes.
    pub cpu: f64,
    pub mem: u64,
    pub pids: Vec<u32>,
}

/// Sums CPU and memory per user. `signed_in` are the uids with a session
/// (from logind); when that is unknown, every person with a process counts.
/// `me` is always listed, so the tab is never empty.
pub fn usage_by_user(procs: &[Proc], cpu_of: &HashMap<u32, f64>, signed_in: Option<&[u32]>, me: u32) -> Vec<UserUsage> {
    let mut by: BTreeMap<u32, UserUsage> = BTreeMap::new();
    let wanted = |uid: u32| match signed_in {
        Some(list) => list.contains(&uid) || uid == me,
        None => is_person(uid) || uid == me,
    };
    for p in procs.iter().filter(|p| wanted(p.uid)) {
        let u = by.entry(p.uid).or_insert(UserUsage { uid: p.uid, cpu: 0.0, mem: 0, pids: Vec::new() });
        u.cpu += cpu_of.get(&p.pid).copied().unwrap_or(0.0);
        u.mem += p.rss;
        u.pids.push(p.pid);
    }
    by.entry(me).or_insert(UserUsage { uid: me, cpu: 0.0, mem: 0, pids: Vec::new() });
    let mut v: Vec<UserUsage> = by.into_values().collect();
    v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.uid.cmp(&b.uid)));
    v
}

/// A systemd service as Task Manager lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// The unit name, "NetworkManager.service".
    pub unit: String,
    pub description: String,
    /// systemd's ActiveState: active, inactive, failed, activating…
    pub active: String,
    /// systemd's SubState: running, dead, exited…
    pub sub: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Stopped,
    Starting,
    Stopping,
    Failed,
}

impl ServiceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Stopped => "Stopped",
            Self::Starting => "Starting",
            Self::Stopping => "Stopping",
            Self::Failed => "Failed",
        }
    }
}

impl Service {
    /// The name without ".service", as Windows lists service names.
    pub fn name(&self) -> &str {
        self.unit.strip_suffix(".service").unwrap_or(&self.unit)
    }

    pub fn status(&self) -> ServiceStatus {
        match self.active.as_str() {
            "active" | "reloading" => ServiceStatus::Running,
            "activating" => ServiceStatus::Starting,
            "deactivating" => ServiceStatus::Stopping,
            "failed" => ServiceStatus::Failed,
            _ => ServiceStatus::Stopped,
        }
    }

    pub fn running(&self) -> bool {
        matches!(self.status(), ServiceStatus::Running | ServiceStatus::Starting)
    }
}

/// Services as Windows sorts them: by name. Templates ("getty@.service")
/// can't be started on their own, so they are left out.
pub fn tidy_services(mut v: Vec<Service>) -> Vec<Service> {
    v.retain(|s| s.unit.ends_with(".service") && !s.unit.contains("@.service"));
    v.sort_by_key(|s| s.name().to_lowercase());
    v.dedup_by(|a, b| a.unit == b.unit);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, uid: u32, rss: u64) -> Proc {
        Proc { pid, uid, rss, ..Default::default() }
    }

    #[test]
    fn passwd() {
        let a = parse_passwd("root:x:0:0:root:/root:/bin/bash\nalex:x:1000:1000:Alex Lee,,,:/home/alex:/bin/bash\nbad line\n");
        assert_eq!(a[&1000].display(), "Alex Lee");
        assert_eq!(a[&0].display(), "root");
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn users_sum_and_include_me() {
        let procs = vec![proc(1, 0, 10), proc(2, 1000, 100), proc(3, 1000, 50), proc(4, 1001, 5)];
        let cpu: HashMap<u32, f64> = [(2, 3.0), (3, 1.5), (4, 9.0)].into_iter().collect();
        let v = usage_by_user(&procs, &cpu, None, 1000);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].uid, v[0].cpu), (1001, 9.0));
        assert_eq!((v[1].mem, v[1].pids.len()), (150, 2));
        // Only signed-in users when logind says who they are; root as "me".
        let v = usage_by_user(&procs, &cpu, Some(&[1000]), 0);
        assert_eq!(v.iter().map(|u| u.uid).collect::<Vec<_>>(), vec![1000, 0]);
    }

    #[test]
    fn service_words() {
        let s = |unit: &str, active: &str| Service { unit: unit.into(), description: String::new(), active: active.into(), sub: String::new(), pid: None };
        assert_eq!(s("sshd.service", "active").status().label(), "Running");
        assert_eq!(s("sshd.service", "inactive").status().label(), "Stopped");
        assert_eq!(s("sshd.service", "failed").status(), ServiceStatus::Failed);
        assert_eq!(s("NetworkManager.service", "active").name(), "NetworkManager");
        let v = tidy_services(vec![s("b.service", "active"), s("getty@.service", "inactive"), s("a.service", "inactive"), s("x.socket", "active")]);
        assert_eq!(v.iter().map(|s| s.name()).collect::<Vec<_>>(), vec!["a", "b"]);
    }
}
