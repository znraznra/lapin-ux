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
use cosmic::iced::mouse;
use crate::taskbar_extras::Pointer;
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
    /// Monitor name (e.g. "eDP-1"), for per-monitor taskbar buttons.
    pub output_name: Option<String>,
    /// Auto-hide: shown at full height right now.
    pub revealed: bool,
    pub hovered: bool,
    pub hide_pending: bool,
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
    /// A folder of pinned apps is open (its stored pin index).
    pub folder: Option<usize>,
    pub pin_drag: Option<crate::start_pins::PinDrag>,
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
    /// Hidden tray icons behind the ^ button.
    TrayOverflow,
    /// Right-click on a pinned file, site or folder in Start.
    PinContext(usize),
    /// Right-click on empty taskbar space.
    TaskbarMenu,
    /// Its "Show taskbar apps on" submenu (several monitors).
    TaskbarDisplays,
    /// Window thumbnails shown while hovering a taskbar button.
    Preview(String),
    /// "This app and its related info will be uninstalled."
    ConfirmUninstall(String),
    /// Right-click on the desktop (`Some` = on an icon).
    DesktopMenu(Option<std::path::PathBuf>, crate::desktop::MenuPage),
}

/// One icon in the notification area.
pub struct TrayEntry {
    pub item: crate::tray::TrayItem,
    /// Stable tray id reported by the app (used to remember placement).
    pub id: Option<String>,
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
    /// A named key pressed in one of our surfaces.
    KeyNamed(window::Id, Named),
    /// Open Start on a given page (Win+X "Installed apps", "Search").
    OpenStartAt(StartPage),
    Panel(crate::panels::PanelMsg),
    /// Super+1…9.
    TaskbarItem(usize),
    TaskbarContext(window::Id),
    TaskbarOption(crate::taskbar_extras::TaskbarOption),
    BarPointer(window::Id, crate::taskbar_extras::Pointer),
    TrayOverflow(window::Id),
    /// Stable tray id, show inline on the taskbar.
    TrayPin(String, bool),
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
    /// Something is dragged onto (true) or off (false) a taskbar button.
    DragHover(String, bool),
    /// The drag stayed long enough over the button with this serial.
    DragHoverFire(u64),
    /// A LauncherEntry update from an app.
    Badge(crate::launcher_entry::Update),
    /// Force-quit an app's processes (taskbar "End task").
    EndTask(String),
    /// Open a jump-list file with the app it belongs to.
    OpenWith(String, std::path::PathBuf),
    TaskClick(window::Id, String),
    TaskContext(window::Id, String),
    StartContext(String),
    ActivateWindow(Handle),
    CloseWindow(Handle),
    CloseApp(String),
    Pin(String),
    Unpin(String),
    PinStart(String),
    Pins(crate::start_pins::PinMsg),
    UnpinStart(String),
    ShowDesktop,
    ClockClick(window::Id),
    /// Click outside an open Start menu or panel.
    CatcherClick,
    FilesIndexed(std::sync::Arc<lapin_ux_core::files::FileIndex>),
    Desktop(crate::desktop::DesktopMsg),
    Clip(crate::clipboard::ClipMsg),
    Preview(crate::previews::PreviewMsg),
    CloseSubmenu,
    /// Auto-hide: pointer entered/left a taskbar.
    BarHover(window::Id, bool),
    /// Auto-hide: hide this bar now if nothing needs it.
    AutoHide(window::Id),
    /// Installed apps changed (e.g. after uninstalling): reload .desktop files.
    ReloadApps,
    UninstallAsk(String),
    Uninstall(String),
    StartActivate(crate::start_search::StartItem),
    ShowInFolder(std::path::PathBuf),
    Quick(crate::quick::QuickMsg),
    /// Super+N: close our panels, then toggle the notification centre.
    ToggleNotifications,
    /// Notification daemon state (unread count, Do Not Disturb).
    NotifyState(crate::notify_client::State),
    /// The notification centre couldn't be reached; show the calendar popup instead.
    CalendarFallback(window::Id),
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
    /// Transparent full-screen surface under Start / panels; see catcher.rs.
    pub(crate) catcher: Option<window::Id>,
    pub(crate) core: Core,
    pub config: ShellConfig,
    pub(crate) config_handle: Option<cosmic_config::Config>,
    pub state: ShellState,
    pub(crate) state_handle: Option<cosmic_config::Config>,
    pub catalog: Catalog,
    pub taskbar: Taskbar<Handle>,
    pub(crate) wayland_tx: Option<calloop::channel::Sender<WaylandRequest>>,
    pub bars: Vec<Bar>,
    pub start: Option<StartMenu>,
    pub(crate) start_closed_at: Instant,
    pub popup: Option<Popup>,
    pub rects: HashMap<String, Rectangle>,
    pub tracker: Option<RectangleTracker<String>>,
    pub clock: ClockStyle,
    pub now: chrono::NaiveDateTime,
    pub calendar: cosmic::widget::calendar::CalendarModel,
    pub notify: crate::notify_client::State,
    pub quick: crate::quick::QuickState,
    /// Start search over the user's folders.
    pub files: std::sync::Arc<lapin_ux_core::files::FileIndex>,
    pub indexing: bool,
    pub desktop: Option<crate::desktop::DesktopView>,
    pub clip: crate::clipboard::ClipState,
    pub thumbs: std::collections::HashMap<Handle, crate::previews::Thumb>,
    /// A cascading submenu of `popup`.
    pub sub: Option<Popup>,
    /// A file being dragged over a taskbar button: (app, serial).
    pub drag_hover: Option<(String, u64)>,
    pub drag_serial: u64,
    /// Badge counts / progress / attention announced by apps, by app id.
    pub badges: HashMap<String, lapin_ux_core::badge::Badge>,
    /// Jump list of the task whose right-click menu is open.
    pub jump: Vec<RecentFile>,
    pub hover: Option<crate::previews::Hover>,
    pub hover_gen: u64,
    /// Windows, most recently focused first (Alt+Tab order).
    pub mru: Vec<Handle>,
    /// Windows minimized by "Show desktop", restored by a second click.
    pub(crate) desktop_shown: Vec<Handle>,
    /// Start was requested before any taskbar existed (Super pressed at login).
    pub(crate) start_pending: bool,
    pub tray: Vec<TrayEntry>,
    /// Submenu path inside the open tray menu.
    pub tray_menu_path: Vec<i32>,
    /// Win+X menu / Run dialog.
    pub panel: Option<crate::panels::Panel>,
    pub(crate) panel_closed_at: Instant,
    /// Last pointer position over a taskbar.
    pub(crate) pointer: Option<(window::Id, cosmic::iced::Point)>,
    pub drag: Option<crate::taskbar_extras::Drag>,
    pub(crate) suppress_click_until: Instant,
    pub(crate) item_context_at: Instant,
}

