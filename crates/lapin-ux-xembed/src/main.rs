// SPDX-License-Identifier: GPL-3.0-only
//! Older X11 tray icons in Lapin UX's tray (#33).
//!
//! Lapin UX's tray shows StatusNotifierItems (D-Bus), which Qt, GTK,
//! Electron and most current apps use. Older X11 apps and Wine programs use
//! the XEmbed system tray instead: they look for an X window owning
//! `_NET_SYSTEM_TRAY_S0` and hand it their icon window. With no owner, Wine
//! shows its own small "system tray" window and others show nothing.
//!
//! This owns that selection on Xwayland, takes each icon window in (kept
//! off-screen), reads its picture through the Composite extension, and
//! shows it in the tray as a StatusNotifierItem of its own. Clicks on the
//! tray icon go back to the X window as button presses (left, middle,
//! right, scroll).
//!
//! Started by lapin-ux-session only, so other desktops keep their own tray.

use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Pixel size icons are given and read at.
const SIZE: u16 = 32;
/// Where the icon windows are kept: off every screen, so the compositor
/// never shows them.
const HIDE_X: i16 = -8000;
const HIDE_Y: i16 = -8000;

type Pixmaps = Vec<(i32, i32, Vec<u8>)>;

/// From the X side to the D-Bus side.
enum FromX {
    Shown { win: u32, id: String, title: String },
    Icon { win: u32, argb: Vec<u8> },
    Gone { win: u32 },
}

/// From a tray icon to its X window.
enum ToX {
    Click { win: u32, button: u8 },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    lapin_ux_core::crash::install("lapin-ux-xembed");
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    if std::env::var_os("DISPLAY").is_none() {
        // No X server in this session (Xwayland turned off): nothing to do,
        // and exiting would only get us restarted.
        info!("no DISPLAY; X11 tray icons are off");
        futures::future::pending::<()>().await;
    }

    let (from_x, mut events) = mpsc::unbounded_channel();
    let (to_x, commands) = std_mpsc::channel();
    let x = std::thread::spawn(move || {
        if let Err(e) = x_tray(from_x, commands) {
            warn!("X tray stopped: {e:#}");
        }
    });

    let bus = zbus::Connection::session().await?;
    let dbus = zbus::fdo::DBusProxy::new(&bus).await?;
    let mut watcher_changes = dbus
        .receive_name_owner_changed_with_args(&[(0, "org.kde.StatusNotifierWatcher")])
        .await?;
    // One D-Bus connection per icon: when an icon goes, its connection
    // closes and the tray drops it (items have no "unregister").
    let mut items: HashMap<u32, zbus::Connection> = HashMap::new();

    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Some(FromX::Shown { win, id, title }) => {
                    if items.contains_key(&win) {
                        continue;
                    }
                    let item = Item { win, id, title, pixmap: Vec::new(), to_x: to_x.clone() };
                    match publish(item).await {
                        Ok(conn) => {
                            info!(win, "X11 tray icon added");
                            items.insert(win, conn);
                        }
                        Err(e) => warn!(win, "couldn't show an X11 tray icon: {e:#}"),
                    }
                }
                Some(FromX::Icon { win, argb }) => {
                    if let Some(conn) = items.get(&win)
                        && let Ok(iface) = conn.object_server().interface::<_, Item>("/StatusNotifierItem").await
                    {
                        iface.get_mut().await.pixmap = vec![(SIZE as i32, SIZE as i32, argb)];
                        let _ = Item::new_icon(iface.signal_emitter()).await;
                    }
                }
                Some(FromX::Gone { win }) => {
                    if items.remove(&win).is_some() {
                        info!(win, "X11 tray icon removed");
                    }
                }
                None => break,
            },
            Some(change) = watcher_changes.next() => {
                // The tray (the shell) started again: show the icons again.
                if change.args().is_ok_and(|a| a.new_owner().is_some()) {
                    for conn in items.values() {
                        let _ = register(conn).await;
                    }
                }
            }
        }
    }
    let _ = x.join();
    // The X side ended (another tray took over, or X went away). Exit so the
    // session starts us again later.
    Ok(())
}

async fn publish(item: Item) -> anyhow::Result<zbus::Connection> {
    let conn = zbus::connection::Builder::session()?
        .serve_at("/StatusNotifierItem", item)?
        .build()
        .await?;
    // The tray may not be up yet; it's registered again when it appears.
    let _ = register(&conn).await;
    Ok(conn)
}

