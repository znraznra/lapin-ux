// SPDX-License-Identifier: GPL-3.0-only
// Parts adapted from cosmic-applets/cosmic-app-list (Copyright 2023 System76, GPL-3.0-only).
//! Window tracking for the taskbar.
//!
//! Runs a second Wayland connection on its own thread and reports window
//! open/close/title/focus changes. Two backends:
//!
//! * cosmic-comp: `ext-foreign-toplevel-list` + `zcosmic_toplevel_info/management`
//! * wlroots compositors (sway, Hyprland, labwc, niri, …):
//!   `zwlr_foreign_toplevel_management_v1`
//!
//! The UI only sees opaque [`WindowId`]s, never protocol objects.

use std::collections::HashMap;
use std::sync::LazyLock;

use cosmic::cctk::{
    self,
    cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1},
    cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1,
    cosmic_protocols::a11y::v1::client::cosmic_a11y_manager_v1::{self, CosmicA11yManagerV1},
    cosmic_protocols::keyboard_layout::v1::client::{
        zcosmic_keyboard_layout_manager_v1::ZcosmicKeyboardLayoutManagerV1,
        zcosmic_keyboard_layout_v1::{self, ZcosmicKeyboardLayoutV1},
    },
    sctk::{
        self,
        activation::{ActivationHandler, ActivationState, RequestData, RequestDataExt},
        output::{OutputHandler, OutputState},
        reexports::protocols_wlr::foreign_toplevel::v1::client::{
            zwlr_foreign_toplevel_handle_v1::{self as wlr_handle, ZwlrForeignToplevelHandleV1},
            zwlr_foreign_toplevel_manager_v1::{self as wlr_manager, ZwlrForeignToplevelManagerV1},
        },
        reexports::{calloop, calloop_wayland_source::WaylandSource},
        registry::{ProvidesRegistryState, RegistryState},
        seat::{SeatHandler, SeatState},
        shm::{Shm, ShmHandler, slot::{Buffer, SlotPool}},
    },
    screencopy::{
        CaptureFrame, CaptureOptions, CaptureSession, CaptureSource, Formats, Frame, ScreencopyFrameData,
        ScreencopyFrameDataExt, ScreencopyHandler, ScreencopySessionData, ScreencopySessionDataExt, ScreencopyState,
    },
    toplevel_info::{ToplevelInfoHandler, ToplevelInfoState},
    workspace::{WorkspaceHandler, WorkspaceState},
    wayland_protocols::ext::workspace::v1::client::{ext_workspace_group_handle_v1, ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1}},
    toplevel_management::{ToplevelManagerHandler, ToplevelManagerState},
    wayland_client::{
        Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child,
        backend::ObjectId,
        globals::{GlobalList, registry_queue_init},
        protocol::{wl_keyboard, wl_output, wl_seat::WlSeat, wl_shm, wl_surface::WlSurface},
    },
    wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
};
use cosmic::iced::{self, Subscription, stream};
use futures::{
    SinkExt, StreamExt,
    channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded},
};
use tokio::sync::Mutex;

/// Opaque, stable id for one window for as long as it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(u64);

#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub id: WindowId,
    pub app_id: String,
    pub title: String,
    pub activated: bool,
    pub minimized: bool,
    /// Monitor names (e.g. "eDP-1") the window is on.
    pub outputs: Vec<String>,
    /// Virtual desktops the window is on.
    pub workspaces: Vec<WorkspaceId>,
}

/// Opaque id of a virtual desktop (workspace).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(u64);

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceInfo {
    pub id: WorkspaceId,
    pub name: String,
    pub active: bool,
    /// Monitors it belongs to.
    pub outputs: Vec<String>,
    pub can_remove: bool,
}

#[derive(Clone, Debug)]
pub enum WaylandUpdate {
    Init(calloop::channel::Sender<WaylandRequest>),
    Finished,
    Window(WindowInfo),
    Closed(WindowId),
    ActivationToken { token: Option<String>, app_id: String, exec: String, terminal: bool },
    /// A preview picture of a window (RGBA).
    Thumbnail { id: WindowId, width: u32, height: u32, rgba: std::sync::Arc<Vec<u8>> },
    /// Accessibility state from the compositor (cosmic-comp only).
    A11y(A11yState),
    /// The active keyboard layout (index into the configured list).
    KeyboardGroup(u32),
    /// Virtual desktops in order, and whether new ones can be made.
    Workspaces(Vec<WorkspaceInfo>, bool),
}

