// SPDX-License-Identifier: GPL-3.0-only
//! Lapin UX Task Manager (Ctrl+Shift+Esc): Processes (apps and background
//! processes with CPU, memory and disk use, End task), Performance (CPU,
//! memory, disk and network graphs), Startup apps (turn sign-in apps on or
//! off), Users (signed-in people and their share of the machine), Details
//! (every process) and Services (system services: start, stop, restart).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cosmic::app::{Settings, Task};
use cosmic::iced::{Alignment, Background, Border, Color, Length, Size, Subscription};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text, text_input};
use cosmic::{ApplicationExt, Core, Element, theme};
use lapin_ux_core::services::{self as svc, Account, Service, ServiceStatus};
use lapin_ux_core::sysmon::{self, Memory, Proc, Startup};

mod bus;
use bus::Verb;

const HISTORY: usize = 60;
const CLK_TCK: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Processes,
    Performance,
    Startup,
    Users,
    Details,
    Services,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sort {
    Name,
    Cpu,
    Memory,
    Disk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Graph {
    Cpu,
    Memory,
    Disk,
    Network,
}

/// Everything read in one go, off the UI thread.
#[derive(Debug, Clone)]
struct Sample {
    at: Instant,
    procs: Vec<Proc>,
    cpu: (u64, u64),
    ncpu: usize,
    mem: Memory,
    net: (u64, u64),
    disk: (u64, u64),
    uptime: f64,
    mhz: f64,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    Sampled(Box<Sample>),
    Search(String),
    Select(u32),
    Sort(Sort),
    EndTask,
    RunOpen(bool),
    RunInput(String),
    RunGo,
    Graph(Graph),
    StartupToggle(String, bool),
    StartupSelect(String),
    Expand(u32),
    /// A page in the sidebar.
    Page(Page),
    /// The ☰ button: icons only, or icons and names.
    ToggleNav,
    UsersLoaded(Option<Vec<u32>>),
    UserExpand(u32),
    ManageAccounts,
    ServicesLoaded(Result<Vec<Service>, String>),
    ServiceSelect(String),
    ServiceDo(String, Verb),
    ServiceDone(Result<(), String>),
}

/// One line in the Processes list: an app (head process plus children)
/// or a background process.
#[derive(Debug, Clone)]
struct Line {
    pid: u32,
    name: String,
    icon: String,
    members: Vec<u32>,
    cpu: f64,
    mem: u64,
    disk: f64,
    app: bool,
}

struct DesktopApp {
    program: String,
    wm_class: Option<String>,
    name: String,
    icon: String,
}

struct App {
    core: Core,
    page: Page,
    nav_collapsed: bool,
    apps: Vec<DesktopApp>,
    last: Option<Sample>,
    procs: Vec<Proc>,
    /// Per-process CPU % and disk bytes/s from the last two samples.
    cpu_of: HashMap<u32, f64>,
    disk_of: HashMap<u32, f64>,
    lines: Vec<Line>,
    cpu_hist: VecDeque<f64>,
    mem_hist: VecDeque<f64>,
    disk_hist: VecDeque<f64>,
    net_hist: VecDeque<f64>,
    cpu_now: f64,
    mem: Memory,
    disk_rate: (f64, f64),
    net_rate: (f64, f64),
    uptime: f64,
    mhz: f64,
    ncpu: usize,
    cpu_model: String,
    search: String,
    selected: Option<u32>,
    expanded: Vec<u32>,
    sort: Sort,
    graph: Graph,
    run: Option<String>,
    startup: Vec<Startup>,
    startup_selected: Option<String>,
    uid: u32,
    accounts: HashMap<u32, Account>,
    /// Uids with a session (logind); `None` until known or when unavailable.
    signed_in: Option<Vec<u32>>,
    user_expanded: Vec<u32>,
    services: Vec<Service>,
    services_note: Option<String>,
    service_selected: Option<String>,
}

fn load_services() -> Task<Message> {
    Task::future(async { cosmic::Action::App(Message::ServicesLoaded(bus::services().await)) })
}

fn load_users() -> Task<Message> {
    Task::future(async { cosmic::Action::App(Message::UsersLoaded(bus::signed_in_uids().await)) })
}

fn sample() -> Sample {
    let proc = Path::new("/proc");
    let stat = std::fs::read_to_string("/proc/stat").unwrap_or_default();
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let mhz: Vec<f64> = cpuinfo
        .lines()
        .filter(|l| l.starts_with("cpu MHz"))
        .filter_map(|l| l.split(':').nth(1)?.trim().parse().ok())
        .collect();
    Sample {
        at: Instant::now(),
        procs: sysmon::read_all(proc),
        cpu: sysmon::parse_cpu(&stat).unwrap_or((0, 1)),
        ncpu: sysmon::cpu_count(&stat),
        mem: sysmon::parse_meminfo(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default()),
        net: sysmon::parse_net(&std::fs::read_to_string("/proc/net/dev").unwrap_or_default()),
        disk: sysmon::parse_diskstats(&std::fs::read_to_string("/proc/diskstats").unwrap_or_default()),
        uptime: std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse().ok())
            .unwrap_or(0.0),
        mhz: if mhz.is_empty() { 0.0 } else { mhz.iter().sum::<f64>() / mhz.len() as f64 },
    }
}