async fn register(conn: &zbus::Connection) -> zbus::Result<()> {
    conn.call_method(
        Some("org.kde.StatusNotifierWatcher"),
        "/StatusNotifierWatcher",
        Some("org.kde.StatusNotifierWatcher"),
        "RegisterStatusNotifierItem",
        &(conn.unique_name().map(|n| n.to_string()).unwrap_or_default()),
    )
    .await
    .map(|_| ())
}

/// One X11 tray icon, as a StatusNotifierItem.
struct Item {
    win: u32,
    id: String,
    title: String,
    pixmap: Pixmaps,
    to_x: std_mpsc::Sender<ToX>,
}

impl Item {
    fn click(&self, button: u8) {
        let _ = self.to_x.send(ToX::Click { win: self.win, button });
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> String {
        "ApplicationStatus".into()
    }
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }
    #[zbus(property)]
    fn title(&self) -> String {
        self.title.clone()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        self.pixmap.clone()
    }
    #[zbus(property)]
    fn overlay_icon_name(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn attention_icon_name(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }
    #[zbus(property)]
    fn tool_tip(&self) -> (String, Pixmaps, String, String) {
        (String::new(), Vec::new(), self.title.clone(), String::new())
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    fn activate(&self, _x: i32, _y: i32) {
        self.click(1);
    }
    fn secondary_activate(&self, _x: i32, _y: i32) {
        self.click(2);
    }
    fn context_menu(&self, _x: i32, _y: i32) {
        self.click(3);
    }
    fn scroll(&self, delta: i32, orientation: &str) {
        let button = match (orientation.eq_ignore_ascii_case("horizontal"), delta > 0) {
            (false, true) => 5,
            (false, false) => 4,
            (true, true) => 7,
            (true, false) => 6,
        };
        self.click(button);
    }

    #[zbus(signal)]
    async fn new_icon(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;
}

// --------------------------------------------------------------------- X side

use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::composite::ConnectionExt as _;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

struct Docked {
    container: u32,
    hidden: bool,
    last: Option<Vec<u8>>,
}

struct Tray {
    conn: RustConnection,
    root: u32,
    depth: u8,
    visual: u32,
    colormap: u32,
    composite: bool,
    xembed: u32,
    net_wm_name: u32,
    utf8: u32,
    icons: HashMap<u32, Docked>,
    tx: mpsc::UnboundedSender<FromX>,
}

fn x_tray(tx: mpsc::UnboundedSender<FromX>, commands: std_mpsc::Receiver<ToX>) -> anyhow::Result<()> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let screen = conn.setup().roots[screen_num].clone();
    let root = screen.root;
    let atom = |n: &str| -> anyhow::Result<u32> { Ok(conn.intern_atom(false, n.as_bytes())?.reply()?.atom) };
    let selection = atom(&format!("_NET_SYSTEM_TRAY_S{screen_num}"))?;
    let opcode = atom("_NET_SYSTEM_TRAY_OPCODE")?;
    let manager = atom("MANAGER")?;
    let orientation = atom("_NET_SYSTEM_TRAY_ORIENTATION")?;
    let tray_visual = atom("_NET_SYSTEM_TRAY_VISUAL")?;
    let xembed = atom("_XEMBED")?;
    let net_wm_name = atom("_NET_WM_NAME")?;
    let utf8 = atom("UTF8_STRING")?;
    let composite = conn.composite_query_version(0, 4).ok().and_then(|c| c.reply().ok()).is_some();
    if !composite {
        warn!("no Composite extension: X11 tray icons will be blank");
    }

    // Icons drawn with transparency need a 32-bit visual.
    let argb = screen
        .allowed_depths
        .iter()
        .find(|d| d.depth == 32)
        .and_then(|d| d.visuals.iter().find(|v| v.class == VisualClass::TRUE_COLOR))
        .map(|v| v.visual_id);
    let (depth, visual, colormap) = match argb {
        Some(v) => {
            let cm = conn.generate_id()?;
            conn.create_colormap(ColormapAlloc::NONE, cm, root, v)?;
            (32, v, cm)
        }
        None => (screen.root_depth, screen.root_visual, screen.default_colormap),
    };

    let owner = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        owner,
        root,
        HIDE_X,
        HIDE_Y,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().override_redirect(1),
    )?;
    conn.change_property32(PropMode::REPLACE, owner, orientation, AtomEnum::CARDINAL, &[0])?;
    conn.change_property32(PropMode::REPLACE, owner, tray_visual, AtomEnum::VISUALID, &[visual])?;

    // Another tray (another desktop's, started by hand) keeps it.
    let mut said = false;
    while conn.get_selection_owner(selection)?.reply()?.owner != NONE {
        if !said {
            info!("another program owns the X11 tray; waiting");
            said = true;
        }
        std::thread::sleep(Duration::from_secs(10));
    }
    conn.set_selection_owner(owner, selection, CURRENT_TIME)?;
    if conn.get_selection_owner(selection)?.reply()?.owner != owner {
        anyhow::bail!("couldn't take the X11 tray");
    }
    let announce = ClientMessageEvent::new(32, root, manager, [CURRENT_TIME, selection, owner, 0, 0]);
    conn.send_event(false, root, EventMask::STRUCTURE_NOTIFY, announce)?;
    conn.flush()?;
    info!(composite, depth, "owning the X11 tray");

    let mut tray = Tray { conn, root, depth, visual, colormap, composite, xembed, net_wm_name, utf8, icons: HashMap::new(), tx };
    let mut next_capture = Instant::now();
    loop {
        while let Ok(ToX::Click { win, button }) = commands.try_recv() {
            tray.click(win, button);
        }
        while let Some(event) = tray.conn.poll_for_event()? {
            match event {
                Event::ClientMessage(e) if e.type_ == opcode && e.window == owner => {
                    let d = e.data.as_data32();
                    // SYSTEM_TRAY_REQUEST_DOCK; balloon messages are ignored.
                    if d[1] == 0 {
                        if let Err(err) = tray.dock(d[2]) {
                            warn!(win = d[2], "couldn't take an X11 tray icon: {err:#}");
                        }
                    }
                }
                Event::DestroyNotify(e) => tray.remove(e.window),
                Event::ReparentNotify(e) => {
                    if tray.icons.get(&e.window).is_some_and(|i| i.container != e.parent) {
                        tray.remove(e.window);
                    }
                }
                Event::UnmapNotify(e) => {
                    if let Some(i) = tray.icons.get_mut(&e.window)
                        && !i.hidden
                    {
                        i.hidden = true;
                        let _ = tray.tx.send(FromX::Gone { win: e.window });
                    }
                }
                Event::MapNotify(e) => {
                    if let Some(i) = tray.icons.get_mut(&e.window)
                        && i.hidden
                    {
                        i.hidden = false;
                        i.last = None;
                        let (id, title) = tray.names(e.window);
                        let _ = tray.tx.send(FromX::Shown { win: e.window, id, title });
                    }
                }
                Event::SelectionClear(e) if e.selection == selection => {
                    info!("another program took the X11 tray");
                    return Ok(());
                }
                _ => {}
            }
        }
        if Instant::now() >= next_capture {
            tray.capture_all();
            next_capture = Instant::now() + Duration::from_millis(500);
        }
        tray.conn.flush()?;
        std::thread::sleep(Duration::from_millis(25));
    }
}

impl Tray {
    fn dock(&mut self, icon: u32) -> anyhow::Result<()> {
        if self.icons.contains_key(&icon) {
            return Ok(());
        }
        let c = &self.conn;
        let container = c.generate_id()?;
        c.create_window(
            self.depth,
            container,
            self.root,
            HIDE_X,
            HIDE_Y,
            SIZE,
            SIZE,
            0,
            WindowClass::INPUT_OUTPUT,
            self.visual,
            &CreateWindowAux::new()
                .background_pixel(0)
                .border_pixel(0)
                .colormap(self.colormap)
                .override_redirect(1),
        )?;
        c.change_window_attributes(icon, &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY))?;
        // If we stop, X gives the icon window back to the app.
        c.change_save_set(SetMode::INSERT, icon)?;
        c.reparent_window(icon, container, 0, 0)?;
        c.configure_window(icon, &ConfigureWindowAux::new().x(0).y(0).width(SIZE as u32).height(SIZE as u32))?;
        c.map_window(icon)?;
        c.map_window(container)?;
        // XEMBED_EMBEDDED_NOTIFY
        let notify = ClientMessageEvent::new(32, icon, self.xembed, [CURRENT_TIME, 0, 0, container, 0]);
        c.send_event(false, icon, EventMask::NO_EVENT, notify)?;
        c.flush()?;
        self.icons.insert(icon, Docked { container, hidden: false, last: None });
        let (id, title) = self.names(icon);
        info!(win = icon, %id, %title, "X11 tray icon docked");
        let _ = self.tx.send(FromX::Shown { win: icon, id, title });
        Ok(())
    }

