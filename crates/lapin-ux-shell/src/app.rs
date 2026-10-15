// SPDX-License-Identifier: GPL-3.0-only
//! Shell state and event handling. Views live in `taskbar_view` and
//! `start_view`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use lapin_ux_core::clock::ClockStyle;
use lapin_ux_core::recent::RecentFile;
use lapin_ux_core::taskbar::{ClickAction, Taskbar, Window};
use cosmic::app::{Core, Task};
use cosmic::cctk::sctk::reexports::calloop;
use cosmic::cctk::sctk::reexports::protocols::xdg::shell::client::xdg_positioner::{Anchor as PAnchor, Gravity};
use cosmic::cctk::wayland_client::protocol::wl_output::WlOutput;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::event::wayland::{self, LayerEvent, OutputEvent, PopupEvent};
use cosmic::iced::event::{PlatformSpecific, listen_raw};
use cosmic::iced::keyboard::{self, Key, key::Named};
use cosmic::iced::platform_specific::runtime::wayland::CornerRadius;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{IcedMargin, IcedOutput, SctkLayerSurfaceSettings};
use cosmic::iced::platform_specific::runtime::wayland::popup::{SctkPopupSettings, SctkPositioner};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer, destroy_layer_surface,
};
use cosmic::iced::platform_specific::shell::commands::popup::destroy_popup;
use cosmic::iced::{Limits, Rectangle, Subscription, window};
use cosmic::surface::action::{LiveSettings, app_layer_shell};
use cosmic::widget::rectangle_tracker::{RectangleTracker, RectangleUpdate, rectangle_tracker_subscription};
use cosmic::widget::text_input;
use cosmic::{Application, Element};

use crate::catalog::{self, Catalog};
use crate::config::{self, ShellConfig, ShellState, TaskbarAlignment};
use crate::style;
use crate::wayland::{self as wl, WaylandRequest, WaylandUpdate, WindowId as Handle, WindowInfo};

pub static SEARCH_ID: std::sync::LazyLock<cosmic::widget::Id> =
    std::sync::LazyLock::new(|| cosmic::widget::Id::new("lapin-ux-start-search"));

static POPUP_AUTOSIZE: std::sync::LazyLock<cosmic::widget::Id> =
    std::sync::LazyLock::new(|| cosmic::widget::Id::new("lapin-ux-popup-autosize"));