fn load_desktop_apps() -> Vec<DesktopApp> {
    use cosmic::desktop::fde;
    let locales = fde::get_languages_from_env();
    fde::Iter::new(fde::default_paths())
        .filter_map(|p| fde::DesktopEntry::from_path(p, Some(&locales)).ok())
        .filter_map(|de| {
            let exec = de.exec()?.to_string();
            let mut words = exec.split_whitespace().filter(|w| !w.contains('='));
            let mut program = words.next()?.trim_matches('"').to_string();
            if program.ends_with("/env") || program == "env" {
                program = words.next().unwrap_or_default().to_string();
            }
            let program = program.rsplit('/').next().unwrap_or(&program).to_string();
            Some(DesktopApp {
                program,
                wm_class: de.startup_wm_class().map(|s| s.to_lowercase()),
                name: de.name(&locales).map(|n| n.to_string()).unwrap_or_default(),
                icon: de.icon().unwrap_or("application-x-executable").to_string(),
            })
        })
        .collect()
}

fn autostart_dirs() -> (PathBuf, Vec<PathBuf>) {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    let sys = std::env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".into());
    (config.join("autostart"), sys.split(':').filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("autostart")).collect())
}

fn desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').map(String::from).collect()
}

fn push(h: &mut VecDeque<f64>, v: f64) {
    h.push_back(v.clamp(0.0, 1.0));
    while h.len() > HISTORY {
        h.pop_front();
    }
}

impl App {
    fn app_for(&self, p: &Proc) -> Option<&DesktopApp> {
        let prog = p.program.to_lowercase();
        let name = p.name.to_lowercase();
        self.apps.iter().find(|a| {
            let ap = a.program.to_lowercase();
            (!ap.is_empty() && (ap == prog || ap == name)) || a.wm_class.as_deref().is_some_and(|w| w == prog || w == name)
        })
    }

    fn absorb(&mut self, s: Sample) {
        if let Some(prev) = &self.last {
            let dt = s.at.duration_since(prev.at).as_secs_f64().max(0.1);
            let before: HashMap<u32, (u64, u64)> = prev.procs.iter().map(|p| (p.pid, (p.cpu_ticks, p.read_bytes + p.write_bytes))).collect();
            self.cpu_of.clear();
            self.disk_of.clear();
            for p in &s.procs {
                if let Some((t, io)) = before.get(&p.pid) {
                    let cpu = (p.cpu_ticks.saturating_sub(*t)) as f64 / CLK_TCK / dt / s.ncpu as f64 * 100.0;
                    self.cpu_of.insert(p.pid, cpu);
                    self.disk_of.insert(p.pid, (p.read_bytes + p.write_bytes).saturating_sub(*io) as f64 / dt);
                }
            }
            let (busy, total) = (s.cpu.0.saturating_sub(prev.cpu.0), s.cpu.1.saturating_sub(prev.cpu.1));
            self.cpu_now = if total == 0 { 0.0 } else { busy as f64 / total as f64 };
            self.disk_rate = (s.disk.0.saturating_sub(prev.disk.0) as f64 / dt, s.disk.1.saturating_sub(prev.disk.1) as f64 / dt);
            self.net_rate = (s.net.0.saturating_sub(prev.net.0) as f64 / dt, s.net.1.saturating_sub(prev.net.1) as f64 / dt);
            push(&mut self.cpu_hist, self.cpu_now);
            // Disk and network graphs scale to 100 MB/s and 10 MB/s.
            push(&mut self.disk_hist, (self.disk_rate.0 + self.disk_rate.1) / (100.0 * 1024.0 * 1024.0));
            push(&mut self.net_hist, (self.net_rate.0 + self.net_rate.1) / (10.0 * 1024.0 * 1024.0));
        }
        self.mem = s.mem;
        push(&mut self.mem_hist, if s.mem.total == 0 { 0.0 } else { s.mem.used() as f64 / s.mem.total as f64 });
        self.uptime = s.uptime;
        self.mhz = s.mhz;
        self.ncpu = s.ncpu;
        self.procs = s.procs.clone();
        self.last = Some(s);
        self.rebuild_lines();
    }

    fn rebuild_lines(&mut self) {
        let groups = sysmon::app_groups(&self.procs);
        let by_pid: HashMap<u32, &Proc> = self.procs.iter().map(|p| (p.pid, p)).collect();
        let mut grouped: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut lines = Vec::new();
        for (head, members) in &groups {
            let Some(p) = by_pid.get(head) else { continue };
            // Only the user's own apps count as apps (others are background).
            if p.uid != self.uid || self.app_for(p).is_none() || p.program.starts_with("lapin-ux") || p.program.starts_with("cosmic-") {
                continue;
            }
            grouped.extend(members.iter().copied());
            let (name, icon) = match self.app_for(p) {
                Some(a) => (a.name.clone(), a.icon.clone()),
                None => (p.name.clone(), "application-x-executable".into()),
            };
            lines.push(self.line(*head, name, icon, members.clone(), true));
        }
        for p in &self.procs {
            if grouped.contains(&p.pid) || p.state == 'Z' {
                continue;
            }
            let (name, icon) = match self.app_for(p) {
                Some(a) => (a.name.clone(), a.icon.clone()),
                None => (if p.program.is_empty() { p.name.clone() } else { p.program.clone() }, "application-x-executable".into()),
            };
            lines.push(self.line(p.pid, name, icon, vec![p.pid], false));
        }
        self.lines = lines;
        self.sort_lines();
    }

    fn line(&self, pid: u32, name: String, icon: String, members: Vec<u32>, app: bool) -> Line {
        let by_pid: HashMap<u32, &Proc> = self.procs.iter().map(|p| (p.pid, p)).collect();
        let cpu = members.iter().filter_map(|m| self.cpu_of.get(m)).sum();
        let mem = members.iter().filter_map(|m| by_pid.get(m)).map(|p| p.rss).sum();
        let disk = members.iter().filter_map(|m| self.disk_of.get(m)).sum();
        Line { pid, name, icon, members, cpu, mem, disk, app }
    }

