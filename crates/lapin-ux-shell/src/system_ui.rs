// SPDX-License-Identifier: GPL-3.0-only
//! System screens bound to familiar keys:
//! - Alt+F4 with no window in front: the "Shut Down" dialog.
//! - Win+P: the "Project" flyout (PC screen only / Duplicate / Extend /
//!   Second screen only), through `cosmic-randr`.
//! - Ctrl+Alt+Del: the full-screen security options (Lock, Switch user,
//!   Sign out, Change a password, Task Manager).

use cosmic::app::Task;
use cosmic::iced::{Alignment, Background, Border, Color, Length};
use cosmic::widget::{self, button, column, container, icon, row, text};
use cosmic::{Application, Element, theme};
use lapin_ux_core::display::{self, Head, Project};

use crate::app::{Message, PowerAction, Shell};
use crate::config::{Role, find_app};
use crate::panels::{Panel, PanelKind};
use crate::style;

pub const SHUTDOWN_WIDTH: u32 = 400;
pub const PROJECT_WIDTH: u32 = 340;

/// What "Shut Down" can do, in the dialog's order.
pub const SHUTDOWN_CHOICES: [(&str, &str); 6] = [
    ("Switch user", "Leaves your apps open and lets someone else sign in."),
    ("Sign out", "Closes all apps and signs you out."),
    ("Lock", "Locks the screen; your apps stay open."),
    ("Sleep", "The PC stays on but uses low power. Apps stay open so you can pick up where you left off."),
    ("Shut down", "Closes all apps and turns off the PC."),
    ("Restart", "Closes all apps, turns off the PC, and then turns it on again."),
];

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPanel {
    pub heads: Vec<Head>,
    pub selected: Project,
    pub loaded: bool,
}