pub struct Bar {
    pub id: window::Id,
    pub output: WlOutput,
    /// Logical output height, used to size the Start menu.
    pub output_height: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPage {
    Home,
    AllApps,
}

pub struct StartMenu {
    pub id: window::Id,
    pub query: String,
    pub page: StartPage,
    /// Keyboard selection. Home and All apps only highlight once the user
    /// starts using the arrow keys; search results highlight the best match.
    pub selected: Option<usize>,
    pub recent_files: Vec<RecentFile>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PopupKind {
    /// Window list for an app with several windows.
    Windows(String),
    /// Right-click menu for a taskbar button.
    TaskContext(String),
    /// Right-click menu for an app in Start.
    StartContext(String),
    Calendar,
    Power,
    /// A tray icon's menu (by item name).
    TrayMenu(String),
}

/// One icon in the notification area.
pub struct TrayEntry {
    pub item: crate::tray::TrayItem,
    pub icon: Option<cosmic::widget::icon::Handle>,
    pub title: Option<String>,
    pub menu: Option<crate::tray::Layout>,
}

pub struct Popup {
    pub id: window::Id,
    pub parent: window::Id,
    pub kind: PopupKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Lock,
    SignOut,
    Sleep,
    Restart,
    ShutDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub enum Message {
    Wayland(WaylandUpdate),
    Output(OutputEvent, WlOutput),
    Layer(LayerEvent, window::Id),
    PopupClosed(window::Id),
    Config(ShellConfig),
    Tick,
    Rect(RectangleUpdate<String>),
    ToggleStart,
    CloseStart,
    /// A navigation key pressed in some surface; only acted on for Start.
    StartKey(window::Id, Box<Message>),
    StartQuery(String),
    StartSubmit,
    StartNav(Nav),
    StartPage(StartPage),
    Launch(String),
    /// A desktop action (jump-list entry): app id, Exec line.
    LaunchAction(String, String),
    CalendarSelect(jiff::civil::Date),
    CalendarPrev,
    CalendarNext,
    OpenFile(std::path::PathBuf),
    TaskClick(window::Id, String),
    TaskContext(window::Id, String),
    StartContext(String),
    ActivateWindow(Handle),
    CloseWindow(Handle),
    CloseApp(String),
    Pin(String),
    Unpin(String),
    PinStart(String),
    UnpinStart(String),
    ShowDesktop,
    ClockClick(window::Id),
    PowerMenu,
    Power(PowerAction),
    Tray(crate::tray::Event),
    TrayIcon(String, crate::tray::IconUpdate),
    TrayLayout(String, Result<crate::tray::Layout, String>),
    TrayClick(window::Id, String),
    TrayContext(window::Id, String),
    /// Item name, dbusmenu id, whether it opens a submenu.
    TrayMenuItem(String, i32, bool),
    TrayMenuBack,
    Noop,
}

pub struct Shell {
    core: Core,
    pub config: ShellConfig,
    config_handle: Option<cosmic_config::Config>,
    pub state: ShellState,
    state_handle: Option<cosmic_config::Config>,
    pub catalog: Catalog,
    pub taskbar: Taskbar<Handle>,
    wayland_tx: Option<calloop::channel::Sender<WaylandRequest>>,
    pub bars: Vec<Bar>,
    pub start: Option<StartMenu>,
    start_closed_at: Instant,
    pub popup: Option<Popup>,
    pub rects: HashMap<String, Rectangle>,
    pub tracker: Option<RectangleTracker<String>>,
    pub clock: ClockStyle,
    pub now: chrono::NaiveDateTime,
    pub calendar: cosmic::widget::calendar::CalendarModel,
    /// Windows minimized by "Show desktop", restored by a second click.
    desktop_shown: Vec<Handle>,
    /// Start was requested before any taskbar existed (Super pressed at login).
    start_pending: bool,
    pub tray: Vec<TrayEntry>,
    /// Submenu path inside the open tray menu.
    pub tray_menu_path: Vec<i32>,
}

/// Key used for rectangle tracking of a taskbar item on a given bar.
pub fn rect_key(bar: window::Id, item: &str) -> String {
    format!("{bar:?}|{item}")
}

impl Shell {
    fn persist_config(&self) {
        if let Some(h) = &self.config_handle
            && let Err(e) = self.config.write_entry(h)
        {
            tracing::error!("saving config: {e}");
        }
    }

    fn persist_state(&self) {
        if let Some(h) = &self.state_handle
            && let Err(e) = self.state.write_entry(h)
        {
            tracing::error!("saving state: {e}");
        }
    }

    fn request(&self, req: WaylandRequest) {
        if let Some(tx) = &self.wayland_tx {
            let _ = tx.send(req);
        }
    }

    pub fn clock_style(&self) -> ClockStyle {
        let mut c = self.clock;
        if let Some(h24) = self.config.clock_24h {
            c.hour12 = !h24;
        }
        c.seconds = self.config.clock_seconds;
        c
    }

    fn create_bar(&mut self, output: WlOutput, output_height: Option<u32>) -> Task<Message> {
        let id = window::Id::unique();
        self.bars.push(Bar { id, output: output.clone(), output_height });
        let task = cosmic::surface::surface_task(app_layer_shell(
            |_: &Shell| LiveSettings { blur: Some(true), ..Default::default() },
            move |_: &mut Shell| SctkLayerSurfaceSettings {
                id,
                layer: Layer::Top,
                keyboard_interactivity: KeyboardInteractivity::None,
                anchor: Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                output: IcedOutput::Output(output.clone()),
                namespace: "lapin-ux-taskbar".into(),
                size: Some((None, Some(style::TASKBAR_HEIGHT))),
                exclusive_zone: style::TASKBAR_HEIGHT as i32,
                size_limits: Limits::NONE.min_height(style::TASKBAR_HEIGHT as f32).max_height(style::TASKBAR_HEIGHT as f32),
                ..Default::default()
            },
            None,
        ));
        if std::mem::take(&mut self.start_pending) {
            return task.chain(cosmic::task::message(Message::ToggleStart));
        }
        task
    }

