// SPDX-License-Identifier: GPL-3.0-only
//! The clipboard thread: its own Wayland connection, separate from the
//! GUI's, that
//! - watches the selection with `ext-data-control-v1` (what clipboard
//!   managers use; cosmic-comp offers it to unsandboxed clients),
//! - sets the selection when an item is picked from the history, and
//! - types text / presses Ctrl+V with `zwp-virtual-keyboard-v1`, so a picked
//!   item or emoji lands in the app the user was typing in.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lapin_ux_core::desktop::FileClip;
use lapin_ux_core::clip::{ClipData, MAX_IMAGE, MAX_TEXT, control_code, typing_keymap};
use wayland_client::backend::ObjectId;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1, zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

/// Requests from the GUI.
#[derive(Debug)]
pub enum Command {
    /// Put this on the clipboard, then press Ctrl+V in the focused app.
    SetAndPaste(ClipData),
    /// Type text into the focused app (emoji, symbols).
    Type(String),
    /// Put files on the clipboard (desktop Cut / Copy).
    SetFiles(FileClip),
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Clipboard history isn't possible (protocol missing).
    Unavailable(String),
    Copied(ClipData),
    /// Files on the clipboard now (None: something else is).
    Files(Option<FileClip>),
}

const FILE_MIMES: &[&str] = &["x-special/gnome-copied-files", "text/uri-list"];

/// What one of our clipboard sources serves.
enum Served {
    Clip(Arc<ClipData>),
    Files(FileClip),
}

impl Served {
    fn bytes(&self, mime: &str) -> Vec<u8> {
        match self {
            Served::Clip(c) => match &**c {
                ClipData::Text(t) => t.as_bytes().to_vec(),
                ClipData::Image { png, .. } => png.as_ref().clone(),
            },
            Served::Files(f) => match mime {
                "x-special/gnome-copied-files" => lapin_ux_core::desktop::gnome_copied_files(f).into_bytes(),
                "text/uri-list" => lapin_ux_core::desktop::uri_list(&f.paths).into_bytes(),
                "application/x-kde-cutselection" => if f.cut { b"1".to_vec() } else { b"0".to_vec() },
                _ => f.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n").into_bytes(),
            },
        }
    }
}

const TEXT_MIMES: &[&str] = &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "STRING", "TEXT"];
/// Password managers mark secrets with this; such copies are never recorded.
const SECRET_HINT: &str = "x-kde-passwordManagerHint";

pub struct Handle {
    tx: mpsc::Sender<Command>,
    wake: OwnedFd,
}

impl Handle {
    pub fn send(&self, c: Command) {
        if self.tx.send(c).is_ok() {
            let _ = rustix_write(&self.wake);
        }
    }
}

fn rustix_write(fd: &OwnedFd) -> std::io::Result<()> {
    let n = unsafe { libc::write(fd.as_raw_fd(), [1u8].as_ptr().cast(), 1) };
    if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Start the thread. `out` receives events; the returned handle sends commands.
type Out = Arc<dyn Fn(Event) + Send + Sync>;

pub fn spawn(out: impl Fn(Event) + Send + Sync + 'static) -> Option<Handle> {
    let (tx, rx) = mpsc::channel();
    let (wake_r, wake_w) = pipe().ok()?;
    let out: Out = Arc::new(out);
    std::thread::Builder::new()
        .name("lapin-ux-clipboard".into())
        .spawn(move || {
            if let Err(e) = run(rx, wake_r, out.clone()) {
                tracing::warn!("clipboard: {e}");
                out(Event::Unavailable(e));
            }
        })
        .ok()?;
    Some(Handle { tx, wake: wake_w })
}

struct State {
    seat: Option<wl_seat::WlSeat>,
    manager: Option<ExtDataControlManagerV1>,
    vk_manager: Option<ZwpVirtualKeyboardManagerV1>,
    vk: Option<ZwpVirtualKeyboardV1>,
    /// Mime types announced for each offer.
    offers: HashMap<ObjectId, Vec<String>>,
    /// What our own sources serve.
    sources: HashMap<ObjectId, Arc<Served>>,
    out: Out,
    started: Instant,
}

fn run(rx: mpsc::Receiver<Command>, wake: OwnedFd, out: Out) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("no Wayland display: {e}"))?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut st = State {
        seat: None,
        manager: None,
        vk_manager: None,
        vk: None,
        offers: HashMap::new(),
        sources: HashMap::new(),
        out,
        started: Instant::now(),
    };
    queue.roundtrip(&mut st).map_err(|e| e.to_string())?;
    let (Some(seat), Some(manager)) = (st.seat.clone(), st.manager.clone()) else {
        return Err("the compositor doesn't offer ext-data-control-v1".into());
    };
    let device = manager.get_data_device(&seat, &qh, ());
    if let Some(m) = &st.vk_manager {
        st.vk = Some(m.create_virtual_keyboard(&seat, &qh, ()));
    }
    queue.roundtrip(&mut st).map_err(|e| e.to_string())?;
    tracing::info!("clipboard history ready");

    loop {
        queue.flush().map_err(|e| e.to_string())?;
        queue.dispatch_pending(&mut st).map_err(|e| e.to_string())?;
        let Some(guard) = queue.prepare_read() else { continue };
        let mut fds = [
            libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: wake.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if n < 0 {
            drop(guard);
            continue;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            guard.read().map_err(|e| e.to_string())?;
        } else {
            drop(guard);
        }
        if fds[1].revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 64];
            unsafe { libc::read(wake.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            while let Ok(cmd) = rx.try_recv() {
                handle(&mut st, &conn, &qh, &device, cmd);
            }
        }
        queue.dispatch_pending(&mut st).map_err(|e| e.to_string())?;
    }
}

