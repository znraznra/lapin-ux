// SPDX-License-Identifier: GPL-3.0-only
//! First-run setup, shown once after the first sign-in (like the end of
//! Windows' setup): connect to a network, pick light or dark mode, and
//! optionally get the Windows 11-style pointers. Every step can be skipped.

use cosmic::app::{Settings, Task};
use cosmic::cosmic_config::{self, ConfigSet};
use cosmic::iced::{Alignment, Background, Border, Color, Length, Size};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text, text_input};
use cosmic::{ApplicationExt, Core, Element, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Hello,
    Network,
    Mode,
    Pointers,
    Done,
}

impl Step {
    fn next(self) -> Step {
        match self {
            Step::Hello => Step::Network,
            Step::Network => Step::Mode,
            Step::Mode => Step::Pointers,
            Step::Pointers | Step::Done => Step::Done,
        }
    }
}

#[derive(Debug, Clone)]
struct Wifi {
    ssid: String,
    signal: u8,
    secure: bool,
    active: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Next,
    Finish,
    Networks(Vec<Wifi>),
    Pick(String),
    Password(String),
    Connect,
    Connected(Result<(), String>),
    Dark(bool),
    Pointers,
    PointersDone(bool),
}

struct App {
    core: Core,
    step: Step,
    name: String,
    networks: Vec<Wifi>,
    picked: Option<String>,
    password: String,
    connecting: bool,
    net_error: Option<String>,
    dark: bool,
    pointers: Option<bool>,
    installing_pointers: bool,
}

fn first_name() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    let full = passwd
        .lines()
        .find(|l| l.split(':').next() == Some(user.as_str()))
        .and_then(|l| l.split(':').nth(4))
        .map(|g| g.split(',').next().unwrap_or_default().trim().to_string())
        .filter(|g| !g.is_empty())
        .unwrap_or(user);
    full.split_whitespace().next().unwrap_or_default().to_string()
}

async fn scan() -> Vec<Wifi> {
    let out = tokio::process::Command::new("nmcli")
        .args(["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list"])
        .output()
        .await;
    let Ok(out) = out else { return Vec::new() };
    let mut list: Vec<Wifi> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            // Fields are ':'-separated with '\:' escapes inside the SSID.
            let mut fields = Vec::new();
            let mut cur = String::new();
            let mut esc = false;
            for c in l.chars() {
                match (esc, c) {
                    (true, c) => {
                        cur.push(c);
                        esc = false;
                    }
                    (false, '\\') => esc = true,
                    (false, ':') => fields.push(std::mem::take(&mut cur)),
                    (false, c) => cur.push(c),
                }
            }
            fields.push(cur);
            let ssid = fields.get(1)?.clone();
            (!ssid.is_empty()).then(|| Wifi {
                active: fields.first().is_some_and(|f| f == "*"),
                signal: fields.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
                secure: fields.get(3).is_some_and(|s| !s.is_empty() && s != "--"),
                ssid,
            })
        })
        .collect();
    list.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    list.dedup_by(|a, b| a.ssid == b.ssid);
    list
}

fn set_dark(dark: bool) {
    if let Ok(cfg) = cosmic_config::Config::new("com.system76.CosmicTheme.Mode", 1) {
        let _ = cfg.set("is_dark", dark);
    }
    // Lapin UX's icon themes come in a light and a dark flavour.
    if let Ok(cfg) = cosmic_config::Config::new("com.system76.CosmicTk", 1) {
        let _ = cfg.set("icon_theme", if dark { "Lapin-UX-dark".to_string() } else { "Lapin-UX".to_string() });
    }
    // Swap Lapin UX's default wallpaper for its light/dark twin.
    let home = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    let all = format!("{home}/cosmic/com.system76.CosmicBackground/v1/all");
    if let Ok(ron) = std::fs::read_to_string(&all) {
        let (from, to) = if dark { ("lapin-ux-light.jpg", "lapin-ux-dark.jpg") } else { ("lapin-ux-dark.jpg", "lapin-ux-light.jpg") };
        if ron.contains(from) {
            let _ = std::fs::write(&all, ron.replace(from, to));
        }
    }
}

fn is_dark() -> bool {
    let home = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    std::fs::read_to_string(format!("{home}/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark")).map(|s| s.trim() != "false").unwrap_or(true)
}