    fn open_start(&mut self) -> Task<Message> {
        let id = window::Id::unique();
        let height = self
            .bars
            .iter()
            .filter_map(|b| b.output_height)
            .min()
            .map(|h| h.saturating_sub(style::TASKBAR_HEIGHT + 24).min(style::START_HEIGHT))
            .unwrap_or(style::START_HEIGHT)
            .max(360);
        self.start = Some(StartMenu {
            id,
            query: String::new(),
            page: StartPage::Home,
            selected: None,
            recent_files: lapin_ux_core::recent::load(6),
        });
        let left = self.config.alignment == TaskbarAlignment::Left;
        let r = style::PANEL_RADIUS as u32;
        Task::batch([
            cosmic::surface::surface_task(app_layer_shell(
                move |_: &Shell| LiveSettings {
                    blur: Some(true),
                    corners: Some(CornerRadius { top_left: r, top_right: r, bottom_left: r, bottom_right: r }),
                    ..Default::default()
                },
                move |_: &mut Shell| SctkLayerSurfaceSettings {
                    id,
                    layer: Layer::Top,
                    keyboard_interactivity: KeyboardInteractivity::Exclusive,
                    anchor: if left { Anchor::BOTTOM | Anchor::LEFT } else { Anchor::BOTTOM },
                    output: IcedOutput::Active,
                    namespace: "lapin-ux-start".into(),
                    margin: IcedMargin { top: 0, right: 0, bottom: 12, left: if left { 12 } else { 0 } },
                    size: Some((Some(style::START_WIDTH), Some(height))),
                    exclusive_zone: 0,
                    size_limits: Limits::NONE
                        .min_width(style::START_WIDTH as f32)
                        .max_width(style::START_WIDTH as f32)
                        .min_height(height as f32)
                        .max_height(height as f32),
                    ..Default::default()
                },
                None,
            )),
            text_input::focus(SEARCH_ID.clone()),
        ])
    }

    fn close_start(&mut self) -> Task<Message> {
        let mut tasks = vec![self.close_popup()];
        if let Some(s) = self.start.take() {
            self.start_closed_at = Instant::now();
            tasks.push(destroy_layer_surface(s.id));
        }
        Task::batch(tasks)
    }

    fn close_popup(&mut self) -> Task<Message> {
        match self.popup.take() {
            Some(p) => destroy_popup(p.id),
            None => Task::none(),
        }
    }

    /// Open a popup anchored above the tracked rectangle `key` on surface `parent`.
    fn open_popup(&mut self, parent: window::Id, key: &str, kind: PopupKind) -> Task<Message> {
        let same = self.popup.as_ref().is_some_and(|p| p.kind == kind);
        let close = self.close_popup();
        if same {
            return close; // second click on the same thing closes it
        }
        let id = window::Id::unique();
        let r = self.rects.get(key).copied().unwrap_or(Rectangle { x: 0.0, y: 0.0, width: 1.0, height: 1.0 });
        let above = self.bars.iter().any(|b| b.id == parent);
        let settings = SctkPopupSettings {
            parent,
            id,
            positioner: SctkPositioner {
                size: None,
                size_limits: Limits::NONE.min_width(1.0).min_height(1.0).max_width(440.0).max_height(720.0),
                anchor_rect: Rectangle {
                    x: r.x as i32,
                    y: r.y as i32,
                    width: (r.width as i32).max(1),
                    height: (r.height as i32).max(1),
                },
                anchor: if above { PAnchor::Top } else { PAnchor::Bottom },
                gravity: if above { Gravity::Top } else { Gravity::Bottom },
                // slide_x | flip_y | resize_x | resize_y
                constraint_adjustment: 1 | 8 | 16 | 32,
                offset: (0, if above { -8 } else { 4 }),
                reactive: true,
            },
            parent_size: None,
            grab: true,
            close_with_children: false,
            input_zone: None,
        };
        self.popup = Some(Popup { id, parent, kind });
        let r = style::PANEL_RADIUS as u32;
        let create = cosmic::surface::surface_task(cosmic::surface::action::app_popup(
            move |_: &Shell| LiveSettings {
                corners: Some(CornerRadius { top_left: r, top_right: r, bottom_left: r, bottom_right: r }),
                ..Default::default()
            },
            move |_: &mut Shell| settings.clone(),
            None,
        ));
        Task::batch([close, create])
    }

