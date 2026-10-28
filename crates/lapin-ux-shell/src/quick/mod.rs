// SPDX-License-Identifier: GPL-3.0-only
//! Quick settings: the network / volume / battery button left of the clock
//! and the panel it opens (Super+A): Wi-Fi, Bluetooth, airplane mode, energy
//! saver and Do not disturb tiles, brightness and volume sliders, battery.

pub mod backend;
pub mod bt_agent;
pub mod media;

use std::time::{Duration, Instant};

use cosmic::app::Task;
use cosmic::iced::{Alignment, Background, Color, Length};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, slider, text, text_input, toggler};
use cosmic::{Element, theme};
use lapin_ux_core::quick::{self as model, Bluetooth, Brightness, Network, Power, Sink, Volume};

use crate::app::{Message, Shell};
use crate::panels::PanelKind;
use crate::style;

pub const WIDTH: u32 = 360;

#[derive(Debug, Default)]
pub struct QuickState {
    pub network: Network,
    pub bluetooth: Bluetooth,
    pub power: Power,
    pub volume: Volume,
    pub sinks: Vec<Sink>,
    pub streams: Vec<model::AppStream>,
    pub a11y: crate::wayland::A11yState,
    /// Active keyboard layout (index into the configured layouts).
    pub kb_group: u32,
    /// Configured keyboard layouts: (code, description).
    pub layouts: Vec<(String, String)>,
    pub screen_reader: bool,
    pub high_contrast: bool,
    /// A question from the Bluetooth pairing agent.
    pub bt_prompt: Option<bt_agent::Prompt>,
    pub bt_pin: String,
    /// Device being paired, and how the last attempt went.
    pub pairing: Option<String>,
    pub pair_result: Option<String>,
    /// Media players, the one playing first.
    pub media: Vec<media::Player>,
    pub backlight: Option<(String, Brightness)>,
    /// Last time a slider sent a value to the system (throttling).
    last_set: Option<Instant>,
    /// Touchpad scrolling over the volume icon, gathered into whole steps.
    scroll_acc: f32,
    /// Ignore volume reports briefly after the user moved the slider,
    /// so the knob doesn't jump back while the system catches up.
    hold_volume_until: Option<Instant>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Main,
    Wifi,
    Bluetooth,
    Sound,
    Vpn,
    Hotspot,
    Keyboard,
    Accessibility,
    /// Looking for Bluetooth devices to pair.
    BtAdd,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuickPanel {
    pub page: Page,
    /// "Edit quick settings": remove, add and drag tiles.
    pub editing: bool,
    /// The "Add" list is open (while editing).
    pub adding: bool,
    pub tile_drag: Option<TileDrag>,
    /// Wi-Fi network whose row is expanded (Connect / password).
    pub selected: Option<String>,
    pub password: String,
}

/// A tile being dragged while editing.
#[derive(Debug, Clone, PartialEq)]
pub struct TileDrag {
    pub id: String,
    pub from: usize,
    origin: cosmic::iced::Point,
    pub active: bool,
    /// Index among the shown tiles it would land at.
    pub target: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum QuickMsg {
    Toggle,
    /// Enter / leave tile editing.
    Edit(bool),
    /// Show / hide the "Add" list.
    AddList(bool),
    UnpinTile(String),
    PinTile(String),
    Event(backend::Event),
    Page(Page),
    Wifi(bool),
    Bluetooth(bool),
    Airplane(bool),
    EnergySaver(bool),
    DoNotDisturb(bool),
    Volume(f32),
    VolumeDone,
    Mute,
    Scroll(f32),
    Brightness(f32),
    BrightnessDone,
    SelectNetwork(String),
    Password(String),
    Connect,
    Disconnect,
    BtConnect(String, bool),
    Sink(String),
    StreamVolume(u32, f32),
    StreamMute(u32, bool),
    Media(Vec<media::Player>),
    MediaCommand(String, media::Command),
    Settings(&'static str),
    Failed(String),
    Hotspot(bool),
    /// Connect (no active path) or disconnect a VPN.
    Vpn(String, Option<String>),
    KeyboardLayout(u32),
    Magnifier(bool),
    ColorFilter(bool),
    ScreenReader(bool),
    /// Contrast themes: unused while they're turned off (no themes yet).
    #[allow(dead_code)]
    HighContrast(bool),
    Agent(bt_agent::AgentEvent),
    BtPair(String),
    /// Pairing finished: device path, error message if it failed.
    BtPaired(String, Option<String>),
    /// Answer the agent: yes / no.
    BtAnswer(bool),
    BtPin(String),
    /// Start a helper app (cast, nearby sharing) and close the panel.
    Run(&'static str),
}

/// Apps behind the Cast and Nearby sharing tiles, first installed wins.
const CAST_APPS: &[&str] = &["gnome-network-displays"];
const NEARBY_APPS: &[&str] = &["localsend_app", "localsend", "kdeconnect-app"];

fn first_installed(list: &'static [&'static str]) -> Option<&'static str> {
    list.iter().copied().find(|b| crate::account::which(b))
}

/// Configured keyboard layouts with readable names.
pub(crate) fn keyboard_layouts() -> Vec<(String, String)> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let config = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let ron = std::fs::read_to_string(config.join("cosmic/com.system76.CosmicComp/v1/xkb_config"))
        .or_else(|_| std::fs::read_to_string("/usr/share/cosmic/com.system76.CosmicComp/v1/xkb_config"))
        .unwrap_or_default();
    let lst = std::fs::read_to_string("/usr/share/X11/xkb/rules/evdev.lst").unwrap_or_default();
    model::xkb_layouts(&ron)
        .into_iter()
        .map(|(l, v)| {
            let desc = model::xkb_description(&lst, &l, &v).unwrap_or_else(|| if v.is_empty() { l.clone() } else { format!("{l} ({v})") });
            (l.to_uppercase(), desc)
        })
        .collect()
}

/// Is Orca (the screen reader) running?
pub(crate) fn orca_running() -> bool {
    std::fs::read_dir("/proc").into_iter().flatten().flatten().any(|e| {
        std::fs::read(e.path().join("cmdline")).is_ok_and(|c| c.split(|b| *b == 0).take(2).any(|a| a.ends_with(b"/orca") || a == b"orca"))
    })
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "Lapin UX".into())
}

fn make_password() -> String {
    // Readable: no 0/O/1/l.
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut bytes = [0u8; 10];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut bytes);
    }
    bytes.iter().map(|b| CHARS[*b as usize % CHARS.len()] as char).collect()
}

static PASSWORD_ID: std::sync::LazyLock<widget::Id> = std::sync::LazyLock::new(|| widget::Id::new("lapin-ux-wifi-password"));

fn done(r: zbus::Result<()>, what: &'static str) -> cosmic::Action<Message> {
    match r {
        Ok(()) => cosmic::Action::App(Message::Noop),
        Err(e) => {
            tracing::warn!("{what}: {e}");
            cosmic::Action::App(Message::Quick(QuickMsg::Failed(format!("Couldn't {what}"))))
        }
    }
}