/// What the compositor reports about its accessibility features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct A11yState {
    /// The compositor supports the accessibility protocol.
    pub available: bool,
    pub magnifier: bool,
    pub inverted: bool,
    /// Colour filter in use (greyscale, colour-blindness corrections…).
    pub filter: u32,
    pub filter_on: bool,
}

#[derive(Clone, Debug)]
pub enum WaylandRequest {
    Activate(WindowId),
    Minimize(WindowId),
    Close(WindowId),
    /// Ask the compositor for an activation token so the launched app gets focus.
    TokenRequest { app_id: String, exec: String, terminal: bool },
    /// Capture one frame of a window for a thumbnail.
    Capture(WindowId),
    SetMagnifier(bool),
    SetColorFilter(bool),
    /// Switch to keyboard layout `n` of the configured list.
    SetKeyboardGroup(u32),
    ActivateWorkspace(WorkspaceId),
    /// "New desktop".
    NewWorkspace,
    RemoveWorkspace(WorkspaceId),
    /// Move a window to another desktop.
    MoveToWorkspace(WindowId, WorkspaceId),
}

/// Thumbnails are captured at most this size.
const THUMB_MAX: (u32, u32) = (480, 300);

struct SessionData {
    window: WindowId,
    inner: ScreencopySessionData,
}
impl ScreencopySessionDataExt for SessionData {
    fn screencopy_session_data(&self) -> &ScreencopySessionData {
        &self.inner
    }
}
struct FrameData {
    window: WindowId,
    inner: ScreencopyFrameData,
}
impl ScreencopyFrameDataExt for FrameData {
    fn screencopy_frame_data(&self) -> &ScreencopyFrameData {
        &self.inner
    }
}

/// One in-flight window capture.
struct Capture {
    session: CaptureSession,
    pool: Option<SlotPool>,
    buffer: Option<Buffer>,
    format: Option<wl_shm::Format>,
    size: (u32, u32),
}

static WAYLAND_RX: LazyLock<Mutex<Option<UnboundedReceiver<WaylandUpdate>>>> = LazyLock::new(|| Mutex::new(None));

pub fn subscription() -> Subscription<WaylandUpdate> {
    Subscription::run_with(std::any::TypeId::of::<WaylandUpdate>(), |_| {
        stream::channel(50, move |mut output: futures::channel::mpsc::Sender<WaylandUpdate>| async move {
            let mut guard = WAYLAND_RX.lock().await;
            if guard.is_none() {
                let (calloop_tx, calloop_rx) = calloop::channel::channel();
                let (tx, rx) = unbounded();
                std::thread::Builder::new()
                    .name("lapin-ux-toplevels".into())
                    .spawn(move || run(tx, calloop_rx))
                    .expect("spawning toplevel thread");
                *guard = Some(rx);
                let _ = output.send(WaylandUpdate::Init(calloop_tx)).await;
            }
            let rx = guard.as_mut().unwrap();
            while let Some(u) = rx.next().await {
                let _ = output.send(u).await;
            }
            let _ = output.send(WaylandUpdate::Finished).await;
            tracing::error!("toplevel thread ended");
            iced::futures::future::pending::<()>().await;
        })
    })
}

enum Backend {
    Cosmic { info: ToplevelInfoState, manager: Option<ToplevelManagerState> },
    Wlr { _manager: ZwlrForeignToplevelManagerV1 },
}

/// Protocol object behind a WindowId.
#[derive(Clone)]
enum Toplevel {
    Cosmic(ExtForeignToplevelHandleV1),
    Wlr(ZwlrForeignToplevelHandleV1),
}

/// wlr sends state in pieces followed by `done`.
#[derive(Default)]
struct WlrPending {
    title: String,
    app_id: String,
    activated: bool,
    minimized: bool,
    outputs: Vec<wl_output::WlOutput>,
}