fn handle(st: &mut State, conn: &Connection, qh: &QueueHandle<State>, device: &ExtDataControlDeviceV1, cmd: Command) {
    let (data, paste) = match cmd {
        Command::SetAndPaste(d) => (Some(d), true),
        Command::Type(t) => {
            type_text(st, conn, &t);
            return;
        }
        Command::SetFiles(f) => {
            if let Some(manager) = st.manager.clone() {
                let source = manager.create_data_source(qh, ());
                for m in FILE_MIMES.iter().chain(["application/x-kde-cutselection", "text/plain;charset=utf-8", "text/plain"].iter()) {
                    source.offer(m.to_string());
                }
                st.sources.insert(source.id(), Arc::new(Served::Files(f)));
                device.set_selection(Some(&source));
                let _ = conn.flush();
            }
            return;
        }
    };
    if let (Some(data), Some(manager)) = (data, st.manager.clone()) {
        let source = manager.create_data_source(qh, ());
        let mimes: &[&str] = match &data {
            ClipData::Text(_) => TEXT_MIMES,
            ClipData::Image { .. } => &["image/png"],
        };
        for m in mimes {
            source.offer(m.to_string());
        }
        st.sources.insert(source.id(), Arc::new(Served::Clip(Arc::new(data))));
        device.set_selection(Some(&source));
        let _ = conn.flush();
    }
    if paste {
        press_ctrl_v(st, conn);
    }
}

fn now_ms(st: &State) -> u32 {
    st.started.elapsed().as_millis() as u32
}

fn upload_keymap(vk: &ZwpVirtualKeyboardV1, keymap: &str) -> bool {
    let Ok(file) = tempfile_with(keymap.as_bytes()) else { return false };
    // 1 = XKB_V1; the size includes the terminating NUL.
    vk.keymap(1, file.as_fd(), keymap.len() as u32 + 1);
    true
}

fn tempfile_with(bytes: &[u8]) -> std::io::Result<std::fs::File> {
    let name = c"lapin-ux-keymap";
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    f.write_all(bytes)?;
    f.write_all(&[0])?;
    Ok(f)
}

/// Give the panel time to close and the app underneath to get the focus back.
fn wait_for_focus() {
    std::thread::sleep(Duration::from_millis(180));
}

fn type_text(st: &mut State, conn: &Connection, text: &str) {
    let Some(vk) = st.vk.clone() else {
        tracing::warn!("typing needs zwp-virtual-keyboard-v1");
        return;
    };
    wait_for_focus();
    let (keymap, codes) = typing_keymap(text);
    if !upload_keymap(&vk, &keymap) {
        tracing::warn!("typing: couldn't upload the keymap");
        return;
    }
    tracing::debug!(keys = codes.len(), "typing");
    for code in codes {
        let t = now_ms(st);
        vk.key(t, code, 1);
        vk.key(t + 1, code, 0);
        // Some apps drop repeated keys that arrive in one burst.
        let _ = conn.flush();
        std::thread::sleep(Duration::from_millis(4));
    }
    let _ = conn.flush();
}

fn press_ctrl_v(st: &mut State, conn: &Connection) {
    let Some(vk) = st.vk.clone() else { return };
    wait_for_focus();
    let (keymap, codes) = typing_keymap("v");
    if !upload_keymap(&vk, &keymap) {
        return;
    }
    let ctrl = control_code("v");
    let t = now_ms(st);
    vk.key(t, ctrl, 1);
    vk.modifiers(4, 0, 0, 0); // Control
    vk.key(t + 1, codes[0], 1);
    vk.key(t + 2, codes[0], 0);
    vk.key(t + 3, ctrl, 0);
    vk.modifiers(0, 0, 0, 0);
    let _ = conn.flush();
}