    fn launch(&mut self, app_id: &str) -> Task<Message> {
        let Some(data) = self.catalog.data_for(app_id).cloned() else {
            tracing::warn!(app_id, "no desktop entry to launch");
            return Task::none();
        };
        let Some(exec) = data.exec.clone() else { return Task::none() };
        self.state.record_launch(&data.id);
        self.persist_state();
        let mut tasks = vec![self.close_start()];
        if self.wayland_tx.is_some() {
            self.request(WaylandRequest::TokenRequest { app_id: data.id.clone(), exec, terminal: data.terminal });
        } else {
            tasks.push(spawn(exec, data.id, data.terminal, None));
        }
        Task::batch(tasks)
    }

    fn window_changed(&mut self, info: WindowInfo) {
        tracing::debug!(?info, "window changed");
        let app = self.catalog.resolve_window(&info.app_id, &info.title).id;
        let app = if app.is_empty() { info.app_id.clone() } else { app };
        let was_active = self.taskbar.windows().iter().any(|w| w.handle == info.id && w.active);
        if info.activated && !was_active && !info.minimized {
            // The user focused something after "Show desktop": a second
            // click should hide again rather than restore the old set.
            self.desktop_shown.clear();
        }
        self.taskbar.upsert(Window {
            handle: info.id,
            app,
            title: info.title,
            active: info.activated,
            minimized: info.minimized,
            urgent: false,
        });
    }

    fn window_closed(&mut self, id: Handle) {
        self.taskbar.remove(&id);
        self.desktop_shown.retain(|h| h != &id);
    }

    /// Ordered list of what the Start menu currently shows, for keyboard nav.
    pub fn start_items(&self) -> Vec<String> {
        let Some(s) = &self.start else { return Vec::new() };
        if !s.query.trim().is_empty() {
            return self.catalog.search(&s.query).into_iter().take(12).map(|a| a.data.id.clone()).collect();
        }
        match s.page {
            StartPage::Home => self.config.pinned_start.iter().filter(|id| self.catalog.get(id).is_some()).cloned().collect(),
            StartPage::AllApps => {
                let names: Vec<&str> = self.catalog.apps().iter().map(|a| a.data.name.as_str()).collect();
                lapin_ux_core::apps::alphabet_sections(&names)
                    .into_iter()
                    .flat_map(|sec| sec.items)
                    .map(|i| self.catalog.apps()[i].data.id.clone())
                    .collect()
            }
        }
    }

    fn power(&mut self, action: PowerAction) -> Task<Message> {
        let close = self.close_start();
        let job = Task::future(async move {
            if let Err(e) = power_action(action).await {
                tracing::error!(?action, "power action failed: {e:#}");
            }
            cosmic::Action::App(Message::Noop)
        });
        Task::batch([close, job])
    }
}

fn spawn(exec: String, app_id: String, terminal: bool, token: Option<String>) -> Task<Message> {
    Task::future(async move {
        catalog::launch(exec, app_id, terminal, token).await;
        cosmic::Action::App(Message::Noop)
    })
}

async fn power_action(action: PowerAction) -> anyhow::Result<()> {
    if action == PowerAction::Lock {
        tokio::process::Command::new("loginctl").arg("lock-session").status().await?;
        return Ok(());
    }
    if action == PowerAction::SignOut {
        let conn = zbus::Connection::session().await?;
        conn.call_method(Some("dev.lapinux.Session"), "/dev/lapinux/Session", Some("dev.lapinux.Session"), "Exit", &())
            .await?;
        return Ok(());
    }
    let method = match action {
        PowerAction::Sleep => "Suspend",
        PowerAction::Restart => "Reboot",
        _ => "PowerOff",
    };
    let conn = zbus::Connection::system().await?;
    conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1",
        Some("org.freedesktop.login1.Manager"),
        method,
        &(true,),
    )
    .await?;
    Ok(())
}

impl Application for Shell {
    type Executor = cosmic::executor::Default;
    type Flags = crate::Args;
    type Message = Message;
    const APP_ID: &'static str = lapin_ux_core::ids::SHELL_APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: crate::Args) -> (Self, Task<Message>) {
        let catalog = Catalog::load();
        let config_handle = config::shell_config();
        let mut cfg: ShellConfig = config::load(config_handle.as_ref());
        if !cfg.initialized {
            crate::defaults::apply_first_run();
            cfg = cfg.with_defaults(&catalog);
            if let Some(h) = &config_handle {
                let _ = cfg.write_entry(h);
            }
        }
        let state_handle = config::shell_state();
        let state: ShellState = config::load(state_handle.as_ref());
        let shell = Shell {
            core,
            taskbar: Taskbar::new(cfg.pinned_taskbar.clone()),
            config: cfg,
            config_handle,
            state,
            state_handle,
            catalog,
            wayland_tx: None,
            bars: Vec::new(),
            start: None,
            start_closed_at: Instant::now() - Duration::from_secs(10),
            popup: None,
            rects: HashMap::new(),
            tracker: None,
            clock: ClockStyle::from_env(),
            now: chrono::Local::now().naive_local(),
            calendar: cosmic::widget::calendar::CalendarModel::now(),
            desktop_shown: Vec::new(),
            start_pending: false,
            tray: Vec::new(),
            tray_menu_path: Vec::new(),
        };
        (shell, Task::none())
    }