struct AppData {
    exit: bool,
    tx: UnboundedSender<WaylandUpdate>,
    queue_handle: QueueHandle<Self>,
    output_state: OutputState,
    registry_state: RegistryState,
    seat_state: SeatState,
    activation_state: Option<ActivationState>,
    backend: Option<Backend>,
    next_id: u64,
    ids: HashMap<ObjectId, WindowId>,
    objects: HashMap<WindowId, Toplevel>,
    wlr_pending: HashMap<ObjectId, WlrPending>,
    screencopy: Option<ScreencopyState>,
    shm: Option<Shm>,
    captures: HashMap<WindowId, Capture>,
    a11y: Option<CosmicA11yManagerV1>,
    a11y_state: A11yState,
    layout_manager: Option<ZcosmicKeyboardLayoutManagerV1>,
    keyboard_layout: Option<ZcosmicKeyboardLayoutV1>,
    workspace_state: WorkspaceState,
    ws_ids: HashMap<ObjectId, WorkspaceId>,
    ws_objects: HashMap<WorkspaceId, ExtWorkspaceHandleV1>,
}

impl AppData {
    fn id_for(&mut self, obj: ObjectId, toplevel: Toplevel) -> WindowId {
        if let Some(id) = self.ids.get(&obj) {
            return *id;
        }
        self.next_id += 1;
        let id = WindowId(self.next_id);
        self.ids.insert(obj, id);
        self.objects.insert(id, toplevel);
        id
    }

    fn forget(&mut self, obj: &ObjectId) -> Option<WindowId> {
        let id = self.ids.remove(obj)?;
        self.objects.remove(&id);
        self.wlr_pending.remove(obj);
        Some(id)
    }

    fn cosmic_handle(&self, ext: &ExtForeignToplevelHandleV1) -> Option<ZcosmicToplevelHandleV1> {
        match &self.backend {
            Some(Backend::Cosmic { info, .. }) => info.info(ext)?.cosmic_toplevel.clone(),
            _ => None,
        }
    }

    fn handle(&mut self, req: WaylandRequest) {
        match req {
            WaylandRequest::SetMagnifier(on) => return self.set_a11y(Some(on), None),
            WaylandRequest::SetColorFilter(on) => return self.set_a11y(None, Some(on)),
            WaylandRequest::SetKeyboardGroup(g) => {
                if let Some(k) = &self.keyboard_layout {
                    k.set_group(g);
                }
                return;
            }
            WaylandRequest::ActivateWorkspace(_)
            | WaylandRequest::NewWorkspace
            | WaylandRequest::RemoveWorkspace(_)
            | WaylandRequest::MoveToWorkspace(..) => return self.workspace_request(req),
            _ => {}
        }
        let target = |id: &WindowId| self.objects.get(id).cloned();
        match req {
            WaylandRequest::SetMagnifier(_)
            | WaylandRequest::SetColorFilter(_)
            | WaylandRequest::SetKeyboardGroup(_)
            | WaylandRequest::ActivateWorkspace(_)
            | WaylandRequest::NewWorkspace
            | WaylandRequest::RemoveWorkspace(_)
            | WaylandRequest::MoveToWorkspace(..) => {}
            WaylandRequest::Activate(id) => match target(&id) {
                Some(Toplevel::Cosmic(ext)) => {
                    if let (Some(Backend::Cosmic { manager: Some(m), .. }), Some(t), Some(seat)) =
                        (&self.backend, self.cosmic_handle(&ext), self.seat_state.seats().next())
                    {
                        m.manager.activate(&t, &seat);
                    }
                }
                Some(Toplevel::Wlr(h)) => {
                    if let Some(seat) = self.seat_state.seats().next() {
                        h.activate(&seat);
                    }
                }
                None => {}
            },
            WaylandRequest::Minimize(id) => match target(&id) {
                Some(Toplevel::Cosmic(ext)) => {
                    if let (Some(Backend::Cosmic { manager: Some(m), .. }), Some(t)) = (&self.backend, self.cosmic_handle(&ext)) {
                        m.manager.set_minimized(&t);
                    }
                }
                Some(Toplevel::Wlr(h)) => h.set_minimized(),
                None => {}
            },
            WaylandRequest::Close(id) => match target(&id) {
                Some(Toplevel::Cosmic(ext)) => {
                    if let (Some(Backend::Cosmic { manager: Some(m), .. }), Some(t)) = (&self.backend, self.cosmic_handle(&ext)) {
                        m.manager.close(&t);
                    }
                }
                Some(Toplevel::Wlr(h)) => h.close(),
                None => {}
            },
            WaylandRequest::Capture(id) => self.start_capture(id),
            WaylandRequest::TokenRequest { app_id, exec, terminal } => match &self.activation_state {
                Some(state) => state.request_token_with_data(
                    &self.queue_handle,
                    ExecRequestData {
                        data: RequestData {
                            app_id: Some(app_id),
                            seat_and_serial: self.seat_state.seats().next().map(|s| (s, 0)),
                            surface: None,
                        },
                        exec,
                        terminal,
                    },
                ),
                None => {
                    let _ = self.tx.unbounded_send(WaylandUpdate::ActivationToken { token: None, app_id, exec, terminal });
                }
            },
        }
    }