    fn sort_lines(&mut self) {
        match self.sort {
            Sort::Name => self.lines.sort_by_key(|l| l.name.to_lowercase()),
            Sort::Cpu => self.lines.sort_by(|a, b| b.cpu.total_cmp(&a.cpu)),
            Sort::Memory => self.lines.sort_by(|a, b| b.mem.cmp(&a.mem)),
            Sort::Disk => self.lines.sort_by(|a, b| b.disk.total_cmp(&a.disk)),
        }
    }

    fn matches(&self, name: &str, pid: u32) -> bool {
        let q = self.search.trim().to_lowercase();
        q.is_empty() || name.to_lowercase().contains(&q) || pid.to_string() == q
    }

    fn reload_startup(&mut self) {
        let (user, sys) = autostart_dirs();
        self.startup = sysmon::startup_apps(&user, &sys, &desktops());
    }

    fn end(&mut self, pids: Vec<u32>) -> Task<Message> {
        Task::future(async move {
            let args: Vec<String> = pids.iter().map(u32::to_string).collect();
            let _ = tokio::process::Command::new("kill").arg("-TERM").args(&args).status().await;
            // Whatever ignores the request is stopped outright.
            tokio::time::sleep(Duration::from_secs(3)).await;
            let alive: Vec<&String> = args.iter().filter(|p| Path::new("/proc").join(p).exists()).collect();
            if !alive.is_empty() {
                let _ = tokio::process::Command::new("kill").arg("-KILL").args(alive).status().await;
            }
            cosmic::Action::App(Message::Tick)
        })
    }
}

// ---------------------------------------------------------------- styling

fn accent(t: &cosmic::Theme) -> Color {
    t.cosmic().accent_color().into()
}

fn row_style(selected: bool) -> theme::Container<'static> {
    theme::Container::custom(move |t| {
        let a = accent(t);
        container::Style {
            background: selected.then_some(Background::Color(Color { a: 0.18, ..a })),
            border: Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        }
    })
}

/// Heat colour for a value cell (Task Manager tints busy cells).
fn heat(frac: f64) -> theme::Container<'static> {
    theme::Container::custom(move |t| {
        let a = accent(t);
        let alpha = (frac.clamp(0.0, 1.0) * 0.45) as f32;
        container::Style { background: (alpha > 0.02).then_some(Background::Color(Color { a: alpha, ..a })), ..Default::default() }
    })
}

fn card() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        container::Style {
            background: Some(Background::Color(c.background(true).component.base.into())),
            border: Border { radius: 8.0.into(), ..Default::default() },
            ..Default::default()
        }
    })
}

/// An area graph: one bar per sample, newest at the right.
fn graph<'a>(hist: &VecDeque<f64>, height: impl Into<Length>) -> Element<'a, Message> {
    let mut bars = Vec::with_capacity(HISTORY);
    for i in 0..HISTORY {
        let v = if i + hist.len() >= HISTORY { hist[i + hist.len() - HISTORY] } else { 0.0 };
        let filled = ((v * 1000.0).round() as u16).max(if v > 0.0 { 1 } else { 0 });
        let mut col = column::with_capacity(2).width(Length::Fill).height(Length::Fill);
        if filled < 1000 {
            col = col.push(widget::Space::new().height(Length::FillPortion(1000 - filled)));
        }
        if filled > 0 {
            col = col.push(
                container(widget::Space::new())
                    .width(Length::Fill)
                    .height(Length::FillPortion(filled))
                    .class(theme::Container::custom(|t| {
                        let a = accent(t);
                        container::Style {
                            background: Some(Background::Color(Color { a: 0.35, ..a })),
                            border: Border { color: a, width: 0.0, radius: 0.0.into() },
                            ..Default::default()
                        }
                    })),
            );
        }
        bars.push(col.into());
    }
    container(row::with_children(bars).height(Length::Fill))
        .height(height)
        .width(Length::Fill)
        .class(theme::Container::custom(|t| {
            let a = accent(t);
            container::Style { border: Border { color: Color { a: 0.8, ..a }, width: 1.0, radius: 2.0.into() }, ..Default::default() }
        }))
        .into()
}

fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / (1024.0 * 1024.0 * 1024.0))
}

fn uptime_text(s: f64) -> String {
    let s = s as u64;
    format!("{}:{:02}:{:02}:{:02}", s / 86400, (s / 3600) % 24, (s / 60) % 60, s % 60)
}

fn kbps(bytes_per_sec: f64) -> String {
    let kb = bytes_per_sec * 8.0 / 1000.0;
    if kb >= 1000.0 { format!("{:.1} Mbps", kb / 1000.0) } else { format!("{kb:.0} Kbps") }
}