impl Shell {
    /// Pointer on the quick settings panel: drag tiles while editing.
    pub(crate) fn quick_pointer(&mut self, ev: crate::taskbar_extras::Pointer) -> Task<Message> {
        use crate::taskbar_extras::Pointer;
        let pid = self.panel_id();
        let avail = available_tiles(self);
        let shown = lapin_ux_core::quick::tiles_shown(&avail, &self.config.quick_tiles_order, &self.config.quick_tiles_hidden);
        let hit = |shell: &Shell, p: cosmic::iced::Point| {
            shown.iter().position(|id| shell.rects.get(&crate::app::rect_key(pid, &format!("qt:{id}"))).is_some_and(|r| r.contains(p)))
        };
        match ev {
            Pointer::Move(p) => {
                self.pointer = Some((pid, p));
                let target = hit(self, p);
                if let Some(q) = self.quick_panel()
                    && let Some(d) = &mut q.tile_drag
                {
                    if !d.active && (p.x - d.origin.x).hypot(p.y - d.origin.y) > 6.0 {
                        d.active = true;
                    }
                    if d.active && target.is_some() {
                        d.target = target;
                    }
                }
            }
            Pointer::Down => {
                let Some((s, p)) = self.pointer else { return Task::none() };
                let editing = self.quick_panel().is_some_and(|q| q.editing && !q.adding);
                if s != pid || !editing {
                    return Task::none();
                }
                let from = hit(self, p);
                if let Some(q) = self.quick_panel() {
                    q.tile_drag = from.map(|i| TileDrag { id: shown[i].clone(), from: i, origin: p, active: false, target: None });
                }
            }
            Pointer::Up => {
                let drag = self.quick_panel().and_then(|q| q.tile_drag.take());
                if let Some(d) = drag
                    && d.active
                    && let Some(to) = d.target
                {
                    self.config.quick_tiles_order = lapin_ux_core::quick::move_tile(&shown, d.from, to);
                    self.persist_config();
                }
            }
        }
        Task::none()
    }

    fn quick_panel(&mut self) -> Option<&mut QuickPanel> {
        match &mut self.panel {
            Some(crate::panels::Panel { kind: PanelKind::Quick(q), .. }) => Some(q),
            _ => None,
        }
    }

    pub(crate) fn quick_open(&self) -> bool {
        matches!(self.panel.as_ref().map(|p| &p.kind), Some(PanelKind::Quick(_)))
    }

