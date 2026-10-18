// SPDX-License-Identifier: GPL-3.0-only
//! Quick settings: the network / volume / battery button left of the clock
//! and the panel it opens (Super+A): Wi-Fi, Bluetooth, airplane mode, energy
//! saver and Do not disturb tiles, brightness and volume sliders, battery.

pub mod backend;

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
    pub backlight: Option<(String, Brightness)>,
    /// Last time a slider sent a value to the system (throttling).
    last_set: Option<Instant>,
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
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuickPanel {
    pub page: Page,
    /// Wi-Fi network whose row is expanded (Connect / password).
    pub selected: Option<String>,
    pub password: String,
}

#[derive(Debug, Clone)]
pub enum QuickMsg {
    Toggle,
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
    Settings(&'static str),
    Failed(String),
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
                let close_centre = Task::future(async {
                    crate::notify_client::close_center().await;
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([close_centre, self.open_panel(PanelKind::Quick(QuickPanel::default()))]);
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
            },
            QuickMsg::Page(page) => {
                let scan = page == Page::Wifi && !q.network.wifi_device.is_empty();
                let device = q.network.wifi_device.clone();
                if let Some(p) = self.quick_panel() {
                    p.page = page;
                    p.selected = None;
                    p.password.clear();
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
                let step = if dy > 0.0 { 0.02 } else { -0.02 };
                let v = (q.volume.level + step).clamp(0.0, 1.0);
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
            QuickMsg::Settings(page) => {
                let close = self.close_panel();
                return Task::batch([close, self.open_settings(page)]);
            }
            QuickMsg::Failed(e) => q.error = Some(e),
        }
        Task::none()
    }

    /// Open the system settings app, on a page if it supports that.
    pub(crate) fn open_settings(&mut self, page: &str) -> Task<Message> {
        let mut cmd = std::process::Command::new("cosmic-settings");
        if !page.is_empty() {
            cmd.arg(page);
        }
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
        if p.page == Page::Main {
            return None;
        }
        p.page = Page::Main;
        Some(Task::none())
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
    button::text(label).on_press(Message::Quick(QuickMsg::Settings(page))).into()
}

fn list_row<'a>(content: impl Into<Element<'a, Message>>, selected: bool, msg: Message) -> Element<'a, Message> {
    button::custom(content).width(Length::Fill).padding([8, 10]).class(style::flat_button(selected)).on_press(msg).into()
}

fn main_page<'a>(shell: &'a Shell) -> Element<'a, Message> {
    let q = &shell.quick;
    let mut tiles: Vec<Element<'a, Message>> = Vec::new();
    if q.network.wifi_present {
        let label = match (&q.network.link, q.network.wifi_enabled) {
            (model::Link::Wifi { .. }, _) => q.network.link_name.clone(),
            (_, true) => "Wi-Fi".into(),
            (_, false) => "Wi-Fi".into(),
        };
        let icon = if q.network.wifi_enabled { "network-wireless-signal-excellent-symbolic" } else { "network-wireless-offline-symbolic" };
        tiles.push(tile(
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
        tiles.push(tile(
            if on { "bluetooth-active-symbolic" } else { "bluetooth-disabled-symbolic" },
            label,
            on,
            Some(Message::Quick(QuickMsg::Bluetooth(!on))),
            Some(Message::Quick(QuickMsg::Page(Page::Bluetooth))),
        ));
    }
    if q.network.wifi_present || q.bluetooth.available {
        let airplane = !q.network.wifi_enabled && (!q.bluetooth.available || !q.bluetooth.powered);
        tiles.push(tile("airplane-mode-symbolic", "Airplane mode", airplane, Some(Message::Quick(QuickMsg::Airplane(!airplane))), None));
    }
    if let Some(p) = &q.power.profile {
        let on = p == "power-saver";
        tiles.push(tile(
            "power-profile-power-saver-symbolic",
            "Energy saver",
            on,
            Some(Message::Quick(QuickMsg::EnergySaver(!on))),
            None,
        ));
    }
    if shell.notify.available {
        let on = shell.notify.dnd;
        tiles.push(tile("notification-disabled-symbolic", "Do not disturb", on, Some(Message::Quick(QuickMsg::DoNotDisturb(!on))), None));
    }

    let mut grid = column::with_capacity(3).spacing(14);
    let mut it = tiles.into_iter().peekable();
    while it.peek().is_some() {
        let r: Vec<Element<'a, Message>> = it.by_ref().take(3).collect();
        grid = grid.push(row::with_children(r).spacing(12));
    }

    let mut body = column::with_capacity(6).spacing(18).padding([20, 20, 12, 20]).push(grid);
    if let Some((_, b)) = &q.backlight {
        body = body.push(
            row::with_children(vec![
                sym("display-brightness-symbolic", &[]).into(),
                slider(0.0..=1.0, b.fraction(), |v| Message::Quick(QuickMsg::Brightness(v)))
                    .step(0.01f32)
                    .on_release(Message::Quick(QuickMsg::BrightnessDone))
                    .width(Length::Fill)
                    .into(),
            ])
            .spacing(12)
            .padding([0, 8])
            .align_y(Alignment::Center),
        );
    }
    if q.volume.available {
        let mut r = row::with_capacity(3).spacing(4).align_y(Alignment::Center);
        r = r.push(
            button::custom(sym(model::volume_icon(&q.volume), &["audio-volume-high-symbolic"]))
                .padding(4)
                .class(style::flat_button(false))
                .on_press(Message::Quick(QuickMsg::Mute)),
        );
        r = r.push(
            slider(0.0..=1.0, q.volume.level.min(1.0), |v| Message::Quick(QuickMsg::Volume(v)))
                .step(0.01f32)
                .on_release(Message::Quick(QuickMsg::VolumeDone))
                .width(Length::Fill),
        );
        if q.sinks.len() > 1 {
            r = r.push(
                button::custom(sym("pan-end-symbolic", &["go-next-symbolic"]))
                    .padding(6)
                    .class(style::flat_button(false))
                    .on_press(Message::Quick(QuickMsg::Page(Page::Sound))),
            );
        }
        body = body.push(container(r).padding([0, 4]));
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
    let footer = container(
        row::with_children(vec![
            container(battery).width(Length::Fill).into(),
            button::custom(sym("emblem-system-symbolic", &["preferences-system-symbolic"]))
                .padding(8)
                .class(style::flat_button(false))
                .on_press(Message::Quick(QuickMsg::Settings("")))
                .into(),
        ])
        .align_y(Alignment::Center),
    )
    .padding([6, 12, 6, 20])
    .class(style::panel_footer());
    column::with_children(vec![body.into(), footer.into()]).into()
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
                        button::standard("Disconnect").on_press(Message::Quick(QuickMsg::Disconnect)).into(),
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
                    button::suggested("Connect").on_press(Message::Quick(QuickMsg::Connect)).into(),
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
        container(link("Add a device or more Bluetooth settings", "bluetooth")).padding([8, 20, 14, 20]).into(),
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
    column::with_children(vec![
        container(header("Sound output", None)).padding([12, 12, 4, 12]).into(),
        container(list).padding([0, 12]).into(),
        container(link("More volume settings", "sound")).padding([8, 20, 14, 20]).into(),
    ])
    .spacing(4)
    .into()
}

pub fn view<'a>(shell: &'a Shell, p: &'a QuickPanel) -> Element<'a, Message> {
    match p.page {
        Page::Main => main_page(shell),
        Page::Wifi => wifi_page(shell, p),
        Page::Bluetooth => bluetooth_page(shell),
        Page::Sound => sound_page(shell),
    }
}