    fn start_capture(&mut self, id: WindowId) {
        if self.captures.contains_key(&id) {
            return;
        }
        let (Some(Toplevel::Cosmic(ext)), Some(sc)) = (self.objects.get(&id).cloned(), &self.screencopy) else { return };
        let data = SessionData { window: id, inner: Default::default() };
        match sc.capturer().create_session(&CaptureSource::Toplevel(ext), CaptureOptions::empty(), &self.queue_handle, data) {
            Ok(session) => {
                self.captures.insert(id, Capture { session, pool: None, buffer: None, format: None, size: (0, 0) });
            }
            Err(e) => tracing::debug!("capture: {e}"),
        }
    }

    fn output_names<'a>(&self, outputs: impl IntoIterator<Item = &'a wl_output::WlOutput>) -> Vec<String> {
        outputs.into_iter().filter_map(|o| self.output_state.info(o).and_then(|i| i.name)).collect()
    }

    fn send_cosmic(&mut self, ext: &ExtForeignToplevelHandleV1) {
        let Some(Backend::Cosmic { info, .. }) = &self.backend else { return };
        let Some(info) = info.info(ext).cloned() else { return };
        let outputs = self.output_names(&info.output);
        let workspaces = info.workspace.iter().map(|w| self.ws_id(w)).collect();
        let id = self.id_for(ext.id(), Toplevel::Cosmic(ext.clone()));
        let _ = self.tx.unbounded_send(WaylandUpdate::Window(WindowInfo {
            id,
            app_id: info.app_id,
            title: info.title,
            activated: info.state.contains(&zcosmic_toplevel_handle_v1::State::Activated),
            minimized: info.state.contains(&zcosmic_toplevel_handle_v1::State::Minimized),
            outputs,
            workspaces,
        }));
    }

    fn ws_id(&mut self, h: &ExtWorkspaceHandleV1) -> WorkspaceId {
        if let Some(id) = self.ws_ids.get(&h.id()) {
            return *id;
        }
        self.next_id += 1;
        let id = WorkspaceId(self.next_id);
        self.ws_ids.insert(h.id(), id);
        self.ws_objects.insert(id, h.clone());
        id
    }

    fn workspace_request(&mut self, req: WaylandRequest) {
        let Ok(manager) = self.workspace_state.workspace_manager().get().cloned() else { return };
        match req {
            WaylandRequest::ActivateWorkspace(id) => {
                if let Some(h) = self.ws_objects.get(&id) {
                    h.activate();
                }
            }
            WaylandRequest::RemoveWorkspace(id) => {
                if let Some(h) = self.ws_objects.get(&id) {
                    h.remove();
                }
            }
            WaylandRequest::NewWorkspace => {
                let group = self
                    .workspace_state
                    .workspace_groups()
                    .find(|g| g.capabilities.contains(ext_workspace_group_handle_v1::GroupCapabilities::CreateWorkspace))
                    .map(|g| g.handle.clone());
                match group {
                    Some(g) => g.create_workspace(String::new()),
                    // COSMIC keeps an empty desktop at the end: go there.
                    None => {
                        let mut all: Vec<_> = self.workspace_state.workspaces().cloned().collect();
                        all.sort_by(|a, b| a.coordinates.cmp(&b.coordinates));
                        if let Some(last) = all.last() {
                            last.handle.activate();
                        }
                    }
                }
            }
            WaylandRequest::MoveToWorkspace(win, ws) => {
                if let (Some(Toplevel::Cosmic(ext)), Some(wh)) = (self.objects.get(&win).cloned(), self.ws_objects.get(&ws).cloned())
                    && let (Some(Backend::Cosmic { manager: Some(m), info }), Some(t)) = (&self.backend, self.cosmic_handle(&ext))
                    && let Some(output) = info.info(&ext).and_then(|i| i.output.iter().next().cloned())
                    && m.manager.version() >= 4
                {
                    m.manager.move_to_ext_workspace(&t, &wh, &output);
                }
            }
            _ => return,
        }
        manager.commit();
    }
}