// ---------------------------------------------------------------- app

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "dev.lapinux.TaskManager";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _: ()) -> (Self, Task<Message>) {
        let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|m| m.trim().to_string()))
            .unwrap_or_default();
        use std::os::unix::fs::MetadataExt;
        let uid = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
        let mut app = App {
            core,
            page: Page::Processes,
            nav_collapsed: false,
            apps: load_desktop_apps(),
            last: None,
            procs: Vec::new(),
            cpu_of: HashMap::new(),
            disk_of: HashMap::new(),
            lines: Vec::new(),
            cpu_hist: VecDeque::new(),
            mem_hist: VecDeque::new(),
            disk_hist: VecDeque::new(),
            net_hist: VecDeque::new(),
            cpu_now: 0.0,
            mem: Memory::default(),
            disk_rate: (0.0, 0.0),
            net_rate: (0.0, 0.0),
            uptime: 0.0,
            mhz: 0.0,
            ncpu: 1,
            cpu_model,
            search: String::new(),
            selected: None,
            expanded: Vec::new(),
            sort: Sort::Cpu,
            graph: Graph::Cpu,
            run: None,
            startup: Vec::new(),
            startup_selected: None,
            uid,
            accounts: svc::parse_passwd(&std::fs::read_to_string("/etc/passwd").unwrap_or_default()),
            signed_in: None,
            user_expanded: Vec::new(),
            services: Vec::new(),
            services_note: None,
            service_selected: None,
        };
        app.reload_startup();
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Task Manager".into(), id),
            None => Task::none(),
        };
        (app, Task::batch([title, Task::done(cosmic::Action::App(Message::Tick))]))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![
            button::custom(icon::from_name("open-menu-symbolic").size(16))
                .padding([8, 12])
                .class(lapin_ux_ui::nav_item(false))
                .on_press(Message::ToggleNav)
                .into(),
            lapin_ux_ui::title_bar("utilities-system-monitor", "Task Manager"),
        ]
    }

    fn header_center(&self) -> Vec<Element<'_, Message>> {
        vec![
            text_input::search_input("Type a name or PID to search", &self.search)
                .on_input(Message::Search)
                .width(Length::Fixed(380.0))
                .into(),
        ]
    }

    fn subscription(&self) -> Subscription<Message> {
        cosmic::iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                return Task::future(async {
                    let s = tokio::task::spawn_blocking(sample).await.ok();
                    match s {
                        Some(s) => cosmic::Action::App(Message::Sampled(Box::new(s))),
                        None => cosmic::Action::None,
                    }
                });
            }
            Message::Sampled(s) => self.absorb(*s),
            Message::Search(q) => self.search = q,
            Message::Select(pid) => self.selected = Some(pid),
            Message::Page(p) => {
                self.page = p;
                match p {
                    Page::Startup => self.reload_startup(),
                    Page::Users => return load_users(),
                    Page::Services => {
                        self.services_note = Some("Loading services…".into());
                        return load_services();
                    }
                    _ => {}
                }
            }
            Message::ToggleNav => self.nav_collapsed = !self.nav_collapsed,
            Message::UsersLoaded(uids) => self.signed_in = uids,
            Message::UserExpand(uid) => {
                if let Some(i) = self.user_expanded.iter().position(|u| *u == uid) {
                    self.user_expanded.remove(i);
                } else {
                    self.user_expanded.push(uid);
                }
            }
            Message::ManageAccounts => {
                let _ = std::process::Command::new("cosmic-settings").arg("users").spawn();
            }
            Message::ServicesLoaded(res) => match res {
                Ok(list) => {
                    self.services = list;
                    self.services_note = None;
                }
                Err(e) => self.services_note = Some(format!("Services can't be listed: {e}")),
            },
            Message::ServiceSelect(unit) => self.service_selected = Some(unit),
            Message::ServiceDo(unit, verb) => {
                return Task::future(async move { cosmic::Action::App(Message::ServiceDone(bus::service_action(unit, verb).await)) });
            }
            Message::ServiceDone(res) => {
                self.services_note = res.err().map(|e| format!("That didn't work: {e}"));
                return load_services();
            }
            Message::Expand(pid) => {
                if let Some(i) = self.expanded.iter().position(|p| *p == pid) {
                    self.expanded.remove(i);
                } else {
                    self.expanded.push(pid);
                }
            }
            Message::Sort(s) => {
                self.sort = s;
                self.sort_lines();
            }
            Message::EndTask => {
                let Some(pid) = self.selected.take() else { return Task::none() };
                let members = self.lines.iter().find(|l| l.pid == pid).map(|l| l.members.clone()).unwrap_or_else(|| vec![pid]);
                return self.end(members);
            }
            Message::RunOpen(open) => self.run = open.then(String::new),
            Message::RunInput(s) => self.run = Some(s),
            Message::RunGo => {
                if let Some(cmd) = self.run.take().filter(|c| !c.trim().is_empty()) {
                    let _ = std::process::Command::new("sh").arg("-c").arg(&cmd).spawn();
                }
            }
            Message::Graph(g) => self.graph = g,
            Message::StartupSelect(f) => self.startup_selected = Some(f),
            Message::StartupToggle(file, on) => {
                let (user, sys) = autostart_dirs();
                if let Err(e) = sysmon::set_startup_enabled(&user, &sys, &file, on) {
                    tracing::warn!("startup {file}: {e}");
                }
                self.reload_startup();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let page = self.page;
        let body = match page {
            Page::Processes => self.processes_view(),
            Page::Performance => self.performance_view(),
            Page::Startup => self.startup_view(),
            Page::Users => self.users_view(),
            Page::Details => self.details_view(),
            Page::Services => self.services_view(),
        };
        let body = container(body).padding([8, 16, 16, 16]).width(Length::Fill).height(Length::Fill);
        row::with_children(vec![self.sidebar(), body.into()]).height(Length::Fill).into()
    }
}

const PAGES: [(&str, &str, Page); 6] = [
    ("Processes", "view-grid-symbolic", Page::Processes),
    ("Performance", "utilities-system-monitor-symbolic", Page::Performance),
    ("Startup apps", "system-run-symbolic", Page::Startup),
    ("Users", "system-users-symbolic", Page::Users),
    ("Details", "view-list-symbolic", Page::Details),
    ("Services", "applications-system-symbolic", Page::Services),
];

/// Space between the sidebar's edge and its icons; the longest name gets
/// the same space on its right.
const NAV_INSET: u16 = 12;
/// Wide enough for "Startup apps" plus NAV_INSET on its right.
const NAV_ITEM_WIDTH: f32 = 132.0;

