//! Video inside the player window (#222): libmpv, hardware decoding, software drawing.
//!
//! iced draws with wgpu and cannot share an OpenGL context with libmpv, so mpv
//! decodes (on the graphics card when that is on) and its software render API
//! hands every frame back as plain pixels, which the window shows with an image
//! widget. Music and radio never come here; they stay on the mpv process in
//! `engine.rs`.
//!
//! One thread owns the libmpv handle and the render context. Everything the
//! window needs to know goes out as the same `engine::Event`s the music engine
//! sends, so the rest of the app does not care which engine plays.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc as std_mpsc};
use std::time::Duration;

use cosmic::iced::{Subscription, stream};
use libmpv2_sys as ffi;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::engine::{self, Engine, Event};

/// What the video engine sends to the window.
#[derive(Debug, Clone)]
pub enum Out {
    Event(Event),
    /// The newest picture, already scaled to the size the window asked for.
    Frame(cosmic::widget::image::Handle),
    /// libmpv could not start; the window falls back to mpv's own window.
    Unavailable(String),
}

// ---- pure parts (tested without libmpv) ------------------------------------

/// How long the control bar stays after the last movement or key press.
pub const BAR_HIDE_AFTER: Duration = Duration::from_secs(3);

/// The bar shows while paused, and for `BAR_HIDE_AFTER` after any activity.
pub fn bar_visible(paused: bool, since_activity: Duration) -> bool {
    paused || since_activity < BAR_HIDE_AFTER
}

/// Methods that give mpv a graphics-card surface it can only hand on without
/// copying. The software render API has no such surface, so these are switched
/// to their `-copy` form, which decodes on the card and copies the picture back.
const ZERO_COPY_ONLY: &[&str] = &["vaapi", "nvdec", "cuda", "vdpau", "d3d11va", "dxva2", "videotoolbox", "mediacodec"];

/// The `hwdec` value to start with.
pub fn hwdec_start(gpu: bool) -> &'static str {
    if gpu { "auto-safe" } else { "no" }
}

/// `-copy` form of a zero-copy-only method, or None when nothing needs changing.
pub fn copy_variant(current: &str) -> Option<String> {
    ZERO_COPY_ONLY.contains(&current).then(|| format!("{current}-copy"))
}

/// What to do after mpv reports the decoder it picked (`hwdec-current`).
/// `gpu` is the Settings switch; `retried` is true once the generic copy
/// choice was already tried for this file. Returns the new `hwdec` value.
pub fn hwdec_next(gpu: bool, current: &str, retried: bool) -> Option<String> {
    if !gpu {
        return None;
    }
    if let Some(copy) = copy_variant(current) {
        return Some(copy);
    }
    // Nothing on the card could be used directly: try the copy methods once.
    if (current.is_empty() || current == "no") && !retried {
        return Some("auto-copy-safe".into());
    }
    None
}

/// Largest picture drawn: more pixels only cost time, the widget scales it.
const MAX_W: u32 = 1920;
const MAX_H: u32 = 1080;

/// The size mpv draws at for a box of `w` x `h` pixels: never empty, never
/// bigger than the cap, same shape as the box.
pub fn render_size(w: u32, h: u32) -> (u32, u32) {
    let (w, h) = (w.max(2), h.max(2));
    let scale = (MAX_W as f64 / w as f64).min(MAX_H as f64 / h as f64).min(1.0);
    let w = ((w as f64 * scale).round() as u32).max(2);
    let h = ((h as f64 * scale).round() as u32).max(2);
    // even sizes keep the picture from shifting by a pixel
    (w & !1, h & !1)
}

pub fn pack_size(w: u32, h: u32) -> u64 {
    ((w as u64) << 32) | h as u64
}

pub fn unpack_size(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, (v & 0xffff_ffff) as u32)
}

/// mpv's "rgb0" has no alpha, so the unused byte is whatever was in memory.
/// The picture is opaque.
pub fn make_opaque(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        px[3] = 255;
    }
}