impl WorkspaceHandler for AppData {
    fn workspace_state(&mut self) -> &mut WorkspaceState {
        &mut self.workspace_state
    }

    fn done(&mut self) {
        let groups: Vec<_> = self.workspace_state.workspace_groups().cloned().collect();
        let can_create = groups.iter().any(|g| g.capabilities.contains(ext_workspace_group_handle_v1::GroupCapabilities::CreateWorkspace));
        let mut all: Vec<_> = self.workspace_state.workspaces().cloned().collect();
        all.sort_by(|a, b| a.coordinates.cmp(&b.coordinates));
        let list: Vec<WorkspaceInfo> = all
            .iter()
            .map(|w| {
                let outputs = groups
                    .iter()
                    .filter(|g| g.workspaces.contains(&w.handle))
                    .flat_map(|g| self.output_names(&g.outputs))
                    .collect();
                WorkspaceInfo {
                    id: self.ws_id(&w.handle),
                    name: w.name.clone(),
                    active: w.state.contains(ext_workspace_handle_v1::State::Active),
                    outputs,
                    can_remove: w.capabilities.contains(ext_workspace_handle_v1::WorkspaceCapabilities::Remove),
                }
            })
            .collect();
        let _ = self.tx.unbounded_send(WaylandUpdate::Workspaces(list, can_create));
    }
}

fn pick_backend(globals: &GlobalList, registry: &RegistryState, qh: &QueueHandle<AppData>) -> Option<Backend> {
    if let Some(info) = ToplevelInfoState::try_new(registry, qh) {
        let manager = ToplevelManagerState::try_new(registry, qh);
        if manager.is_none() {
            tracing::warn!("no toplevel management protocol; taskbar clicks cannot focus windows");
        }
        tracing::info!("window tracking: cosmic protocols");
        return Some(Backend::Cosmic { info, manager });
    }
    if let Ok(m) = globals.bind::<ZwlrForeignToplevelManagerV1, _, _>(qh, 1..=3, ()) {
        tracing::info!("window tracking: wlr-foreign-toplevel-management");
        return Some(Backend::Wlr { _manager: m });
    }
    None
}

fn run(tx: UnboundedSender<WaylandUpdate>, rx: calloop::channel::Channel<WaylandRequest>) {
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("toplevel thread: cannot connect to Wayland: {e}");
            return;
        }
    };
    let (globals, event_queue) = registry_queue_init(&conn).expect("wayland registry");
    let mut event_loop = calloop::EventLoop::<AppData>::try_new().expect("calloop");
    let qh = event_queue.handle();
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .expect("insert wayland source");

    if event_loop
        .handle()
        .insert_source(rx, |event, (), state| match event {
            calloop::channel::Event::Msg(req) => state.handle(req),
            calloop::channel::Event::Closed => state.exit = true,
        })
        .is_err()
    {
        return;
    }

    let registry_state = RegistryState::new(&globals);
    let registry_state_for_ws = RegistryState::new(&globals);
    let backend = pick_backend(&globals, &registry_state, &qh);
    if backend.is_none() {
        tracing::warn!("compositor offers no window-list protocol; running windows will not be shown");
        return;
    }
    let mut data = AppData {
        exit: false,
        tx,
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        activation_state: ActivationState::bind::<AppData>(&globals, &qh).ok(),
        registry_state,
        queue_handle: qh.clone(),
        backend,
        next_id: 0,
        ids: HashMap::new(),
        objects: HashMap::new(),
        wlr_pending: HashMap::new(),
        screencopy: Some(ScreencopyState::new(&globals, &qh)),
        shm: Shm::bind(&globals, &qh).ok(),
        captures: HashMap::new(),
        a11y: globals.bind::<CosmicA11yManagerV1, _, _>(&qh, 1..=3, ()).ok(),
        a11y_state: A11yState::default(),
        layout_manager: globals.bind::<ZcosmicKeyboardLayoutManagerV1, _, _>(&qh, 1..=1, ()).ok(),
        keyboard_layout: None,
        workspace_state: WorkspaceState::new(&registry_state_for_ws, &qh),
        ws_ids: HashMap::new(),
        ws_objects: HashMap::new(),
    };
    if data.a11y.is_some() {
        data.a11y_state.available = true;
        let _ = data.tx.unbounded_send(WaylandUpdate::A11y(data.a11y_state));
    }

    while !data.exit {
        if let Err(e) = event_loop.dispatch(None, &mut data) {
            tracing::error!("toplevel thread: {e}");
            break;
        }
    }
}