impl App {
    /// The page list on the left: names and icons, or icons only after ☰.
    fn sidebar(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(PAGES.len()).spacing(4);
        for (label, ic, page) in PAGES {
            let mut content = row::with_capacity(2).spacing(12).align_y(Alignment::Center);
            content = content.push(icon::from_name(ic).size(16));
            if !self.nav_collapsed {
                content = content.push(text::body(label));
            }
            col = col.push(
                button::custom(content)
                    .width(if self.nav_collapsed { Length::Shrink } else { Length::Fixed(NAV_ITEM_WIDTH) })
                    .padding([8, NAV_INSET])
                    .class(lapin_ux_ui::nav_item(self.page == page))
                    .on_press(Message::Page(page)),
            );
        }
        container(col)
            .padding(8)
            .height(Length::Fill)
            .into()
    }
}

impl App {
    /// Page title with its buttons on the right.
    fn page_head<'a>(&'a self, title: &'a str, mut buttons: Vec<Element<'a, Message>>) -> Element<'a, Message> {
        let mut r = row::with_capacity(buttons.len() + 2).align_y(Alignment::Center).spacing(8);
        r = r.push(text::title4(title).width(Length::Fill));
        for b in buttons.drain(..) {
            r = r.push(b);
        }
        let mut col = column::with_capacity(2).spacing(8).push(r);
        if let Some(cmd) = &self.run {
            col = col.push(
                container(
                    row::with_children(vec![
                        text::body("Open:").into(),
                        text_input::text_input("Type the name of a program", cmd)
                            .on_input(Message::RunInput)
                            .on_submit(|_| Message::RunGo)
                            .width(Length::Fill)
                            .into(),
                        lapin_ux_ui::button::suggested("OK").on_press(Message::RunGo).into(),
                        lapin_ux_ui::button::standard("Cancel").on_press(Message::RunOpen(false)).into(),
                    ])
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding(10)
                .class(card()),
            );
        }
        col.into()
    }

    fn head_cell<'a>(&self, label: String, sort: Sort, width: Length, end: bool) -> Element<'a, Message> {
        let mark = if self.sort == sort { if sort == Sort::Name { " ↑" } else { " ↓" } } else { "" };
        let t = text::caption(format!("{label}{mark}"));
        button::custom(container(t).width(Length::Fill).align_x(if end { Alignment::End } else { Alignment::Start }))
            .width(width)
            .padding([4, 8])
            .class(theme::Button::MenuItem)
            .on_press(Message::Sort(sort))
            .into()
    }

    fn processes_view(&self) -> Element<'_, Message> {
        let mem_frac = if self.mem.total == 0 { 0.0 } else { self.mem.used() as f64 / self.mem.total as f64 };
        let buttons = vec![
            lapin_ux_ui::button::standard("Run new task").leading_icon(icon::from_name("list-add-symbolic").size(16)).on_press(Message::RunOpen(true)).into(),
            lapin_ux_ui::button::standard("End task")
                .leading_icon(icon::from_name("process-stop-symbolic").size(16))
                .on_press_maybe(self.selected.map(|_| Message::EndTask))
                .into(),
        ];
        let header = row::with_children(vec![
            self.head_cell("Name".into(), Sort::Name, Length::Fill, false),
            self.head_cell(format!("{:.0}%\nCPU", self.cpu_now * 100.0), Sort::Cpu, Length::Fixed(90.0), true),
            self.head_cell(format!("{:.0}%\nMemory", mem_frac * 100.0), Sort::Memory, Length::Fixed(110.0), true),
            self.head_cell(format!("{}\nDisk", sysmon::rate_text(self.disk_rate.0 + self.disk_rate.1)), Sort::Disk, Length::Fixed(110.0), true),
        ]);
        let shown: Vec<&Line> = self.lines.iter().filter(|l| self.matches(&l.name, l.pid) || l.members.iter().any(|m| m.to_string() == self.search.trim())).collect();
        let apps: Vec<&&Line> = shown.iter().filter(|l| l.app).collect();
        let bg: Vec<&&Line> = shown.iter().filter(|l| !l.app).collect();
        let mut list = column::with_capacity(shown.len() + 4).spacing(1);
        list = list.push(text::heading(format!("Apps ({})", apps.len())).width(Length::Fill)).push(widget::Space::new().height(4));
        for l in apps {
            list = list.push(self.line_row(l));
            if self.expanded.contains(&l.pid) && l.members.len() > 1 {
                for m in &l.members {
                    if let Some(p) = self.procs.iter().find(|p| p.pid == *m) {
                        list = list.push(self.sub_row(p));
                    }
                }
            }
        }
        list = list.push(widget::Space::new().height(12)).push(text::heading(format!("Background processes ({})", bg.len()))).push(widget::Space::new().height(4));
        for l in bg {
            list = list.push(self.line_row(l));
        }
        column::with_children(vec![
            self.page_head("Processes", buttons),
            container(header).padding([0, 14, 0, 0]).class(card()).into(),
            scrollable(container(list).padding([0, 14, 0, 0])).height(Length::Fill).into(),
        ])
        .spacing(8)
        .into()
    }

    fn line_row<'a>(&'a self, l: &'a Line) -> Element<'a, Message> {
        let selected = self.selected == Some(l.pid);
        let count = if l.members.len() > 1 { format!(" ({})", l.members.len()) } else { String::new() };
        let chevron: Element<'a, Message> = if l.members.len() > 1 {
            button::custom(icon::from_name(if self.expanded.contains(&l.pid) { "pan-down-symbolic" } else { "pan-end-symbolic" }).size(12))
                .padding(2)
                .class(lapin_ux_ui::text())
                .on_press(Message::Expand(l.pid))
                .into()
        } else {
            widget::Space::new().width(16).into()
        };
        let mem_frac = if self.mem.total == 0 { 0.0 } else { l.mem as f64 / self.mem.total as f64 * 10.0 };
        let cells = row::with_children(vec![
            row::with_children(vec![
                chevron,
                icon::from_name(l.icon.as_str()).size(20).into(),
                text::body(format!("{}{count}", l.name)).into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into(),
            container(text::body(format!("{:.1}%", l.cpu))).width(90).align_x(Alignment::End).padding([6, 8]).class(heat(l.cpu / 50.0)).into(),
            container(text::body(sysmon::bytes_text(l.mem))).width(110).align_x(Alignment::End).padding([6, 8]).class(heat(mem_frac)).into(),
            container(text::body(sysmon::rate_text(l.disk))).width(110).align_x(Alignment::End).padding([6, 8]).class(heat(l.disk / 20e6)).into(),
        ])
        .align_y(Alignment::Center);
        button::custom(container(cells).width(Length::Fill).padding([0, 0, 0, 8]).class(row_style(selected)))
            .padding(0)
            .width(Length::Fill)
            .class(theme::Button::MenuItem)
            .on_press(Message::Select(l.pid))
            .into()
    }

    fn sub_row<'a>(&'a self, p: &'a Proc) -> Element<'a, Message> {
        let cpu = self.cpu_of.get(&p.pid).copied().unwrap_or(0.0);
        row::with_children(vec![
            text::body(format!("{} ({})", if p.program.is_empty() { &p.name } else { &p.program }, p.pid)).width(Length::Fill).into(),
            container(text::caption(format!("{cpu:.1}%"))).width(90).align_x(Alignment::End).padding([4, 8]).into(),
            container(text::caption(sysmon::bytes_text(p.rss))).width(110).align_x(Alignment::End).padding([4, 8]).into(),
            widget::Space::new().width(110).into(),
        ])
        .padding([0, 0, 0, 60])
        .into()
    }

    fn perf_tile<'a>(&'a self, g: Graph, title: &'a str, value: String, hist: &VecDeque<f64>) -> Element<'a, Message> {
        let selected = self.graph == g;
        let body = row::with_children(vec![
            container(graph(hist, 36.0)).width(64).into(),
            column::with_children(vec![text::body(title).into(), text::caption(value).into()]).spacing(2).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center);
        button::custom(container(body).padding(8).width(Length::Fill).class(row_style(selected)))
            .padding(0)
            .width(Length::Fill)
            .class(theme::Button::MenuItem)
            .on_press(Message::Graph(g))
            .into()
    }

    fn performance_view(&self) -> Element<'_, Message> {
        let mem_frac = if self.mem.total == 0 { 0.0 } else { self.mem.used() as f64 / self.mem.total as f64 };
        let tiles = column::with_children(vec![
            self.perf_tile(Graph::Cpu, "CPU", format!("{:.0}%  {:.2} GHz", self.cpu_now * 100.0, self.mhz / 1000.0), &self.cpu_hist),
            self.perf_tile(Graph::Memory, "Memory", format!("{}/{} ({:.0}%)", gb(self.mem.used()), gb(self.mem.total), mem_frac * 100.0), &self.mem_hist),
            self.perf_tile(Graph::Disk, "Disk", format!("R {}  W {}", sysmon::rate_text(self.disk_rate.0), sysmon::rate_text(self.disk_rate.1)), &self.disk_hist),
            self.perf_tile(Graph::Network, "Network", format!("S: {}  R: {}", kbps(self.net_rate.1), kbps(self.net_rate.0)), &self.net_hist),
        ])
        .spacing(4)
        .width(230);

        let threads: u32 = self.procs.iter().map(|p| p.threads).sum();
        let stat = |label: &'static str, value: String| -> Element<'_, Message> {
            column::with_children(vec![text::caption(label).into(), text::title4(value).into()]).spacing(2).width(130).into()
        };
        let (title, sub, hist, caption, stats): (&str, String, &VecDeque<f64>, &str, Vec<Element<'_, Message>>) = match self.graph {
            Graph::Cpu => (
                "CPU",
                self.cpu_model.clone(),
                &self.cpu_hist,
                "% Utilization over 60 seconds",
                vec![
                    stat("Utilization", format!("{:.0}%", self.cpu_now * 100.0)),
                    stat("Speed", format!("{:.2} GHz", self.mhz / 1000.0)),
                    stat("Processes", self.procs.len().to_string()),
                    stat("Threads", threads.to_string()),
                    stat("Logical processors", self.ncpu.to_string()),
                    stat("Up time", uptime_text(self.uptime)),
                ],
            ),
            Graph::Memory => (
                "Memory",
                format!("{} total", gb(self.mem.total)),
                &self.mem_hist,
                "Memory usage",
                vec![
                    stat("In use", gb(self.mem.used())),
                    stat("Available", gb(self.mem.available)),
                    stat("Cached", gb(self.mem.cached)),
                    stat("Swap in use", gb(self.mem.swap_total.saturating_sub(self.mem.swap_free))),
                ],
            ),
            Graph::Disk => (
                "Disk",
                "All drives".into(),
                &self.disk_hist,
                "Transfer rate (up to 100 MB/s)",
                vec![stat("Read speed", sysmon::rate_text(self.disk_rate.0)), stat("Write speed", sysmon::rate_text(self.disk_rate.1))],
            ),
            Graph::Network => (
                "Network",
                "All adapters".into(),
                &self.net_hist,
                "Throughput (up to 80 Mbps)",
                vec![stat("Send", kbps(self.net_rate.1)), stat("Receive", kbps(self.net_rate.0))],
            ),
        };
        let mut grid = column::with_capacity(stats.len() / 3 + 1).spacing(16);
        let mut it = stats.into_iter().peekable();
        while it.peek().is_some() {
            grid = grid.push(row::with_children(it.by_ref().take(3).collect::<Vec<_>>()).spacing(16));
        }
        // Like Windows: the card never scrolls; the heading and the stats
        // keep their size and only the graph grows or shrinks.
        let detail = column::with_children(vec![
            text::title3(title).into(),
            text::caption(sub).into(),
            text::caption(caption).into(),
            graph(hist, Length::Fill),
            grid.into(),
        ])
        .spacing(10)
        .width(Length::Fill)
        .height(Length::Fill);
        column::with_children(vec![
            self.page_head("Performance", vec![lapin_ux_ui::button::standard("Run new task").on_press(Message::RunOpen(true)).into()]),
            row::with_children(vec![
                tiles.into(),
                container(detail).padding(16).class(card()).width(Length::Fill).height(Length::Fill).into(),
            ])
            .spacing(16)
            .into(),
        ])
        .spacing(8)
        .into()
    }

    fn startup_view(&self) -> Element<'_, Message> {
        let sel = self.startup.iter().find(|s| Some(&s.file) == self.startup_selected.as_ref());
        let buttons: Vec<Element<'_, Message>> = vec![
            lapin_ux_ui::button::standard("Enable").on_press_maybe(sel.filter(|s| !s.enabled).map(|s| Message::StartupToggle(s.file.clone(), true))).into(),
            lapin_ux_ui::button::standard("Disable").on_press_maybe(sel.filter(|s| s.enabled).map(|s| Message::StartupToggle(s.file.clone(), false))).into(),
        ];
        let header = row::with_children(vec![
            text::caption("Name").width(Length::Fill).into(),
            text::caption("Command").width(Length::FillPortion(1)).into(),
            text::caption("Status").width(120).into(),
        ])
        .padding([6, 12]);
        let mut list = column::with_capacity(self.startup.len()).spacing(1);
        for s in self.startup.iter().filter(|s| self.matches(&s.name, 0)) {
            let selected = self.startup_selected.as_ref() == Some(&s.file);
            let cells = row::with_children(vec![
                row::with_children(vec![icon::from_name(s.icon.as_str()).size(20).into(), text::body(s.name.clone()).into()])
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .width(Length::Fill)
                    .into(),
                text::caption(s.exec.clone()).width(Length::FillPortion(1)).into(),
                text::body(if s.enabled { "Enabled" } else { "Disabled" }).width(120).into(),
            ])
            .align_y(Alignment::Center)
            .padding([6, 12]);
            list = list.push(
                button::custom(container(cells).width(Length::Fill).class(row_style(selected)))
                    .padding(0)
                    .width(Length::Fill)
                    .class(theme::Button::MenuItem)
                    .on_press(Message::StartupSelect(s.file.clone())),
            );
        }
        if self.startup.is_empty() {
            list = list.push(text::body("No apps start when you sign in."));
        }
        column::with_children(vec![
            self.page_head("Startup apps", buttons),
            container(header).class(card()).into(),
            scrollable(list).height(Length::Fill).into(),
        ])
        .spacing(8)
        .into()
    }

    fn users_view(&self) -> Element<'_, Message> {
        let mem_frac = if self.mem.total == 0 { 0.0 } else { self.mem.used() as f64 / self.mem.total as f64 };
        let users = svc::usage_by_user(&self.procs, &self.cpu_of, self.signed_in.as_deref(), self.uid);
        let header = row::with_children(vec![
            text::caption("User").width(Length::Fill).into(),
            text::caption("Status").width(120).into(),
            container(text::caption(format!("{:.0}%\nCPU", self.cpu_now * 100.0))).width(90).align_x(Alignment::End).into(),
            container(text::caption(format!("{:.0}%\nMemory", mem_frac * 100.0))).width(110).align_x(Alignment::End).into(),
        ])
        .padding([6, 22, 6, 12]);
        let mut list = column::with_capacity(users.len() * 2).spacing(1);
        for u in &users {
            let name = self.accounts.get(&u.uid).map(|a| a.display().to_string()).unwrap_or_else(|| u.uid.to_string());
            let open = self.user_expanded.contains(&u.uid);
            let status = if u.uid == self.uid { "Active" } else { "Signed in" };
            let cells = row::with_children(vec![
                row::with_children(vec![
                    icon::from_name(if open { "pan-down-symbolic" } else { "pan-end-symbolic" }).size(12).into(),
                    icon::from_name("avatar-default-symbolic").size(20).into(),
                    text::body(format!("{name} ({})", u.pids.len())).into(),
                ])
                .spacing(8)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .into(),
                text::body(status).width(120).into(),
                container(text::body(format!("{:.1}%", u.cpu))).width(90).align_x(Alignment::End).padding([6, 8]).class(heat(u.cpu / 50.0)).into(),
                container(text::body(sysmon::bytes_text(u.mem)))
                    .width(110)
                    .align_x(Alignment::End)
                    .padding([6, 8])
                    .class(heat(if self.mem.total == 0 { 0.0 } else { u.mem as f64 / self.mem.total as f64 * 4.0 }))
                    .into(),
            ])
            .align_y(Alignment::Center)
            .padding([0, 0, 0, 12]);
            list = list.push(
                button::custom(container(cells).width(Length::Fill).class(row_style(false)))
                    .padding(0)
                    .width(Length::Fill)
                    .class(theme::Button::MenuItem)
                    .on_press(Message::UserExpand(u.uid)),
            );
            if open {
                let mut procs: Vec<&Proc> = self.procs.iter().filter(|p| u.pids.contains(&p.pid)).collect();
                procs.sort_by(|a, b| self.cpu_of.get(&b.pid).unwrap_or(&0.0).total_cmp(self.cpu_of.get(&a.pid).unwrap_or(&0.0)));
                for p in procs.into_iter().take(50) {
                    list = list.push(
                        button::custom(container(self.sub_row(p)).width(Length::Fill).class(row_style(self.selected == Some(p.pid))))
                            .padding(0)
                            .width(Length::Fill)
                            .class(theme::Button::MenuItem)
                            .on_press(Message::Select(p.pid)),
                    );
                }
            }
        }
        let buttons = vec![
            lapin_ux_ui::button::standard("End task").on_press_maybe(self.selected.map(|_| Message::EndTask)).into(),
            lapin_ux_ui::button::standard("Manage user accounts").on_press(Message::ManageAccounts).into(),
        ];
        column::with_children(vec![
            self.page_head("Users", buttons),
            container(header).class(card()).into(),
            scrollable(container(list).padding([0, 14, 0, 0])).height(Length::Fill).into(),
        ])
        .spacing(8)
        .into()
    }

    fn services_view(&self) -> Element<'_, Message> {
        let sel = self.services.iter().find(|s| Some(&s.unit) == self.service_selected.as_ref());
        let act = |verb: Verb, ok: fn(&Service) -> bool| sel.filter(|s| ok(s)).map(|s| Message::ServiceDo(s.unit.clone(), verb));
        let buttons: Vec<Element<'_, Message>> = vec![
            lapin_ux_ui::button::standard("Start").on_press_maybe(act(Verb::Start, |s| !s.running())).into(),
            lapin_ux_ui::button::standard("Stop").on_press_maybe(act(Verb::Stop, |s| s.running())).into(),
            lapin_ux_ui::button::standard("Restart").on_press_maybe(act(Verb::Restart, |s| s.running())).into(),
        ];
        let header = row::with_children(vec![
            text::caption("Name").width(220).into(),
            text::caption("PID").width(70).into(),
            text::caption("Description").width(Length::Fill).into(),
            text::caption("Status").width(90).into(),
        ])
        .padding([6, 12]);
        let mut list = column::with_capacity(self.services.len() + 1).spacing(1);
        if let Some(note) = &self.services_note {
            list = list.push(container(text::body(note.clone())).padding([6, 12]));
        }
        for s in self.services.iter().filter(|s| self.matches(s.name(), s.pid.unwrap_or(0)) || self.matches(&s.description, 0)) {
            let status = s.status();
            let status_text = text::body(status.label()).width(90);
            let cells = row::with_children(vec![
                text::body(s.name().to_string()).width(220).into(),
                text::body(s.pid.map(|p| p.to_string()).unwrap_or_default()).width(70).into(),
                text::body(s.description.clone()).width(Length::Fill).height(20).into(),
                if status == ServiceStatus::Failed {
                    status_text.class(theme::Text::Color(Color::from_rgb(0.9, 0.35, 0.3))).into()
                } else {
                    status_text.into()
                },
            ])
            .align_y(Alignment::Center)
            .padding([4, 12]);
            list = list.push(
                button::custom(container(cells).width(Length::Fill).class(row_style(self.service_selected.as_ref() == Some(&s.unit))))
                    .padding(0)
                    .width(Length::Fill)
                    .class(theme::Button::MenuItem)
                    .on_press(Message::ServiceSelect(s.unit.clone())),
            );
        }
        column::with_children(vec![
            self.page_head("Services", buttons),
            container(header).class(card()).into(),
            scrollable(container(list).padding([0, 14, 0, 0])).height(Length::Fill).into(),
        ])
        .spacing(8)
        .into()
    }

    fn details_view(&self) -> Element<'_, Message> {
        let header = row::with_children(vec![
            text::caption("Name").width(180).into(),
            text::caption("PID").width(70).into(),
            text::caption("Status").width(90).into(),
            text::caption("CPU").width(60).into(),
            text::caption("Memory").width(90).into(),
            text::caption("Command line").width(Length::Fill).into(),
        ])
        .padding([6, 12]);
        let mut procs: Vec<&Proc> = self.procs.iter().filter(|p| self.matches(&p.name, p.pid)).collect();
        procs.sort_by_key(|p| p.name.to_lowercase());
        let mut list = column::with_capacity(procs.len()).spacing(1);
        for p in procs {
            let status = match p.state {
                'R' => "Running",
                'S' | 'I' => "Running",
                'D' => "Waiting",
                'T' | 't' => "Suspended",
                'Z' => "Ended",
                _ => "Running",
            };
            let cells = row::with_children(vec![
                text::body(p.name.clone()).width(180).into(),
                text::body(p.pid.to_string()).width(70).into(),
                text::body(status).width(90).into(),
                text::body(format!("{:.0}", self.cpu_of.get(&p.pid).copied().unwrap_or(0.0))).width(60).into(),
                text::body(sysmon::bytes_text(p.rss)).width(90).into(),
                text::caption(p.cmdline.clone()).width(Length::Fill).height(18).into(),
            ])
            .align_y(Alignment::Center)
            .padding([4, 12]);
            list = list.push(
                button::custom(container(cells).width(Length::Fill).class(row_style(self.selected == Some(p.pid))))
                    .padding(0)
                    .width(Length::Fill)
                    .class(theme::Button::MenuItem)
                    .on_press(Message::Select(p.pid)),
            );
        }
        let buttons = vec![
            lapin_ux_ui::button::standard("Run new task").on_press(Message::RunOpen(true)).into(),
            lapin_ux_ui::button::standard("End task").on_press_maybe(self.selected.map(|_| Message::EndTask)).into(),
        ];
        column::with_children(vec![
            self.page_head("Details", buttons),
            container(header).class(card()).into(),
            scrollable(list).height(Length::Fill).into(),
        ])
        .spacing(8)
        .into()
    }
}

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .init();
    cosmic::app::run::<App>(Settings::default().size(Size::new(1000.0, 700.0)).size_limits(cosmic::iced::Limits::NONE.min_width(640.0).min_height(420.0)), ())
}