/// A property as the music engine's mapping expects it. `vid` is a string in
/// libmpv ("no", "auto" or a number), the music engine reads a number or false.
pub fn map_property(name: &str, value: &Value) -> Vec<Event> {
    match name {
        "vid" => {
            let on = match value {
                Value::String(s) => s != "no" && !s.is_empty(),
                Value::Number(_) => true,
                _ => false,
            };
            vec![Event::HasVideo(on)]
        }
        "track-list" => vec![Event::Tracks(parse_tracks(value.as_str().unwrap_or("")))],
        "chapter-list" => vec![Event::Chapters(parse_chapters(value.as_str().unwrap_or("")))],
        "chapter" => vec![Event::Chapter(value.as_f64().map_or(-1, |c| c as i64))],
        "ab-loop-a" | "ab-loop-b" => vec![Event::LoopPoint(name.ends_with('b'), loop_is_set(value))],
        "metadata/by-key/Artist" => vec![Event::Artist(value.as_str().unwrap_or("").into())],
        "metadata/by-key/Album" => vec![Event::Album(value.as_str().unwrap_or("").into())],
        _ => engine::property(name, value),
    }
}

/// `ab-loop-a` and `ab-loop-b` read as text: "no" while unset, else the time.
pub fn loop_is_set(value: &Value) -> bool {
    match value {
        Value::String(s) => !s.is_empty() && s != "no",
        Value::Number(_) => true,
        _ => false,
    }
}

/// The chapters in libmpv's `chapter-list` (JSON text): title and start time.
pub fn parse_chapters(json_text: &str) -> Vec<(String, f64)> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json_text) else { return Vec::new() };
    items
        .iter()
        .map(|c| (c.get("title").and_then(Value::as_str).unwrap_or("").to_string(), c.get("time").and_then(Value::as_f64).unwrap_or(0.0)))
        .collect()
}

/// What a chapter is called in the menu: its title (or "Chapter N") and where it starts.
pub fn chapter_label(title: &str, time: f64, number: usize) -> String {
    let secs = time.max(0.0) as u64;
    let at = if secs >= 3600 { format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60) } else { format!("{}:{:02}", secs / 60, secs % 60) };
    if title.trim().is_empty() { format!("Chapter {number}  {at}") } else { format!("{}  {at}", title.trim()) }
}

/// The A-B loop button: what pressing it does next (mpv's `ab-loop` cycles
/// start, end, clear).
pub fn loop_label(a: bool, b: bool) -> &'static str {
    match (a, b) {
        (false, false) => "Repeat a part: set the start",
        (true, false) => "Repeat a part: set the end",
        _ => "Repeat a part: stop",
    }
}

/// Speeds offered in the speed menu.
pub const SPEEDS: [f64; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];

/// A speed as text: "1x", "0.75x", "1.25x".
pub fn speed_label(speed: f64) -> String {
    let s = format!("{:.2}", (speed * 100.0).round() / 100.0);
    format!("{}x", s.trim_end_matches('0').trim_end_matches('.'))
}

/// Subtitle colours offered in the Adjust panel: name and `sub-color` value.
pub const SUB_COLORS: [(&str, &str); 4] = [("White", "#FFFFFF"), ("Yellow", "#FFFF00"), ("Cyan", "#00FFFF"), ("Green", "#00FF00")];

/// The audio, subtitle and video tracks in libmpv's `track-list` (JSON text).
/// Cover pictures in a music file are not video tracks. Anything unreadable is left out.
pub fn parse_tracks(json_text: &str) -> Vec<engine::Track> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json_text) else { return Vec::new() };
    items
        .iter()
        .filter_map(|t| {
            let kind = match t.get("type")?.as_str()? {
                "audio" => engine::TrackKind::Audio,
                "sub" => engine::TrackKind::Subtitle,
                "video" if !t.get("albumart").and_then(Value::as_bool).unwrap_or(false) => engine::TrackKind::Video,
                _ => return None,
            };
            let text = |k: &str| t.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            Some(engine::Track { id: t.get("id")?.as_i64()?, kind, title: text("title"), lang: text("lang"), selected: t.get("selected").and_then(Value::as_bool).unwrap_or(false) })
        })
        .collect()
}

/// What a track is called in the menu: its title and language when the file
/// has them, else "Track N".
pub fn track_label(t: &engine::Track, number: usize) -> String {
    match (t.title.is_empty(), t.lang.is_empty()) {
        (false, false) => format!("{} ({})", t.title, t.lang),
        (false, true) => t.title.clone(),
        (true, false) => t.lang.clone(),
        (true, true) => format!("Track {number}"),
    }
}