/// Key used for rectangle tracking of a taskbar item on a given bar.
pub fn rect_key(bar: window::Id, item: &str) -> String {
    format!("{bar:?}|{item}")
}

impl Shell {
    pub(crate) fn persist_config(&self) {
        if let Some(h) = &self.config_handle
            && let Err(e) = self.config.write_entry(h)
        {
            tracing::error!("saving config: {e}");
        }
    }

    pub(crate) fn persist_state(&self) {
        if let Some(h) = &self.state_handle
            && let Err(e) = self.state.write_entry(h)
        {
            tracing::error!("saving state: {e}");
        }
    }

    pub(crate) fn request(&self, req: WaylandRequest) {
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

    fn create_bar(&mut self, output: WlOutput, output_height: Option<u32>, output_name: Option<String>) -> Task<Message> {
        let id = window::Id::unique();
        self.bars.push(Bar { id, output: output.clone(), output_name, output_height, revealed: !self.config.auto_hide, hovered: false, hide_pending: false });
        let zone = if self.config.auto_hide { 0 } else { style::TASKBAR_HEIGHT as i32 };
        let hidden_input = self.config.auto_hide.then(|| {
            let h = style::TASKBAR_HEIGHT as f32;
            vec![Rectangle { x: 0.0, y: h - crate::autohide::STRIP as f32, width: 100_000.0, height: crate::autohide::STRIP as f32 }]
        });
        let task = cosmic::surface::surface_task(app_layer_shell(
            move |shell: &Shell| LiveSettings { blur: Some(shell.bar_visible(id)), ..Default::default() },
            move |_: &mut Shell| SctkLayerSurfaceSettings {
                id,
                layer: Layer::Top,
                keyboard_interactivity: KeyboardInteractivity::None,
                anchor: Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                output: IcedOutput::Output(output.clone()),
                namespace: "lapin-ux-taskbar".into(),
                size: Some((None, Some(style::TASKBAR_HEIGHT))),
                exclusive_zone: zone,
                input_zone: hidden_input.clone(),
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

    pub(crate) fn open_start(&mut self) -> Task<Message> {
        let id = window::Id::unique();
        let height = self
            .bars
            .iter()
            .filter_map(|b| b.output_height)
            .min()
            .map(|h| h.saturating_sub(style::TASKBAR_HEIGHT + 24).min(style::START_HEIGHT))
            .unwrap_or(style::START_HEIGHT)
            .max(360);
        if self.catalog.changed_on_disk() {
            self.catalog = Catalog::load();
        }
        let index = self.refresh_file_index();
        self.start = Some(StartMenu {
            id,
            query: String::new(),
            page: StartPage::Home,
            selected: None,
            recent_files: lapin_ux_core::recent::load(6),
            folder: None,
            pin_drag: None,
        });
        let left = self.config.alignment == TaskbarAlignment::Left;
        let above = self.above_bar();
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
                    layer: Layer::Overlay,
                    keyboard_interactivity: KeyboardInteractivity::Exclusive,
                    anchor: if left { Anchor::BOTTOM | Anchor::LEFT } else { Anchor::BOTTOM },
                    output: IcedOutput::Active,
                    namespace: "lapin-ux-start".into(),
                    margin: IcedMargin { top: 0, right: 0, bottom: above, left: if left { 12 } else { 0 } },
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
            index,
        ])
    }

    pub(crate) fn close_start(&mut self) -> Task<Message> {
        let mut tasks = vec![self.close_popup()];
        if let Some(s) = self.start.take() {
            self.start_closed_at = Instant::now();
            tasks.push(destroy_layer_surface(s.id));
        }
        Task::batch(tasks)
    }

    pub(crate) fn close_popup(&mut self) -> Task<Message> {
        let sub = self.close_submenu();
        match self.popup.take() {
            Some(p) => Task::batch([sub, destroy_popup(p.id)]),
            None => sub,
        }
    }

    /// Open a popup anchored above the tracked rectangle `key` on surface `parent`.
    pub(crate) fn open_popup(&mut self, parent: window::Id, key: &str, kind: PopupKind) -> Task<Message> {
        let same = self.popup.as_ref().is_some_and(|p| p.kind == kind);
        let close = self.close_popup();
        if same {
            return close; // second click on the same thing closes it
        }
        let id = window::Id::unique();
        let r = self.rects.get(key).copied().unwrap_or(Rectangle { x: 0.0, y: 0.0, width: 1.0, height: 1.0 });
        let above = self.bars.iter().any(|b| b.id == parent);
        let at_pointer = matches!(kind, PopupKind::TaskbarMenu | PopupKind::DesktopMenu(..));
        // Hover previews must not take the pointer or keyboard.
        let grab = !matches!(kind, PopupKind::Preview(_));
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
                // Menus opened at the pointer grow up and to the right of
                // it; the rest are centred over the button they belong to.
                anchor: match (at_pointer, above) {
                    (true, _) => PAnchor::TopLeft,
                    (false, true) => PAnchor::Top,
                    (false, false) => PAnchor::Bottom,
                },
                gravity: match (at_pointer, above) {
                    (true, true) => Gravity::TopRight,
                    (true, false) => Gravity::BottomRight,
                    (false, true) => Gravity::Top,
                    (false, false) => Gravity::Bottom,
                },
                // slide_x | flip_y | resize_x | resize_y
                constraint_adjustment: 1 | 8 | 16 | 32,
                offset: (0, if above { -8 } else { 4 }),
                reactive: true,
            },
            parent_size: None,
            grab,
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

    pub(crate) fn launch(&mut self, app_id: &str) -> Task<Message> {
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
        if info.activated && !was_active {
            self.note_activation(info.id);
        } else if !self.mru.contains(&info.id) {
            self.mru.push(info.id);
        }
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
            outputs: info.outputs,
        });
    }

    fn window_closed(&mut self, id: Handle) {
        self.forget_window(id);
        self.taskbar.remove(&id);
        self.desktop_shown.retain(|h| h != &id);
    }

    /// Ordered list of what the Start menu currently shows, for keyboard nav.
    pub fn start_items(&self) -> Vec<crate::start_search::StartItem> {
        use crate::start_search::StartItem;
        let Some(s) = &self.start else { return Vec::new() };
        if !s.query.trim().is_empty() {
            return self.search(&s.query).flat();
        }
        let ids: Vec<String> = match s.page {
            StartPage::Home => {
                if let Some((_, _, apps)) = self.open_folder() {
                    return apps.into_iter().map(StartItem::App).collect();
                }
                return self
                    .visible_pins()
                    .into_iter()
                    .map(|(i, p)| match p {
                        lapin_ux_core::pins::Pin::App(a) => StartItem::App(a),
                        _ => StartItem::Pin(i),
                    })
                    .collect();
            }
            StartPage::AllApps => {
                let names: Vec<&str> = self.catalog.apps().iter().map(|a| a.data.name.as_str()).collect();
                lapin_ux_core::apps::alphabet_sections(&names)
                    .into_iter()
                    .flat_map(|sec| sec.items)
                    .map(|i| self.catalog.apps()[i].data.id.clone())
                    .collect()
            }
        };
        ids.into_iter().map(StartItem::App).collect()
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

impl Shell {
    /// An app's badge; ids are matched loosely since apps report their
    /// desktop id and windows their app id ("org.mozilla.firefox" vs "firefox").
    pub(crate) fn badge_for(&self, app: &str) -> Option<&lapin_ux_core::badge::Badge> {
        self.badges.get(app).or_else(|| {
            let short = |s: &str| s.rsplit('.').next().unwrap_or(s).to_lowercase();
            let a = short(app);
            self.badges.iter().find(|(k, _)| short(k) == a).map(|(_, b)| b)
        })
    }

    /// Files this app opened recently (newest first), for its taskbar menu.
    fn jump_list(&self, app: &str) -> Vec<RecentFile> {
        let Some(data) = self.catalog.data_for(app) else { return Vec::new() };
        let all = lapin_ux_core::recent::load(300);
        lapin_ux_core::recent::jump_list(&all, &data.name, data.exec.as_deref(), 8).into_iter().cloned().collect()
    }
}

/// SIGTERM the app's processes, then SIGKILL whatever is left 2 s later.
async fn end_task(m: lapin_ux_core::procs::AppMatch) {
    use std::os::unix::fs::MetadataExt;
    let proc = std::path::Path::new("/proc");
    let Ok(uid) = std::fs::metadata("/proc/self").map(|m| m.uid()) else { return };
    let me = std::process::id();
    let pids: Vec<u32> = lapin_ux_core::procs::find(proc, uid, &m).into_iter().filter(|p| *p != me).collect();
    tracing::info!(?m, ?pids, "end task");
    for &p in &pids {
        unsafe { libc::kill(p as i32, libc::SIGTERM) };
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    for p in lapin_ux_core::procs::find(proc, uid, &m) {
        if pids.contains(&p) {
            unsafe { libc::kill(p as i32, libc::SIGKILL) };
        }
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
            cfg.defaults_version = crate::defaults::DEFAULTS_VERSION;
            if let Some(h) = &config_handle {
                let _ = cfg.write_entry(h);
            }
        } else if cfg.defaults_version < crate::defaults::DEFAULTS_VERSION {
            crate::defaults::apply_upgrade(cfg.defaults_version);
            cfg.defaults_version = crate::defaults::DEFAULTS_VERSION;
            if let Some(h) = &config_handle {
                let _ = cfg.set_defaults_version(h, cfg.defaults_version);
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
            catcher: None,
            popup: None,
            rects: HashMap::new(),
            tracker: None,
            clock: ClockStyle::from_env(),
            now: chrono::Local::now().naive_local(),
            calendar: cosmic::widget::calendar::CalendarModel::now(),
            notify: Default::default(),
            quick: Default::default(),
            files: Default::default(),
            indexing: false,
            desktop: None,
            clip: Default::default(),
            thumbs: Default::default(),
            sub: None,
            jump: Vec::new(),
            badges: HashMap::new(),
            drag_hover: None,
            drag_serial: 0,
            hover: None,
            hover_gen: 0,
            mru: Vec::new(),
            desktop_shown: Vec::new(),
            start_pending: false,
            tray: Vec::new(),
            tray_menu_path: Vec::new(),
            panel: None,
            panel_closed_at: Instant::now() - Duration::from_secs(10),
            pointer: None,
            drag: None,
            suppress_click_until: Instant::now(),
            item_context_at: Instant::now() - Duration::from_secs(10),
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
        if self.catcher == Some(id) {
            return crate::catcher::view();
        }
        if self.desktop.as_ref().is_some_and(|d| d.id == id) {
            return crate::desktop::view(self);
        }
        if self.desktop.as_ref().is_some_and(|d| d.rename_surface == Some(id)) {
            return crate::desktop::rename_view(self);
        }
        if let Some(start) = self.start.as_ref().filter(|s| s.id == id) {
            return crate::start_view::start_menu(self, start);
        }
        if let Some(panel) = self.panel.as_ref().filter(|p| p.id == id) {
            return crate::panels::view(self, panel);
        }
        if let Some(sub) = self.sub.as_ref().filter(|p| p.id == id) {
            return cosmic::widget::autosize::autosize(crate::taskbar_view::popup(self, sub), crate::submenu::SUB_AUTOSIZE.clone())
                .min_width(1.0)
                .min_height(1.0)
                .max_width(440.0)
                .max_height(720.0)
                .into();
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
            cosmic::dbus_activation::Details::ActivateAction { action, args } => match action.as_str() {
                "start" => self.update(Message::ToggleStart),
                "desktop" => self.update(Message::ShowDesktop),
                "winx" => self.update(Message::Panel(crate::panels::PanelMsg::OpenWinX)),
                "run" => self.update(Message::Panel(crate::panels::PanelMsg::OpenRun)),
                "quick" => self.update(Message::Quick(crate::quick::QuickMsg::Toggle)),
                "alttab" => self.update(Message::Preview(crate::previews::PreviewMsg::AltTab(args.first().is_some_and(|a| a == "prev")))),
                "clipboard" => self.update(Message::Clip(crate::clipboard::ClipMsg::Open(crate::clipboard::Tab::Clipboard))),
                "emoji" => self.update(Message::Clip(crate::clipboard::ClipMsg::Open(crate::clipboard::Tab::Emoji))),
                "notifications" => self.update(Message::ToggleNotifications),
                "taskbar-item" => match args.first().and_then(|a| a.parse::<usize>().ok()) {
                    Some(n) => self.update(Message::TaskbarItem(n)),
                    None => Task::none(),
                },
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
            crate::notify_client::subscription().map(Message::NotifyState),
            crate::launcher_entry::subscription().map(Message::Badge),
            crate::clipboard::subscription().map(Message::Clip),
            crate::quick::backend::subscriptions().map(|e| Message::Quick(crate::quick::QuickMsg::Event(e))),
            wl::subscription().map(Message::Wayland),
            self.core.watch_config::<ShellConfig>(lapin_ux_core::ids::SHELL_APP_ID).map(|u| Message::Config(u.config)),
            rectangle_tracker_subscription(0).map(|(_, u)| Message::Rect(u)),
            cosmic::iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick),
            listen_raw(|event, _status, window| {
                match event {
                cosmic::iced::Event::PlatformSpecific(PlatformSpecific::Wayland(w)) => match w {
                    wayland::Event::Layer(e, _, id) => Some(Message::Layer(e, id)),
                    wayland::Event::Output(e, output) => Some(Message::Output(e, output)),
                    wayland::Event::Popup(PopupEvent::Done, _, id) => Some(Message::PopupClosed(id)),
                    _ => None,
                },
                cosmic::iced::Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(named), .. }) => {
                    Some(Message::KeyNamed(window, named))
                }
                // The pointer reaching the hidden taskbar strip (it's too thin
                // for a widget to see any motion inside it).
                cosmic::iced::Event::Mouse(mouse::Event::CursorEntered) => Some(Message::BarHover(window, true)),
                cosmic::iced::Event::Mouse(mouse::Event::CursorLeft) => {
                    Some(Message::Preview(crate::previews::PreviewMsg::SurfaceLeft(window)))
                }
                cosmic::iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                    Some(Message::Preview(crate::previews::PreviewMsg::Alt(window, m.alt())))
                }
                cosmic::iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::BarPointer(window, Pointer::Move(position)))
                }
                cosmic::iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    Some(Message::BarPointer(window, Pointer::Down))
                }
                cosmic::iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Message::BarPointer(window, Pointer::Up))
                }
                _ => None,
            }}),
        ]))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        Task::batch([task, self.sync_catcher(), self.sync_autohide()])
    }
}