    fn remove(&mut self, icon: u32) {
        if let Some(d) = self.icons.remove(&icon) {
            let _ = self.conn.destroy_window(d.container);
            let _ = self.tx.send(FromX::Gone { win: icon });
        }
    }

    /// The app's name for the icon (class) and its title, for the tooltip.
    fn names(&self, win: u32) -> (String, String) {
        let prop = |p: u32, t: u32| -> Option<String> {
            let r = self.conn.get_property(false, win, p, t, 0, 256).ok()?.reply().ok()?;
            let s = String::from_utf8_lossy(&r.value).trim_end_matches('\0').to_string();
            (!s.is_empty()).then_some(s)
        };
        let class = prop(AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into())
            .and_then(|c| c.split('\0').filter(|s| !s.is_empty()).last().map(str::to_string));
        let title = prop(self.net_wm_name, self.utf8)
            .or_else(|| prop(AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()))
            .or_else(|| class.clone())
            .unwrap_or_else(|| "App".into());
        (class.unwrap_or_else(|| format!("xembed-{win}")), title)
    }

    fn capture_all(&mut self) {
        let wins: Vec<u32> = self.icons.iter().filter(|(_, d)| !d.hidden).map(|(w, _)| *w).collect();
        for win in wins {
            let Some(container) = self.icons.get(&win).map(|d| d.container) else { continue };
            let Some(argb) = self.capture(container) else { continue };
            let d = self.icons.get_mut(&win).unwrap();
            if d.last.as_ref() != Some(&argb) {
                d.last = Some(argb.clone());
                let _ = self.tx.send(FromX::Icon { win, argb });
            }
        }
    }

