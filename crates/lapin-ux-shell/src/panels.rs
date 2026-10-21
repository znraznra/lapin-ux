// SPDX-License-Identifier: GPL-3.0-only
//! Keyboard-invoked panels: the Win+X power-user menu and the Run dialog.
//!
//! These open from a shortcut, where no click gives us an input serial for
//! a popup grab, so they are layer surfaces with exclusive keyboard focus
//! (like the Start menu) and close when they lose focus.

use std::sync::LazyLock;

use cosmic::app::Task;
use cosmic::iced::keyboard::key::Named;
use cosmic::iced::platform_specific::runtime::wayland::CornerRadius;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{IcedMargin, IcedOutput, SctkLayerSurfaceSettings};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer, destroy_layer_surface,
};
use cosmic::iced::{Alignment, Length, Limits, window};
use cosmic::surface::action::{LiveSettings, app_layer_shell};
use cosmic::widget::{self, button, column, container, icon, row, text, text_input};
use cosmic::{Application, Element};

use crate::app::{Message, PowerAction, Shell, StartPage, rect_key};
use crate::config::{Role, find_app};
use crate::style;

static AUTOSIZE: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("lapin-ux-panel-autosize"));
pub(crate) static RUN_INPUT: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("lapin-ux-run-input"));

const WINX_WIDTH: u32 = 280;
const RUN_WIDTH: u32 = 420;

#[derive(Debug, Clone, PartialEq)]
pub enum PanelKind {
    /// Power-user menu. `power` shows the shut down / sign out list.
    WinX { power: bool, selected: usize },
    Run { input: String },
    Quick(crate::quick::QuickPanel),
    Clip(crate::clipboard::ClipPanel),
    AltTab(crate::previews::AltTab),
    TaskView(crate::taskview::TaskView),
    Snap(crate::snap::SnapPanel),
    /// Alt+F4 on the desktop.
    ShutDown { choice: usize },
    /// Win+P.
    Project(crate::system_ui::ProjectPanel),
    /// A file's Properties window.
    Properties(Box<crate::properties::PropsPanel>),
    /// Ctrl+Alt+Del. `power`: the power button's menu is open.
    Security { power: bool },
    /// Win+Shift+S snipping overlay (whole screen).
    Snip(Box<crate::snip::SnipPanel>),
    /// "How do you want to open this file?"
    OpenWith(Box<crate::open_with::OpenWithPanel>),
}

pub struct Panel {
    pub id: window::Id,
    pub kind: PanelKind,
}

#[derive(Debug, Clone)]
pub enum PanelMsg {
    OpenWinX,
    OpenRun,
    Close,
    /// Close the panel, then perform the message.
    Do(Box<Message>),
    WinXPower(bool),
    RunInput(String),
    RunSubmit,
}

/// One menu row: icon, label, action.
pub struct Item {
    pub icon: &'static str,
    pub label: &'static str,
    pub msg: Message,
}

fn item(icon: &'static str, label: &'static str, msg: Message) -> Item {
    Item { icon, label, msg }
}

