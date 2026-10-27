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
    /// A page of Settings: a COSMIC page or a Lapin page id (`lapin-…`).
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
        target: Target::Settings("lapin-system"),
        tasks: &[s("Review your computer's status", "about"), s("Power options", "power"), r("Task Manager", "lapin-ux-taskmgr")],
    },
    Category {
        title: "Network and Internet",
        icon: "preferences-system-network",
        target: Target::Settings("lapin-network"),
        tasks: &[s("View network status and tasks", "lapin-network"), s("Wi-Fi", "wireless"), s("VPN", "vpn")],
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
        target: Target::Settings("lapin-apps"),
        tasks: &[s("Uninstall a program", "lapin-installed-apps"), s("Check for updates", "lapin-update"), s("Default programs", "default-apps"), s("Startup apps", "startup-apps")],
    },
    Category {
        title: "User Accounts",
        icon: "system-users",
        target: Target::Settings("lapin-accounts"),
        tasks: &[s("Change account type", "users")],
    },
    Category {
        title: "Appearance and Personalization",
        icon: "preferences-desktop-theme",
        target: Target::Settings("lapin-personalization"),
        tasks: &[s("Change the theme", "appearance"), s("Change desktop background", "wallpaper"), s("Taskbar and navigation", "lapin-taskbar")],
    },
    Category {
        title: "Clock and Region",
        icon: "preferences-system-time",
        target: Target::Settings("lapin-time"),
        tasks: &[s("Change date, time, or number formats", "region-language"), s("Set the time and date", "date-time")],
    },
    Category {
        title: "Ease of Access",
        icon: "preferences-desktop-accessibility",
        target: Target::Settings("lapin-accessibility"),
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
    ("Network and Sharing Center", "preferences-system-network", Target::Settings("lapin-network")),
    ("Personalization", "preferences-desktop-theme", Target::Settings("lapin-personalization")),
    ("Power Options", "preferences-system-power", Target::Settings("power")),
    ("Programs and Features", "system-software-install", Target::Settings("lapin-installed-apps")),
    ("Update", "system-software-update", Target::Settings("lapin-update")),
    ("Region", "preferences-desktop-locale", Target::Settings("region-language")),
    ("Sound", "audio-card", Target::Settings("sound")),
    ("Startup Apps", "system-run", Target::Settings("startup-apps")),
    ("System", "computer", Target::Settings("about")),
    ("Task Manager", "utilities-system-monitor", Target::Run("lapin-ux-taskmgr")),
    ("Touchpad", "input-touchpad", Target::Settings("touchpad")),
    ("User Accounts", "system-users", Target::Settings("users")),
    ("Wallpaper", "preferences-desktop-wallpaper", Target::Settings("wallpaper")),
    ("Wi-Fi", "network-wireless", Target::Settings("wireless")),
    ("Taskbar and Navigation", "user-desktop", Target::Settings("lapin-taskbar")),
    ("Windows and Desktops", "preferences-system-windows", Target::Settings("lapin-multitasking")),
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
        Target::Settings(page) => {
            // Lapin pages (lapin-…) open with `cosmic-settings lapin <id>`.
            let argv = lapin_ux_core::system::settings_command(page, lapin_ux_core::system::lapin_settings_installed());
            std::process::Command::new(&argv[0]).args(&argv[1..]).spawn()
        }
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

/// Every category's links get the width of "Hardware and Sound", as in
/// Windows; longer names wrap onto the next line.
const LINKS_WIDTH: f32 = 206.0;

fn link<'a>(label: &'a str, t: Target, big: bool) -> Element<'a, Message> {
    // Tight lines, as in Windows: a wrapped title reads as one block.
    use cosmic::iced::widget::text::LineHeight;
    let txt: Element<'a, Message> = if big {
        text::title4(label).line_height(LineHeight::Relative(1.15)).width(Length::Fill).into()
    } else {
        text::body(label).line_height(LineHeight::Relative(1.2)).width(Length::Fill).into()
    };
    button::custom(txt)
        .width(Length::Fill)
        .padding([1, 0])
        .class(theme::Button::Link)
        .on_press_maybe(available(t).then_some(Message::Open(t)))
        .into()
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
                .class(lapin_ux_ui::text())
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
                        row::with_children(vec![icon(win11_icon(ic)).size(32).into(), text::body(*name).into()]).spacing(12).align_y(Alignment::Center),
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
                    let mut links = column::with_capacity(c.tasks.len() + 2).spacing(0).push(link(c.title, c.target, true)).push(widget::Space::new().height(2));
                    for t in c.tasks {
                        links = links.push(link(t.label, t.target, false));
                    }
                    row::with_children(vec![icon(win11_icon(c.icon)).size(48).into(), links.width(Length::Fixed(LINKS_WIDTH)).into()])
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

/// Control Panel's pictures are the full-colour Win11 icons, as in Windows,
/// not the Fluent glyphs the Lapin UX icon theme uses for small symbolic
/// icons. Looks in the installed Win11 theme (the dark variant first in dark
/// mode) and falls back to a normal theme lookup.
fn win11_icon(name: &str) -> icon::Handle {
    static DIRS: std::sync::LazyLock<Vec<std::path::PathBuf>> = std::sync::LazyLock::new(|| {
        let mut bases: Vec<std::path::PathBuf> = Vec::new();
        if let Some(d) = dirs::data_dir() {
            bases.push(d.join("icons"));
        }
        let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        bases.extend(data.split(':').filter(|d| !d.is_empty()).map(|d| std::path::Path::new(d).join("icons")));
        let themes: &[&str] = if cosmic::theme::is_dark() { &["Win11-dark", "Win11"] } else { &["Win11"] };
        let mut out = Vec::new();
        for theme in themes {
            for base in &bases {
                for ctx in ["apps", "preferences", "devices", "places", "categories", "status", "actions"] {
                    for size in ["scalable", "48", "32", "24"] {
                        out.push(base.join(theme).join(ctx).join(size));
                    }
                }
            }
        }
        out
    });
    DIRS.iter()
        .map(|d| d.join(format!("{name}.svg")))
        .find(|p| p.exists())
        .map(icon::from_path)
        .unwrap_or_else(|| icon::from_name(name).handle())
}

fn main() -> cosmic::iced::Result {
    lapin_ux_core::crash::install("lapin-ux-control");
    cosmic::app::run::<App>(Settings::default().size(Size::new(900.0, 620.0)), ())
}