/// Why a file ended, as the music engine tells it: a finished file is `None`,
/// a failure carries libmpv's words, a stop or quit that we asked for is not an event.
pub fn end_event(reason: u32, error_text: impl FnOnce() -> String) -> Option<Event> {
    match reason {
        0 => Some(Event::Ended { error: None }),
        4 => Some(Event::Ended { error: Some(error_text()) }),
        _ => None,
    }
}

/// Which mpv setting the "Adjust" panel changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tune {
    SubDelay,
    AudioDelay,
    SubScale,
    /// 0 is the top of the picture, 100 the bottom.
    SubPos,
}

impl Tune {
    pub fn property(self) -> &'static str {
        match self {
            Tune::SubDelay => "sub-delay",
            Tune::AudioDelay => "audio-delay",
            Tune::SubScale => "sub-scale",
            Tune::SubPos => "sub-pos",
        }
    }
    pub fn default_value(self) -> f64 {
        match self {
            Tune::SubScale => 1.0,
            Tune::SubPos => 100.0,
            _ => 0.0,
        }
    }
    /// One press of the minus (`dir` < 0) or plus (`dir` > 0) button; 0 resets.
    pub fn step(self, value: f64, dir: i8) -> f64 {
        if dir == 0 {
            return self.default_value();
        }
        let d = f64::from(dir.signum());
        if self == Tune::SubPos {
            return ((value / 5.0).round() * 5.0 + d * 5.0).clamp(0.0, 100.0);
        }
        let (lo, hi) = if self == Tune::SubScale { (0.3, 3.0) } else { (-60.0, 60.0) };
        // tenths, rounded, so repeated presses never drift (0.1 + 0.2 style sums)
        (((value * 10.0).round() / 10.0 + d / 10.0).clamp(lo, hi) * 10.0).round() / 10.0
    }
    pub fn label(self, value: f64) -> String {
        if self == Tune::SubPos {
            format!("{}%", value.round() as i64)
        } else if self == Tune::SubScale {
            format!("{}%", (value * 100.0).round() as i64)
        } else {
            let v = (value * 10.0).round() / 10.0;
            if v == 0.0 { "0 s".to_string() } else { format!("{v:+.1} s") }
        }
    }
}


/// Command arguments as libmpv wants them: text. `true` becomes "yes".
pub fn arg_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "yes".into(),
        Value::Bool(false) => "no".into(),
        other => other.to_string(),
    }
}

/// Property observed from libmpv: name and the format it is read in.
#[derive(Clone, Copy)]
enum Kind {
    Flag,
    Double,
    Text,
}

const OBSERVED: &[(&str, Kind)] = &[
    ("pause", Kind::Flag),
    ("time-pos", Kind::Double),
    ("duration", Kind::Double),
    ("volume", Kind::Double),
    ("mute", Kind::Flag),
    ("speed", Kind::Double),
    ("media-title", Kind::Text),
    ("vid", Kind::Text),
    ("track-list", Kind::Text),
    ("chapter-list", Kind::Text),
    ("chapter", Kind::Double),
    ("ab-loop-a", Kind::Text),
    ("ab-loop-b", Kind::Text),
    ("hwdec-current", Kind::Text),
    ("metadata/by-key/Artist", Kind::Text),
    ("metadata/by-key/Album", Kind::Text),
];

// ---- libmpv thread ----------------------------------------------------------

enum Msg {
    Cmd(Vec<Value>, Option<u64>),
    /// libmpv has events or a new frame; look.
    Wake,
    Quit,
}

/// Frames waiting between the thread and the window; more are not drawn.
const MAX_PENDING_FRAMES: usize = 2;

struct Shared {
    out: mpsc::UnboundedSender<Out>,
    pending: Arc<AtomicUsize>,
    size: Arc<AtomicU64>,
}

unsafe extern "C" fn wake_cb(d: *mut c_void) {
    // SAFETY: `d` is the leaked Sender made in `run`, alive until the thread ends.
    let tx = unsafe { &*(d as *const std_mpsc::Sender<Msg>) };
    let _ = tx.send(Msg::Wake);
}

fn error_text(code: c_int) -> String {
    // SAFETY: mpv_error_string returns a static C string.
    unsafe { CStr::from_ptr(ffi::mpv_error_string(code)) }.to_string_lossy().into_owned()
}

struct Player {
    h: *mut ffi::mpv_handle,
    rc: *mut ffi::mpv_render_context,
    gpu: bool,
    retried: bool,
    buf: Vec<u8>,
    last: (u32, u32),
}