/// The Win+X entries that make sense on this system (apps that aren't
/// installed are left out). `None` marks a divider.
pub fn winx_items(shell: &Shell, power: bool) -> Vec<Option<Item>> {
    if power {
        return vec![
            Some(item("go-previous-symbolic", "Back", Message::Panel(PanelMsg::WinXPower(false)))),
            None,
            Some(item("system-log-out-symbolic", "Sign out", Message::Power(PowerAction::SignOut))),
            Some(item("system-lock-screen-symbolic", "Lock", Message::Power(PowerAction::Lock))),
            Some(item("weather-clear-night-symbolic", "Sleep", Message::Power(PowerAction::Sleep))),
            Some(item("system-shutdown-symbolic", "Shut down", Message::Power(PowerAction::ShutDown))),
            Some(item("system-reboot-symbolic", "Restart", Message::Power(PowerAction::Restart))),
        ];
    }
    let app = |role| find_app(&shell.catalog, role);
    // As on Windows 11: these open the matching Settings pages.
    let mut v = vec![
        Some(item("view-app-grid-symbolic", "Installed apps", Message::OpenSettings("lapin-installed-apps"))),
        Some(item("battery-symbolic", "Power Options", Message::OpenSettings("power"))),
        Some(item("preferences-system-symbolic", "System", Message::OpenSettings("about"))),
        Some(item("network-wired-symbolic", "Network Connections", Message::OpenSettings("lapin-network"))),
    ];
    if let Some(id) = app(Role::Disks) {
        v.push(Some(item("drive-harddisk-symbolic", "Disk Management", Message::Launch(id))));
    }
    if let Some(id) = app(Role::Terminal) {
        v.push(Some(item("utilities-terminal-symbolic", "Terminal", Message::Launch(id))));
    }
    v.push(None);
    if let Some(id) = app(Role::TaskManager) {
        v.push(Some(item("utilities-system-monitor-symbolic", "Task Manager", Message::Launch(id))));
    }
    v.push(Some(item("emblem-system-symbolic", "Settings", Message::OpenSettings(""))));
    if let Some(id) = app(Role::Files) {
        v.push(Some(item("system-file-manager-symbolic", "File Explorer", Message::Launch(id))));
    }
    v.push(Some(item("system-search-symbolic", "Search", Message::OpenStartAt(StartPage::Home))));
    v.push(Some(item("system-run-symbolic", "Run", Message::Panel(PanelMsg::OpenRun))));
    v.push(None);
    v.push(Some(item("system-shutdown-symbolic", "Shut down or sign out", Message::Panel(PanelMsg::WinXPower(true)))));
    v.push(Some(item("user-desktop-symbolic", "Desktop", Message::ShowDesktop)));
    v
}

/// Selectable rows only (dividers skipped), for keyboard navigation.
fn selectable(shell: &Shell, power: bool) -> Vec<Message> {
    winx_items(shell, power).into_iter().flatten().map(|i| i.msg).collect()
}

/// Run-dialog input: open paths/URLs with the default app, otherwise run
/// it as a command line.
pub fn run_command(input: &str) -> std::process::Command {
    let t = input.trim();
    let home = std::env::var("HOME").unwrap_or_default();
    let expanded = if t == "~" {
        home.clone()
    } else if let Some(rest) = t.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        t.to_string()
    };
    let looks_like_target = expanded.contains("://") || (expanded.starts_with('/') && std::path::Path::new(&expanded).exists());
    if looks_like_target && !expanded.contains(' ') {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(expanded);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(t);
        c
    }
}

impl Shell {
    /// The open panel's surface (or a placeholder id).
    pub(crate) fn panel_id(&self) -> window::Id {
        self.panel.as_ref().map_or(window::Id::NONE, |p| p.id)
    }

    pub(crate) fn close_panel(&mut self) -> Task<Message> {
        match self.panel.take() {
            Some(p) => {
                self.panel_closed_at = std::time::Instant::now();
                // Closing on the "Add a device" page ends the Bluetooth search.
                let stop = if matches!(&p.kind, PanelKind::Quick(q) if q.page == crate::quick::Page::BtAdd) {
                    let adapter = self.quick.bluetooth.adapter.clone();
                    Task::future(async move {
                        let _ = crate::quick::backend::set_discovery(adapter, false).await;
                        cosmic::Action::App(Message::Noop)
                    })
                } else {
                    Task::none()
                };
                Task::batch([stop, destroy_layer_surface(p.id)])
            }
            None => Task::none(),
        }
    }