// ---- wlr backend -------------------------------------------------------------

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: wlr_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wlr_manager::Event::Toplevel { toplevel } = event {
            state.wlr_pending.insert(toplevel.id(), WlrPending::default());
            state.id_for(toplevel.id(), Toplevel::Wlr(toplevel));
        }
    }

    event_created_child!(AppData, ZwlrForeignToplevelManagerV1, [
        wlr_manager::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for AppData {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: wlr_handle::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let key = handle.id();
        match event {
            wlr_handle::Event::Title { title } => state.wlr_pending.entry(key).or_default().title = title,
            wlr_handle::Event::AppId { app_id } => state.wlr_pending.entry(key).or_default().app_id = app_id,
            wlr_handle::Event::State { state: bytes } => {
                let flags: Vec<u32> =
                    bytes.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect();
                let p = state.wlr_pending.entry(key).or_default();
                p.activated = flags.contains(&(wlr_handle::State::Activated as u32));
                p.minimized = flags.contains(&(wlr_handle::State::Minimized as u32));
            }
            wlr_handle::Event::OutputEnter { output } => state.wlr_pending.entry(key).or_default().outputs.push(output),
            wlr_handle::Event::OutputLeave { output } => state.wlr_pending.entry(key).or_default().outputs.retain(|o| o != &output),
            wlr_handle::Event::Done => {
                let id = state.id_for(key.clone(), Toplevel::Wlr(handle.clone()));
                if let Some(p) = state.wlr_pending.get(&key) {
                    let outputs = state.output_names(&p.outputs);
                    let _ = state.tx.unbounded_send(WaylandUpdate::Window(WindowInfo {
                        id,
                        app_id: p.app_id.clone(),
                        title: p.title.clone(),
                        activated: p.activated,
                        minimized: p.minimized,
                        outputs,
                        workspaces: Vec::new(),
                    }));
                }
            }
            wlr_handle::Event::Closed => {
                if let Some(id) = state.forget(&key) {
                    let _ = state.tx.unbounded_send(WaylandUpdate::Closed(id));
                }
                handle.destroy();
            }
            _ => {}
        }
    }
}

// ---- cosmic backend ----------------------------------------------------------

impl ToplevelManagerHandler for AppData {
    fn toplevel_manager_state(&mut self) -> &mut ToplevelManagerState {
        match &mut self.backend {
            Some(Backend::Cosmic { manager: Some(m), .. }) => m,
            _ => unreachable!("manager events are only dispatched when the manager is bound"),
        }
    }
    fn capabilities(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: Vec<WEnum<zcosmic_toplevel_manager_v1::ZcosmicToplelevelManagementCapabilitiesV1>>,
    ) {
    }
}

impl ToplevelInfoHandler for AppData {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        match &mut self.backend {
            Some(Backend::Cosmic { info, .. }) => info,
            _ => unreachable!("toplevel info events are only dispatched for the cosmic backend"),
        }
    }
    fn new_toplevel(&mut self, _: &Connection, _: &QueueHandle<Self>, toplevel: &ExtForeignToplevelHandleV1) {
        self.send_cosmic(toplevel);
    }
    fn update_toplevel(&mut self, _: &Connection, _: &QueueHandle<Self>, toplevel: &ExtForeignToplevelHandleV1) {
        self.send_cosmic(toplevel);
    }
    fn toplevel_closed(&mut self, _: &Connection, _: &QueueHandle<Self>, toplevel: &ExtForeignToplevelHandleV1) {
        if let Some(id) = self.forget(&toplevel.id()) {
            let _ = self.tx.unbounded_send(WaylandUpdate::Closed(id));
        }
    }
}

// ---- shared plumbing ---------------------------------------------------------