/// Read an offer on a helper thread (the owner may be ourselves, and we
/// must keep answering requests while reading).
fn receive(offer: &ExtDataControlOfferV1, mime: &str, conn: &Connection, image: bool, out: Out) {
    let Ok((r, w)) = pipe() else { return };
    offer.receive(mime.to_string(), w.as_fd());
    drop(w);
    let _ = conn.flush();
    std::thread::spawn(move || {
        let mut f = std::fs::File::from(r);
        let mut buf = Vec::new();
        let limit = if image { MAX_IMAGE } else { MAX_TEXT } as u64 + 1;
        if (&mut f).take(limit).read_to_end(&mut buf).is_err() || buf.len() as u64 >= limit {
            return;
        }
        let data = if image {
            match png_size(&buf) {
                Some((width, height)) => ClipData::Image { png: Arc::new(buf), width, height },
                None => return,
            }
        } else {
            match String::from_utf8(buf) {
                Ok(t) => ClipData::Text(t),
                Err(_) => return,
            }
        };
        out(Event::Copied(data));
    });
}

fn receive_files(offer: &ExtDataControlOfferV1, mime: &'static str, conn: &Connection, out: Out) {
    let Ok((r, w)) = pipe() else { return };
    offer.receive(mime.to_string(), w.as_fd());
    drop(w);
    let _ = conn.flush();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::fs::File::from(r).take(1 << 20).read_to_string(&mut buf);
        out(Event::Files(lapin_ux_core::desktop::parse_file_clip(mime, &buf)));
    });
}

/// Width and height from a PNG header.
fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
    Some((w, h))
}

// ---- dispatch ------------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(st: &mut Self, reg: &wl_registry::WlRegistry, ev: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = ev {
            match interface.as_str() {
                "wl_seat" if st.seat.is_none() => st.seat = Some(reg.bind(name, version.min(7), qh, ())),
                "ext_data_control_manager_v1" => st.manager = Some(reg.bind(name, 1, qh, ())),
                "zwp_virtual_keyboard_manager_v1" => st.vk_manager = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ExtDataControlManagerV1, ()> for State {
    fn event(_: &mut Self, _: &ExtDataControlManagerV1, _: <ExtDataControlManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for State {
    fn event(_: &mut Self, _: &ZwpVirtualKeyboardManagerV1, _: <ZwpVirtualKeyboardManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ZwpVirtualKeyboardV1, ()> for State {
    fn event(_: &mut Self, _: &ZwpVirtualKeyboardV1, _: <ZwpVirtualKeyboardV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(st: &mut Self, _: &ExtDataControlDeviceV1, ev: ext_data_control_device_v1::Event, _: &(), conn: &Connection, _: &QueueHandle<Self>) {
        match ev {
            ext_data_control_device_v1::Event::DataOffer { id } => {
                st.offers.insert(id.id(), Vec::new());
            }
            ext_data_control_device_v1::Event::Selection { id: Some(offer) } => {
                let mimes = st.offers.get(&offer.id()).cloned().unwrap_or_default();
                // Files (for Paste on the desktop) are read separately from history.
                match FILE_MIMES.iter().find(|m| mimes.iter().any(|x| x == *m)) {
                    Some(m) => receive_files(&offer, m, conn, st.out.clone()),
                    None => (st.out)(Event::Files(None)),
                }
                if mimes.iter().any(|m| m == SECRET_HINT) {
                    tracing::debug!("clipboard: skipping a password manager secret");
                } else if let Some(m) = TEXT_MIMES.iter().find(|m| mimes.iter().any(|x| x == *m)) {
                    receive(&offer, m, conn, false, st.out.clone());
                } else if mimes.iter().any(|m| m == "image/png") {
                    receive(&offer, "image/png", conn, true, st.out.clone());
                }
                // Earlier offers are finished with.
                st.offers.retain(|id, _| *id == offer.id());
            }
            ext_data_control_device_v1::Event::Selection { id: None } => (st.out)(Event::Files(None)),
            ext_data_control_device_v1::Event::Finished => {
                (st.out)(Event::Unavailable("clipboard device finished".into()));
            }
            _ => {}
        }
    }

    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for State {
    fn event(st: &mut Self, offer: &ExtDataControlOfferV1, ev: ext_data_control_offer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = ev {
            st.offers.entry(offer.id()).or_default().push(mime_type);
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for State {
    fn event(st: &mut Self, source: &ExtDataControlSourceV1, ev: ext_data_control_source_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            ext_data_control_source_v1::Event::Send { fd, mime_type } => {
                let Some(data) = st.sources.get(&source.id()).cloned() else { return };
                // Write off-thread so a slow reader can't stall us.
                std::thread::spawn(move || {
                    let mut f = std::fs::File::from(fd);
                    let _ = f.write_all(&data.bytes(&mime_type));
                });
            }
            ext_data_control_source_v1::Event::Cancelled => {
                st.sources.remove(&source.id());
                source.destroy();
            }
            _ => {}
        }
    }
}