    pub(crate) fn open_panel(&mut self, kind: PanelKind) -> Task<Message> {
        let mut tasks = vec![self.close_start(), self.close_popup(), self.close_panel()];
        let id = window::Id::unique();
        let above = self.above_bar();
        let (anchor, margin, width) = match &kind {
            PanelKind::WinX { .. } => {
                // Just above the Start button, like the benchmark.
                let left = self
                    .bars
                    .first()
                    .and_then(|b| self.rects.get(&rect_key(b.id, "start")))
                    .map_or(12, |r| r.x.max(0.0) as i32);
                (Anchor::BOTTOM | Anchor::LEFT, IcedMargin { top: 0, right: 0, bottom: above - 4, left }, WINX_WIDTH)
            }
            PanelKind::Run { .. } => (Anchor::BOTTOM | Anchor::LEFT, IcedMargin { top: 0, right: 0, bottom: above, left: 12 }, RUN_WIDTH),
            PanelKind::AltTab(a) => (Anchor::empty(), IcedMargin::default(), crate::previews::switcher_width(a.order.len())),
            // Top centre, where the benchmark shows it over a window's maximize button.
            PanelKind::Snap(_) => (Anchor::TOP, IcedMargin { top: 12, right: 0, bottom: 0, left: 0 }, crate::snap::WIDTH),
            // Everything above the taskbar, which stays usable.
            PanelKind::TaskView(_) => (
                Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                IcedMargin { top: 0, right: 0, bottom: if self.config.auto_hide { 0 } else { style::taskbar_height() as i32 }, left: 0 },
                0,
            ),
            PanelKind::Clip(_) => (Anchor::BOTTOM, IcedMargin { top: 0, right: 0, bottom: above, left: 0 }, crate::clipboard::WIDTH),
            PanelKind::Quick(_) => (
                Anchor::BOTTOM | Anchor::RIGHT,
                IcedMargin { top: 0, right: 12, bottom: above, left: 0 },
                crate::quick::WIDTH,
            ),
            PanelKind::ShutDown { .. } => (Anchor::empty(), IcedMargin::default(), crate::system_ui::SHUTDOWN_WIDTH),
            PanelKind::Properties(_) => (Anchor::empty(), IcedMargin::default(), crate::properties::WIDTH),
            PanelKind::OpenWith(_) => (Anchor::empty(), IcedMargin::default(), crate::open_with::WIDTH),
            PanelKind::Project(_) => (
                Anchor::BOTTOM | Anchor::RIGHT,
                IcedMargin { top: 0, right: 12, bottom: above, left: 0 },
                crate::system_ui::PROJECT_WIDTH,
            ),
            // The whole screen, taskbar included.
            PanelKind::Security { .. } | PanelKind::Snip(_) => (Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT, IcedMargin::default(), 0),
        };
        let focus_run = matches!(kind, PanelKind::Run { .. });
        // Task view covers the whole screen.
        let full = matches!(kind, PanelKind::TaskView(_) | PanelKind::Security { .. } | PanelKind::Snip(_));
        self.panel = Some(Panel { id, kind });
        let r = if full { 0 } else { style::PANEL_RADIUS as u32 };
        tasks.push(cosmic::surface::surface_task(app_layer_shell(
            move |_: &Shell| LiveSettings {
                blur: Some(true),
                corners: Some(CornerRadius { top_left: r, top_right: r, bottom_left: r, bottom_right: r }),
                ..Default::default()
            },
            move |_: &mut Shell| SctkLayerSurfaceSettings {
                id,
                layer: Layer::Overlay,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                anchor,
                output: IcedOutput::Active,
                namespace: "lapin-ux-panel".into(),
                margin,
                size: if full { Some((None, None)) } else { Some((Some(width), Some(1))) },
                exclusive_zone: if full { -1 } else { 0 },
                size_limits: if full {
                    Limits::NONE
                } else {
                    Limits::NONE.min_width(width as f32).max_width(width as f32).min_height(1.0).max_height(900.0)
                },
                ..Default::default()
            },
            None,
        )));
        if focus_run {
            tasks.push(text_input::focus(RUN_INPUT.clone()));
            tasks.push(text_input::select_all(RUN_INPUT.clone()));
        }
        Task::batch(tasks)
    }

    pub(crate) fn update_panel(&mut self, msg: PanelMsg) -> Task<Message> {
        match msg {
            PanelMsg::OpenWinX => {
                if matches!(self.panel.as_ref().map(|p| &p.kind), Some(PanelKind::WinX { .. })) {
                    return self.close_panel();
                }
                self.open_panel(PanelKind::WinX { power: false, selected: 0 })
            }
            PanelMsg::OpenRun => {
                let input = self.state.run_history.first().cloned().unwrap_or_default();
                self.open_panel(PanelKind::Run { input })
            }
            PanelMsg::Close => self.close_panel(),
            PanelMsg::Do(m) => {
                let close = self.close_panel();
                Task::batch([close, self.update(*m)])
            }
            PanelMsg::WinXPower(power) => {
                if let Some(Panel { kind: PanelKind::WinX { power: p, selected }, .. }) = &mut self.panel {
                    *p = power;
                    *selected = 0;
                }
                Task::none()
            }
            PanelMsg::RunInput(s) => {
                if let Some(Panel { kind: PanelKind::Run { input }, .. }) = &mut self.panel {
                    *input = s;
                }
                Task::none()
            }
            PanelMsg::RunSubmit => {
                let Some(Panel { kind: PanelKind::Run { input }, .. }) = &self.panel else { return Task::none() };
                let input = input.trim().to_string();
                if input.is_empty() {
                    return Task::none();
                }
                self.state.record_run(&input);
                self.persist_state();
                let close = self.close_panel();
                let cmd = run_command(&input);
                Task::batch([
                    close,
                    Task::future(async move {
                        if cosmic::process::spawn(cmd).await.is_none() {
                            tracing::warn!("run: could not start {input:?}");
                        }
                        cosmic::Action::App(Message::Noop)
                    }),
                ])
            }
        }
    }