    pub(crate) fn update_quick(&mut self, msg: QuickMsg) -> Task<Message> {
        let q = &mut self.quick;
        match msg {
            QuickMsg::Toggle => {
                if self.quick_open() {
                    return self.close_panel();
                }
                self.quick.backlight = backend::backlight();
                self.quick.error = None;
                self.quick.layouts = keyboard_layouts();
                self.quick.screen_reader = orca_running();
                self.quick.high_contrast = crate::defaults::high_contrast();
                let close_centre = Task::future(async {
                    crate::notify_client::close_center().await;
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([close_centre, self.open_panel(PanelKind::Quick(QuickPanel::default()))]);
            }
            QuickMsg::Edit(on) => {
                if let Some(p) = self.quick_panel() {
                    p.editing = on;
                    p.adding = false;
                    p.tile_drag = None;
                }
            }
            QuickMsg::AddList(on) => {
                if let Some(p) = self.quick_panel() {
                    p.adding = on;
                }
            }
            QuickMsg::UnpinTile(id) => {
                if !self.config.quick_tiles_hidden.contains(&id) {
                    self.config.quick_tiles_hidden.push(id);
                }
                self.persist_config();
            }
            QuickMsg::PinTile(id) => {
                self.config.quick_tiles_hidden.retain(|h| h != &id);
                // Added tiles go to the end.
                let avail = available_tiles(self);
                let mut order = lapin_ux_core::quick::tiles_shown(&avail, &self.config.quick_tiles_order, &self.config.quick_tiles_hidden);
                order.retain(|o| o != &id);
                order.push(id);
                self.config.quick_tiles_order = order;
                self.persist_config();
                if let Some(p) = self.quick_panel() {
                    p.adding = false;
                }
            }
            QuickMsg::Event(ev) => match ev {
                backend::Event::Network(n) => q.network = n,
                backend::Event::Bluetooth(b) => q.bluetooth = b,
                backend::Event::Power(p) => q.power = p,
                backend::Event::Volume(v) => {
                    if q.hold_volume_until.is_none_or(|t| Instant::now() > t) {
                        q.volume = v;
                    }
                }
                backend::Event::Sinks(s) => q.sinks = s,
                backend::Event::Streams(s) => {
                    if q.hold_volume_until.is_none_or(|t| Instant::now() > t) {
                        q.streams = s;
                    }
                }
            },
            QuickMsg::Page(page) => {
                let scan = page == Page::Wifi && !q.network.wifi_device.is_empty();
                let device = q.network.wifi_device.clone();
                let adapter = q.bluetooth.adapter.clone();
                let was = self.quick_panel().map(|p| p.page.clone());
                if let Some(p) = self.quick_panel() {
                    p.page = page.clone();
                    p.selected = None;
                    p.password.clear();
                }
                // Look for devices only while the "Add device" page is up.
                let discover = match (was == Some(Page::BtAdd), page == Page::BtAdd) {
                    (false, true) => Some(true),
                    (true, false) => Some(false),
                    _ => None,
                };
                if let Some(on) = discover {
                    self.quick.pair_result = None;
                    return Task::future(async move {
                        if let Err(e) = backend::set_discovery(adapter, on).await {
                            tracing::warn!("bluetooth discovery: {e}");
                        }
                        cosmic::Action::App(Message::Noop)
                    });
                }
                if scan {
                    return Task::future(async move { done(backend::wifi_scan(device).await, "scan for networks") });
                }
            }
            QuickMsg::Wifi(on) => {
                q.network.wifi_enabled = on;
                return Task::future(async move { done(backend::set_wifi(on).await, "change Wi-Fi") });
            }
            QuickMsg::Bluetooth(on) => {
                q.bluetooth.powered = on;
                let adapter = q.bluetooth.adapter.clone();
                return Task::future(async move { done(backend::set_bluetooth(adapter, on).await, "change Bluetooth") });
            }
            QuickMsg::Airplane(on) => {
                // Airplane mode = every radio off; turning it off brings Wi-Fi
                // and Bluetooth back on.
                q.network.wifi_enabled = !on;
                let bt = q.bluetooth.available.then(|| q.bluetooth.adapter.clone());
                if bt.is_some() {
                    q.bluetooth.powered = !on;
                }
                return Task::future(async move {
                    let r = backend::set_wifi(!on).await;
                    if let Some(a) = bt {
                        let _ = backend::set_bluetooth(a, !on).await;
                    }
                    done(r, "change airplane mode")
                });
            }
            QuickMsg::EnergySaver(on) => {
                let profile = if on { "power-saver" } else { "balanced" };
                q.power.profile = Some(profile.into());
                return Task::future(async move { done(backend::set_profile(profile).await, "change the power mode") });
            }
            QuickMsg::DoNotDisturb(on) => {
                self.notify.dnd = on;
                return Task::future(async move {
                    crate::notify_client::set_dnd(on).await;
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::Volume(v) => {
                q.volume.level = v;
                q.hold_volume_until = Some(Instant::now() + Duration::from_millis(800));
                let unmute = std::mem::take(&mut q.volume.muted);
                // Throttle: the release sends the final value anyway.
                if q.last_set.is_some_and(|t| t.elapsed() < Duration::from_millis(60)) && !unmute {
                    return Task::none();
                }
                q.last_set = Some(Instant::now());
                return Task::future(async move {
                    if unmute {
                        backend::set_mute(false).await;
                    }
                    backend::set_volume(v).await;
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::VolumeDone => {
                lapin_ux_core::sound::play("audio-volume-change");
                let v = q.volume.level;
                return Task::future(async move {
                    backend::set_volume(v).await;
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::Scroll(dy) => {
                if !q.volume.available || dy == 0.0 {
                    return Task::none();
                }
                // A wheel notch is one 2% step; touchpads send many small
                // amounts, gathered into whole steps (as COSMIC 1.11 does for
                // its volume applet). Nothing happens past 0% or 100%.
                q.scroll_acc += dy;
                let steps = q.scroll_acc.trunc();
                if steps == 0.0 {
                    return Task::none();
                }
                q.scroll_acc -= steps;
                let v = (q.volume.level + 0.02 * steps).clamp(0.0, 1.0);
                if (v - q.volume.level).abs() < f32::EPSILON {
                    return Task::none();
                }
                return self.update_quick(QuickMsg::Volume(v));
            }
            QuickMsg::Mute => {
                let m = !q.volume.muted;
                q.volume.muted = m;
                return Task::future(async move {
                    backend::set_mute(m).await;
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::Brightness(f) => {
                let Some((dev, b)) = &mut q.backlight else { return Task::none() };
                b.current = b.raw_for(f);
                let (dev, value) = (dev.clone(), b.current);
                if q.last_set.is_some_and(|t| t.elapsed() < Duration::from_millis(40)) {
                    return Task::none();
                }
                q.last_set = Some(Instant::now());
                return Task::future(async move { done(backend::set_brightness(dev, value).await, "change brightness") });
            }
            QuickMsg::BrightnessDone => {
                let Some((dev, b)) = &q.backlight else { return Task::none() };
                let (dev, value) = (dev.clone(), b.current);
                return Task::future(async move { done(backend::set_brightness(dev, value).await, "change brightness") });
            }
            QuickMsg::SelectNetwork(ssid) => {
                let focus = {
                    let n = q.network.networks.iter().find(|n| n.ssid == ssid);
                    n.is_some_and(|n| n.secured && !n.known && !n.active)
                };
                if let Some(p) = self.quick_panel() {
                    if p.selected.as_deref() == Some(ssid.as_str()) {
                        p.selected = None;
                    } else {
                        p.selected = Some(ssid);
                        p.password.clear();
                    }
                }
                if focus {
                    return text_input::focus(PASSWORD_ID.clone());
                }
            }
            QuickMsg::Password(pw) => {
                if let Some(p) = self.quick_panel() {
                    p.password = pw;
                }
            }
            QuickMsg::Connect => {
                let device = self.quick.network.wifi_device.clone();
                let Some(p) = self.quick_panel() else { return Task::none() };
                let Some(ssid) = p.selected.clone() else { return Task::none() };
                let password = std::mem::take(&mut p.password);
                let Some(net) = self.quick.network.networks.iter().find(|n| n.ssid == ssid).cloned() else {
                    return Task::none();
                };
                if net.secured && !net.known && password.is_empty() {
                    return text_input::focus(PASSWORD_ID.clone());
                }
                let password = (net.secured && !net.known).then_some(password);
                return Task::future(async move {
                    done(backend::wifi_connect(device, net.ap_path, net.ssid, password).await, "connect")
                });
            }
            QuickMsg::Disconnect => {
                let device = q.network.wifi_device.clone();
                return Task::future(async move { done(backend::wifi_disconnect(device).await, "disconnect") });
            }
            QuickMsg::BtConnect(path, connect) => {
                return Task::future(async move {
                    done(backend::bluetooth_connect(path, connect).await, if connect { "connect the device" } else { "disconnect the device" })
                });
            }
            QuickMsg::Sink(name) => {
                for s in &mut q.sinks {
                    s.default = s.name == name;
                }
                return Task::future(async move {
                    backend::set_default_sink(name).await;
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::StreamVolume(i, v) => {
                if let Some(s) = q.streams.iter_mut().find(|s| s.index == i) {
                    s.level = v;
                }
                q.hold_volume_until = Some(Instant::now() + Duration::from_millis(800));
                if q.last_set.is_some_and(|t| t.elapsed() < Duration::from_millis(60)) {
                    return Task::none();
                }
                q.last_set = Some(Instant::now());
                return Task::future(async move {
                    backend::set_stream_volume(i, v).await;
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::StreamMute(i, m) => {
                if let Some(s) = q.streams.iter_mut().find(|s| s.index == i) {
                    s.muted = m;
                }
                return Task::future(async move {
                    backend::set_stream_mute(i, m).await;
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::Media(players) => q.media = players,
            QuickMsg::MediaCommand(bus, c) => {
                return Task::future(async move {
                    media::command(bus, c).await;
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::Settings(page) => {
                let close = self.close_panel();
                return Task::batch([close, self.open_settings(page)]);
            }
            QuickMsg::Failed(e) => q.error = Some(e),
            QuickMsg::Hotspot(on) => {
                q.network.hotspot = on;
                let device = q.network.wifi_device.clone();
                if self.config.hotspot_password.is_empty() {
                    self.config.hotspot_password = make_password();
                    self.persist_config();
                }
                let pw = self.config.hotspot_password.clone();
                return Task::future(async move { done(backend::hotspot(device, on, hostname(), pw).await, "change the mobile hotspot") });
            }
            QuickMsg::Vpn(settings, active) => {
                let what = if active.is_some() { "disconnect the VPN" } else { "connect the VPN" };
                return Task::future(async move { done(backend::vpn(settings, active).await, what) });
            }
            QuickMsg::KeyboardLayout(g) => {
                q.kb_group = g;
                self.request(crate::wayland::WaylandRequest::SetKeyboardGroup(g));
            }
            QuickMsg::Magnifier(on) => {
                q.a11y.magnifier = on;
                self.request(crate::wayland::WaylandRequest::SetMagnifier(on));
            }
            QuickMsg::ColorFilter(on) => {
                q.a11y.filter_on = on;
                self.request(crate::wayland::WaylandRequest::SetColorFilter(on));
            }
            QuickMsg::HighContrast(on) => {
                q.high_contrast = on;
                return Task::future(async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Err(e) = crate::defaults::set_high_contrast(on) {
                            tracing::warn!("contrast themes: {e}");
                        }
                    })
                    .await;
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::ScreenReader(on) => {
                q.screen_reader = on;
                return Task::future(async move {
                    let args: &[&str] = if on { &["--replace"] } else { &["--quit"] };
                    if let Err(e) = tokio::process::Command::new("orca").args(args).spawn() {
                        tracing::warn!("orca: {e}");
                    }
                    cosmic::Action::App(Message::Noop)
                });
            }
            QuickMsg::Agent(ev) => match ev {
                bt_agent::AgentEvent::Prompt(p) => {
                    q.bt_pin.clear();
                    q.bt_prompt = Some(p);
                    // A device asking on its own: bring the panel up.
                    if !self.quick_open() {
                        let mut panel = QuickPanel::default();
                        panel.page = Page::BtAdd;
                        return self.open_panel(PanelKind::Quick(panel));
                    }
                }
                bt_agent::AgentEvent::Cancelled => q.bt_prompt = None,
            },
            QuickMsg::BtPair(path) => {
                q.pairing = Some(path.clone());
                q.pair_result = None;
                return Task::future(async move {
                    let r = backend::bluetooth_pair(path.clone()).await;
                    if let Err(e) = &r {
                        tracing::warn!("pairing: {e}");
                    }
                    cosmic::Action::App(Message::Quick(QuickMsg::BtPaired(path, r.err().map(|_| "Couldn't connect. Make sure the device is ready to pair.".to_string()))))
                });
            }
            QuickMsg::BtPaired(path, err) => {
                if q.pairing.as_deref() == Some(path.as_str()) {
                    q.pairing = None;
                }
                q.bt_prompt = None;
                q.pair_result = Some(err.unwrap_or_else(|| "Your device is ready to go!".into()));
            }
            QuickMsg::BtAnswer(yes) => {
                if let Some(p) = q.bt_prompt.take() {
                    let reply = yes.then(|| std::mem::take(&mut q.bt_pin));
                    bt_agent::answer(p.id, reply);
                }
            }
            QuickMsg::BtPin(pin) => q.bt_pin = pin.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect(),
            QuickMsg::Run(bin) => {
                let close = self.close_panel();
                let run = Task::future(async move {
                    if let Err(e) = tokio::process::Command::new(bin).spawn() {
                        tracing::warn!("{bin}: {e}");
                    }
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([close, run]);
            }
        }
        Task::none()
    }

    /// Open the system settings app, on a page if it supports that.
    pub(crate) fn open_settings(&mut self, page: &str) -> Task<Message> {
        // Lapin pages (lapin-…) open with `cosmic-settings lapin <id>`.
        let argv = lapin_ux_core::system::settings_command(page, lapin_ux_core::system::lapin_settings_installed());
        let mut cmd = std::process::Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        Task::future(async move {
            if cosmic::process::spawn(cmd).await.is_none() {
                tracing::warn!("could not start cosmic-settings");
            }
            cosmic::Action::App(Message::Noop)
        })
    }

    /// Esc on a sub-page goes back to the main page.
    pub(crate) fn quick_escape(&mut self) -> Option<Task<Message>> {
        let p = self.quick_panel()?;
        let back = match p.page {
            Page::Main => return None,
            Page::BtAdd => Page::Bluetooth,
            _ => Page::Main,
        };
        Some(self.update_quick(QuickMsg::Page(back)))
    }
}

// ---- taskbar button ------------------------------------------------------------

pub fn sym(name: &str, fallback: &'static [&'static str]) -> icon::Icon {
    icon::from_name(name.to_string())
        .fallback(Some(icon::IconFallback::Names(fallback.iter().map(|s| (*s).into()).collect())))
        .size(16)
        .icon()
}

fn battery_icon(b: &model::Battery) -> icon::Icon {
    sym(&model::battery_icon(b), &["battery-good-symbolic", "battery-symbolic"])
}

/// Network, volume and battery icons in one button, like the benchmark.
pub fn status_icons(shell: &Shell) -> Vec<Element<'_, Message>> {
    let q = &shell.quick;
    let mut v: Vec<Element<'_, Message>> = Vec::new();
    if q.network.available {
        v.push(sym(model::network_icon(&q.network), &["network-wireless-disconnected-symbolic", "network-wired-symbolic"]).into());
    }
    if q.volume.available {
        v.push(sym(model::volume_icon(&q.volume), &["audio-volume-high-symbolic"]).into());
    }
    if let Some(b) = &q.power.battery {
        v.push(battery_icon(b).into());
    }
    v
}

// ---- panel -------------------------------------------------------------------------

fn tile_style(on: bool) -> theme::Button {
    theme::Button::Custom {
        active: Box::new(move |_, t| tile_look(t, on, 0.0)),
        disabled: Box::new(move |t| {
            let mut s = tile_look(t, false, 0.0);
            s.text_color = Some(Color { a: 0.4, ..s.text_color.unwrap_or(Color::WHITE) });
            s.icon_color = s.text_color;
            s
        }),
        hovered: Box::new(move |_, t| tile_look(t, on, 0.08)),
        pressed: Box::new(move |_, t| tile_look(t, on, 0.14)),
    }
}

fn tile_look(t: &cosmic::Theme, on: bool, lift: f32) -> button::Style {
    let c = t.cosmic();
    let (bg, fg): (Color, Color) = if on {
        (c.accent_color().into(), c.on_accent_color().into())
    } else {
        (c.background(true).component.base.into(), c.on_bg_color().into())
    };
    let bg = Color { r: (bg.r + lift).min(1.0), g: (bg.g + lift).min(1.0), b: (bg.b + lift).min(1.0), a: bg.a };
    button::Style {
        background: Some(Background::Color(bg)),
        border_radius: style::CONTROL_RADIUS.into(),
        border_width: if on { 0.0 } else { 1.0 },
        border_color: c.bg_divider().into(),
        text_color: Some(fg),
        icon_color: Some(fg),
        ..Default::default()
    }
}

const TILE_W: f32 = 100.0;
const TILE_H: f32 = 48.0;

/// A tile: toggles on click; with `more`, a chevron half opens a sub-page.
fn tile<'a>(
    icon_name: &'static str,
    label: impl Into<String>,
    on: bool,
    toggle: Option<Message>,
    more: Option<Message>,
) -> Element<'a, Message> {
    let main = button::custom(container(sym(icon_name, &[])).center(Length::Fill))
        .height(TILE_H)
        .width(Length::Fill)
        .padding(0)
        .class(tile_style(on))
        .on_press_maybe(toggle);
    let top: Element<'a, Message> = match more {
        Some(m) => row::with_children(vec![
            main.into(),
            button::custom(container(sym("pan-end-symbolic", &["go-next-symbolic"])).center(Length::Fill))
                .height(TILE_H)
                .width(30)
                .padding(0)
                .class(tile_style(on))
                .on_press(m)
                .into(),
        ])
        .spacing(1)
        .into(),
        None => main.into(),
    };
    column::with_children(vec![top, container(text::caption(label.into())).center_x(Length::Fill).into()])
        .spacing(6)
        .width(TILE_W)
        .into()
}

/// A header switch: its state and the message for a new state.
type HeaderToggle = (bool, fn(bool) -> Message);

fn header<'a>(title: &'static str, toggle: Option<HeaderToggle>) -> Element<'a, Message> {
    let mut r = row::with_capacity(3).spacing(8).align_y(Alignment::Center).push(
        button::custom(sym("go-previous-symbolic", &[]))
            .padding(8)
            .class(style::flat_button(false))
            .on_press(Message::Quick(QuickMsg::Page(Page::Main))),
    );
    r = r.push(text::title4(title).width(Length::Fill));
    if let Some((on, f)) = toggle {
        r = r.push(toggler(on).on_toggle(f));
    }
    r.into()
}

fn link<'a>(label: &'static str, page: &'static str) -> Element<'a, Message> {
    lapin_ux_ui::button::text(label).on_press(Message::Quick(QuickMsg::Settings(page))).into()
}

fn list_row<'a>(content: impl Into<Element<'a, Message>>, selected: bool, msg: Message) -> Element<'a, Message> {
    button::custom(content).width(Length::Fill).padding([8, 10]).class(style::flat_button(selected)).on_press(msg).into()
}

/// One quick settings tile, before it's drawn.
struct TileSpec {
    id: &'static str,
    icon: &'static str,
    label: String,
    on: bool,
    toggle: Option<Message>,
    more: Option<Message>,
}

impl TileSpec {
    fn new(id: &'static str, icon: &'static str, label: impl Into<String>, on: bool, toggle: Option<Message>, more: Option<Message>) -> Self {
        Self { id, icon, label: label.into(), on, toggle, more }
    }
}

/// Human name of a tile, for the Add list.
fn tile_title(id: &str) -> &'static str {
    match id {
        "wifi" => "Wi-Fi",
        "bluetooth" => "Bluetooth",
        "airplane" => "Airplane mode",
        "energy" => "Energy saver",
        "accessibility" => "Accessibility",
        "hotspot" => "Mobile hotspot",
        "nearby" => "Nearby sharing",
        "cast" => "Cast",
        "keyboard" => "Keyboard layout",
        "vpn" => "VPN",
        "dnd" => "Do not disturb",
        "nightlight" => "Night light",
        "rotation" => "Rotation lock",
        _ => "Tile",
    }
}

/// A tile while editing: not clickable, with an unpin badge.
fn edit_tile<'a>(t: &TileSpec, lifted: bool) -> Element<'a, Message> {
    let face = container(sym(t.icon, &[]))
        .center_x(Length::Fill)
        .center_y(Length::Fixed(TILE_H as f32))
        .class(cosmic::theme::Container::custom(move |th| {
            let c = th.cosmic();
            let fg: cosmic::iced::Color = c.on_bg_color().into();
            container::Style {
                background: Some(cosmic::iced::Background::Color(cosmic::iced::Color { a: if lifted { 0.22 } else { 0.10 }, ..fg })),
                border: cosmic::iced::Border { color: cosmic::iced::Color { a: 0.35, ..fg }, width: 1.0, radius: 4.0.into() },
                ..Default::default()
            }
        }));
    let badge = container(
        button::custom(sym("view-unpin-symbolic", &["view-pin-symbolic"]))
            .padding(3)
            .class(style::flat_button(true))
            .on_press(Message::Quick(QuickMsg::UnpinTile(t.id.to_string()))),
    )
    .width(Length::Fill)
    .align_x(Alignment::End);
    let top = cosmic::iced::widget::Stack::with_children(vec![face.into(), badge.into()]);
    column::with_children(vec![top.into(), container(text::caption(t.label.clone())).center_x(Length::Fill).into()])
        .spacing(6)
        .width(TILE_W)
        .into()
}

/// Where a dragged tile will land.
fn drop_marker<'a>() -> Element<'a, Message> {
    container(widget::Space::new().width(3).height(TILE_H))
        .class(cosmic::theme::Container::custom(|th| container::Style {
            background: Some(cosmic::iced::Background::Color(th.cosmic().accent_color().into())),
            border: cosmic::iced::Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        }))
        .into()
}

/// The "Add" tile at the end while editing.
fn add_tile<'a>(any: bool) -> Element<'a, Message> {
    column::with_children(vec![
        button::custom(container(sym("list-add-symbolic", &[])).center(Length::Fill))
            .height(TILE_H)
            .width(Length::Fill)
            .padding(0)
            .class(tile_style(false))
            .on_press_maybe(any.then_some(Message::Quick(QuickMsg::AddList(true))))
            .into(),
        container(text::caption("Add")).center_x(Length::Fill).into(),
    ])
    .spacing(6)
    .width(TILE_W)
    .into()
}

/// The list of removed tiles to add back.
fn add_list<'a>(shell: &'a Shell, available: &[&'static str]) -> Element<'a, Message> {
    let hidden = lapin_ux_core::quick::tiles_hidden(available, &shell.config.quick_tiles_hidden);
    let mut col = column::with_capacity(hidden.len() + 1).spacing(2).push(
        row::with_children(vec![
            button::custom(sym("go-previous-symbolic", &[]))
                .padding(8)
                .class(style::flat_button(false))
                .on_press(Message::Quick(QuickMsg::AddList(false)))
                .into(),
            text::title4("Add").into(),
        ])
        .spacing(8)
        .align_y(Alignment::Center),
    );
    for id in hidden {
        col = col.push(list_row(text::body(tile_title(&id)), false, Message::Quick(QuickMsg::PinTile(id.clone()))));
    }
    container(col).padding([20, 20, 12, 20]).into()
}

fn tile_specs(shell: &Shell) -> Vec<TileSpec> {
    let q = &shell.quick;
    let mut specs: Vec<TileSpec> = Vec::new();
    if q.network.wifi_present {
        let label = match (&q.network.link, q.network.wifi_enabled) {
            (model::Link::Wifi { .. }, _) => q.network.link_name.clone(),
            (_, true) => "Wi-Fi".into(),
            (_, false) => "Wi-Fi".into(),
        };
        let icon = if q.network.wifi_enabled { "network-wireless-signal-excellent-symbolic" } else { "network-wireless-offline-symbolic" };
        specs.push(TileSpec::new("wifi", 
            icon,
            label,
            q.network.wifi_enabled,
            Some(Message::Quick(QuickMsg::Wifi(!q.network.wifi_enabled))),
            Some(Message::Quick(QuickMsg::Page(Page::Wifi))),
        ));
    }
    if q.bluetooth.available {
        let on = q.bluetooth.powered;
        let label = q.bluetooth.devices.iter().find(|d| d.connected && on).map_or("Bluetooth".to_string(), |d| d.name.clone());
        specs.push(TileSpec::new("bluetooth", 
            if on { "bluetooth-active-symbolic" } else { "bluetooth-disabled-symbolic" },
            label,
            on,
            Some(Message::Quick(QuickMsg::Bluetooth(!on))),
            Some(Message::Quick(QuickMsg::Page(Page::Bluetooth))),
        ));
    }
    if q.network.wifi_present || q.bluetooth.available {
        let airplane = !q.network.wifi_enabled && (!q.bluetooth.available || !q.bluetooth.powered);
        specs.push(TileSpec::new("airplane", "airplane-mode-symbolic", "Airplane mode", airplane, Some(Message::Quick(QuickMsg::Airplane(!airplane))), None));
    }
    if let Some(p) = &q.power.profile {
        let on = p == "power-saver";
        specs.push(TileSpec::new("energy", 
            "power-profile-power-saver-symbolic",
            "Energy saver",
            on,
            Some(Message::Quick(QuickMsg::EnergySaver(!on))),
            None,
        ));
    }
    // Night light and rotation lock (lapin-ux-comp / iio-sensor-proxy).
    if shell.extras.version >= 2 {
        let on = shell.config.night_light;
        let toggle = Message::Extras(crate::comp_extras::ExtrasMsg::NightLight(!on));
        specs.push(TileSpec::new("nightlight", "night-light-symbolic", "Night light", on, Some(toggle), None));
    }
    if shell.extras.can_rotate {
        let on = shell.config.rotation_lock;
        let icon = if on { "rotation-locked-symbolic" } else { "rotation-allowed-symbolic" };
        let toggle = Message::Extras(crate::comp_extras::ExtrasMsg::RotationLock(!on));
        specs.push(TileSpec::new("rotation", icon, "Rotation lock", on, Some(toggle), None));
    }
    if q.a11y.available || lapin_ux_core::system::screen_reader_ready() {
        let on = q.a11y.magnifier || q.a11y.filter_on || q.screen_reader;
        specs.push(TileSpec::new("accessibility", 
            "preferences-desktop-accessibility-symbolic",
            "Accessibility",
            on,
            Some(Message::Quick(QuickMsg::Page(Page::Accessibility))),
            Some(Message::Quick(QuickMsg::Page(Page::Accessibility))),
        ));
    }
    if q.network.wifi_present {
        let on = q.network.hotspot;
        specs.push(TileSpec::new("hotspot", 
            "network-wireless-hotspot-symbolic",
            "Mobile hotspot",
            on,
            Some(Message::Quick(QuickMsg::Hotspot(!on))),
            Some(Message::Quick(QuickMsg::Page(Page::Hotspot))),
        ));
    }
    if let Some(bin) = first_installed(NEARBY_APPS) {
        specs.push(TileSpec::new("nearby", "send-to-symbolic", "Nearby sharing", false, Some(Message::Quick(QuickMsg::Run(bin))), None));
    }
    if let Some(bin) = first_installed(CAST_APPS) {
        specs.push(TileSpec::new("cast", "video-display-symbolic", "Cast", false, Some(Message::Quick(QuickMsg::Run(bin))), None));
    }
    if q.layouts.len() > 1 {
        let current = q.layouts.get(q.kb_group as usize).map_or("Keyboard".to_string(), |(c, _)| c.clone());
        let next = (q.kb_group + 1) % q.layouts.len() as u32;
        specs.push(TileSpec::new("keyboard", 
            "input-keyboard-symbolic",
            current,
            false,
            Some(Message::Quick(QuickMsg::KeyboardLayout(next))),
            Some(Message::Quick(QuickMsg::Page(Page::Keyboard))),
        ));
    }
    if !q.network.vpns.is_empty() {
        let up = q.network.vpns.iter().find(|v| v.active.is_some());
        let label = up.map_or("VPN".to_string(), |v| v.name.clone());
        specs.push(TileSpec::new("vpn", 
            "network-vpn-symbolic",
            label,
            up.is_some(),
            Some(Message::Quick(QuickMsg::Page(Page::Vpn))),
            Some(Message::Quick(QuickMsg::Page(Page::Vpn))),
        ));
    }
    if shell.notify.available {
        let on = shell.notify.dnd;
        specs.push(TileSpec::new("dnd", "notification-disabled-symbolic", "Do not disturb", on, Some(Message::Quick(QuickMsg::DoNotDisturb(!on))), None));
    }
    specs
}

/// Ids of the tiles this system can show, in default order.
pub(crate) fn available_tiles(shell: &Shell) -> Vec<&'static str> {
    tile_specs(shell).into_iter().map(|t| t.id).collect()
}

fn main_page<'a>(shell: &'a Shell, p: &'a QuickPanel) -> Element<'a, Message> {
    let q = &shell.quick;
    let specs = tile_specs(shell);
    // The user's arrangement (Edit quick settings).
    let ids: Vec<&str> = specs.iter().map(|t| t.id).collect();
    let shown = lapin_ux_core::quick::tiles_shown(&ids, &shell.config.quick_tiles_order, &shell.config.quick_tiles_hidden);
    let drag = p.tile_drag.as_ref().filter(|d| d.active);
    let mut tiles: Vec<Element<'a, Message>> = Vec::new();
    for (i, id) in shown.iter().enumerate() {
        let Some(t) = specs.iter().find(|t| t.id == id) else { continue };
        let lifted = drag.is_some_and(|d| d.id == *id);
        let el = if p.editing { edit_tile(t, lifted) } else { tile(t.icon, t.label.clone(), t.on, t.toggle.clone(), t.more.clone()) };
        // A gap where a dragged tile would land.
        if drag.is_some_and(|d| d.target == Some(i) && d.id != *id) {
            tiles.push(drop_marker());
        }
        tiles.push(crate::taskbar_view::track(shell, crate::app::rect_key(shell.panel_id(), &format!("qt:{id}")), el));
    }
    if p.editing {
        let hidden = lapin_ux_core::quick::tiles_hidden(&ids, &shell.config.quick_tiles_hidden);
        tiles.push(add_tile(!hidden.is_empty()));
    }

    let mut grid = column::with_capacity(3).spacing(14);
    let mut it = tiles.into_iter().peekable();
    while it.peek().is_some() {
        let r: Vec<Element<'a, Message>> = it.by_ref().take(3).collect();
        grid = grid.push(row::with_children(r).spacing(12));
    }

    let mut body = column::with_capacity(7).spacing(18).padding([20, 20, 12, 20]);
    if let Some(p) = q.media.first() {
        body = body.push(media_card(shell, p));
    }
    body = body.push(grid);
    // Brightness and volume rows share one layout: a 24 px icon cell, then
    // the slider. The volume row ends with the sound-output arrow; the
    // brightness slider runs on to the arrow's tip (its glyph reaches 2 px
    // past the button), so both rows end at the same place.
    let slider_row = |lead: Element<'a, Message>, bar: Element<'a, Message>, end: Option<Element<'a, Message>>| -> Element<'a, Message> {
        let lead: Element<'a, Message> = container(lead).width(24).height(24).center(Length::Fixed(24.0)).into();
        let (cells, right) = match end {
            Some(end) => (vec![lead, bar, end], 4),
            None => (vec![lead, bar], 2),
        };
        container(row::with_children(cells).spacing(10).align_y(Alignment::Center))
            .padding(cosmic::iced::Padding { top: 0.0, right: right as f32, bottom: 0.0, left: 4.0 })
            .into()
    };
    if let Some((_, b)) = &q.backlight {
        body = body.push(slider_row(
            sym("display-brightness-symbolic", &[]).into(),
            slider(0.0..=1.0, b.fraction(), |v| Message::Quick(QuickMsg::Brightness(v)))
                .class(lapin_ux_ui::slider())
                .step(0.01f32)
                .on_release(Message::Quick(QuickMsg::BrightnessDone))
                .width(Length::Fill)
                .into(),
            None,
        ));
    }
    if q.volume.available {
        body = body.push(slider_row(
            button::custom(sym(model::volume_icon(&q.volume), &["audio-volume-high-symbolic"]))
                .padding(4)
                .class(style::flat_button(false))
                .on_press(Message::Quick(QuickMsg::Mute))
                .into(),
            slider(0.0..=1.0, q.volume.level.min(1.0), |v| Message::Quick(QuickMsg::Volume(v)))
                .class(lapin_ux_ui::slider())
                .step(0.01f32)
                .on_release(Message::Quick(QuickMsg::VolumeDone))
                .width(Length::Fill)
                .into(),
            Some(
                button::custom(sym("pan-end-symbolic", &["go-next-symbolic"]))
                    .width(28)
                    .height(28)
                    .padding(6)
                    .class(style::flat_button(false))
                    .on_press(Message::Quick(QuickMsg::Page(Page::Sound)))
                    .into(),
            ),
        ));
    }
    if let Some(e) = &q.error {
        body = body.push(text::caption(e.clone()).class(style::dim_text()));
    }

    // Footer: battery on the left, settings on the right.
    let battery: Element<'a, Message> = match &q.power.battery {
        Some(b) => row::with_children(vec![battery_icon(b).into(), text::body(model::battery_label(b)).into()])
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        None => widget::Space::new().into(),
    };
    // Battery on the left; Edit (pencil) and Settings on the right, or Done while editing.
    let right: Vec<Element<'a, Message>> = if p.editing {
        vec![lapin_ux_ui::button::suggested("Done").on_press(Message::Quick(QuickMsg::Edit(false))).into()]
    } else {
        vec![
            button::custom(sym("document-edit-symbolic", &["edit-symbolic"]))
                .padding(8)
                .class(style::flat_button(false))
                .on_press(Message::Quick(QuickMsg::Edit(true)))
                .into(),
            button::custom(sym("emblem-system-symbolic", &["preferences-system-symbolic"]))
                .padding(8)
                .class(style::flat_button(false))
                .on_press(Message::Quick(QuickMsg::Settings("")))
                .into(),
        ]
    };
    let mut footer_row = row::with_capacity(3).align_y(Alignment::Center).push(container(battery).width(Length::Fill));
    for r in right {
        footer_row = footer_row.push(r);
    }
    let footer = container(footer_row.spacing(2)).padding([6, 12, 6, 20]).class(style::panel_footer());
    if p.adding {
        return column::with_children(vec![add_list(shell, &ids), footer.into()]).into();
    }
    column::with_children(vec![body.into(), footer.into()]).into()
}

fn media_card<'a>(shell: &'a Shell, p: &'a media::Player) -> Element<'a, Message> {
    let app = if p.desktop_entry.is_empty() { p.identity.to_lowercase() } else { p.desktop_entry.clone() };
    let ctl = |name: &'static str, on: bool, c: media::Command| -> Element<'a, Message> {
        button::custom(sym(name, &[]))
            .padding(8)
            .class(style::flat_button(false))
            .on_press_maybe(on.then(|| Message::Quick(QuickMsg::MediaCommand(p.bus.clone(), c))))
            .into()
    };
    let mut info = column::with_capacity(2).push(text::body(p.title.clone()));
    if !p.artist.is_empty() {
        info = info.push(text::caption(p.artist.clone()).class(style::dim_text()));
    }
    container(
        column::with_children(vec![
            row::with_children(vec![
                icon(crate::taskbar_view::app_icon(shell, &app)).size(16).into(),
                text::caption(p.identity.clone()).class(style::dim_text()).into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            row::with_children(vec![
                info.width(Length::Fill).into(),
                ctl("media-skip-backward-symbolic", p.can_prev, media::Command::Previous),
                ctl(
                    if p.playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" },
                    p.can_toggle,
                    media::Command::PlayPause,
                ),
                ctl("media-skip-forward-symbolic", p.can_next, media::Command::Next),
            ])
            .spacing(4)
            .align_y(Alignment::Center)
            .into(),
        ])
        .spacing(8),
    )
    .padding(12)
    .class(style::folder_card())
    .into()
}

fn wifi_page<'a>(shell: &'a Shell, p: &'a QuickPanel) -> Element<'a, Message> {
    let n = &shell.quick.network;
    let mut list = column::with_capacity(n.networks.len()).spacing(2);
    if !n.wifi_enabled {
        list = list.push(container(text::body("Wi-Fi is turned off").class(style::dim_text())).padding(12));
    } else if n.networks.is_empty() {
        list = list.push(container(text::body("Looking for networks…").class(style::dim_text())).padding(12));
    }
    for net in n.networks.iter().filter(|_| n.wifi_enabled) {
        let selected = p.selected.as_deref() == Some(net.ssid.as_str());
        let status = match (net.active, net.secured) {
            (true, true) => "Connected, secured",
            (true, false) => "Connected",
            (false, true) => "Secured",
            (false, false) => "Open",
        };
        let icon_name = if net.secured {
            model::wifi_icon(net.strength).replace("network-wireless-signal", "network-wireless-secure-signal")
        } else {
            model::wifi_icon(net.strength).to_string()
        };
        let mut content = column::with_capacity(3).spacing(8).push(
            row::with_children(vec![
                sym(&icon_name, &["network-wireless-signal-good-symbolic"]).into(),
                column::with_children(vec![text::body(net.ssid.clone()).into(), text::caption(status).class(style::dim_text()).into()])
                    .width(Length::Fill)
                    .into(),
            ])
            .spacing(12)
            .align_y(Alignment::Center),
        );
        if selected {
            if net.active {
                content = content.push(
                    row::with_children(vec![
                        widget::Space::new().width(Length::Fill).into(),
                        lapin_ux_ui::button::standard("Disconnect").on_press(Message::Quick(QuickMsg::Disconnect)).into(),
                    ]),
                );
            } else {
                if net.secured && !net.known {
                    content = content.push(
                        text_input::secure_input("Enter the network security key", &p.password, None, true)
                            .id(PASSWORD_ID.clone())
                            .on_input(|s| Message::Quick(QuickMsg::Password(s)))
                            .on_submit(|_| Message::Quick(QuickMsg::Connect)),
                    );
                }
                content = content.push(row::with_children(vec![
                    widget::Space::new().width(Length::Fill).into(),
                    lapin_ux_ui::button::suggested("Connect").on_press(Message::Quick(QuickMsg::Connect)).into(),
                ]));
            }
        }
        list = list.push(list_row(content, selected, Message::Quick(QuickMsg::SelectNetwork(net.ssid.clone()))));
    }
    column::with_children(vec![
        container(header("Wi-Fi", Some((n.wifi_enabled, |on| Message::Quick(QuickMsg::Wifi(on)))))).padding([12, 12, 4, 12]).into(),
        scrollable(container(list).padding([0, 12])).height(Length::Fixed(360.0)).into(),
        container(link("More Wi-Fi settings", "wireless")).padding([8, 20, 14, 20]).into(),
    ])
    .spacing(4)
    .into()
}

fn bluetooth_page<'a>(shell: &'a Shell) -> Element<'a, Message> {
    let bt = &shell.quick.bluetooth;
    let mut list = column::with_capacity(bt.devices.len() + 1).spacing(2);
    if !bt.powered {
        list = list.push(container(text::body("Bluetooth is turned off").class(style::dim_text())).padding(12));
    } else if bt.devices.is_empty() {
        list = list.push(container(text::body("No paired devices").class(style::dim_text())).padding(12));
    }
    for d in bt.devices.iter().filter(|_| bt.powered) {
        let icon_name = if d.icon.is_empty() { "bluetooth-active-symbolic".to_string() } else { format!("{}-symbolic", d.icon) };
        let content = row::with_children(vec![
            sym(&icon_name, &["bluetooth-active-symbolic"]).into(),
            column::with_children(vec![
                text::body(d.name.clone()).into(),
                text::caption(if d.connected { "Connected" } else { "Paired" }).class(style::dim_text()).into(),
            ])
            .width(Length::Fill)
            .into(),
            text::caption(if d.connected { "Disconnect" } else { "Connect" }).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center);
        list = list.push(list_row(content, d.connected, Message::Quick(QuickMsg::BtConnect(d.path.clone(), !d.connected))));
    }
    column::with_children(vec![
        container(header("Bluetooth", Some((bt.powered, |on| Message::Quick(QuickMsg::Bluetooth(on)))))).padding([12, 12, 4, 12]).into(),
        scrollable(container(list).padding([0, 12])).height(Length::Fixed(300.0)).into(),
        container(
            row::with_children(vec![
                lapin_ux_ui::button::standard("Add device").on_press_maybe(bt.powered.then_some(Message::Quick(QuickMsg::Page(Page::BtAdd)))).into(),
                widget::Space::new().width(Length::Fill).into(),
                link("More Bluetooth settings", "bluetooth"),
            ])
            .align_y(Alignment::Center),
        )
        .padding([8, 20, 14, 20])
        .into(),
    ])
    .spacing(4)
    .into()
}

fn sound_page<'a>(shell: &'a Shell) -> Element<'a, Message> {
    let mut list = column::with_capacity(shell.quick.sinks.len()).spacing(2);
    for s in &shell.quick.sinks {
        let content = row::with_children(vec![
            sym(if s.default { "object-select-symbolic" } else { "audio-speakers-symbolic" }, &["audio-card-symbolic"]).into(),
            text::body(s.description.clone()).width(Length::Fill).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center);
        list = list.push(list_row(content, s.default, Message::Quick(QuickMsg::Sink(s.name.clone()))));
    }
    let mut mixer = column::with_capacity(shell.quick.streams.len() + 1).spacing(10);
    if !shell.quick.streams.is_empty() {
        mixer = mixer.push(text::heading("Volume mixer"));
    }
    for s in &shell.quick.streams {
        let id = if s.binary.is_empty() { s.icon.clone() } else { s.binary.clone() };
        let handle = if shell.catalog.get(&id).is_some() || s.icon.is_empty() {
            crate::taskbar_view::app_icon(shell, &id)
        } else {
            crate::icons::full(&s.icon)
        };
        let (index, muted) = (s.index, s.muted);
        mixer = mixer.push(
            column::with_children(vec![
                text::caption(s.name.clone()).into(),
                row::with_children(vec![
                    button::custom(icon(handle).size(20))
                        .padding(4)
                        .class(style::flat_button(muted))
                        .on_press(Message::Quick(QuickMsg::StreamMute(index, !muted)))
                        .into(),
                    slider(0.0..=1.0, s.level.min(1.0), move |v| Message::Quick(QuickMsg::StreamVolume(index, v)))
                .class(lapin_ux_ui::slider())
                        .step(0.01f32)
                        .width(Length::Fill)
                        .into(),
                ])
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
            ])
            .spacing(4),
        );
    }
    column::with_children(vec![
        container(header("Sound output", None)).padding([12, 12, 4, 12]).into(),
        container(list).padding([0, 12]).into(),
        container(mixer).padding([8, 20, 0, 20]).into(),
        container(link("More volume settings", "sound")).padding([8, 20, 14, 20]).into(),
    ])
    .spacing(4)
    .into()
}

fn page<'a>(title: &'static str, body: impl Into<Element<'a, Message>>, settings: Option<(&'static str, &'static str)>) -> Element<'a, Message> {
    let mut c = column::with_capacity(3).spacing(4);
    c = c.push(container(header(title, None)).padding([12, 12, 4, 12]));
    c = c.push(container(body).padding([0, 12]));
    if let Some((label, p)) = settings {
        c = c.push(container(link(label, p)).padding([8, 20, 14, 20]));
    } else {
        c = c.push(widget::Space::new().height(14));
    }
    c.into()
}

fn switch_row<'a>(icon_name: &'static str, label: &'static str, on: bool, f: impl Fn(bool) -> Message + 'a) -> Element<'a, Message> {
    container(
        row::with_children(vec![
            sym(icon_name, &[]).into(),
            text::body(label).width(Length::Fill).into(),
            toggler(on).on_toggle(f).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding([10, 10])
    .into()
}

fn accessibility_page<'a>(shell: &'a Shell) -> Element<'a, Message> {
    let q = &shell.quick;
    let mut list = column::with_capacity(3).spacing(2);
    if q.a11y.available {
        list = list.push(switch_row("zoom-in-symbolic", "Magnifier", q.a11y.magnifier, |on| Message::Quick(QuickMsg::Magnifier(on))));
        list = list.push(switch_row("color-select-symbolic", "Color filters", q.a11y.filter_on, |on| Message::Quick(QuickMsg::ColorFilter(on))));
    }
    if lapin_ux_core::system::screen_reader_ready() {
        list = list.push(switch_row("audio-speakers-symbolic", "Narrator", q.screen_reader, |on| Message::Quick(QuickMsg::ScreenReader(on))));
    }
    // Contrast themes are off for now: Lapin UX has no contrast themes of its
    // own yet (COSMIC's high-contrast palette didn't work as one).
    page("Accessibility", list, Some(("More accessibility settings", "accessibility")))
}

fn hotspot_page<'a>(shell: &'a Shell) -> Element<'a, Message> {
    let on = shell.quick.network.hotspot;
    let mut c = column::with_capacity(4).spacing(10);
    c = c.push(switch_row("network-wireless-hotspot-symbolic", "Mobile hotspot", on, |on| Message::Quick(QuickMsg::Hotspot(on))));
    let pw = if shell.config.hotspot_password.is_empty() { "Made when you turn it on".to_string() } else { shell.config.hotspot_password.clone() };
    for (k, v) in [("Network name", hostname()), ("Password", pw)] {
        c = c.push(
            container(column::with_children(vec![text::caption(k).class(style::dim_text()).into(), text::body(v).into()]).spacing(2))
                .padding([0, 10]),
        );
    }
    c = c.push(container(text::caption("Shares this computer's internet connection over Wi-Fi.").class(style::dim_text())).padding([0, 10]));
    page("Mobile hotspot", c, Some(("More hotspot settings", "lapin-network")))
}

fn vpn_page<'a>(shell: &'a Shell) -> Element<'a, Message> {
    let mut list = column::with_capacity(shell.quick.network.vpns.len()).spacing(2);
    for v in &shell.quick.network.vpns {
        let up = v.active.is_some();
        let content = row::with_children(vec![
            sym("network-vpn-symbolic", &[]).into(),
            column::with_children(vec![
                text::body(v.name.clone()).into(),
                text::caption(if up { "Connected" } else { "Not connected" }).class(style::dim_text()).into(),
            ])
            .width(Length::Fill)
            .into(),
            text::caption(if up { "Disconnect" } else { "Connect" }).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center);
        list = list.push(list_row(content, up, Message::Quick(QuickMsg::Vpn(v.settings.clone(), v.active.clone()))));
    }
    page("VPN", list, Some(("More VPN settings", "vpn")))
}

fn keyboard_page<'a>(shell: &'a Shell) -> Element<'a, Message> {
    let q = &shell.quick;
    let mut list = column::with_capacity(q.layouts.len()).spacing(2);
    for (i, (code, desc)) in q.layouts.iter().enumerate() {
        let on = i as u32 == q.kb_group;
        let content = row::with_children(vec![
            container(text::body(code.clone())).width(40).into(),
            text::body(desc.clone()).width(Length::Fill).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center);
        list = list.push(list_row(content, on, Message::Quick(QuickMsg::KeyboardLayout(i as u32))));
    }
    page("Keyboard layout", list, Some(("More keyboard settings", "keyboard")))
}

/// Pairing: nearby devices, and the agent's questions.
fn bt_add_page<'a>(shell: &'a Shell) -> Element<'a, Message> {
    let q = &shell.quick;
    let mut c = column::with_capacity(4).spacing(8);
    if let Some(p) = &q.bt_prompt {
        let who = if p.device.is_empty() { "The device".to_string() } else { p.device.clone() };
        let mut card = column::with_capacity(4).spacing(10);
        let yes_no = |yes: &'static str| {
            row::with_children(vec![
                widget::Space::new().width(Length::Fill).into(),
                lapin_ux_ui::button::standard("Cancel").on_press(Message::Quick(QuickMsg::BtAnswer(false))).into(),
                lapin_ux_ui::button::suggested(yes).on_press(Message::Quick(QuickMsg::BtAnswer(true))).into(),
            ])
            .spacing(8)
        };
        match &p.ask {
            bt_agent::Ask::Confirm(code) => {
                card = card
                    .push(text::body(format!("Make sure this PIN matches the one on {who}.")))
                    .push(text::title3(format!("{code:06}")))
                    .push(yes_no("Connect"));
            }
            bt_agent::Ask::Display(code) => {
                card = card.push(text::body(format!("Type this PIN on {who}, then press Enter there."))).push(text::title3(code.clone()));
            }
            bt_agent::Ask::EnterPin => {
                card = card
                    .push(text::body(format!("Enter the PIN for {who}.")))
                    .push(
                        text_input::text_input("PIN", &q.bt_pin)
                            .on_input(|s| Message::Quick(QuickMsg::BtPin(s)))
                            .on_submit(|_| Message::Quick(QuickMsg::BtAnswer(true))),
                    )
                    .push(yes_no("Connect"));
            }
            bt_agent::Ask::Authorize => {
                card = card.push(text::body(format!("Allow {who} to connect?"))).push(yes_no("Allow"));
            }
        }
        c = c.push(container(card).padding(12).class(style::folder_card()));
    } else if let Some(r) = &q.pair_result {
        c = c.push(container(text::body(r.clone())).padding([4, 10]));
    }
    let bt = &q.bluetooth;
    if bt.nearby.is_empty() {
        let msg = if bt.discovering { "Looking for devices… Make sure yours is turned on and ready to pair." } else { "Starting the search…" };
        c = c.push(container(text::body(msg).class(style::dim_text())).padding(10));
    }
    let mut list = column::with_capacity(bt.nearby.len()).spacing(2);
    for d in &bt.nearby {
        let busy = q.pairing.as_deref() == Some(d.path.as_str());
        let icon_name = if d.icon.is_empty() { "bluetooth-active-symbolic".to_string() } else { format!("{}-symbolic", d.icon) };
        let content = row::with_children(vec![
            sym(&icon_name, &["bluetooth-active-symbolic"]).into(),
            text::body(d.name.clone()).width(Length::Fill).into(),
            text::caption(if busy { "Connecting…" } else { "" }).class(style::dim_text()).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center);
        let msg = (q.pairing.is_none()).then(|| Message::Quick(QuickMsg::BtPair(d.path.clone())));
        list = list.push(button::custom(content).width(Length::Fill).padding([8, 10]).class(style::flat_button(busy)).on_press_maybe(msg));
    }
    c = c.push(scrollable(list).height(Length::Shrink));
    column::with_children(vec![
        container(
            row::with_children(vec![
                button::custom(sym("go-previous-symbolic", &[]))
                    .padding(8)
                    .class(style::flat_button(false))
                    .on_press(Message::Quick(QuickMsg::Page(Page::Bluetooth)))
                    .into(),
                text::title4("Add a device").into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([12, 12, 4, 12])
        .into(),
        container(c).padding([0, 12, 14, 12]).into(),
    ])
    .into()
}

pub fn view<'a>(shell: &'a Shell, p: &'a QuickPanel) -> Element<'a, Message> {
    match p.page {
        Page::BtAdd => bt_add_page(shell),
        Page::Vpn => vpn_page(shell),
        Page::Hotspot => hotspot_page(shell),
        Page::Keyboard => keyboard_page(shell),
        Page::Accessibility => accessibility_page(shell),
        Page::Main => main_page(shell, p),
        Page::Wifi => wifi_page(shell, p),
        Page::Bluetooth => bluetooth_page(shell),
        Page::Sound => sound_page(shell),
    }
}