    fn view(&self) -> Element<'_, Message> {
        unreachable!("the shell has no main window")
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if let Some(bar) = self.bars.iter().find(|b| b.id == id) {
            return crate::taskbar_view::taskbar(self, bar);
        }
        if let Some(start) = self.start.as_ref().filter(|s| s.id == id) {
            return crate::start_view::start_menu(self, start);
        }
        if let Some(popup) = self.popup.as_ref().filter(|p| p.id == id) {
            // Popups size themselves to their content.
            return cosmic::widget::autosize::autosize(crate::taskbar_view::popup(self, popup), POPUP_AUTOSIZE.clone())
                .min_width(1.0)
                .min_height(1.0)
                .max_width(440.0)
                .max_height(720.0)
                .into();
        }
        cosmic::widget::Space::new().into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(style::transparent_window())
    }

    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Message> {
        match msg.msg {
            cosmic::dbus_activation::Details::ActivateAction { action, .. } => match action.as_str() {
                "start" => self.update(Message::ToggleStart),
                "desktop" => self.update(Message::ShowDesktop),
                other => {
                    tracing::warn!(action = other, "unknown action");
                    Task::none()
                }
            },
            _ => Task::none(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let tray_subs = self.tray.iter().flat_map(|t| {
            let name = t.item.name().to_string();
            [
                t.item.icon_subscription().with(name.clone()).map(|(n, u)| Message::TrayIcon(n, u)),
                t.item.layout_subscription().with(name).map(|(n, l)| Message::TrayLayout(n, l)),
            ]
        });
        Subscription::batch(tray_subs.chain([
            crate::tray::subscription().map(Message::Tray),
            wl::subscription().map(Message::Wayland),
            self.core.watch_config::<ShellConfig>(lapin_ux_core::ids::SHELL_APP_ID).map(|u| Message::Config(u.config)),
            rectangle_tracker_subscription(0).map(|(_, u)| Message::Rect(u)),
            cosmic::iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick),
            listen_raw(|event, _status, window| match event {
                cosmic::iced::Event::PlatformSpecific(PlatformSpecific::Wayland(w)) => match w {
                    wayland::Event::Layer(e, _, id) => Some(Message::Layer(e, id)),
                    wayland::Event::Output(e, output) => Some(Message::Output(e, output)),
                    wayland::Event::Popup(PopupEvent::Done, _, id) => Some(Message::PopupClosed(id)),
                    _ => None,
                },
                cosmic::iced::Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(named), .. }) => {
                    let msg = match named {
                        Named::Escape => Message::CloseStart,
                        Named::ArrowDown => Message::StartNav(Nav::Down),
                        Named::ArrowUp => Message::StartNav(Nav::Up),
                        Named::ArrowLeft => Message::StartNav(Nav::Left),
                        Named::ArrowRight => Message::StartNav(Nav::Right),
                        _ => return None,
                    };
                    Some(Message::StartKey(window, Box::new(msg)))
                }
                _ => None,
            }),
        ]))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if !matches!(message, Message::Tick | Message::Rect(_) | Message::Wayland(_)) {
            tracing::trace!(?message);
        }
        match message {
            Message::Wayland(u) => match u {
                WaylandUpdate::Init(tx) => self.wayland_tx = Some(tx),
                WaylandUpdate::Finished => self.wayland_tx = None,
                WaylandUpdate::Window(info) => self.window_changed(info),
                WaylandUpdate::Closed(id) => self.window_closed(id),
                WaylandUpdate::ActivationToken { token, app_id, exec, terminal } => {
                    return spawn(exec, app_id, terminal, token);
                }
            },
            Message::Output(event, output) => match event {
                OutputEvent::Created(info) => {
                    if !self.bars.iter().any(|b| b.output == output) {
                        let h = info.and_then(|i| i.logical_size).map(|(_, h)| h.max(0) as u32);
                        return self.create_bar(output, h);
                    }
                }
                OutputEvent::InfoUpdate(info) => {
                    if let Some(bar) = self.bars.iter_mut().find(|b| b.output == output) {
                        bar.output_height = info.logical_size.map(|(_, h)| h.max(0) as u32);
                    }
                }
                OutputEvent::Removed => {
                    if let Some(pos) = self.bars.iter().position(|b| b.output == output) {
                        let bar = self.bars.remove(pos);
                        return destroy_layer_surface(bar.id);
                    }
                }
            },
            Message::Layer(LayerEvent::Unfocused, id) if self.start.as_ref().is_some_and(|s| s.id == id) => {
                // A Start-menu popup (right-click menu) takes focus without
                // meaning "the user clicked away".
                if self.popup.as_ref().is_some_and(|p| p.parent == id) {
                    return Task::none();
                }
                return self.close_start();
            }
            Message::Layer(..) => {}
            Message::PopupClosed(id) => {
                if self.popup.as_ref().is_some_and(|p| p.id == id) {
                    self.popup = None;
                }
            }
            Message::Config(cfg) => {
                if cfg.pinned_taskbar != *self.taskbar.pinned() {
                    self.taskbar.set_pinned(cfg.pinned_taskbar.clone());
                }
                self.config = cfg;
            }
            Message::Tick => {
                self.now = chrono::Local::now().naive_local();
            }
            Message::Rect(u) => match u {
                RectangleUpdate::Init(t) => self.tracker = Some(t),
                RectangleUpdate::Rectangle((key, r)) => {
                    self.rects.insert(key, r);
                }
            },
            Message::ToggleStart => {
                if self.start.is_some() {
                    return self.close_start();
                }
                // The click that moved focus away already closed it.
                if self.start_closed_at.elapsed() < Duration::from_millis(200) {
                    return Task::none();
                }
                if self.bars.is_empty() {
                    self.start_pending = true;
                    return Task::none();
                }
                let close = self.close_popup();
                return Task::batch([close, self.open_start()]);
            }
            Message::StartKey(id, msg) => {
                if self.start.as_ref().is_some_and(|s| s.id == id) {
                    return self.update(*msg);
                }
            }
            Message::CloseStart => {
                if self.popup.is_some() {
                    return self.close_popup();
                }
                if let Some(s) = &mut self.start
                    && (!s.query.is_empty() || s.page != StartPage::Home)
                {
                    // Esc steps back: clear search / leave All apps first.
                    s.query.clear();
                    s.page = StartPage::Home;
                    s.selected = None;
                    return Task::none();
                }
                return self.close_start();
            }
            Message::StartQuery(q) => {
                if let Some(s) = &mut self.start {
                    s.selected = (!q.trim().is_empty()).then_some(0);
                    s.query = q;
                }
            }
            Message::StartSubmit => {
                let items = self.start_items();
                let selected = self.start.as_ref().and_then(|s| s.selected).unwrap_or(0);
                if let Some(id) = items.get(selected).or(items.first()).cloned() {
                    return self.launch(&id);
                }
            }
            Message::StartNav(nav) => {
                let len = self.start_items().len();
                let searching = self.start.as_ref().is_some_and(|s| !s.query.trim().is_empty());
                let cols = match self.start.as_ref().map(|s| s.page) {
                    Some(StartPage::Home) if !searching => crate::start_view::PINNED_COLUMNS,
                    _ => 1,
                };
                if let Some(s) = &mut self.start
                    && len > 0
                {
                    let Some(cur) = s.selected else {
                        s.selected = Some(0);
                        return Task::none();
                    };
                    let i = cur as isize;
                    let next = match nav {
                        Nav::Right => i + 1,
                        Nav::Left => i - 1,
                        Nav::Down => i + cols as isize,
                        Nav::Up => i - cols as isize,
                    };
                    s.selected = Some(next.clamp(0, len as isize - 1) as usize);
                }
            }
            Message::StartPage(page) => {
                if let Some(s) = &mut self.start {
                    s.page = page;
                    s.selected = None;
                    s.query.clear();
                }
                return text_input::focus(SEARCH_ID.clone());
            }
            Message::Launch(id) => return self.launch(&id),
            Message::LaunchAction(app_id, exec) => {
                let terminal = self.catalog.data_for(&app_id).is_some_and(|d| d.terminal);
                let close = Task::batch([self.close_popup(), self.close_start()]);
                return Task::batch([close, spawn(exec, app_id, terminal, None)]);
            }
            Message::CalendarSelect(date) => self.calendar.set_selected_visible(date),
            Message::CalendarPrev => self.calendar.show_prev_month(),
            Message::CalendarNext => self.calendar.show_next_month(),
            Message::OpenFile(path) => {
                let close = self.close_start();
                let open = Task::future(async move {
                    let _ = tokio::process::Command::new("xdg-open").arg(path).spawn();
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([close, open]);
            }
            Message::TaskClick(bar, app) => {
                let close = self.close_popup();
                let action = self.taskbar.click(&app);
                let next = match action {
                    ClickAction::Launch(id) => self.launch(&id),
                    ClickAction::Activate(h) => {
                        self.request(WaylandRequest::Activate(h));
                        Task::none()
                    }
                    ClickAction::Minimize(h) => {
                        self.request(WaylandRequest::Minimize(h));
                        Task::none()
                    }
                    ClickAction::ShowPicker => {
                        let key = rect_key(bar, &app);
                        return self.open_popup(bar, &key, PopupKind::Windows(app));
                    }
                };
                return Task::batch([close, next]);
            }
            Message::TaskContext(bar, app) => {
                let key = rect_key(bar, &app);
                return self.open_popup(bar, &key, PopupKind::TaskContext(app));
            }
            Message::StartContext(app) => {
                if let Some(s) = &self.start {
                    let parent = s.id;
                    let key = rect_key(parent, &app);
                    return self.open_popup(parent, &key, PopupKind::StartContext(app));
                }
            }
            Message::ActivateWindow(h) => {
                self.request(WaylandRequest::Activate(h));
                return self.close_popup();
            }
            Message::CloseWindow(h) => {
                self.request(WaylandRequest::Close(h));
            }
            Message::CloseApp(app) => {
                for w in self.taskbar.windows().iter().filter(|w| w.app == app) {
                    self.request(WaylandRequest::Close(w.handle));
                }
                return self.close_popup();
            }
            Message::Pin(app) => {
                if self.taskbar.pin(&app) {
                    self.config.pinned_taskbar = self.taskbar.pinned().to_vec();
                    self.persist_config();
                }
                return self.close_popup();
            }
            Message::Unpin(app) => {
                if self.taskbar.unpin(&app) {
                    self.config.pinned_taskbar = self.taskbar.pinned().to_vec();
                    self.persist_config();
                }
                return self.close_popup();
            }
            Message::PinStart(app) => {
                if !self.config.pinned_start.contains(&app) {
                    self.config.pinned_start.push(app);
                    self.persist_config();
                }
                return self.close_popup();
            }
            Message::UnpinStart(app) => {
                self.config.pinned_start.retain(|a| a != &app);
                self.persist_config();
                return self.close_popup();
            }
            Message::ShowDesktop => {
                if self.desktop_shown.is_empty() {
                    // Remember visible windows, focused one last, so restoring
                    // in order leaves it on top with focus.
                    let mut visible: Vec<&Window<Handle>> = self.taskbar.windows().iter().filter(|w| !w.minimized).collect();
                    visible.sort_by_key(|w| w.active);
                    self.desktop_shown = visible.into_iter().map(|w| w.handle).collect();
                    for h in &self.desktop_shown {
                        self.request(WaylandRequest::Minimize(*h));
                    }
                } else {
                    for h in std::mem::take(&mut self.desktop_shown) {
                        self.request(WaylandRequest::Activate(h));
                    }
                }
            }
            Message::ClockClick(bar) => {
                self.calendar = cosmic::widget::calendar::CalendarModel::now();
                let key = rect_key(bar, "clock");
                return self.open_popup(bar, &key, PopupKind::Calendar);
            }
            Message::PowerMenu => {
                if let Some(s) = &self.start {
                    let parent = s.id;
                    let key = rect_key(parent, "power");
                    return self.open_popup(parent, &key, PopupKind::Power);
                }
            }
            Message::Power(action) => return self.power(action),
            Message::Tray(ev) => match ev {
                crate::tray::Event::Connected => tracing::info!("tray host ready"),
                crate::tray::Event::Registered(item) => {
                    self.tray.retain(|t| t.item.name() != item.name());
                    self.tray.push(TrayEntry { item, icon: None, title: None, menu: None });
                }
                crate::tray::Event::Unregistered(name) => {
                    self.tray.retain(|t| t.item.name() != name);
                    if matches!(&self.popup, Some(Popup { kind: PopupKind::TrayMenu(n), .. }) if *n == name) {
                        return self.close_popup();
                    }
                }
                crate::tray::Event::Error(e) => tracing::debug!("tray: {e}"),
            },
            Message::TrayIcon(name, update) => {
                if let Some(t) = self.tray.iter_mut().find(|t| t.item.name() == name) {
                    t.title = update.title.clone();
                    t.icon = Some(crate::taskbar_view::tray_icon(update));
                }
            }
            Message::TrayLayout(name, layout) => {
                if let (Some(t), Ok(l)) = (self.tray.iter_mut().find(|t| t.item.name() == name), layout) {
                    t.menu = Some(l);
                }
            }
            Message::TrayClick(bar, name) => {
                let Some(t) = self.tray.iter().find(|t| t.item.name() == name) else { return Task::none() };
                if t.item.is_menu() && t.item.has_menu() {
                    return self.update(Message::TrayContext(bar, name));
                }
                let proxy = t.item.proxy().clone();
                let close = self.close_popup();
                return Task::batch([
                    close,
                    Task::future(async move {
                        if let Err(e) = proxy.activate(0, 0).await {
                            tracing::debug!("tray activate: {e}");
                        }
                        cosmic::Action::App(Message::Noop)
                    }),
                ]);
            }
            Message::TrayContext(bar, name) => {
                let Some(t) = self.tray.iter().find(|t| t.item.name() == name) else { return Task::none() };
                match t.item.menu_proxy().cloned() {
                    Some(menu) => {
                        self.tray_menu_path.clear();
                        let key = rect_key(bar, &format!("tray:{name}"));
                        let open = self.open_popup(bar, &key, PopupKind::TrayMenu(name));
                        let prepare = Task::future(async move {
                            let _ = menu.about_to_show(0).await;
                            cosmic::Action::App(Message::Noop)
                        });
                        return Task::batch([prepare, open]);
                    }
                    None => {
                        let proxy = t.item.proxy().clone();
                        return Task::future(async move {
                            let _ = proxy.secondary_activate(0, 0).await;
                            cosmic::Action::App(Message::Noop)
                        });
                    }
                }
            }
            Message::TrayMenuItem(name, id, submenu) => {
                let Some(menu) = self.tray.iter().find(|t| t.item.name() == name).and_then(|t| t.item.menu_proxy().cloned())
                else {
                    return Task::none();
                };
                if submenu {
                    self.tray_menu_path.push(id);
                    return Task::future(async move {
                        let _ = menu.about_to_show(id).await;
                        cosmic::Action::App(Message::Noop)
                    });
                }
                let close = self.close_popup();
                return Task::batch([
                    close,
                    Task::future(async move {
                        let data = zbus::zvariant::OwnedValue::from(0i32);
                        if let Err(e) = menu.event(id, "clicked", &data, 0).await {
                            tracing::debug!("tray menu click: {e}");
                        }
                        cosmic::Action::App(Message::Noop)
                    }),
                ]);
            }
            Message::TrayMenuBack => {
                self.tray_menu_path.pop();
            }
            Message::Noop => {}
        }
        Task::none()
    }
}
