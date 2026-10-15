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
    },
    toplevel_info::{ToplevelInfoHandler, ToplevelInfoState},
    toplevel_management::{ToplevelManagerHandler, ToplevelManagerState},
    wayland_client::{
        Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child,
        backend::ObjectId,
        globals::{GlobalList, registry_queue_init},
        protocol::{wl_output, wl_seat::WlSeat, wl_surface::WlSurface},
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
}

#[derive(Clone, Debug)]
pub enum WaylandUpdate {
    Init(calloop::channel::Sender<WaylandRequest>),
    Finished,
    Window(WindowInfo),
    Closed(WindowId),
    ActivationToken { token: Option<String>, app_id: String, exec: String, terminal: bool },
}

#[derive(Clone, Debug)]
pub enum WaylandRequest {
    Activate(WindowId),
    Minimize(WindowId),
    Close(WindowId),
    /// Ask the compositor for an activation token so the launched app gets focus.
    TokenRequest { app_id: String, exec: String, terminal: bool },
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
                    .name("atrium-toplevels".into())
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
        let target = |id: &WindowId| self.objects.get(id).cloned();
        match req {
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

    fn send_cosmic(&mut self, ext: &ExtForeignToplevelHandleV1) {
        let Some(Backend::Cosmic { info, .. }) = &self.backend else { return };
        let Some(info) = info.info(ext).cloned() else { return };
        let id = self.id_for(ext.id(), Toplevel::Cosmic(ext.clone()));
        let _ = self.tx.unbounded_send(WaylandUpdate::Window(WindowInfo {
            id,
            app_id: info.app_id,
            title: info.title,
            activated: info.state.contains(&zcosmic_toplevel_handle_v1::State::Activated),
            minimized: info.state.contains(&zcosmic_toplevel_handle_v1::State::Minimized),
        }));
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
        queue_handle: qh,
        backend,
        next_id: 0,
        ids: HashMap::new(),
        objects: HashMap::new(),
        wlr_pending: HashMap::new(),
    };

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
            wlr_handle::Event::Done => {
                let id = state.id_for(key.clone(), Toplevel::Wlr(handle.clone()));
                if let Some(p) = state.wlr_pending.get(&key) {
                    let _ = state.tx.unbounded_send(WaylandUpdate::Window(WindowInfo {
                        id,
                        app_id: p.app_id.clone(),
                        title: p.title.clone(),
                        activated: p.activated,
                        minimized: p.minimized,
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
    fn new_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat, _: sctk::seat::Capability) {}
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

sctk::delegate_seat!(AppData);
sctk::delegate_registry!(AppData);
sctk::delegate_output!(AppData);
sctk::delegate_activation!(AppData, ExecRequestData);
cctk::delegate_toplevel_info!(AppData);
cctk::delegate_toplevel_manager!(AppData);