impl ProvidesRegistryState for AppData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    sctk::registry_handlers!();
}

impl OutputHandler for AppData {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for AppData {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: WlSeat, cap: sctk::seat::Capability) {
        // The keyboard-layout protocol hangs off a keyboard.
        if cap == sctk::seat::Capability::Keyboard
            && self.keyboard_layout.is_none()
            && let Some(m) = &self.layout_manager
        {
            let kb = seat.get_keyboard(qh, ());
            self.keyboard_layout = Some(m.get_keyboard_layout(&kb, qh, ()));
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat, _: sctk::seat::Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
}

struct ExecRequestData {
    data: RequestData,
    exec: String,
    terminal: bool,
}

impl RequestDataExt for ExecRequestData {
    fn app_id(&self) -> Option<&str> {
        self.data.app_id()
    }
    fn seat_and_serial(&self) -> Option<(&WlSeat, u32)> {
        self.data.seat_and_serial()
    }
    fn surface(&self) -> Option<&WlSurface> {
        self.data.surface()
    }
}

impl ActivationHandler for AppData {
    type RequestData = ExecRequestData;
    fn new_token(&mut self, token: String, data: &ExecRequestData) {
        let _ = self.tx.unbounded_send(WaylandUpdate::ActivationToken {
            token: Some(token),
            app_id: data.app_id().unwrap_or_default().to_string(),
            exec: data.exec.clone(),
            terminal: data.terminal,
        });
    }
}

// ---- thumbnails -----------------------------------------------------------------

impl ShmHandler for AppData {
    fn shm_state(&mut self) -> &mut Shm {
        self.shm.as_mut().expect("shm events only arrive when shm is bound")
    }
}

impl ScreencopyHandler for AppData {
    fn screencopy_state(&mut self) -> &mut ScreencopyState {
        self.screencopy.as_mut().expect("screencopy events only arrive when it is set up")
    }

    fn init_done(&mut self, _: &Connection, qh: &QueueHandle<Self>, session: &CaptureSession, formats: &Formats) {
        let Some(id) = session.data::<SessionData>().map(|d| d.window) else { return };
        let preferred = [wl_shm::Format::Abgr8888, wl_shm::Format::Xbgr8888, wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888];
        let Some(format) = preferred.into_iter().find(|f| formats.shm_formats.contains(f)) else {
            tracing::debug!("capture: no usable shm format");
            self.captures.remove(&id);
            return;
        };
        let (w, h) = formats.buffer_size;
        let Some(shm) = &self.shm else { return };
        let Ok(mut pool) = SlotPool::new((w * h * 4) as usize, shm) else { return };
        let Ok((buffer, _)) = pool.create_buffer(w as i32, h as i32, (w * 4) as i32, format) else { return };
        let Some(c) = self.captures.get_mut(&id) else { return };
        c.session.capture(buffer.wl_buffer(), &[], qh, FrameData { window: id, inner: Default::default() });
        c.pool = Some(pool);
        c.buffer = Some(buffer);
        c.format = Some(format);
        c.size = (w, h);
    }

    fn stopped(&mut self, _: &Connection, _: &QueueHandle<Self>, session: &CaptureSession) {
        if let Some(d) = session.data::<SessionData>() {
            self.captures.remove(&d.window);
        }
    }

    fn ready(&mut self, _: &Connection, _: &QueueHandle<Self>, frame: &CaptureFrame, _: Frame) {
        let Some(id) = frame.data::<FrameData>().map(|d| d.window) else { return };
        // One frame is enough; dropping the capture ends the session.
        let Some(mut c) = self.captures.remove(&id) else { return };
        let (Some(pool), Some(buffer), Some(format)) = (c.pool.as_mut(), c.buffer.as_ref(), c.format) else { return };
        let Some(canvas) = buffer.canvas(pool) else { return };
        use lapin_ux_core::thumb::{Frame as Pixels, PixelOrder, shrink};
        let (order, opaque) = match format {
            wl_shm::Format::Abgr8888 => (PixelOrder::Rgba, false),
            wl_shm::Format::Xbgr8888 => (PixelOrder::Rgba, true),
            wl_shm::Format::Argb8888 => (PixelOrder::Bgra, false),
            _ => (PixelOrder::Bgra, true),
        };
        let (w, h) = c.size;
        if let Some((width, height, rgba)) = shrink(Pixels { data: canvas, width: w, height: h, stride: w * 4, order, opaque }, THUMB_MAX.0, THUMB_MAX.1) {
            let _ = self.tx.unbounded_send(WaylandUpdate::Thumbnail { id, width, height, rgba: std::sync::Arc::new(rgba) });
        }
    }