    /// A panel's surface got the keyboard: focus its text field. (Asking
    /// earlier does nothing — the widget doesn't exist until the surface does.)
    pub(crate) fn panel_focused(&self) -> Task<Message> {
        match self.panel.as_ref().map(|p| &p.kind) {
            Some(PanelKind::Run { .. }) => Task::batch([text_input::focus(RUN_INPUT.clone()), text_input::select_all(RUN_INPUT.clone())]),
            Some(PanelKind::Clip(_)) => text_input::focus(crate::clipboard::SEARCH_ID.clone()),
            Some(PanelKind::AltTab(_)) => self.alt_tab_focused(),
            _ => Task::none(),
        }
    }

    /// Arrow keys / Enter / Esc inside a panel.
    pub(crate) fn panel_key(&mut self, key: Named) -> Task<Message> {
        if key != Named::Escape
            && let Some(t) = self.system_key(key)
        {
            return t;
        }
        let Some(panel) = &self.panel else { return Task::none() };
        match (&panel.kind, key) {
            (PanelKind::WinX { power: true, .. }, Named::Escape) => self.update_panel(PanelMsg::WinXPower(false)),
            (PanelKind::AltTab(_), k) => self.alt_tab_key(k),
            (PanelKind::Quick(_), Named::Escape) => match self.quick_escape() {
                Some(t) => t,
                None => self.close_panel(),
            },
            (_, Named::Escape) => self.close_panel(),
            (PanelKind::WinX { power, selected }, Named::ArrowDown | Named::ArrowUp) => {
                let n = selectable(self, *power).len();
                let next = if key == Named::ArrowDown { (*selected + 1) % n.max(1) } else { (*selected + n.max(1) - 1) % n.max(1) };
                if let Some(Panel { kind: PanelKind::WinX { selected, .. }, .. }) = &mut self.panel {
                    *selected = next;
                }
                Task::none()
            }
            (PanelKind::WinX { power, selected }, Named::Enter) => {
                let msgs = selectable(self, *power);
                match msgs.get(*selected).cloned() {
                    Some(m @ Message::Panel(PanelMsg::WinXPower(_))) => self.update(m),
                    Some(m @ Message::Panel(PanelMsg::OpenRun)) => self.update(m),
                    Some(m) => self.update_panel(PanelMsg::Do(Box::new(m))),
                    None => Task::none(),
                }
            }
            _ => Task::none(),
        }
    }
}