/// A big choice card (light / dark).
fn choice<'a>(label: &'a str, selected: bool, preview: Color, ink: Color, msg: Message) -> Element<'a, Message> {
    let sample = container(
        column::with_children(vec![
            container(widget::Space::new().width(Length::Fill).height(10))
                .class(theme::Container::custom(move |_| container::Style {
                    background: Some(Background::Color(Color { a: 0.35, ..ink })),
                    border: Border { radius: 3.0.into(), ..Default::default() },
                    ..Default::default()
                }))
                .into(),
            container(widget::Space::new().width(Length::Fixed(90.0)).height(10))
                .class(theme::Container::custom(move |_| container::Style {
                    background: Some(Background::Color(Color { a: 0.2, ..ink })),
                    border: Border { radius: 3.0.into(), ..Default::default() },
                    ..Default::default()
                }))
                .into(),
        ])
        .spacing(8),
    )
    .padding(18)
    .width(200)
    .height(120)
    .class(theme::Container::custom(move |t| {
        let a: Color = t.cosmic().accent_color().into();
        container::Style {
            background: Some(Background::Color(preview)),
            border: Border { radius: 8.0.into(), width: if selected { 3.0 } else { 1.0 }, color: if selected { a } else { Color { a: 0.3, ..ink } } },
            ..Default::default()
        }
    }));
    button::custom(column::with_children(vec![sample.into(), text::body(label).into()]).spacing(8).align_x(Alignment::Center))
        .padding(6)
        .class(theme::Button::MenuItem)
        .on_press(msg)
        .into()
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "dev.lapinux.Welcome";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _: ()) -> (Self, Task<Message>) {
        let mut app = App {
            core,
            step: Step::Hello,
            name: first_name(),
            networks: Vec::new(),
            picked: None,
            password: String::new(),
            connecting: false,
            net_error: None,
            dark: is_dark(),
            pointers: None,
            installing_pointers: false,
        };
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Welcome".into(), id),
            None => Task::none(),
        };
        (app, title)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Next => {
                self.step = self.step.next();
                if self.step == Step::Network {
                    return Task::future(async { cosmic::Action::App(Message::Networks(scan().await)) });
                }
            }
            Message::Finish => std::process::exit(0),
            Message::Networks(n) => {
                // Already online: nothing to set up here.
                self.networks = n;
            }
            Message::Pick(ssid) => {
                self.picked = Some(ssid);
                self.password.clear();
                self.net_error = None;
            }
            Message::Password(p) => self.password = p,
            Message::Connect => {
                let Some(ssid) = self.picked.clone() else { return Task::none() };
                let password = self.password.clone();
                self.connecting = true;
                return Task::future(async move {
                    let mut c = tokio::process::Command::new("nmcli");
                    c.args(["dev", "wifi", "connect", &ssid]);
                    if !password.is_empty() {
                        c.args(["password", &password]);
                    }
                    let r = match c.output().await {
                        Ok(o) if o.status.success() => Ok(()),
                        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
                        Err(e) => Err(e.to_string()),
                    };
                    cosmic::Action::App(Message::Connected(r))
                });
            }
            Message::Connected(r) => {
                self.connecting = false;
                match r {
                    Ok(()) => {
                        self.step = self.step.next();
                    }
                    Err(e) => self.net_error = Some(if e.is_empty() { "Couldn't connect to this network.".into() } else { e }),
                }
            }
            Message::Dark(d) => {
                self.dark = d;
                set_dark(d);
            }
            Message::Pointers => {
                self.installing_pointers = true;
                return Task::future(async {
                    let ok = tokio::process::Command::new("lapin-ux-cursors").status().await.is_ok_and(|s| s.success());
                    cosmic::Action::App(Message::PointersDone(ok))
                });
            }
            Message::PointersDone(ok) => {
                self.installing_pointers = false;
                self.pointers = Some(ok);
                if ok {
                    self.step = self.step.next();
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let (title, sub, body, primary): (String, String, Element<'_, Message>, Element<'_, Message>) = match self.step {
            Step::Hello => (
                if self.name.is_empty() { "Hi there".into() } else { format!("Hi, {}", self.name) },
                "Let's get a few things ready. It only takes a minute.".into(),
                icon::from_name("dev.lapinux.Shell").size(128).into(),
                button::suggested("Next").on_press(Message::Next).into(),
            ),
            Step::Network => {
                let mut list = column::with_capacity(self.networks.len()).spacing(4);
                if self.networks.is_empty() {
                    list = list.push(text::body("No Wi-Fi networks found. You can connect later from Quick settings."));
                }
                for n in &self.networks {
                    let selected = self.picked.as_deref() == Some(n.ssid.as_str());
                    let strength = match n.signal {
                        0..=30 => "network-wireless-signal-weak-symbolic",
                        31..=55 => "network-wireless-signal-ok-symbolic",
                        56..=80 => "network-wireless-signal-good-symbolic",
                        _ => "network-wireless-signal-excellent-symbolic",
                    };
                    let mut rowc = column::with_capacity(3).spacing(8).push(
                        row::with_children(vec![
                            icon::from_name(strength).size(20).into(),
                            column::with_children(vec![
                                text::body(n.ssid.clone()).into(),
                                text::caption(if n.active { "Connected" } else if n.secure { "Secured" } else { "Open" }).into(),
                            ])
                            .width(Length::Fill)
                            .into(),
                        ])
                        .spacing(12)
                        .align_y(Alignment::Center),
                    );
                    if selected && !n.active {
                        if n.secure {
                            rowc = rowc.push(
                                text_input::secure_input("Enter the network security key", &self.password, None, true)
                                    .on_input(Message::Password)
                                    .on_submit(|_| Message::Connect),
                            );
                        }
                        rowc = rowc.push(
                            row::with_children(vec![
                                widget::Space::new().width(Length::Fill).into(),
                                button::suggested(if self.connecting { "Connecting…" } else { "Connect" })
                                    .on_press_maybe((!self.connecting).then_some(Message::Connect))
                                    .into(),
                            ]),
                        );
                        if let Some(e) = &self.net_error {
                            rowc = rowc.push(text::caption(e.clone()));
                        }
                    }
                    list = list.push(
                        button::custom(container(rowc).padding(10).width(Length::Fill))
                            .width(Length::Fill)
                            .padding(0)
                            .class(if selected { theme::Button::Standard } else { theme::Button::MenuItem })
                            .on_press(Message::Pick(n.ssid.clone())),
                    );
                }
                let online = self.networks.iter().any(|n| n.active);
                (
                    "Let's connect you to a network".into(),
                    "You'll need an internet connection for updates and apps.".into(),
                    scrollable(list).height(Length::Fill).into(),
                    button::suggested(if online { "Next" } else { "Skip for now" }).on_press(Message::Next).into(),
                )
            }
            Step::Mode => (
                "Choose your mode".into(),
                "Change how things look on your screen. You can change it later in Settings.".into(),
                row::with_children(vec![
                    choice("Light", !self.dark, Color::from_rgb8(0xF3, 0xF3, 0xF3), Color::from_rgb8(0x1A, 0x1A, 0x1A), Message::Dark(false)),
                    choice("Dark", self.dark, Color::from_rgb8(0x20, 0x20, 0x20), Color::WHITE, Message::Dark(true)),
                ])
                .spacing(24)
                .into(),
                button::suggested("Next").on_press(Message::Next).into(),
            ),
            Step::Pointers => (
                "Windows 11-style mouse pointers".into(),
                "\"Cursor Concept W11 Tail HDPi\", designed by JepriCreations (deviantart.com/jepricreations). Lapin UX can't include them, so they're downloaded and converted for your own use.".into(),
                match self.pointers {
                    Some(false) => text::body("They couldn't be installed. You can try again later with lapin-ux-cursors.").into(),
                    _ if self.installing_pointers => text::body("Getting them…").into(),
                    _ => button::standard("Get the pointers").on_press(Message::Pointers).into(),
                },
                button::suggested("Next").on_press(Message::Next).into(),
            ),
            Step::Done => (
                "You're all set".into(),
                "Start is in the middle of the taskbar. Press the Windows key any time.".into(),
                icon::from_name("emblem-ok-symbolic").size(96).into(),
                button::suggested("Get started").on_press(Message::Finish).into(),
            ),
        };
        let page = column::with_children(vec![
            text::title1(title).into(),
            text::body(sub).width(Length::Fixed(520.0)).align_x(cosmic::iced::alignment::Horizontal::Center).into(),
            widget::Space::new().height(16).into(),
            container(body).width(Length::Fixed(520.0)).height(Length::Fill).align_x(Alignment::Center).into(),
            row::with_children(vec![widget::Space::new().width(Length::Fill).into(), primary]).width(Length::Fixed(520.0)).into(),
        ])
        .spacing(12)
        .align_x(Alignment::Center);
        container(page).padding([40, 32, 32, 32]).width(Length::Fill).height(Length::Fill).center_x(Length::Fill).into()
    }
}

fn main() -> cosmic::iced::Result {
    cosmic::app::run::<App>(Settings::default().size(Size::new(760.0, 600.0)).resizable(None), ())
}