    fn failed(&mut self, _: &Connection, _: &QueueHandle<Self>, frame: &CaptureFrame, reason: WEnum<cosmic::cctk::wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_frame_v1::FailureReason>) {
        if let Some(d) = frame.data::<FrameData>() {
            tracing::debug!(?reason, "capture failed");
            self.captures.remove(&d.window);
        }
    }
}

// ---- accessibility and keyboard layout (cosmic-comp) ----------------------------

impl Dispatch<CosmicA11yManagerV1, ()> for AppData {
    fn event(state: &mut Self, _: &CosmicA11yManagerV1, event: cosmic_a11y_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let on = |v: WEnum<cosmic_a11y_manager_v1::ActiveState>| matches!(v, WEnum::Value(cosmic_a11y_manager_v1::ActiveState::Enabled));
        let filter = |v: WEnum<cosmic_a11y_manager_v1::Filter>| match v {
            WEnum::Value(f) => f as u32,
            WEnum::Unknown(n) => n,
        };
        match event {
            cosmic_a11y_manager_v1::Event::Magnifier { active } => state.a11y_state.magnifier = on(active),
            cosmic_a11y_manager_v1::Event::ScreenFilter { inverted, filter: f } => {
                state.a11y_state.inverted = on(inverted);
                state.a11y_state.filter = filter(f);
                state.a11y_state.filter_on = state.a11y_state.filter > 1;
            }
            cosmic_a11y_manager_v1::Event::ScreenFilter2 { inverted, filter: f, filter_state } => {
                state.a11y_state.inverted = on(inverted);
                state.a11y_state.filter = filter(f);
                state.a11y_state.filter_on = on(filter_state);
            }
            _ => return,
        }
        let _ = state.tx.unbounded_send(WaylandUpdate::A11y(state.a11y_state));
    }
}

impl Dispatch<ZcosmicKeyboardLayoutManagerV1, ()> for AppData {
    fn event(_: &mut Self, _: &ZcosmicKeyboardLayoutManagerV1, _: <ZcosmicKeyboardLayoutManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZcosmicKeyboardLayoutV1, ()> for AppData {
    fn event(state: &mut Self, _: &ZcosmicKeyboardLayoutV1, event: zcosmic_keyboard_layout_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zcosmic_keyboard_layout_v1::Event::Group { group } = event {
            let _ = state.tx.unbounded_send(WaylandUpdate::KeyboardGroup(group));
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for AppData {
    fn event(_: &mut Self, _: &wl_keyboard::WlKeyboard, _: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl AppData {
    fn set_a11y(&mut self, magnifier: Option<bool>, filter_on: Option<bool>) {
        use cosmic_a11y_manager_v1::{ActiveState, Filter};
        let Some(m) = &self.a11y else { return };
        let st = |b: bool| if b { ActiveState::Enabled } else { ActiveState::Disabled };
        if let Some(on) = magnifier {
            m.set_magnifier(st(on));
        }
        if let Some(on) = filter_on {
            let s = self.a11y_state;
            if m.version() >= 3 {
                // Keep the chosen filter; greyscale if none was ever picked.
                let f = if s.filter > 1 { Filter::try_from(s.filter).unwrap_or(Filter::Greyscale) } else { Filter::Greyscale };
                m.set_screen_filter2(st(s.inverted), f, st(on));
            } else if m.version() >= 2 {
                m.set_screen_filter(st(s.inverted), if on { Filter::Greyscale } else { Filter::Disabled });
            }
        }
    }
}

sctk::delegate_shm!(AppData);
cctk::delegate_workspace!(AppData);
cctk::delegate_screencopy!(AppData);
sctk::delegate_seat!(AppData);
sctk::delegate_registry!(AppData);
sctk::delegate_output!(AppData);
sctk::delegate_activation!(AppData, ExecRequestData);
cctk::delegate_toplevel_info!(AppData);
cctk::delegate_toplevel_manager!(AppData);