pub fn view<'a>(shell: &'a Shell, panel: &'a Panel) -> Element<'a, Message> {
    if let PanelKind::TaskView(t) = &panel.kind {
        return crate::taskview::view(shell, t, panel.id);
    }
    if let PanelKind::Security { power } = &panel.kind {
        return crate::system_ui::security_view(shell, *power);
    }
    if let PanelKind::Snip(s) = &panel.kind {
        return crate::snip::view(shell, s);
    }
    let content: Element<'_, Message> = match &panel.kind {
        PanelKind::TaskView(_) | PanelKind::Security { .. } | PanelKind::Snip(_) => unreachable!(),
        PanelKind::ShutDown { choice } => crate::system_ui::shutdown_view(*choice),
        PanelKind::Project(p) => crate::system_ui::project_view(p),
        PanelKind::Properties(p) => crate::properties::view(shell, p),
        PanelKind::OpenWith(p) => crate::open_with::view(shell, p),
        PanelKind::Snap(s) => crate::snap::view(shell, s),
        PanelKind::WinX { power, selected } => {
            let mut col = column::with_capacity(20).spacing(1);
            let mut idx = 0usize;
            for entry in winx_items(shell, *power) {
                match entry {
                    None => col = col.push(container(widget::divider::horizontal::light()).padding([4, 8])),
                    Some(it) => {
                        let msg = match it.msg {
                            m @ Message::Panel(_) => m,
                            m => Message::Panel(PanelMsg::Do(Box::new(m))),
                        };
                        col = col.push(
                            button::custom(
                                row::with_children(vec![
                                    icon::from_name(it.icon).size(16).into(),
                                    text::body(it.label).width(Length::Fill).into(),
                                ])
                                .spacing(12)
                                .align_y(Alignment::Center),
                            )
                            .width(Length::Fill)
                            .padding([7, 12])
                            .class(style::flat_button(idx == *selected))
                            .on_press(msg),
                        );
                        idx += 1;
                    }
                }
            }
            container(col).padding(4).into()
        }
        PanelKind::Run { input } => {
            let field = text_input::text_input("", input)
                .id(RUN_INPUT.clone())
                .on_input(|s| Message::Panel(PanelMsg::RunInput(s)))
                .on_submit(|_| Message::Panel(PanelMsg::RunSubmit))
                .width(Length::Fill);
            // A title bar like the benchmark's Run window.
            let title = container(
                row::with_children(vec![
                    icon::from_name("system-run-symbolic").size(16).into(),
                    text::body("Run").width(Length::Fill).into(),
                    button::custom(icon::from_name("window-close-symbolic").size(16))
                        .padding(6)
                        .class(style::flat_button(false))
                        .on_press(Message::Panel(PanelMsg::Close))
                        .into(),
                ])
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([4, 4, 4, 14])
            .class(style::panel_footer());
            let body = column::with_children(vec![
                row::with_children(vec![
                    icon::from_name("system-run-symbolic").size(32).into(),
                    text::body("Type the name of a program, folder, document or website, and Lapin UX will open it for you.")
                        .width(Length::Fill)
                        .into(),
                ])
                .spacing(16)
                .align_y(Alignment::Center)
                .into(),
                row::with_children(vec![text::body("Open:").into(), field.into()])
                    .spacing(12)
                    .align_y(Alignment::Center)
                    .into(),
                row::with_children(vec![
                    widget::Space::new().width(Length::Fill).into(),
                    lapin_ux_ui::button::suggested("OK").on_press(Message::Panel(PanelMsg::RunSubmit)).into(),
                    lapin_ux_ui::button::standard("Cancel").on_press(Message::Panel(PanelMsg::Close)).into(),
                ])
                .spacing(8)
                .into(),
            ])
            .spacing(16)
            .padding(20);
            column::with_children(vec![title.into(), body.into()]).into()
        }
        PanelKind::Quick(q) => crate::quick::view(shell, q),
        PanelKind::Clip(c) => crate::clipboard::view(shell, c),
        PanelKind::AltTab(a) => crate::previews::switcher(shell, a),
    };
    let width = match &panel.kind {
        PanelKind::WinX { .. } => WINX_WIDTH,
        PanelKind::Run { .. } => RUN_WIDTH,
        PanelKind::Quick(_) => crate::quick::WIDTH,
        PanelKind::Clip(_) => crate::clipboard::WIDTH,
        PanelKind::AltTab(a) => crate::previews::switcher_width(a.order.len()),
        PanelKind::TaskView(_) | PanelKind::Security { .. } | PanelKind::Snip(_) => 0,
        PanelKind::Snap(_) => crate::snap::WIDTH,
        PanelKind::ShutDown { .. } => crate::system_ui::SHUTDOWN_WIDTH,
        PanelKind::Properties(_) => crate::properties::WIDTH,
        PanelKind::OpenWith(_) => crate::open_with::WIDTH,
        PanelKind::Project(_) => crate::system_ui::PROJECT_WIDTH,
    } as f32;
    widget::autosize::autosize(
        container(content).width(Length::Fill).class(style::panel_bg()),
        AUTOSIZE.clone(),
    )
    .min_width(width)
    .max_width(width)
    .min_height(1.0)
    .max_height(900.0)
    .into()
}