impl Player {
    fn start(gpu: bool, wake: *mut c_void) -> Result<Player, String> {
        // SAFETY: plain libmpv calls on a handle we own; every failure path frees it.
        unsafe {
            let h = ffi::mpv_create();
            if h.is_null() {
                return Err("The video engine (libmpv) could not start.".into());
            }
            let conf = engine::config_dir().join("mpv");
            let _ = std::fs::create_dir_all(&conf);
            let opts: [(&str, String); 14] = [
                // Our own folder: the user's ~/.config/mpv is neither read nor changed.
                ("config", "no".into()),
                ("config-dir", conf.display().to_string()),
                ("vo", "libmpv".into()),
                ("hwdec", hwdec_start(gpu).into()),
                ("idle", "yes".into()),
                ("keep-open", "no".into()),
                ("osc", "no".into()),
                ("input-default-bindings", "no".into()),
                ("input-vo-keyboard", "no".into()),
                ("load-scripts", "no".into()),
                ("terminal", "no".into()),
                ("audio-display", "no".into()),
                ("audio-client-name", "Lapin Media Player".into()),
                ("sub-auto", "fuzzy".into()),
            ];
            for (k, v) in &opts {
                let (k, v) = (CString::new(*k).unwrap(), CString::new(v.as_str()).unwrap_or_default());
                // An option an older libmpv does not know is not a reason to give up.
                ffi::mpv_set_option_string(h, k.as_ptr(), v.as_ptr());
            }
            let rc = ffi::mpv_initialize(h);
            if rc < 0 {
                ffi::mpv_terminate_destroy(h);
                return Err(format!("The video engine (libmpv) could not start ({}).", error_text(rc)));
            }
            let mut ctx: *mut ffi::mpv_render_context = std::ptr::null_mut();
            let mut params = [
                ffi::mpv_render_param { type_: ffi::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE, data: ffi::MPV_RENDER_API_TYPE_SW.as_ptr() as *mut c_void },
                ffi::mpv_render_param { type_: ffi::mpv_render_param_type_MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
            ];
            let rc = ffi::mpv_render_context_create(&mut ctx, h, params.as_mut_ptr());
            if rc < 0 || ctx.is_null() {
                ffi::mpv_terminate_destroy(h);
                return Err(format!("The video engine could not draw pictures ({}).", error_text(rc)));
            }
            ffi::mpv_set_wakeup_callback(h, Some(wake_cb), wake);
            ffi::mpv_render_context_set_update_callback(ctx, Some(wake_cb), wake);
            for (i, (name, kind)) in OBSERVED.iter().enumerate() {
                let n = CString::new(*name).unwrap();
                let f = match kind {
                    Kind::Flag => ffi::mpv_format_MPV_FORMAT_FLAG,
                    Kind::Double => ffi::mpv_format_MPV_FORMAT_DOUBLE,
                    Kind::Text => ffi::mpv_format_MPV_FORMAT_STRING,
                };
                ffi::mpv_observe_property(h, i as u64 + 1, n.as_ptr(), f);
            }
            Ok(Player { h, rc: ctx, gpu, retried: false, buf: Vec::new(), last: (0, 0) })
        }
    }

    fn command(&self, args: &[Value], id: Option<u64>, out: &Shared) {
        let Some(first) = args.first().and_then(Value::as_str) else { return };
        let text: Vec<CString> = args.iter().map(|a| CString::new(arg_text(a).replace('\0', "")).unwrap_or_default()).collect();
        // SAFETY: the pointers live in `text` for the whole call; the list ends with null.
        let rc = unsafe {
            if first == "set_property" && text.len() == 3 {
                ffi::mpv_set_property_string(self.h, text[1].as_ptr(), text[2].as_ptr())
            } else if first == "observe_property" {
                0 // this engine observes its own list
            } else {
                let mut argv: Vec<*const c_char> = text.iter().map(|c| c.as_ptr()).collect();
                argv.push(std::ptr::null());
                ffi::mpv_command(self.h, argv.as_mut_ptr())
            }
        };
        if rc < 0 {
            if let Some(id) = id {
                let _ = out.out.send(Out::Event(Event::Rejected { id, why: error_text(rc) }));
            }
        }
    }

    fn set_hwdec(&self, value: &str) {
        let (k, v) = (CString::new("hwdec").unwrap(), CString::new(value).unwrap_or_default());
        // SAFETY: valid C strings, handle alive.
        unsafe { ffi::mpv_set_property_string(self.h, k.as_ptr(), v.as_ptr()) };
    }

    fn events(&mut self, out: &Shared) {
        loop {
            // SAFETY: the event is valid until the next wait_event call.
            let ev = unsafe { &*ffi::mpv_wait_event(self.h, 0.0) };
            match ev.event_id {
                ffi::mpv_event_id_MPV_EVENT_NONE => break,
                ffi::mpv_event_id_MPV_EVENT_SHUTDOWN => break,
                ffi::mpv_event_id_MPV_EVENT_START_FILE => {
                    self.retried = false;
                    let _ = out.out.send(Out::Event(Event::Started));
                }
                ffi::mpv_event_id_MPV_EVENT_FILE_LOADED => {
                    let _ = out.out.send(Out::Event(Event::Loaded));
                }
                ffi::mpv_event_id_MPV_EVENT_END_FILE => {
                    // SAFETY: END_FILE events carry an mpv_event_end_file.
                    let end = unsafe { &*(ev.data as *const ffi::mpv_event_end_file) };
                    if let Some(e) = end_event(end.reason, || error_text(end.error)) {
                        let _ = out.out.send(Out::Event(e));
                    }
                }
                ffi::mpv_event_id_MPV_EVENT_PROPERTY_CHANGE => {
                    // SAFETY: PROPERTY_CHANGE events carry an mpv_event_property.
                    let p = unsafe { &*(ev.data as *const ffi::mpv_event_property) };
                    let name = unsafe { CStr::from_ptr(p.name) }.to_string_lossy().into_owned();
                    let value = unsafe { read_value(p) };
                    if name == "hwdec-current" {
                        let now = value.as_str().unwrap_or("");
                        // Only once the file is playing does "no" mean the card was refused.
                        if let Some(next) = hwdec_next(self.gpu, now, self.retried) {
                            if next == "auto-copy-safe" {
                                self.retried = true;
                            }
                            self.set_hwdec(&next);
                        }
                    } else {
                        for e in map_property(&name, &value) {
                            let _ = out.out.send(Out::Event(e));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Draw the newest frame when libmpv has one, or the old one again when
    /// the box changed size.
    fn draw(&mut self, out: &Shared, force: bool) {
        // SAFETY: render context alive.
        let flags = unsafe { ffi::mpv_render_context_update(self.rc) };
        let (bw, bh) = unpack_size(out.size.load(Ordering::Relaxed));
        if bw == 0 || bh == 0 {
            return;
        }
        let size = render_size(bw, bh);
        let fresh = flags & ffi::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME as u64 != 0;
        if !(fresh || (force && size != self.last)) {
            return;
        }
        if out.pending.load(Ordering::Relaxed) >= MAX_PENDING_FRAMES {
            return; // the window is behind: this picture is skipped
        }
        let (w, h) = size;
        self.buf.resize(w as usize * h as usize * 4, 0);
        let mut dims = [w as c_int, h as c_int];
        let mut stride: usize = w as usize * 4;
        let format = b"rgb0\0";
        let mut params = [
            ffi::mpv_render_param { type_: ffi::mpv_render_param_type_MPV_RENDER_PARAM_SW_SIZE, data: dims.as_mut_ptr() as *mut c_void },
            ffi::mpv_render_param { type_: ffi::mpv_render_param_type_MPV_RENDER_PARAM_SW_FORMAT, data: format.as_ptr() as *mut c_void },
            ffi::mpv_render_param { type_: ffi::mpv_render_param_type_MPV_RENDER_PARAM_SW_STRIDE, data: &mut stride as *mut usize as *mut c_void },
            ffi::mpv_render_param { type_: ffi::mpv_render_param_type_MPV_RENDER_PARAM_SW_POINTER, data: self.buf.as_mut_ptr() as *mut c_void },
            ffi::mpv_render_param { type_: ffi::mpv_render_param_type_MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        // SAFETY: the buffer holds w*h*4 bytes and stays put for the call.
        let rc = unsafe { ffi::mpv_render_context_render(self.rc, params.as_mut_ptr()) };
        if rc < 0 {
            return;
        }
        make_opaque(&mut self.buf);
        self.last = size;
        out.pending.fetch_add(1, Ordering::Relaxed);
        let handle = cosmic::widget::image::Handle::from_rgba(w, h, self.buf.clone());
        let _ = out.out.send(Out::Frame(handle));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // SAFETY: the render context goes first, then the handle (libmpv's rule).
        unsafe {
            ffi::mpv_render_context_free(self.rc);
            ffi::mpv_terminate_destroy(self.h);
        }
    }
}

/// Value of an observed property as JSON.
///
/// # Safety
/// `p` must come from a PROPERTY_CHANGE event that is still current.
unsafe fn read_value(p: &ffi::mpv_event_property) -> Value {
    if p.data.is_null() {
        return Value::Null;
    }
    match p.format {
        f if f == ffi::mpv_format_MPV_FORMAT_FLAG => json!(unsafe { *(p.data as *const c_int) } != 0),
        f if f == ffi::mpv_format_MPV_FORMAT_DOUBLE => json!(unsafe { *(p.data as *const f64) }),
        f if f == ffi::mpv_format_MPV_FORMAT_STRING => {
            let s = unsafe { *(p.data as *const *const c_char) };
            if s.is_null() { Value::Null } else { json!(unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()) }
        }
        _ => Value::Null,
    }
}

fn run(rx: std_mpsc::Receiver<Msg>, wake: *mut c_void, gpu: bool, out: Shared) {
    let mut player = match Player::start(gpu, wake) {
        Ok(p) => p,
        Err(why) => {
            let _ = out.out.send(Out::Unavailable(why));
            return;
        }
    };
    loop {
        let msg = rx.recv_timeout(Duration::from_millis(100));
        match msg {
            Ok(Msg::Quit) | Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Msg::Cmd(args, id)) => player.command(&args, id, &out),
            Ok(Msg::Wake) => player.events(&out),
            // A timeout is the moment to notice the box changed size.
            Err(std_mpsc::RecvTimeoutError::Timeout) => {}
        }
        // A new frame, or the same one again when the box changed size.
        player.draw(&out, true);
    }
}

/// The video engine as an iced subscription. `gpu` is the Settings switch and
/// `size` the box the picture is drawn for (see `pack_size`), written by the view.
pub fn subscription(gpu: bool, size: Arc<AtomicU64>) -> Subscription<Out> {
    struct Key {
        gpu: bool,
        size: Arc<AtomicU64>,
    }
    impl std::hash::Hash for Key {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.gpu.hash(state); // a changed switch restarts the engine
        }
    }
    Subscription::run_with(Key { gpu, size }, |key| {
        let size = key.size.clone();
        let gpu = key.gpu;
        stream::channel(16, move |mut sink: futures::channel::mpsc::Sender<Out>| async move {
            use futures::SinkExt;
            let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Out>();
            let pending = Arc::new(AtomicUsize::new(0));
            let (tx, rx) = std_mpsc::channel::<Msg>();
            // Leaked for libmpv's callbacks, freed after the thread has destroyed libmpv.
            let wake = Box::into_raw(Box::new(tx.clone()));
            let wake_addr = wake as usize;
            let shared = Shared { out: out_tx, pending: pending.clone(), size };
            let thread = std::thread::Builder::new().name("lapin-video".into()).spawn(move || {
                run(rx, wake_addr as *mut c_void, gpu, shared);
                // SAFETY: libmpv is gone, nothing calls the callbacks any more.
                drop(unsafe { Box::from_raw(wake_addr as *mut std_mpsc::Sender<Msg>) });
            });
            if let Err(e) = thread {
                let _ = sink.send(Out::Unavailable(format!("The video engine could not start ({e})."))).await;
                std::future::pending::<()>().await;
                return;
            }
            let (engine, mut cmds) = Engine::channel();
            let _ = sink.send(Out::Event(Event::Ready(engine))).await;
            let mut newest: Option<Out> = None;
            loop {
                tokio::select! {
                    cmd = cmds.recv() => match cmd {
                        Some((args, id)) => { let _ = tx.send(Msg::Cmd(args, id)); }
                        None => break,
                    },
                    item = out_rx.recv() => {
                        let Some(item) = item else { break };
                        // Only the newest picture matters; events keep their order.
                        let mut batch = vec![item];
                        while let Ok(more) = out_rx.try_recv() { batch.push(more); }
                        for it in batch {
                            match it {
                                Out::Frame(_) => {
                                    if newest.is_some() { pending.fetch_sub(1, Ordering::Relaxed); }
                                    newest = Some(it);
                                }
                                other => { let _ = sink.send(other).await; }
                            }
                        }
                        if let Some(frame) = newest.take() {
                            pending.fetch_sub(1, Ordering::Relaxed);
                            let _ = sink.send(frame).await;
                        }
                    }
                }
            }
            let _ = tx.send(Msg::Quit);
            std::future::pending::<()>().await;
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hwdec_follows_the_setting() {
        assert_eq!(hwdec_start(true), "auto-safe");
        assert_eq!(hwdec_start(false), "no");
    }

    #[test]
    fn zero_copy_methods_get_a_copy_form() {
        assert_eq!(copy_variant("vaapi").as_deref(), Some("vaapi-copy"));
        assert_eq!(copy_variant("nvdec").as_deref(), Some("nvdec-copy"));
        assert_eq!(copy_variant("vaapi-copy"), None);
        assert_eq!(copy_variant("no"), None);
    }

    #[test]
    fn hwdec_next_changes_only_when_needed() {
        assert_eq!(hwdec_next(false, "vaapi", false), None);
        assert_eq!(hwdec_next(true, "vaapi", false).as_deref(), Some("vaapi-copy"));
        assert_eq!(hwdec_next(true, "vaapi-copy", false), None);
        assert_eq!(hwdec_next(true, "no", false).as_deref(), Some("auto-copy-safe"));
        assert_eq!(hwdec_next(true, "no", true), None);
    }

    #[test]
    fn bar_hides_after_three_seconds_unless_paused() {
        assert!(bar_visible(false, Duration::from_millis(2900)));
        assert!(!bar_visible(false, Duration::from_secs(3)));
        assert!(bar_visible(true, Duration::from_secs(60)));
    }

    #[test]
    fn render_size_is_capped_even_and_never_empty() {
        assert_eq!(render_size(0, 0), (2, 2));
        assert_eq!(render_size(800, 600), (800, 600));
        let (w, h) = render_size(3840, 2160);
        assert_eq!((w, h), (1920, 1080));
        let (w, h) = render_size(1001, 601);
        assert!(w % 2 == 0 && h % 2 == 0);
    }

    #[test]
    fn size_packs_and_unpacks() {
        assert_eq!(unpack_size(pack_size(1280, 720)), (1280, 720));
        assert_eq!(unpack_size(pack_size(0, 0)), (0, 0));
    }

    #[test]
    fn pixels_become_opaque() {
        let mut px = [1u8, 2, 3, 0, 4, 5, 6, 7];
        make_opaque(&mut px);
        assert_eq!(px, [1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn properties_map_to_events() {
        assert!(matches!(map_property("vid", &json!("no"))[..], [Event::HasVideo(false)]));
        assert!(matches!(map_property("vid", &json!("auto"))[..], [Event::HasVideo(true)]));
        assert!(matches!(map_property("vid", &json!(1))[..], [Event::HasVideo(true)]));
        assert!(matches!(&map_property("metadata/by-key/Artist", &json!("A"))[..], [Event::Artist(a)] if a == "A"));
    }

    #[test]
    fn end_events() {
        assert!(matches!(end_event(0, String::new), Some(Event::Ended { error: None })));
        assert!(matches!(end_event(4, || "bad".into()), Some(Event::Ended { error: Some(e) }) if e == "bad"));
        assert!(end_event(2, String::new).is_none());
    }

    #[test]
    fn tracks_are_read_from_the_list() {
        let json = r#"[{"id":1,"type":"video","selected":true},
            {"id":1,"type":"audio","lang":"eng","title":"Stereo","selected":true},
            {"id":2,"type":"audio","lang":"fra"},
            {"id":1,"type":"sub","external":true},
            {"id":2,"type":"video","albumart":true},
            {"type":"sub"}]"#;
        let t = parse_tracks(json);
        assert_eq!(t.len(), 4);
        assert!(t[0].kind == engine::TrackKind::Video && t[0].selected && t[0].id == 1);
        assert!(t[1].kind == engine::TrackKind::Audio && t[1].selected);
        assert!(!t[2].selected && t[2].lang == "fra");
        assert!(t[3].kind == engine::TrackKind::Subtitle);
        assert!(parse_tracks("").is_empty());
        assert!(parse_tracks("{}").is_empty());
        assert!(matches!(&map_property("track-list", &json!("[]"))[..], [Event::Tracks(v)] if v.is_empty()));
    }

    #[test]
    fn chapters_loop_and_speed() {
        let c = parse_chapters(r#"[{"title":"Intro","time":0.0},{"title":"","time":65.4},{"time":3725}]"#);
        assert_eq!(c.len(), 3);
        assert_eq!(chapter_label(&c[0].0, c[0].1, 1), "Intro  0:00");
        assert_eq!(chapter_label(&c[1].0, c[1].1, 2), "Chapter 2  1:05");
        assert_eq!(chapter_label(&c[2].0, c[2].1, 3), "Chapter 3  1:02:05");
        assert!(parse_chapters("").is_empty());
        assert!(matches!(&map_property("chapter-list", &json!("[]"))[..], [Event::Chapters(v)] if v.is_empty()));
        assert!(matches!(map_property("chapter", &json!(2.0))[..], [Event::Chapter(2)]));
        assert!(matches!(map_property("chapter", &Value::Null)[..], [Event::Chapter(-1)]));
        assert!(matches!(map_property("ab-loop-a", &json!("12.5"))[..], [Event::LoopPoint(false, true)]));
        assert!(matches!(map_property("ab-loop-b", &json!("no"))[..], [Event::LoopPoint(true, false)]));
        assert_ne!(loop_label(false, false), loop_label(true, false));
        assert_eq!(loop_label(true, true), loop_label(true, true));
        assert_ne!(loop_label(true, true), loop_label(false, false));
        assert_eq!(speed_label(1.0), "1x");
        assert_eq!(speed_label(0.75), "0.75x");
        assert_eq!(speed_label(1.5), "1.5x");
        assert_eq!(speed_label(2.0), "2x");
        assert!(SPEEDS.contains(&1.0));
    }

    #[test]
    fn subtitle_position() {
        assert_eq!(Tune::SubPos.default_value(), 100.0);
        assert_eq!(Tune::SubPos.step(100.0, 1), 100.0);
        assert_eq!(Tune::SubPos.step(100.0, -1), 95.0);
        assert_eq!(Tune::SubPos.step(2.0, -1), 0.0);
        assert_eq!(Tune::SubPos.step(40.0, 0), 100.0);
        assert_eq!(Tune::SubPos.label(95.0), "95%");
        assert_eq!(Tune::SubPos.property(), "sub-pos");
    }

    #[test]
    fn track_labels() {
        let t = |title: &str, lang: &str| engine::Track { id: 1, kind: engine::TrackKind::Audio, title: title.into(), lang: lang.into(), selected: false };
        assert_eq!(track_label(&t("Stereo", "eng"), 1), "Stereo (eng)");
        assert_eq!(track_label(&t("Stereo", ""), 1), "Stereo");
        assert_eq!(track_label(&t("", "eng"), 1), "eng");
        assert_eq!(track_label(&t("", ""), 2), "Track 2");
    }

    #[test]
    fn arguments_are_text() {
        assert_eq!(arg_text(&json!("a")), "a");
        assert_eq!(arg_text(&json!(true)), "yes");
        assert_eq!(arg_text(&json!(false)), "no");
        assert_eq!(arg_text(&json!(5)), "5");
    }

    #[test]
    fn tune_steps_and_labels() {
        assert_eq!(Tune::SubDelay.step(0.0, 1), 0.1);
        assert_eq!(Tune::SubDelay.step(0.3, -1), 0.2);
        assert_eq!(Tune::AudioDelay.step(60.0, 1), 60.0);
        assert_eq!(Tune::SubScale.step(1.0, 1), 1.1);
        assert_eq!(Tune::SubScale.step(0.3, -1), 0.3);
        assert_eq!(Tune::SubScale.step(1.4, 0), 1.0);
        assert_eq!(Tune::SubDelay.step(2.5, 0), 0.0);
        let mut v = 0.0;
        for _ in 0..30 {
            v = Tune::SubDelay.step(v, 1);
        }
        assert_eq!(Tune::SubDelay.label(v), "+3.0 s");
        assert_eq!(Tune::AudioDelay.label(-0.25), "-0.3 s");
        assert_eq!(Tune::AudioDelay.label(0.0), "0 s");
        assert_eq!(Tune::SubScale.label(1.1), "110%");
        assert_eq!(Tune::SubScale.property(), "sub-scale");
    }
}