impl Shell {
    fn handle(&mut self, message: Message) -> Task<Message> {
        if !matches!(message, Message::Tick | Message::Rect(_) | Message::Wayland(_) | Message::BarPointer(..)) {
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
                WaylandUpdate::Thumbnail { id, width, height, rgba } => {
                    return self.update_preview(crate::previews::PreviewMsg::Thumbnail(id, width, height, rgba));
                }
            },
            Message::Output(event, output) => match event {
                OutputEvent::Created(info) => {
                    if !self.bars.iter().any(|b| b.output == output) {
                        let size = info.as_ref().and_then(|i| i.logical_size).map(|(w, h)| (w.max(0) as u32, h.max(0) as u32));
                        let name = info.as_ref().and_then(|i| i.name.clone());
                        let bar = self.create_bar(output.clone(), size.map(|s| s.1), name);
                        // Desktop icons live on the first monitor, like the benchmark's main display.
                        let desk = self.create_desktop(output, size.unwrap_or((1280, 720)));
                        return Task::batch([bar, desk]);
                    }
                }
                OutputEvent::InfoUpdate(info) => {
                    if let Some(bar) = self.bars.iter_mut().find(|b| b.output == output) {
                        bar.output_height = info.logical_size.map(|(_, h)| h.max(0) as u32);
                        if info.name.is_some() {
                            bar.output_name = info.name.clone();
                        }
                    }
                    if let Some((w, h)) = info.logical_size {
                        self.desktop_resized(&output, (w.max(0) as u32, h.max(0) as u32));
                    }
                }
                OutputEvent::Removed => {
                    let mut tasks = Vec::new();
                    if self.desktop.as_ref().is_some_and(|d| d.output == output) {
                        tasks.push(self.destroy_desktop());
                    }
                    if let Some(pos) = self.bars.iter().position(|b| b.output == output) {
                        let bar = self.bars.remove(pos);
                        tasks.push(destroy_layer_surface(bar.id));
                    }
                    // Move the desktop to a remaining monitor.
                    if self.desktop.is_none()
                        && let Some(b) = self.bars.first()
                    {
                        let (o, h) = (b.output.clone(), b.output_height.unwrap_or(720));
                        tasks.push(self.create_desktop(o, (1920, h)));
                    }
                    return Task::batch(tasks);
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
            Message::Layer(LayerEvent::Focused, id) if self.panel.as_ref().is_some_and(|p| p.id == id) => {
                return self.panel_focused();
            }
            Message::Layer(LayerEvent::Focused, id) if self.desktop.as_ref().is_some_and(|d| d.rename_surface == Some(id)) => {
                return self.rename_focused();
            }
            Message::Layer(LayerEvent::Unfocused, id) if self.desktop.as_ref().is_some_and(|d| d.rename_surface == Some(id)) => {
                // Clicked somewhere else: keep the new name.
                return self.update_desktop(crate::desktop::DesktopMsg::RenameCommit);
            }
            Message::Layer(LayerEvent::Unfocused, id) if self.panel.as_ref().is_some_and(|p| p.id == id) => {
                return self.close_panel();
            }
            Message::Layer(..) => {}
            Message::PopupClosed(id) => {
                if self.popup.as_ref().is_some_and(|p| p.id == id) {
                    self.popup = None;
                }
                if self.sub.as_ref().is_some_and(|p| p.id == id) {
                    self.sub = None;
                }
            }
            Message::Config(cfg) => {
                if cfg.pinned_taskbar != *self.taskbar.pinned() {
                    self.taskbar.set_pinned(cfg.pinned_taskbar.clone());
                }
                let auto_hide_changed = cfg.auto_hide != self.config.auto_hide;
                self.config = cfg;
                if auto_hide_changed {
                    return self.auto_hide_changed();
                }
            }
            Message::Tick => {
                self.now = chrono::Local::now().naive_local();
                self.desktop_tick();
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
                let close = Task::batch([self.close_popup(), self.close_panel()]);
                return Task::batch([close, self.open_start()]);
            }
            Message::KeyNamed(id, named) => {
                if self.start.as_ref().is_some_and(|s| s.id == id) {
                    let msg = match named {
                        Named::Escape if self.start.as_ref().is_some_and(|s| s.folder.is_some()) => {
                            Message::Pins(crate::start_pins::PinMsg::CloseFolder)
                        }
                        Named::Escape => Message::CloseStart,
                        Named::ArrowDown => Message::StartNav(Nav::Down),
                        Named::ArrowUp => Message::StartNav(Nav::Up),
                        Named::ArrowLeft => Message::StartNav(Nav::Left),
                        Named::ArrowRight => Message::StartNav(Nav::Right),
                        _ => return Task::none(),
                    };
                    return self.update(msg);
                }
                if self.panel.as_ref().is_some_and(|p| p.id == id) {
                    return self.panel_key(named);
                }
                if self.desktop.as_ref().is_some_and(|d| d.id == id || d.rename_surface == Some(id)) {
                    return self.update_desktop(crate::desktop::DesktopMsg::Key(named));
                }
            }
            Message::OpenStartAt(page) => {
                let close = self.close_panel();
                let open = if self.start.is_none() { self.open_start() } else { Task::none() };
                if let Some(s) = &mut self.start {
                    s.page = page;
                    s.selected = None;
                    s.query.clear();
                }
                return Task::batch([close, open]);
            }
            Message::Panel(m) => return self.update_panel(m),
            Message::TaskbarItem(n) => return self.taskbar_item(n),
            Message::TaskbarContext(bar) => return self.taskbar_context(bar),
            Message::TaskbarOption(o) => return self.taskbar_option(o),
            Message::BarPointer(id, ev) => {
                if self.start.as_ref().is_some_and(|s| s.id == id) {
                    return self.start_pointer(ev);
                }
                return self.bar_pointer(id, ev);
            }
            Message::TrayOverflow(bar) => return self.tray_overflow(bar),
            Message::TrayPin(id, inline) => return self.tray_pin(id, inline),
            Message::CloseStart => {
                if self.popup.is_some() {
                    return self.close_popup();
                }
                if let Some(s) = &mut self.start
                    && (!s.query.is_empty() || s.page != StartPage::Home)
                {
                    // Esc steps back: clear search / leave All apps first.
                    // (The text box drops focus on Esc; take it back so typing works.)
                    s.query.clear();
                    s.page = StartPage::Home;
                    s.selected = None;
                    return text_input::focus(SEARCH_ID.clone());
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
                if let Some(item) = items.get(selected).or(items.first()).cloned() {
                    return self.activate(item);
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
            Message::DragHover(app, inside) => return self.drag_hover(app, inside),
            Message::DragHoverFire(serial) => return self.drag_hover_fire(serial),
            Message::Badge((app, props)) => {
                let b = self.badges.entry(app.clone()).or_default();
                for p in props {
                    b.apply(p);
                }
                if b.is_empty() {
                    self.badges.remove(&app);
                }
            }
            Message::EndTask(app) => {
                let close = self.close_popup();
                let exec = self.catalog.data_for(&app).and_then(|d| d.exec.clone());
                let Some(m) = lapin_ux_core::procs::app_match(&app, exec.as_deref()) else { return close };
                let end = Task::future(async move {
                    end_task(m).await;
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([close, end]);
            }
            Message::OpenWith(app, path) => {
                let close = self.close_popup();
                let exec = self.catalog.data_for(&app).and_then(|d| d.exec.clone());
                let open = Task::future(async move {
                    let args = match exec {
                        Some(e) => lapin_ux_core::recent::exec_with_file(&e, &path),
                        None => vec!["xdg-open".into(), path.to_string_lossy().into_owned()],
                    };
                    if let Some((prog, rest)) = args.split_first() {
                        if let Err(e) = tokio::process::Command::new(prog).args(rest).spawn() {
                            tracing::warn!("jump list: {prog}: {e}");
                        }
                    }
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([close, open]);
            }
            Message::OpenFile(path) => {
                let close = self.close_start();
                let open = Task::future(async move {
                    let _ = tokio::process::Command::new("xdg-open").arg(path).spawn();
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([close, open]);
            }
            Message::TaskClick(bar, app) => {
                if self.click_suppressed() {
                    return Task::none();
                }
                let close = Task::batch([self.close_preview(), self.close_popup()]);
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
                        self.hover = None;
                        for w in self.taskbar.windows().iter().filter(|w| w.app == app) {
                            self.request(WaylandRequest::Capture(w.handle));
                        }
                        let key = rect_key(bar, &app);
                        return self.open_popup(bar, &key, PopupKind::Windows(app));
                    }
                };
                return Task::batch([close, next]);
            }
            Message::TaskContext(bar, app) => {
                self.note_item_context();
                let key = rect_key(bar, &app);
                self.jump = self.jump_list(&app);
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
                self.pin_app(app);
                return self.close_popup();
            }
            Message::UnpinStart(app) => {
                self.unpin_app(&app);
                return self.close_popup();
            }
            Message::Pins(m) => return self.update_pins(m),
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
            Message::NotifyState(s) => self.notify = s,
            Message::CatcherClick => return self.catcher_click(),
            Message::FilesIndexed(idx) => {
                self.files = idx;
                self.indexing = false;
            }
            Message::ReloadApps => self.catalog = Catalog::load(),
            Message::Desktop(m) => return self.update_desktop(m),
            Message::Clip(m) => return self.update_clip(m),
            Message::Preview(m) => return self.update_preview(m),
            Message::CloseSubmenu => return self.close_submenu(),
            Message::BarHover(id, inside) => self.bar_hover(id, inside),
            Message::AutoHide(id) => return self.auto_hide_now(id),
            Message::UninstallAsk(app) => return self.uninstall_ask(app),
            Message::Uninstall(app) => return self.uninstall(app),
            Message::StartActivate(item) => return self.activate(item),
            Message::ShowInFolder(path) => {
                let close = self.close_start();
                return Task::batch([close, crate::start_search::show_in_folder(path)]);
            }
            Message::Quick(m) => return self.update_quick(m),
            Message::ToggleNotifications => {
                let close = Task::batch([self.close_start(), self.close_popup(), self.close_panel()]);
                let toggle = Task::future(async {
                    crate::notify_client::toggle_center().await;
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([close, toggle]);
            }
            Message::ClockClick(bar) => {
                if self.notify.available {
                    let close = Task::batch([self.close_start(), self.close_popup(), self.close_panel()]);
                    let toggle = Task::future(async move {
                        if crate::notify_client::toggle_center().await {
                            cosmic::Action::App(Message::Noop)
                        } else {
                            cosmic::Action::App(Message::CalendarFallback(bar))
                        }
                    });
                    return Task::batch([close, toggle]);
                }
                return self.update(Message::CalendarFallback(bar));
            }
            Message::CalendarFallback(bar) => {
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
                    self.tray.push(TrayEntry { item, id: None, icon: None, title: None, menu: None });
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
                    if update.id.is_some() {
                        t.id = update.id.clone();
                    }
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
                self.note_item_context();
                let Some(t) = self.tray.iter().find(|t| t.item.name() == name) else { return Task::none() };
                // Icons in the overflow menu anchor their menu to the ^ button.
                let anchor_key = if self.tray_inline(t) {
                    rect_key(bar, &format!("tray:{name}"))
                } else {
                    rect_key(bar, "tray-overflow")
                };
                match t.item.menu_proxy().cloned() {
                    Some(menu) => {
                        self.tray_menu_path.clear();
                        let open = self.open_popup(bar, &anchor_key, PopupKind::TrayMenu(name));
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