#[derive(Debug, Clone)]
pub enum SystemMsg {
    /// Alt+F4: close the window in front, or the Shut Down dialog on the desktop.
    AltF4,
    ShutDownPick(usize),
    ShutDownOk,
    /// Win+P (again: next choice).
    Project,
    ProjectHeads(Vec<Head>),
    ProjectApply(Project),
    ProjectDone(Result<(), String>),
    /// Ctrl+Alt+Del.
    Security,
    /// The power button in the corner: show or hide Sleep / Shut down / Restart.
    SecurityPower,
    SecurityDo(SecurityAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityAction {
    Lock,
    SwitchUser,
    SignOut,
    ChangePassword,
    TaskManager,
    Network,
    Accessibility,
    Sleep,
    ShutDown,
    Restart,
}

async fn randr(args: &[String]) -> Result<String, String> {
    let out = tokio::process::Command::new("cosmic-randr")
        .args(args)
        .output()
        .await
        .map_err(|e| format!("cosmic-randr isn't available ({e})"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

impl Shell {
    fn shutdown_choice(&mut self) -> Option<&mut usize> {
        match &mut self.panel {
            Some(Panel { kind: PanelKind::ShutDown { choice }, .. }) => Some(choice),
            _ => None,
        }
    }

    fn project_panel(&mut self) -> Option<&mut ProjectPanel> {
        match &mut self.panel {
            Some(Panel { kind: PanelKind::Project(p), .. }) => Some(p),
            _ => None,
        }
    }

    pub(crate) fn update_system(&mut self, msg: SystemMsg) -> Task<Message> {
        match msg {
            SystemMsg::AltF4 => {
                // A window in front (and the desktop not clicked since): close it.
                let active = self.taskbar.windows().iter().find(|w| w.active).map(|w| w.handle);
                match active {
                    Some(h) if !self.desktop_focused => return self.update(Message::CloseWindow(h)),
                    _ => {
                        if matches!(self.panel.as_ref().map(|p| &p.kind), Some(PanelKind::ShutDown { .. })) {
                            return Task::none();
                        }
                        // Shut down is preselected, as on Windows.
                        return self.open_panel(PanelKind::ShutDown { choice: 4 });
                    }
                }
            }
            SystemMsg::ShutDownPick(i) => {
                if let Some(c) = self.shutdown_choice() {
                    *c = i.min(SHUTDOWN_CHOICES.len() - 1);
                }
            }
            SystemMsg::ShutDownOk => {
                let Some(c) = self.shutdown_choice().map(|c| *c) else { return Task::none() };
                let close = self.close_panel();
                let next = match c {
                    0 => self.update(Message::Account(crate::account::AccountMsg::SwitchUser)),
                    1 => self.update(Message::Power(PowerAction::SignOut)),
                    2 => self.update(Message::Power(PowerAction::Lock)),
                    3 => self.update(Message::Power(PowerAction::Sleep)),
                    4 => self.update(Message::Power(PowerAction::ShutDown)),
                    _ => self.update(Message::Power(PowerAction::Restart)),
                };
                return Task::batch([close, next]);
            }
            SystemMsg::Project => {
                // Pressing Win+P again moves to the next choice, as on Windows.
                if let Some(p) = self.project_panel() {
                    let i = Project::ALL.iter().position(|x| *x == p.selected).unwrap_or(0);
                    p.selected = Project::ALL[(i + 1) % Project::ALL.len()];
                    return Task::none();
                }
                let open = self.open_panel(PanelKind::Project(ProjectPanel { heads: Vec::new(), selected: Project::PcOnly, loaded: false }));
                let load = Task::future(async {
                    let heads = randr(&["list".into(), "--kdl".into()]).await.map(|t| display::parse_kdl(&t)).unwrap_or_default();
                    cosmic::Action::App(Message::System(SystemMsg::ProjectHeads(heads)))
                });
                return Task::batch([open, load]);
            }
            SystemMsg::ProjectHeads(heads) => {
                let current = display::current_project(&heads).unwrap_or(Project::PcOnly);
                if let Some(p) = self.project_panel() {
                    p.selected = current;
                    p.heads = heads;
                    p.loaded = true;
                }
            }
            SystemMsg::ProjectApply(choice) => {
                let Some(p) = self.project_panel() else { return Task::none() };
                p.selected = choice;
                let cmds = display::project_commands(&p.heads, choice);
                let close = self.close_panel();
                let run = Task::future(async move {
                    let mut res = Ok(());
                    for c in cmds {
                        if let Err(e) = randr(&c).await {
                            res = Err(e);
                            break;
                        }
                    }
                    cosmic::Action::App(Message::System(SystemMsg::ProjectDone(res)))
                });
                return Task::batch([close, run]);
            }
            SystemMsg::ProjectDone(res) => {
                if let Err(e) = res {
                    tracing::warn!("display switch failed: {e}");
                    return Task::future(async move {
                        crate::start_search::notify("Couldn't change the display setup", &e).await;
                        cosmic::Action::App(Message::Noop)
                    });
                }
            }
            SystemMsg::Security => {
                if matches!(self.panel.as_ref().map(|p| &p.kind), Some(PanelKind::Security { .. })) {
                    return self.close_panel();
                }
                return self.open_panel(PanelKind::Security { power: false });
            }
            SystemMsg::SecurityPower => {
                if let Some(Panel { kind: PanelKind::Security { power }, .. }) = &mut self.panel {
                    *power = !*power;
                }
            }
            SystemMsg::SecurityDo(a) => {
                let close = self.close_panel();
                let next = match a {
                    SecurityAction::Lock => self.update(Message::Power(PowerAction::Lock)),
                    SecurityAction::SwitchUser => self.update(Message::Account(crate::account::AccountMsg::SwitchUser)),
                    SecurityAction::SignOut => self.update(Message::Power(PowerAction::SignOut)),
                    SecurityAction::ChangePassword => self.open_settings("users"),
                    SecurityAction::TaskManager => match find_app(&self.catalog, Role::TaskManager) {
                        Some(id) => self.update(Message::Launch(id)),
                        None => Task::none(),
                    },
                    SecurityAction::Network => self.update(Message::Quick(crate::quick::QuickMsg::Toggle)),
                    SecurityAction::Accessibility => self.open_settings("accessibility"),
                    SecurityAction::Sleep => self.update(Message::Power(PowerAction::Sleep)),
                    SecurityAction::ShutDown => self.update(Message::Power(PowerAction::ShutDown)),
                    SecurityAction::Restart => self.update(Message::Power(PowerAction::Restart)),
                };
                return Task::batch([close, next]);
            }
        }
        Task::none()
    }

    /// Arrow keys / Enter in these screens.
    pub(crate) fn system_key(&mut self, key: cosmic::iced::keyboard::key::Named) -> Option<Task<Message>> {
        use cosmic::iced::keyboard::key::Named;
        match self.panel.as_ref().map(|p| &p.kind) {
            Some(PanelKind::ShutDown { choice }) => {
                let c = *choice;
                match key {
                    Named::ArrowDown => Some(self.update_system(SystemMsg::ShutDownPick(c + 1))),
                    Named::ArrowUp => Some(self.update_system(SystemMsg::ShutDownPick(c.saturating_sub(1)))),
                    Named::Enter => Some(self.update_system(SystemMsg::ShutDownOk)),
                    _ => None,
                }
            }
            Some(PanelKind::Project(p)) => {
                let i = Project::ALL.iter().position(|x| *x == p.selected).unwrap_or(0);
                let n = Project::ALL.len();
                match key {
                    Named::ArrowDown => {
                        let next = Project::ALL[(i + 1) % n];
                        self.project_panel().map(|p| p.selected = next);
                        Some(Task::none())
                    }
                    Named::ArrowUp => {
                        let next = Project::ALL[(i + n - 1) % n];
                        self.project_panel().map(|p| p.selected = next);
                        Some(Task::none())
                    }
                    Named::Enter => Some(self.update_system(SystemMsg::ProjectApply(p.selected))),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

// ---- views ---------------------------------------------------------------------

static OS: std::sync::LazyLock<lapin_ux_core::system::OsInfo> = std::sync::LazyLock::new(lapin_ux_core::system::os_info);

/// Height of the OS banner at the top of the Shut Down dialog.
const BANNER_HEIGHT: f32 = 72.0;

/// The OS's logo and name on a band, where Windows shows its own banner.
fn os_banner() -> Element<'static, Message> {
    let logo: Element<'static, Message> = match OS.logo.as_deref() {
        Some(name) => icon::from_name(name).size(40).into(),
        None => icon(icon::from_svg_bytes(crate::taskbar_view::START_ICON)).size(40).into(),
    };
    container(
        row::with_children(vec![logo, text::title3(OS.pretty_name.as_str()).font(cosmic::font::default()).into()])
            .spacing(14)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(BANNER_HEIGHT))
    .padding([0, 24])
    .align_y(Alignment::Center)
    .class(theme::Container::custom(|t| {
        let c = t.cosmic();
        let accent: Color = c.accent_color().into();
        let deep = Color { r: accent.r * 0.35, g: accent.g * 0.35, b: accent.b * 0.45, a: 1.0 };
        container::Style {
            background: Some(Background::Gradient(
                cosmic::iced::gradient::Linear::new(cosmic::iced::Radians(std::f32::consts::FRAC_PI_2))
                    .add_stop(0.0, deep)
                    .add_stop(1.0, Color { a: 1.0, ..accent })
                    .into(),
            )),
            text_color: Some(Color::WHITE),
            icon_color: Some(Color::WHITE),
            ..Default::default()
        }
    }))
    .into()
}

/// The classic "Shut Down" dialog: a fixed-size window.
pub fn shutdown_view(choice: usize) -> Element<'static, Message> {
    let title = container(
        row::with_children(vec![
            text::body(format!("Shut Down {}", OS.name)).width(Length::Fill).into(),
            button::custom(icon::from_name("window-close-symbolic").size(16))
                .padding(6)
                .class(style::flat_button(false))
                .on_press(Message::Panel(crate::panels::PanelMsg::Close))
                .into(),
        ])
        .align_y(Alignment::Center),
    )
    .padding([4, 4, 4, 14])
    .class(style::panel_footer());
    let names: Vec<String> = SHUTDOWN_CHOICES.iter().map(|(n, _)| n.to_string()).collect();
    let names: &'static [String] = Box::leak(names.into_boxed_slice());
    let pick = widget::dropdown(names, Some(choice), |i| Message::System(SystemMsg::ShutDownPick(i))).width(Length::Fill);
    let body = column::with_children(vec![
        text::body("What do you want the computer to do?").into(),
        pick.into(),
        // Two lines reserved so the window doesn't change size between choices.
        container(text::caption(SHUTDOWN_CHOICES[choice.min(SHUTDOWN_CHOICES.len() - 1)].1).class(style::dim_text()))
            .height(Length::Fixed(36.0))
            .into(),
        widget::Space::new().height(Length::Fill).into(),
        row::with_children(vec![
            widget::Space::new().width(Length::Fill).into(),
            lapin_ux_ui::button::sized("OK", 88.0, lapin_ux_ui::suggested()).on_press(Message::System(SystemMsg::ShutDownOk)).into(),
            lapin_ux_ui::button::sized("Cancel", 88.0, lapin_ux_ui::standard()).on_press(Message::Panel(crate::panels::PanelMsg::Close)).into(),
        ])
        .spacing(8)
        .into(),
    ])
    .spacing(10)
    .padding([16, 24, 20, 24])
    .height(Length::Fixed(SHUTDOWN_BODY_HEIGHT));
    column::with_children(vec![title.into(), os_banner(), body.into()]).into()
}

/// Everything under the banner, so the dialog keeps one size.
const SHUTDOWN_BODY_HEIGHT: f32 = 196.0;

/// A tiny drawing of each Win+P choice: one or two screens.
fn project_icon(p: Project) -> Element<'static, Message> {
    let screen = |filled: bool| -> Element<'static, Message> {
        container(widget::Space::new().width(26).height(18))
            .class(theme::Container::custom(move |t| {
                let c = t.cosmic();
                let fg: Color = c.on_bg_color().into();
                container::Style {
                    background: filled.then(|| Background::Color(Color { a: 0.85, ..c.accent_color().into() })),
                    border: Border { color: fg, width: 1.5, radius: 2.0.into() },
                    ..Default::default()
                }
            }))
            .into()
    };
    let one = |left: bool| -> Element<'static, Message> {
        row::with_children(vec![screen(left), screen(!left)]).spacing(4).into()
    };
    match p {
        Project::PcOnly => one(true),
        Project::SecondOnly => one(false),
        Project::Duplicate => row::with_children(vec![screen(true), screen(true)]).spacing(4).into(),
        Project::Extend => row::with_children(vec![screen(true), screen(true)]).spacing(0).into(),
    }
}

pub fn project_view(p: &ProjectPanel) -> Element<'_, Message> {
    let single = p.loaded && p.heads.len() < 2;
    let mut col = column::with_capacity(7).spacing(4).padding(12);
    col = col.push(container(text::heading("Project")).padding([4, 8, 8, 8]));
    for choice in Project::ALL {
        let label = column::with_children(vec![text::body(choice.label()).into()]);
        col = col.push(
            button::custom(
                row::with_children(vec![container(project_icon(choice)).width(64).into(), label.into()])
                    .spacing(14)
                    .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([10, 12])
            .class(style::flat_button(choice == p.selected))
            .on_press_maybe((!single || choice == Project::PcOnly).then_some(Message::System(SystemMsg::ProjectApply(choice)))),
        );
    }
    if single {
        col = col.push(container(text::caption("Only one display is connected.").class(style::dim_text())).padding([6, 12]));
    }
    col.into()
}

fn security_link(label: &'static str, a: SecurityAction) -> Element<'static, Message> {
    // Regular weight, as on Windows.
    button::custom(text::title4(label).font(cosmic::font::default()))
        .padding([8, 14])
        .class(style::flat_button(false))
        .on_press(Message::System(SystemMsg::SecurityDo(a)))
        .into()
}

fn corner_button(icon_name: &'static str, a: SecurityAction) -> Element<'static, Message> {
    button::custom(icon::from_name(icon_name).size(20))
        .padding(10)
        .class(style::flat_button(false))
        .on_press(Message::System(SystemMsg::SecurityDo(a)))
        .into()
}

/// Ctrl+Alt+Del: a full-screen list of security options.
pub fn security_view(shell: &Shell, power: bool) -> Element<'_, Message> {
    let mut links = column::with_capacity(5).spacing(6).align_x(Alignment::Start);
    links = links.push(security_link("Lock", SecurityAction::Lock));
    if shell.can_switch_user {
        links = links.push(security_link("Switch user", SecurityAction::SwitchUser));
    }
    links = links.push(security_link("Sign out", SecurityAction::SignOut));
    links = links.push(security_link("Change a password", SecurityAction::ChangePassword));
    if find_app(&shell.catalog, Role::TaskManager).is_some() {
        links = links.push(security_link("Task Manager", SecurityAction::TaskManager));
    }
    let corner = row::with_children(vec![
        corner_button("network-wireless-symbolic", SecurityAction::Network),
        corner_button("preferences-desktop-accessibility-symbolic", SecurityAction::Accessibility),
        button::custom(icon::from_name("system-shutdown-symbolic").size(20))
            .padding(10)
            .class(style::flat_button(power))
            .on_press(Message::System(SystemMsg::SecurityPower))
            .into(),
    ])
    .spacing(6);
    // The power button's menu, above it.
    let power_menu: Element<'_, Message> = if power {
        let item = |label: &'static str, icon_name: &'static str, a: SecurityAction| -> Element<'static, Message> {
            button::custom(row::with_children(vec![icon::from_name(icon_name).size(16).into(), text::body(label).into()]).spacing(12).align_y(Alignment::Center))
                .width(Length::Fill)
                .padding([8, 14])
                .class(style::flat_button(false))
                .on_press(Message::System(SystemMsg::SecurityDo(a)))
                .into()
        };
        container(
            container(
                column::with_children(vec![
                    item("Sleep", "weather-clear-night-symbolic", SecurityAction::Sleep),
                    item("Shut down", "system-shutdown-symbolic", SecurityAction::ShutDown),
                    item("Restart", "system-reboot-symbolic", SecurityAction::Restart),
                ])
                .spacing(2),
            )
            .padding(4)
            .width(200)
            .class(style::panel_bg()),
        )
        .width(Length::Fill)
        .align_x(Alignment::End)
        .padding([0, 24, 8, 0])
        .into()
    } else {
        widget::Space::new().height(0).into()
    };
    // Cancel sits right under the list, lined up with it.
    links = links.push(widget::Space::new().height(18));
    links = links.push(
        container(lapin_ux_ui::button::sized("Cancel", 120.0, lapin_ux_ui::standard()).on_press(Message::Panel(crate::panels::PanelMsg::Close)))
            .padding([0, 14]),
    );
    let bottom = row::with_children(vec![widget::Space::new().width(Length::Fill).into(), corner.into()])
    .align_y(Alignment::Center)
    .padding([0, 24, 24, 24]);
    let backdrop = theme::Container::custom(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.02, 0.05, 0.12, 0.88))),
        text_color: Some(Color::WHITE),
        icon_color: Some(Color::WHITE),
        ..Default::default()
    });
    container(
        column::with_children(vec![
            container(links).width(Length::Fill).height(Length::Fill).center(Length::Fill).into(),
            power_menu,
            bottom.into(),
        ]),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .class(backdrop)
    .into()
}