    /// The icon's picture as ARGB32, network byte order (StatusNotifierItem).
    fn capture(&self, container: u32) -> Option<Vec<u8>> {
        let c = &self.conn;
        let mut pixmap = None;
        let drawable = if self.composite {
            let p = c.generate_id().ok()?;
            c.composite_name_window_pixmap(container, p).ok()?;
            pixmap = Some(p);
            p
        } else {
            container
        };
        let image = c.get_image(ImageFormat::Z_PIXMAP, drawable, 0, 0, SIZE, SIZE, !0).ok()?.reply();
        if let Some(p) = pixmap {
            let _ = c.free_pixmap(p);
        }
        let image = image.ok()?;
        if image.data.len() < SIZE as usize * SIZE as usize * 4 {
            return None;
        }
        let alpha = image.depth == 32;
        let mut out = Vec::with_capacity(image.data.len());
        for px in image.data.chunks_exact(4).take(SIZE as usize * SIZE as usize) {
            // X gives B, G, R, A (little-endian); pixels are premultiplied.
            let a = if alpha { px[3] } else { 255 };
            let un = |v: u8| if a == 0 { 0 } else { ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
            out.extend_from_slice(&[a, un(px[2]), un(px[1]), un(px[0])]);
        }
        Some(out)
    }

    /// A click on the tray icon, delivered to the X window.
    fn click(&self, win: u32, button: u8) {
        if !self.icons.contains_key(&win) {
            return;
        }
        let half = (SIZE / 2) as i16;
        let mut event = ButtonPressEvent {
            response_type: BUTTON_PRESS_EVENT,
            detail: button,
            sequence: 0,
            time: CURRENT_TIME,
            root: self.root,
            event: win,
            child: NONE,
            root_x: HIDE_X + half,
            root_y: HIDE_Y + half,
            event_x: half,
            event_y: half,
            state: KeyButMask::default(),
            same_screen: true,
        };
        let _ = self.conn.send_event(false, win, EventMask::BUTTON_PRESS, event);
        event.response_type = BUTTON_RELEASE_EVENT;
        event.state = KeyButMask::from(1u16 << (7 + button.min(5)));
        let _ = self.conn.send_event(false, win, EventMask::BUTTON_RELEASE, event);
        let _ = self.conn.flush();
    }
}
