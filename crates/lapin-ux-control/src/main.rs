// SPDX-License-Identifier: GPL-3.0-only
//! Lapin UX Control Panel: the classic "Adjust your computer's settings"
//! window. Categories with their common tasks, or every item as large
//! icons; each opens the matching page of Settings or a Lapin UX tool.

use cosmic::app::{Settings, Task};
use cosmic::iced::{Alignment, Length, Size};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text, text_input};
use cosmic::{ApplicationExt, Core, Element, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// A page of COSMIC Settings (`cosmic-settings <page>`).
    Settings(&'static str),
    /// A program, run if it's installed.
    Run(&'static str),
}

struct Task_ {
    label: &'static str,
    target: Target,
}

struct Category {
    title: &'static str,
    icon: &'static str,
    target: Target,
    tasks: &'static [Task_],
}

const fn s(label: &'static str, page: &'static str) -> Task_ {
    Task_ { label, target: Target::Settings(page) }
}

const fn r(label: &'static str, cmd: &'static str) -> Task_ {
    Task_ { label, target: Target::Run(cmd) }
}

const CATEGORIES: &[Category] = &[
    Category {
        title: "System and Security",
        icon: "preferences-system",
        target: Target::Settings("system"),
        tasks: &[s("Review your computer's status", "about"), s("Power options", "power"), r("Task Manager", "lapin-ux-taskmgr")],
    },
    Category {
        title: "Network and Internet",
        icon: "preferences-system-network",
        target: Target::Settings("network"),
        tasks: &[s("View network status and tasks", "network"), s("Wi-Fi", "wireless"), s("VPN", "vpn")],
    },
    Category {
        title: "Hardware and Sound",
        icon: "audio-card",
        target: Target::Settings("sound"),
        tasks: &[s("Sound", "sound"), s("Mouse", "mouse"), s("Display", "displays"), s("Bluetooth devices", "bluetooth")],
    },
    Category {
        title: "Programs",
        icon: "system-software-install",
        target: Target::Settings("applications"),
        tasks: &[r("Uninstall a program", "cosmic-store"), r("Check for updates", "lapin-ux-update"), s("Default programs", "default-apps"), s("Startup apps", "startup-apps")],
    },
    Category {
        title: "User Accounts",
        icon: "system-users",
        target: Target::Settings("users"),
        tasks: &[s("Change account type", "users")],
    },
    Category {
        title: "Appearance and Personalization",
        icon: "preferences-desktop-theme",
        target: Target::Settings("appearance"),
        tasks: &[s("Change the theme", "appearance"), s("Change desktop background", "wallpaper"), s("Window management", "window-management")],
    },
    Category {
        title: "Clock and Region",
        icon: "preferences-system-time",
        target: Target::Settings("time"),
        tasks: &[s("Change date, time, or number formats", "region-language"), s("Set the time and date", "date-time")],
    },
    Category {
        title: "Ease of Access",
        icon: "preferences-desktop-accessibility",
        target: Target::Settings("accessibility"),
        tasks: &[s("Optimize visual display", "accessibility"), s("Magnifier", "accessibility-magnifier"), s("On-screen keyboard", "accessibility-osk")],
    },
];

/// Every item for the "Large icons" view.
const ITEMS: &[(&str, &str, Target)] = &[
    ("Bluetooth Devices", "bluetooth", Target::Settings("bluetooth")),
    ("Date and Time", "preferences-system-time", Target::Settings("date-time")),
    ("Default Programs", "preferences-desktop-default-applications", Target::Settings("default-apps")),
    ("Display", "preferences-desktop-display", Target::Settings("displays")),
    ("Ease of Access Center", "preferences-desktop-accessibility", Target::Settings("accessibility")),
    ("File Explorer", "system-file-manager", Target::Run("cosmic-files")),
    ("Keyboard", "preferences-desktop-keyboard", Target::Settings("keyboard")),
    ("Mouse", "preferences-desktop-peripherals", Target::Settings("mouse")),
    ("Network and Sharing Center", "preferences-system-network", Target::Settings("network")),
    ("Personalization", "preferences-desktop-theme", Target::Settings("appearance")),
    ("Power Options", "preferences-system-power", Target::Settings("power")),
    ("Programs and Features", "system-software-install", Target::Run("cosmic-store")),
    ("Update", "system-software-update", Target::Run("lapin-ux-update")),
    ("Region", "preferences-desktop-locale", Target::Settings("region-language")),
    ("Sound", "audio-card", Target::Settings("sound")),
    ("Startup Apps", "system-run", Target::Settings("startup-apps")),
    ("System", "computer", Target::Settings("about")),
    ("Task Manager", "utilities-system-monitor", Target::Run("lapin-ux-taskmgr")),
    ("Touchpad", "input-touchpad", Target::Settings("touchpad")),
    ("User Accounts", "system-users", Target::Settings("users")),
    ("Wallpaper", "preferences-desktop-wallpaper", Target::Settings("wallpaper")),
    ("Wi-Fi", "network-wireless", Target::Settings("wireless")),
    ("Windows and Desktops", "preferences-system-windows", Target::Settings("window-management")),
];

#[derive(Debug, Clone)]
enum Message {
    Open(Target),
    View(bool),
    Search(String),
    Home,
}

struct App {
    core: Core,
    large_icons: bool,
    search: String,
}

fn installed(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

fn open(t: Target) {
    let result = match t {
        Target::Settings(page) => std::process::Command::new("cosmic-settings").arg(page).spawn(),
        Target::Run(cmd) => std::process::Command::new(cmd).spawn(),
    };
    if let Err(e) = result {
        eprintln!("lapin-ux-control: couldn't open {t:?}: {e}");
    }
}

fn available(t: Target) -> bool {
    match t {
        Target::Settings(_) => installed("cosmic-settings"),
        Target::Run(cmd) => installed(cmd),
    }
}

fn link<'a>(label: &'a str, t: Target, big: bool) -> Element<'a, Message> {
    let txt: Element<'a, Message> = if big { text::title4(label).into() } else { text::body(label).into() };
    button::custom(txt).padding([2, 0]).class(theme::Button::Link).on_press_maybe(available(t).then_some(Message::Open(t))).into()
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "dev.lapinux.ControlPanel";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _: ()) -> (Self, Task<Message>) {
        let mut app = App { core, large_icons: false, search: String::new() };
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Control Panel".into(), id),
            None => Task::none(),
        };
        (app, title)
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![
            button::custom(row::with_children(vec![icon::from_name("go-home-symbolic").size(16).into(), text::body("Control Panel").into()]).spacing(8).align_y(Alignment::Center))
                .padding([4, 10])
                .class(theme::Button::Text)
                .on_press(Message::Home)
                .into(),
        ]
    }

    fn header_end(&self) -> Vec<Element<'_, Message>> {
        vec![text_input::search_input("Search Control Panel", &self.search).on_input(Message::Search).width(240).into()]
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open(t) => open(t),
            Message::View(large) => self.large_icons = large,
            Message::Search(q) => self.search = q,
            Message::Home => {
                self.search.clear();
                self.large_icons = false;
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let q = self.search.trim().to_lowercase();
        let head = row::with_children(vec![
            text::title3(if q.is_empty() { "Adjust your computer's settings" } else { "Search results" }).width(Length::Fill).into(),
            text::body("View by:").into(),
            widget::dropdown(&["Category", "Large icons"], Some(self.large_icons as usize), |i| Message::View(i == 1)).into(),
        ])
        .spacing(10)
        .align_y(Alignment::Center);

        let body: Element<'_, Message> = if self.large_icons || !q.is_empty() {
            let items: Vec<Element<'_, Message>> = ITEMS
                .iter()
                .filter(|(name, ..)| q.is_empty() || name.to_lowercase().contains(&q))
                .map(|(name, ic, t)| {
                    button::custom(
                        row::with_children(vec![icon::from_name(*ic).size(32).into(), text::body(*name).into()]).spacing(12).align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding(8)
                    .class(theme::Button::MenuItem)
                    .on_press_maybe(available(*t).then_some(Message::Open(*t)))
                    .into()
                })
                .collect();
            let mut col = column::with_capacity(items.len() / 3 + 1).spacing(4);
            let mut it = items.into_iter().peekable();
            while it.peek().is_some() {
                let mut chunk: Vec<Element<'_, Message>> = it.by_ref().take(3).collect();
                while chunk.len() < 3 {
                    chunk.push(widget::Space::new().width(Length::Fill).into());
                }
                col = col.push(row::with_children(chunk).spacing(8));
            }
            col.into()
        } else {
            let cards: Vec<Element<'_, Message>> = CATEGORIES
                .iter()
                .map(|c| {
                    let mut links = column::with_capacity(c.tasks.len() + 1).spacing(2).push(link(c.title, c.target, true));
                    for t in c.tasks {
                        links = links.push(link(t.label, t.target, false));
                    }
                    row::with_children(vec![icon::from_name(c.icon).size(48).into(), links.into()])
                        .spacing(16)
                        .width(Length::Fill)
                        .into()
                })
                .collect();
            let mut col = column::with_capacity(4).spacing(28);
            let mut it = cards.into_iter().peekable();
            while it.peek().is_some() {
                let mut pair: Vec<Element<'_, Message>> = it.by_ref().take(2).collect();
                if pair.len() < 2 {
                    pair.push(widget::Space::new().width(Length::Fill).into());
                }
                col = col.push(row::with_children(pair).spacing(32));
            }
            col.into()
        };
        container(column::with_children(vec![head.into(), scrollable(container(body).padding([8, 12, 8, 0])).height(Length::Fill).into()]).spacing(24))
            .padding([16, 32])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn main() -> cosmic::iced::Result {
    cosmic::app::run::<App>(Settings::default().size(Size::new(900.0, 620.0)), ())
}
