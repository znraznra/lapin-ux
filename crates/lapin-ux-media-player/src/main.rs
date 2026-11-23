// SPDX-License-Identifier: GPL-3.0-only
//! Lapin Media Player (#222): music library, radio and video, on an mpv engine.
//! It follows the light or dark choice of the session like every other Lapin
//! window. Nothing is fetched from the network until the user plays an
//! address or a station they added themselves.

mod engine;
mod library;
mod mpris;
mod pick;
mod settings;
mod tags;

use std::path::{Path, PathBuf};

use cosmic::app::{Settings, Task};
use cosmic::iced::{Alignment, Length, Size, Subscription};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, slider, text, text_input};
use cosmic::{ApplicationExt, Core, Element, theme};
use serde_json::json;

use library::{Kind, Station, Track};

const APP_ID: &str = "dev.lapinux.MediaPlayer";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    ListenNow,
    Artists,
    Albums,
    Songs,
    Favourites,
    Videos,
    Radio,
    MadeForYou,
    /// Not in the library list: opened from the gear at the bottom of the sidebar.
    Settings,
}

impl Section {
    const ALL: [Section; 8] = [
        Section::ListenNow,
        Section::Artists,
        Section::Albums,
        Section::Songs,
        Section::Favourites,
        Section::Videos,
        Section::Radio,
        Section::MadeForYou,
    ];
    fn label(self) -> &'static str {
        match self {
            Section::ListenNow => "Listen Now",
            Section::Artists => "Artists",
            Section::Albums => "Albums",
            Section::Songs => "Songs",
            Section::Favourites => "Favourite Songs",
            Section::Videos => "Videos",
            Section::Radio => "Radio",
            Section::MadeForYou => "Made for You",
            Section::Settings => "Settings",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Section::ListenNow => "media-playback-start-symbolic",
            Section::Artists => "avatar-default-symbolic",
            Section::Albums => "media-optical-symbolic",
            Section::Songs => "audio-x-generic-symbolic",
            Section::Favourites => "emblem-favorite-symbolic",
            Section::Videos => "video-x-generic-symbolic",
            Section::Radio => "network-wireless-symbolic",
            Section::MadeForYou => "starred-symbolic",
            Section::Settings => "preferences-system-symbolic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuId {
    File,
    Edit,
    Media,
    View,
    Controls,
    Playlist,
    Window,
    Help,
}

impl MenuId {
    const ALL: [MenuId; 8] =
        [MenuId::File, MenuId::Edit, MenuId::Media, MenuId::View, MenuId::Controls, MenuId::Playlist, MenuId::Window, MenuId::Help];
    fn label(self) -> &'static str {
        match self {
            MenuId::File => "File",
            MenuId::Edit => "Edit",
            MenuId::Media => "Media",
            MenuId::View => "View",
            MenuId::Controls => "Controls",
            MenuId::Playlist => "Playlist",
            MenuId::Window => "Window",
            MenuId::Help => "Help",
        }
    }
    fn items(self) -> &'static [(&'static str, Action)] {
        use Action::*;
        match self {
            MenuId::File => &[("Open file...", Pick(pick::Job::File)), ("Open folder...", Pick(pick::Job::Folder)), ("Open web address...", Prompt(Ask::Open)), ("Settings", Show(Section::Settings)), ("Quit", Quit)],
            MenuId::Edit => &[("Add radio station...", AddStation), ("Import stations...", Pick(pick::Job::Stations)), ("Export stations...", Pick(pick::Job::SaveStations))],
            MenuId::Media => &[("Play / Pause", PlayPause), ("Stop", Stop), ("Next", Next), ("Previous", Prev)],
            MenuId::View => &[("Show queue", ToggleQueue), ("Output device", ToggleDevices), ("Mini player", ToggleMini), ("Songs", Show(Section::Songs)), ("Videos", Show(Section::Videos)), ("Radio", Show(Section::Radio))],
            MenuId::Controls => &[
                ("Jump back 10 seconds", Back10),
                ("Jump forward 10 seconds", Fwd10),
                ("Faster", Faster),
                ("Slower", Slower),
                ("Normal speed", NormalSpeed),
                ("Frame step", FrameStep),
                ("Louder", VolUp),
                ("Quieter", VolDown),
                ("Mute", Mute),
            ],
            MenuId::Playlist => &[("Open playlist...", Pick(pick::Job::Playlist)), ("Save queue as playlist...", Pick(pick::Job::SavePlaylist)), ("Clear queue", ClearQueue)],
            MenuId::Window => &[("Full screen video", Fullscreen)],
            MenuId::Help => &[("About Lapin Media Player", About)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Open,
    ImportPlaylist,
    ExportPlaylist,
    ImportStations,
    ExportStations,
    LibraryFolder,
}

impl Ask {
    fn label(self) -> &'static str {
        match self {
            Ask::Open => "Web address or file path to play",
            Ask::ImportPlaylist => "Playlist file (.m3u, .m3u8, .pls)",
            Ask::ExportPlaylist => "Save the queue as (.m3u)",
            Ask::ImportStations => "Stations file (.m3u or .json)",
            Ask::ExportStations => "Save stations as (.json or .m3u)",
            Ask::LibraryFolder => "Folder to add to the library",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Prompt(Ask),
    /// A file dialog from the desktop portal.
    Pick(pick::Job),
    Quit,
    AddStation,
    PlayPause,
    Stop,
    Next,
    Prev,
    ToggleQueue,
    ToggleDevices,
    ToggleMini,
    Show(Section),
    Back10,
    Fwd10,
    Faster,
    Slower,
    NormalSpeed,
    FrameStep,
    VolUp,
    VolDown,
    Mute,
    ClearQueue,
    Fullscreen,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repeat {
    Off,
    All,
    One,
}

#[derive(Debug, Clone)]
struct Item {
    title: String,
    artist: String,
    target: String,
    /// An address on the network, not a file. Never recorded in the play counts.
    stream: bool,
    #[allow(dead_code)]
    kind: Kind,
}

impl Item {
    fn new(title: String, artist: String, target: String, kind: Kind) -> Item {
        let stream = target.contains("://");
        Item { title, artist, target, stream, kind }
    }
}

impl From<&Track> for Item {
    fn from(t: &Track) -> Self {
        Item::new(t.title.clone(), t.artist.clone(), t.path.to_string_lossy().into_owned(), t.kind)
    }
}

/// Decode `%20` style escapes in a `file://` address.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A command line argument or typed address as something mpv can open:
/// `file://` addresses become decoded paths, everything else is kept.
fn open_target(arg: &str) -> String {
    match arg.strip_prefix("file://") {
        Some(rest) => {
            // file://localhost/path and file:///path both mean /path
            let rest = rest.strip_prefix("localhost").unwrap_or(rest);
            percent_decode(rest)
        }
        None => arg.to_string(),
    }
}

/// Expand a leading `~` or `~/` only, never `~name`.
fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(s));
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    }
    PathBuf::from(s)
}

/// A local path made absolute so an exported playlist works from anywhere.
fn absolute_target(t: &str) -> String {
    if t.contains("://") {
        return t.to_string();
    }
    let p = Path::new(t);
    if p.is_absolute() {
        return t.to_string();
    }
    match std::env::current_dir() {
        Ok(d) => d.join(p).to_string_lossy().into_owned(),
        Err(_) => t.to_string(),
    }
}

#[derive(Debug, Default, Clone)]
struct Now {
    title: String,
    artist: String,
    album: String,
    stream_title: String,
    pos: f64,
    dur: f64,
    paused: bool,
    has_video: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Engine(engine::Event),
    Mpris(mpris::Cmd),
    SetDevice(String),
    Menu(MenuId),
    Do(Action),
    Show(Section),
    Search(String),
    /// The list is shared, so building a row does not copy the whole queue.
    PlayList(std::sync::Arc<Vec<Item>>, usize),
    PlayStation(usize),
    PlayPause,
    Next,
    Prev,
    Shuffle,
    Repeat,
    SeekDrag(f32),
    SeekRelease,
    Volume(f32),
    /// The tracks found, and how many folders or files could not be read.
    Scanned(u64, Vec<Track>, usize),
    /// The same tracks with titles, artists and albums from the files' own tags.
    Tagged(u64, Vec<Track>),
    /// A small picture for each folder that has one: (folder, picture file).
    Covers(u64, Vec<(PathBuf, PathBuf)>),
    /// A switch on the Settings page.
    Toggle(Switch),
    RemoveFolder(usize),
    Rescan,
    ResetPlays,
    /// A file dialog answered: the job, and the chosen path or why it could not open.
    Picked(pick::Job, pick::Picked),
    AskInput(String),
    AskSubmit,
    AskCancel,
    StationName(String),
    StationUrl(String),
    StationGenre(String),
    StationSave,
    StationDelete(usize),
    StationFavourite(usize),
    /// Star or unstar a song, by file path.
    ToggleFavourite(String),
    StationCancel,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Switch {
    UseTags,
    ShowCovers,
}

struct App {
    core: Core,
    engine: Option<engine::Engine>,
    problem: Option<String>,
    notice: Option<String>,
    tracks: Vec<Track>,
    scanning: bool,
    stations: Vec<Station>,
    section: Section,
    menu: Option<MenuId>,
    search: String,
    queue: Vec<Item>,
    index: Option<usize>,
    /// Station being played, so the star and the stream title can follow it.
    station: Option<usize>,
    show_queue: bool,
    /// The person closed the playlist beside the video list.
    hide_video_queue: bool,
    /// A library folder was added, so the next message handler looks again.
    scan_wanted: bool,
    /// Sound outputs mpv found: (name, description). `device` is the one in use.
    devices: Vec<(String, String)>,
    device: String,
    show_devices: bool,
    mini: bool,
    shuffle: bool,
    repeat: Repeat,
    now: Now,
    volume: f64,
    muted: bool,
    drag: Option<f32>,
    ask: Option<(Ask, String)>,
    /// Name, address, genre of the station being added.
    new_station: Option<(String, String, String)>,
    about: bool,
    start: Vec<String>,
    /// Number of times each file was played, for Made for You.
    plays: std::collections::HashMap<String, u32>,
    /// Paths of the songs the user starred.
    favourites: std::collections::HashSet<String>,
    /// A load was sent and mpv has not said it started yet, so an end-of-file
    /// that arrives now belongs to the file being replaced.
    awaiting_start: bool,
    /// Files in a row that failed to play, so the queue stops instead of looping.
    fail_streak: usize,
    /// Last position mpv reported, to go back to when it refuses a seek.
    confirmed_pos: f64,
    /// stations.json exists but could not be read, so it is never overwritten.
    stations_error: bool,
    settings: settings::Settings,
    /// Album pictures, by the folder they belong to.
    covers: std::collections::HashMap<PathBuf, widget::image::Handle>,
    /// Which scan the answers belong to, so a slow older scan cannot replace a newer one.
    scan_id: u64,
}

fn fmt_time(s: f64) -> String {
    let s = s.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn plays_path() -> PathBuf {
    engine::config_dir().join("plays.json")
}

fn device_path() -> PathBuf {
    engine::config_dir().join("output-device")
}

fn favourites_path() -> PathBuf {
    engine::config_dir().join("favourites.json")
}

fn pseudo_random(n: usize) -> usize {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(7);
    (t ^ (t >> 7)).wrapping_mul(2654435761) % n.max(1)
}

/// The tracks found, and how many folders or files could not be read.
fn run_scan(roots: Vec<PathBuf>) -> (Vec<Track>, usize) {
    let mut out: Vec<Track> = Vec::new();
    let mut skipped = 0;
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for d in roots {
        let (t, s) = library::scan_report(&d);
        // folders may overlap (Music inside a folder that was added too)
        for x in t {
            if seen.insert(x.path.clone()) {
                out.push(x);
            }
        }
        skipped += s;
    }
    (out, skipped)
}

impl App {
    fn play_index(&mut self, i: usize) {
        let Some(item) = self.queue.get(i).cloned() else { return };
        let Some(engine) = &self.engine else {
            self.notice = Some(self.problem.clone().unwrap_or_else(|| "The player engine is still starting.".into()));
            return;
        };
        self.index = Some(i);
        self.notice = None;
        self.now = Now { title: item.title.clone(), artist: item.artist.clone(), ..Default::default() };
        self.confirmed_pos = 0.0;
        self.awaiting_start = true;
        engine.load(&item.target);
        // Only files on this computer are counted; an address typed in or a
        // station is never written to disk.
        if !item.stream {
            *self.plays.entry(item.target).or_insert(0) += 1;
            if let Some(dir) = plays_path().parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(plays_path(), serde_json::to_string(&self.plays).unwrap_or_default());
        }
    }

    fn play_list(&mut self, items: Vec<Item>, at: usize) {
        // Playing a station again from the queue keeps its star and stream title.
        self.station = match items.get(at) {
            Some(it) if it.stream => self.stations.iter().position(|s| s.url == it.target),
            _ => None,
        };
        self.queue = items;
        self.fail_streak = 0;
        self.play_index(at);
    }

    /// Move to another item. `ended` means the current one finished by itself;
    /// `failed` that it could not be played, which never repeats it.
    fn step(&mut self, forward: bool, ended: bool, failed: bool) {
        let n = self.queue.len();
        let Some(i) = self.index else { return };
        if n == 0 {
            return;
        }
        if failed {
            self.fail_streak += 1;
            if self.fail_streak >= n {
                // every item failed in a row: stop, and leave the notice up
                self.index = None;
                self.now = Now::default();
                return;
            }
        }
        if ended && !failed && self.repeat == Repeat::One {
            return self.play_index(i);
        }
        if self.station.is_some() && ended {
            return; // a station that ended dropped; the notice says so
        }
        let next = if self.shuffle && n > 1 {
            let mut j = pseudo_random(n);
            if j == i {
                j = (j + 1) % n;
            }
            Some(j)
        } else if forward {
            if i + 1 < n { Some(i + 1) } else if self.repeat == Repeat::All || !ended { Some(0) } else { None }
        } else if i > 0 {
            Some(i - 1)
        } else {
            Some(n - 1)
        };
        match next {
            Some(j) => {
                let keep = if failed { self.notice.clone() } else { None };
                self.play_index(j);
                if keep.is_some() {
                    self.notice = keep; // play_index clears it, the error should stay readable
                }
            }
            None => {
                self.index = None;
                self.now = Now::default();
            }
        }
    }

    fn engine_event(&mut self, e: engine::Event) {
        use engine::Event as E;
        match e {
            E::Ready(eng) => {
                eng.set("volume", json!(self.volume));
                if let Ok(d) = std::fs::read_to_string(device_path()) {
                    let d = d.trim();
                    if !d.is_empty() && d != "auto" {
                        eng.set("audio-device", json!(d));
                    }
                }
                self.engine = Some(eng);
                self.problem = None;
                for target in std::mem::take(&mut self.start) {
                    let p = Path::new(&target);
                    let item = Item::new(p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(target.clone()), String::new(), target.clone(), library::kind_of(p).unwrap_or(Kind::Audio));
                    self.queue.push(item);
                }
                if !self.queue.is_empty() && self.index.is_none() {
                    self.play_index(0);
                }
            }
            E::Pause(p) => self.now.paused = p,
            E::Time(t) => {
                self.now.pos = t;
                self.confirmed_pos = t;
            }
            E::Started => self.awaiting_start = false,
            E::Rejected { id, why } => {
                if id == engine::SEEK_ID {
                    self.now.pos = self.confirmed_pos;
                    self.notice = Some(format!("The player did not accept that jump ({why})."));
                }
            }
            E::Duration(d) => self.now.dur = d,
            E::Volume(v) => self.volume = v,
            E::Mute(m) => self.muted = m,
            E::Speed(_) => {}
            E::Devices(d) => self.devices = d,
            E::Device(d) => self.device = d,
            E::Title(t) => {
                if !t.is_empty() && (self.now.title.is_empty() || self.station.is_some()) {
                    self.now.title = t;
                }
            }
            E::Artist(a) => {
                if !a.is_empty() || self.station.is_none() {
                    self.now.artist = if a.is_empty() { self.now.artist.clone() } else { a };
                }
            }
            E::Album(a) => self.now.album = a,
            E::StreamTitle(t) => self.now.stream_title = t,
            E::HasVideo(v) => {
                if v && !self.now.has_video {
                    self.hide_video_queue = false;
                }
                self.now.has_video = v;
            }
            E::Loaded => {
                self.awaiting_start = false;
                self.notice = None;
                self.fail_streak = 0;
            }
            // The end of the file that was just replaced, not of the new one.
            E::Ended { .. } if self.awaiting_start => {}
            E::Ended { error: Some(why) } => {
                self.notice = Some(match self.station {
                    Some(i) => format!("{} could not be played ({why}). Check the address in Radio.", self.stations.get(i).map(|s| s.name.as_str()).unwrap_or("The station")),
                    None => format!("{} could not be played ({why}).", self.now.title),
                });
                if self.station.is_none() {
                    self.step(true, true, true);
                }
            }
            E::Ended { error: None } => self.step(true, true, false),
            E::Failed(why) => {
                self.engine = None;
                self.problem = Some(why);
            }
        }
    }

    fn do_action(&mut self, a: Action) -> Task<Message> {
        let eng = self.engine.clone();
        let send = |f: &dyn Fn(&engine::Engine)| {
            if let Some(e) = &eng {
                f(e)
            }
        };
        match a {
            Action::Prompt(k) => self.ask = Some((k, String::new())),
            Action::Pick(job) => {
                return Task::future(async move { cosmic::Action::App(Message::Picked(job, pick::pick(job).await)) });
            }
            Action::Quit => {
                return cosmic::iced::exit();
            }
            Action::AddStation => {
                self.section = Section::Radio;
                self.new_station = Some(Default::default());
            }
            Action::PlayPause => {
                // Same as the button in the bar: with a queue and nothing
                // playing it starts the first item, otherwise it pauses or resumes.
                if self.index.is_none() && !self.queue.is_empty() {
                    self.fail_streak = 0;
                    self.play_index(0);
                } else {
                    send(&|e| e.toggle("pause"));
                }
            }
            Action::Stop => {
                send(&|e| e.stop());
                self.awaiting_start = false;
                self.index = None;
                self.station = None;
                self.now = Now::default();
            }
            Action::Next => {
                self.fail_streak = 0;
                self.step(true, false, false)
            }
            Action::Prev => {
                if self.now.pos > 3.0 {
                    send(&|e| e.seek_to(0.0));
                } else {
                    self.fail_streak = 0;
                    self.step(false, false, false)
                }
            }
            Action::ToggleQueue => {
                if self.show_queue {
                    self.show_queue = false;
                    self.hide_video_queue = true;
                } else if self.inline_queue() {
                    self.hide_video_queue = true;
                } else {
                    self.show_queue = true;
                    self.hide_video_queue = false;
                }
            }
            Action::ToggleDevices => self.show_devices = !self.show_devices,
            Action::ToggleMini => {
                self.mini = !self.mini;
                if let Some(id) = self.core.main_window_id() {
                    let size = if self.mini { Size::new(760.0, 150.0) } else { Size::new(1100.0, 720.0) };
                    return cosmic::iced::window::resize(id, size);
                }
            }
            Action::Show(s) => self.section = s,
            Action::Back10 => send(&|e| e.seek_by(-10.0)),
            Action::Fwd10 => send(&|e| e.seek_by(10.0)),
            Action::Faster => send(&|e| e.command(vec![json!("multiply"), json!("speed"), json!(1.1)])),
            Action::Slower => send(&|e| e.command(vec![json!("multiply"), json!("speed"), json!(1.0 / 1.1)])),
            Action::NormalSpeed => send(&|e| e.set("speed", json!(1.0))),
            Action::FrameStep => send(&|e| e.frame_step()),
            Action::VolUp => self.set_volume((self.volume + 5.0).min(130.0)),
            Action::VolDown => self.set_volume((self.volume - 5.0).max(0.0)),
            Action::Mute => send(&|e| e.toggle("mute")),
            Action::ClearQueue => {
                self.queue.clear();
                self.index = None;
            }
            Action::Fullscreen => send(&|e| e.toggle("fullscreen")),
            Action::About => self.about = !self.about,
        }
        Task::none()
    }

    /// The playlist that opens beside the video list while a video plays.
    fn inline_queue(&self) -> bool {
        self.section == Section::Videos && self.now.has_video && !self.show_queue && !self.hide_video_queue && !self.queue.is_empty()
    }

    fn set_volume(&mut self, v: f64) {
        self.volume = v;
        if let Some(e) = &self.engine {
            e.set("volume", json!(v));
        }
    }

    fn submit_ask(&mut self) {
        let Some((kind, value)) = self.ask.take() else { return };
        self.apply_ask(kind, value.trim().to_string());
    }

    /// A file dialog answered. A dialog that could not open (no portal) falls
    /// back to typing the path, so nothing is lost.
    /// Looks through the library folders again. Titles and pictures follow once
    /// the files are listed, so the lists show up first.
    fn start_scan(&mut self) -> Task<Message> {
        self.scan_id += 1;
        self.scanning = true;
        let id = self.scan_id;
        let roots = self.settings.roots();
        Task::future(async move {
            let (found, skipped) = tokio::task::spawn_blocking(move || run_scan(roots)).await.unwrap_or_default();
            cosmic::Action::App(Message::Scanned(id, found, skipped))
        })
    }

    /// Titles and pictures for the files just listed, in the background.
    fn enrich(&self, tracks: &[Track]) -> Task<Message> {
        let id = self.scan_id;
        let mut jobs = Vec::new();
        if self.settings.use_tags {
            let t = tracks.to_vec();
            jobs.push(Task::future(async move {
                let tagged = tokio::task::spawn_blocking(move || tags::apply(t)).await;
                match tagged {
                    Ok(v) => cosmic::Action::App(Message::Tagged(id, v)),
                    Err(_) => cosmic::Action::App(Message::Tagged(id, Vec::new())),
                }
            }));
        }
        if self.settings.show_covers {
            let t = tracks.to_vec();
            jobs.push(Task::future(async move {
                let found = tokio::task::spawn_blocking(move || tags::covers(&t)).await.unwrap_or_default();
                cosmic::Action::App(Message::Covers(id, found))
            }));
        }
        Task::batch(jobs)
    }

    fn save_settings(&mut self) {
        if let Err(e) = self.settings.save() {
            tracing::warn!("could not save settings: {e}");
            self.notice = Some("The settings could not be saved, so they will be back as they were next time.".into());
        }
    }

    fn add_library_folder(&mut self, dir: &str) {
        if !Path::new(dir).is_dir() {
            self.notice = Some(format!("{dir} is not a folder."));
            return;
        }
        if self.settings.add_folder(dir) {
            self.save_settings();
            self.scan_wanted = true;
        } else {
            self.notice = Some("That folder is already in the library.".into());
        }
    }

    fn picked(&mut self, job: pick::Job, r: pick::Picked) {
        use pick::Job;
        let kind = match job {
            Job::LibraryFolder => {
                match r {
                    Ok(Some(dir)) => self.add_library_folder(&dir),
                    Ok(None) => {}
                    Err(why) => {
                        tracing::warn!("file dialog unavailable: {why}");
                        self.notice = Some("The file dialog could not open, so type the path instead.".into());
                        self.ask = Some((Ask::LibraryFolder, String::new()));
                    }
                }
                return;
            }
            Job::File | Job::Folder => Ask::Open,
            Job::Playlist => Ask::ImportPlaylist,
            Job::Stations => Ask::ImportStations,
            Job::SavePlaylist => Ask::ExportPlaylist,
            Job::SaveStations => Ask::ExportStations,
        };
        match r {
            Ok(Some(path)) => self.apply_ask(kind, path),
            Ok(None) => {}
            Err(why) => {
                tracing::warn!("file dialog unavailable: {why}");
                self.notice = Some("The file dialog could not open, so type the path instead.".into());
                self.ask = Some((kind, String::new()));
            }
        }
    }

    fn apply_ask(&mut self, kind: Ask, value: String) {
        if value.is_empty() {
            return;
        }
        let path = expand_tilde(&open_target(&value));
        match kind {
            Ask::LibraryFolder => self.add_library_folder(&path.to_string_lossy()),
            Ask::Open => {
                if library::valid_stream_url(&value) {
                    self.station = None;
                    self.play_list(vec![Item::new(value.clone(), String::new(), value, Kind::Audio)], 0);
                } else if path.is_dir() {
                    let found = library::scan(&path);
                    if found.is_empty() {
                        self.notice = Some("No music or videos found in that folder.".into());
                    } else {
                        self.play_list(found.iter().map(Item::from).collect(), 0);
                    }
                } else if path.is_file() {
                    let kind = library::kind_of(&path).unwrap_or(Kind::Audio);
                    let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    self.play_list(vec![Item::new(title, String::new(), path.to_string_lossy().into_owned(), kind)], 0);
                } else {
                    self.notice = Some(format!("{value} was not found."));
                }
            }
            Ask::ImportPlaylist => match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let base = path.parent().unwrap_or(Path::new("."));
                    let items: Vec<Item> = library::parse_playlist(&text, base)
                        .into_iter()
                        .map(|t| Item::new(Path::new(&t).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(t.clone()), String::new(), t.clone(), library::kind_of(Path::new(&t)).unwrap_or(Kind::Audio)))
                        .collect();
                    if items.is_empty() {
                        self.notice = Some("That playlist has nothing in it.".into());
                    } else {
                        self.show_queue = true;
                        self.play_list(items, 0);
                    }
                }
                Err(e) => self.notice = Some(format!("The playlist could not be read: {e}")),
            },
            Ask::ExportPlaylist => {
                // the file is always M3U, so a .pls name would hold the wrong contents
                let path = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pls")) { path.with_extension("m3u") } else { path };
                let list: Vec<(String, String)> = self.queue.iter().map(|i| (i.title.clone(), absolute_target(&i.target))).collect();
                self.notice = Some(match library::write_m3u(&list) {
                    Ok(text) => match std::fs::write(&path, text) {
                        Ok(()) => format!("Saved {}.", path.display()),
                        Err(e) => format!("The playlist could not be saved: {e}"),
                    },
                    Err(why) => format!("The playlist was not saved: {why}"),
                });
            }
            Ask::ImportStations => match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let mut added = 0;
                    for s in library::import_stations(&text) {
                        if !self.stations.iter().any(|x| x.url == s.url) {
                            self.stations.push(s);
                            added += 1;
                        }
                    }
                    self.save_stations();
                    if self.notice.is_none() {
                        self.notice = Some(format!("Added {added} station{}.", if added == 1 { "" } else { "s" }));
                    }
                }
                Err(e) => self.notice = Some(format!("The stations file could not be read: {e}")),
            },
            Ask::ExportStations => {
                let body = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8")) {
                    library::write_m3u(&self.stations.iter().map(|s| (s.name.clone(), s.url.clone())).collect::<Vec<_>>())
                } else {
                    Ok(serde_json::to_string_pretty(&self.stations).unwrap_or_default())
                };
                self.notice = Some(match body {
                    Ok(body) => match std::fs::write(&path, body) {
                        Ok(()) => format!("Saved {}.", path.display()),
                        Err(e) => format!("The stations could not be saved: {e}"),
                    },
                    Err(why) => format!("The stations were not saved: {why}"),
                });
            }
        }
    }

    fn toggle_favourite(&mut self, path: String) {
        if !self.favourites.remove(&path) {
            self.favourites.insert(path);
        }
        if let Some(dir) = favourites_path().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut sorted: Vec<&String> = self.favourites.iter().collect();
        sorted.sort();
        if let Err(e) = std::fs::write(favourites_path(), serde_json::to_string(&sorted).unwrap_or_default()) {
            self.notice = Some(format!("Your favourite songs could not be saved: {e}"));
        }
    }

    fn save_stations(&mut self) {
        if self.stations_error {
            // stations.json is there but could not be read: keep it as it is
            // instead of replacing it with an empty list.
            self.notice = Some("Your saved stations could not be read, so changes are not saved. Fix or move stations.json in the player's settings folder.".into());
            return;
        }
        if let Err(e) = library::save_stations(&self.stations) {
            self.notice = Some(format!("The stations could not be saved: {e}"));
        }
    }

    // ---- views ----

    fn menu_bar(&self) -> Element<'_, Message> {
        let mut bar = row::with_capacity(8).spacing(2);
        for m in MenuId::ALL {
            bar = bar.push(button::text(m.label()).class(if self.menu == Some(m) { theme::Button::Suggested } else { theme::Button::Text }).on_press(Message::Menu(m)));
        }
        let mut col = column::with_capacity(2).push(bar);
        if let Some(m) = self.menu {
            let mut strip = row::with_capacity(9).spacing(2);
            for (label, action) in m.items() {
                strip = strip.push(button::text(*label).class(theme::Button::Standard).on_press(Message::Do(*action)));
            }
            col = col.push(strip);
        }
        container(col).padding([4, 8]).width(Length::Fill).into()
    }

    fn transport(&self) -> Element<'_, Message> {
        let icon_btn = |name: &'static str, label: &'static str, msg: Message| button::icon(icon::from_name(name)).name(label).on_press(msg);
        let play_icon = if self.now.paused || self.index.is_none() { "media-playback-start-symbolic" } else { "media-playback-pause-symbolic" };
        let left = row::with_capacity(5)
            .spacing(4)
            .align_y(Alignment::Center)
            .push(icon_btn("media-playlist-shuffle-symbolic", if self.shuffle { "Shuffle, on" } else { "Shuffle, off" }, Message::Shuffle).selected(self.shuffle))
            .push(icon_btn("media-skip-backward-symbolic", "Previous", Message::Prev))
            .push(icon_btn(play_icon, if self.now.paused { "Play" } else { "Pause" }, Message::PlayPause))
            .push(icon_btn("media-skip-forward-symbolic", "Next", Message::Next))
            .push(icon_btn(
                if self.repeat == Repeat::One { "media-playlist-repeat-song-symbolic" } else { "media-playlist-repeat-symbolic" },
                match self.repeat {
                    Repeat::Off => "Repeat, off",
                    Repeat::All => "Repeat all",
                    Repeat::One => "Repeat one",
                },
                Message::Repeat,
            ).selected(self.repeat != Repeat::Off));

        let title = if self.index.is_some() || self.station.is_some() { self.now.title.as_str() } else { "Nothing playing" };
        let sub = if !self.now.stream_title.is_empty() {
            self.now.stream_title.clone()
        } else {
            [self.now.artist.as_str(), self.now.album.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" - ")
        };
        let shown = self.drag.map(f64::from).unwrap_or(if self.now.dur > 0.0 { self.now.pos / self.now.dur } else { 0.0 });
        let live = self.station.is_some() && self.now.dur <= 0.0;
        let mut centre = column::with_capacity(3).spacing(2).align_x(Alignment::Center).push(text::heading(title)).push(text::caption(sub));
        if !live {
            let seek = slider(0.0..=1.0f32, shown as f32, Message::SeekDrag)
                .name("Seek")
                .class(lapin_ux_ui::slider())
                .step(0.001f32)
                .on_release(Message::SeekRelease)
                .width(Length::Fill);
            centre = centre.push(
                row::with_capacity(3)
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .push(text::caption(fmt_time(shown * self.now.dur)))
                    .push(seek)
                    .push(text::caption(format!("-{}", fmt_time(self.now.dur - shown * self.now.dur)))),
            );
        } else {
            centre = centre.push(text::caption("Live"));
        }
        let mut card = row::with_capacity(3).spacing(12).align_y(Alignment::Center);
        card = card.push(container(icon::from_name("audio-x-generic-symbolic").size(40)).padding(8).class(theme::Container::Card));
        card = card.push(centre.width(Length::Fill));
        if let Some(i) = self.station {
            let fav = self.stations.get(i).is_some_and(|s| s.favourite);
            card = card.push(icon_btn(if fav { "starred-symbolic" } else { "non-starred-symbolic" }, if fav { "Remove from favourites" } else { "Add to favourites" }, Message::StationFavourite(i)));
        }
        if self.station.is_none() {
            if let Some(item) = self.index.and_then(|i| self.queue.get(i)) {
                let fav = self.favourites.contains(&item.target);
                card = card.push(icon_btn(if fav { "starred-symbolic" } else { "non-starred-symbolic" }, if fav { "Remove from favourites" } else { "Add to favourites" }, Message::ToggleFavourite(item.target.clone())));
            }
        }
        let volume = slider(0.0..=100.0f32, self.volume.min(100.0) as f32, Message::Volume).name("Volume").class(lapin_ux_ui::slider()).width(Length::Fixed(110.0));
        let vol_icon = if self.muted || self.volume <= 0.0 { "audio-volume-muted-symbolic" } else { "audio-volume-high-symbolic" };
        let right = row::with_capacity(5)
            .spacing(4)
            .align_y(Alignment::Center)
            .push(icon_btn(vol_icon, "Mute", Message::Do(Action::Mute)))
            .push(volume)
            .push(icon_btn("audio-speakers-symbolic", "Output device", Message::Do(Action::ToggleDevices)).selected(self.show_devices))
            .push(icon_btn("view-list-symbolic", "Queue", Message::Do(Action::ToggleQueue)).selected(self.show_queue || self.inline_queue()));
        let bar = row::with_capacity(3).spacing(20).align_y(Alignment::Center).push(left).push(container(card).width(Length::FillPortion(3)).padding([6, 12]).class(theme::Container::Card)).push(right);
        container(lapin_ux_ui::a11y::group("Playback controls", bar)).padding([6, 12]).width(Length::Fill).into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut nav = column::with_capacity(12).spacing(2);
        nav = nav.push(text_input::search_input("Search", &self.search).on_input(Message::Search).on_clear(Message::Search(String::new())));
        for s in Section::ALL {
            if s == Section::Artists {
                nav = nav.push(container(text::caption("Library")).padding([8, 12, 2, 12]));
            }
            let content = row::with_capacity(2).spacing(12).align_y(Alignment::Center).push(icon::from_name(s.icon()).size(16)).push(text::body(s.label()));
            nav = nav.push(button::custom(content).width(Length::Fill).padding([8, 12]).class(lapin_ux_ui::nav_item(self.section == s)).on_press(Message::Show(s)));
        }
        let s = Section::Settings;
        let content = row::with_capacity(2).spacing(12).align_y(Alignment::Center).push(icon::from_name(s.icon()).size(16)).push(text::body(s.label()));
        let gear = button::custom(content).width(Length::Fill).padding([8, 12]).class(lapin_ux_ui::nav_item(self.section == s)).on_press(Message::Show(s));
        let page = column::with_capacity(2).push(scrollable(nav).height(Length::Fill)).push(gear);
        container(lapin_ux_ui::a11y::group("Library", page)).width(Length::Fixed(230.0)).padding(8).into()
    }

    fn visible_tracks(&self, kind: Kind) -> Vec<&Track> {
        self.tracks.iter().filter(|t| t.kind == kind && library::matches(t, &self.search)).collect()
    }

    fn track_rows(&self, tracks: Vec<&Track>) -> Element<'_, Message> {
        if tracks.is_empty() {
            return self.empty(if self.scanning { "Looking for your music and videos..." } else { "Nothing here yet. Put music in your Music folder or videos in Videos, or use File > Open." });
        }
        let items: std::sync::Arc<Vec<Item>> = std::sync::Arc::new(tracks.iter().map(|t| Item::from(*t)).collect());
        let mut col = column::with_capacity(tracks.len()).spacing(2);
        for (i, t) in tracks.iter().enumerate() {
            let sub = if t.artist.is_empty() { String::new() } else { format!("{} - {}", t.artist, t.album).trim_end_matches(" - ").to_string() };
            let content = row::with_capacity(3)
                .spacing(12)
                .align_y(Alignment::Center)
                .push(icon::from_name(if t.kind == Kind::Video { "video-x-generic-symbolic" } else { "audio-x-generic-symbolic" }).size(16))
                .push(text::body(t.title.clone()).width(Length::FillPortion(3)))
                .push(text::caption(sub).width(Length::FillPortion(2)));
            let key = t.path.to_string_lossy().into_owned();
            let fav = self.favourites.contains(&key);
            let play = button::custom(content).width(Length::Fill).padding([6, 12]).class(lapin_ux_ui::nav_item(self.queue.get(self.index.unwrap_or(usize::MAX)).is_some_and(|q| q.target == items[i].target))).on_press(Message::PlayList(items.clone(), i));
            let star = button::icon(icon::from_name(if fav { "starred-symbolic" } else { "non-starred-symbolic" })).name(if fav { "Remove from favourites" } else { "Add to favourites" }).on_press(Message::ToggleFavourite(key));
            col = col.push(row::with_capacity(2).spacing(4).align_y(Alignment::Center).push(play).push(star));
        }
        scrollable(col).height(Length::Fill).into()
    }

    fn empty(&self, msg: &str) -> Element<'_, Message> {
        container(text::body(msg.to_string())).center(Length::Fill).into()
    }

    /// Shelf of cover cards: one card per artist or album. A card plays everything in it.
    fn shelf(&self, by_album: bool) -> Element<'_, Message> {
        let audio = self.visible_tracks(Kind::Audio);
        let mut groups: Vec<(String, String, Vec<&Track>)> = Vec::new();
        for t in audio {
            let key = if by_album { t.album.clone() } else { t.artist.clone() };
            if key.is_empty() {
                continue;
            }
            match groups.iter_mut().find(|g| g.0 == key) {
                Some(g) => g.2.push(t),
                None => groups.push((key, if by_album { t.artist.clone() } else { String::new() }, vec![t])),
            }
        }
        if groups.is_empty() {
            return self.empty(if self.scanning { "Looking for your music..." } else { "No music found yet. Put music in your Music folder, in folders named Artist/Album." });
        }
        let mut cards: Vec<Element<'_, Message>> = Vec::new();
        for (name, sub, tracks) in groups {
            let items: Vec<Item> = tracks.iter().map(|t| Item::from(*t)).collect();
            let cover = if by_album && self.settings.show_covers { tracks.first().and_then(|t| t.path.parent()).and_then(|d| self.covers.get(d)) } else { None };
            let picture: Element<'_, Message> = match cover {
                Some(h) => container(widget::image(h.clone()).width(Length::Fixed(150.0)).height(Length::Fixed(150.0))).class(theme::Container::Card).into(),
                None => container(icon::from_name(if by_album { "media-optical-symbolic" } else { "avatar-default-symbolic" }).size(64)).center_x(Length::Fixed(150.0)).padding([24, 0]).class(theme::Container::Card).into(),
            };
            let face = column::with_capacity(3)
                .spacing(6)
                .align_x(Alignment::Center)
                .push(picture)
                .push(text::body(name).width(Length::Fixed(150.0)))
                .push(text::caption(sub).width(Length::Fixed(150.0)));
            cards.push(button::custom(face).padding(4).class(theme::Button::Text).on_press(Message::PlayList(std::sync::Arc::new(items), 0)).into());
        }
        scrollable(container(widget::flex_row(cards).row_spacing(12).column_spacing(12)).padding(16)).height(Length::Fill).into()
    }

    fn radio(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(self.stations.len() + 3).spacing(8).padding(16);
        col = col.push(
            row::with_capacity(2)
                .spacing(8)
                .align_y(Alignment::Center)
                .push(text::title3("Radio").width(Length::Fill))
                .push(button::standard("Add station").on_press(Message::Do(Action::AddStation))),
        );
        if let Some((name, url, genre)) = &self.new_station {
            let ok = !name.trim().is_empty() && library::valid_stream_url(url);
            let form = column::with_capacity(5)
                .spacing(8)
                .push(text_input::text_input("Name", name).on_input(Message::StationName))
                .push(text_input::text_input("Stream address (https://...)", url).on_input(Message::StationUrl))
                .push(text_input::text_input("Genre (optional)", genre).on_input(Message::StationGenre))
                .push(text::caption(if url.is_empty() || ok || name.trim().is_empty() { "" } else { "The address must start with http:// or https://." }))
                .push(row::with_capacity(2).spacing(8).push(button::suggested("Save").on_press_maybe(ok.then_some(Message::StationSave))).push(button::standard("Cancel").on_press(Message::StationCancel)));
            col = col.push(container(form).padding(12).class(theme::Container::Card));
        }
        if self.stations.is_empty() && self.new_station.is_none() {
            col = col.push(text::body("No stations yet. Add the stations you listen to: a name and a stream address. Nothing is contacted until you play one."));
        }
        for (i, s) in self.stations.iter().enumerate() {
            if !self.search.is_empty() && !s.name.to_lowercase().contains(&self.search.to_lowercase()) {
                continue;
            }
            let info = column::with_capacity(2).push(text::body(s.name.clone())).push(text::caption(s.genre.clone()));
            let line = row::with_capacity(4)
                .spacing(8)
                .align_y(Alignment::Center)
                .push(button::custom(row::with_capacity(2).spacing(12).align_y(Alignment::Center).push(icon::from_name("network-wireless-symbolic").size(16)).push(info)).width(Length::Fill).padding([6, 12]).class(lapin_ux_ui::nav_item(self.station == Some(i))).on_press(Message::PlayStation(i)))
                .push(button::icon(icon::from_name(if s.favourite { "starred-symbolic" } else { "non-starred-symbolic" })).name("Favourite").on_press(Message::StationFavourite(i)))
                .push(button::icon(icon::from_name("edit-delete-symbolic")).name("Delete station").on_press(Message::StationDelete(i)));
            col = col.push(line);
        }
        scrollable(col).height(Length::Fill).into()
    }

    fn listen_now(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(4).spacing(12).padding(16);
        let recent: Vec<&Track> = {
            let mut v: Vec<&Track> = self.tracks.iter().filter(|t| t.kind == Kind::Audio && library::matches(t, &self.search)).collect();
            v.sort_by_key(|t| std::cmp::Reverse(self.plays.get(&t.path.to_string_lossy().into_owned()).copied().unwrap_or(0)));
            v.truncate(30);
            v
        };
        let favs: Vec<(usize, &Station)> = self.stations.iter().enumerate().filter(|(_, s)| s.favourite).collect();
        if recent.is_empty() && favs.is_empty() {
            return self.empty(if self.scanning { "Looking for your music..." } else { "Welcome. Put music in your Music folder, add a radio station, or use File > Open." });
        }
        if !favs.is_empty() {
            col = col.push(text::title3("Favourite stations"));
            for (i, s) in favs {
                col = col.push(button::custom(row::with_capacity(2).spacing(12).push(icon::from_name("network-wireless-symbolic").size(16)).push(text::body(s.name.clone()))).width(Length::Fill).padding([6, 12]).class(lapin_ux_ui::nav_item(false)).on_press(Message::PlayStation(i)));
            }
        }
        if !recent.is_empty() {
            col = col.push(text::title3("Your music"));
            col = col.push(container(self.track_rows(recent)).height(Length::Fixed(360.0)));
        }
        scrollable(col).height(Length::Fill).into()
    }

    /// Made for You: mixes from how often you played things, kept on this computer.
    fn made_for_you(&self) -> Element<'_, Message> {
        let count = |t: &Track| self.plays.get(&t.path.to_string_lossy().into_owned()).copied().unwrap_or(0);
        let mut played: Vec<&Track> = self.tracks.iter().filter(|t| t.kind == Kind::Audio && count(t) > 0).collect();
        if played.is_empty() {
            return self.empty("Play some music and your mixes will show up here. Everything stays on this computer.");
        }
        played.sort_by_key(|t| std::cmp::Reverse(count(t)));
        let mut col = column::with_capacity(4).spacing(12).padding(16);
        let most: Vec<Item> = played.iter().take(25).map(|t| Item::from(*t)).collect();
        col = col.push(text::title3("Most played"));
        col = col.push(button::suggested("Play the most played").on_press(Message::PlayList(std::sync::Arc::new(most), 0)));
        if let Some(top) = played.first().map(|t| t.artist.clone()).filter(|a| !a.is_empty()) {
            let mix: Vec<Item> = self.tracks.iter().filter(|t| t.kind == Kind::Audio && t.artist == top).map(Item::from).collect();
            col = col.push(text::title3(format!("More from {top}")));
            col = col.push(button::standard("Play this mix").on_press(Message::PlayList(std::sync::Arc::new(mix), 0)));
        }
        scrollable(col).height(Length::Fill).into()
    }

    fn settings_page(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(16).spacing(10).padding(16).push(text::title3("Settings"));
        col = col.push(text::heading("Where your music and videos are"));
        let roots: Vec<String> = if self.settings.folders.is_empty() {
            self.settings.roots().iter().map(|p| p.to_string_lossy().into_owned()).collect()
        } else {
            self.settings.folders.clone()
        };
        for (i, f) in roots.into_iter().enumerate() {
            let mut line = row::with_capacity(2).spacing(8).align_y(Alignment::Center).push(text::body(f).width(Length::Fill));
            if !self.settings.folders.is_empty() {
                line = line.push(button::standard("Remove").name(format!("Remove folder {}", i + 1)).on_press(Message::RemoveFolder(i)));
            }
            col = col.push(line);
        }
        col = col.push(
            row::with_capacity(2)
                .spacing(8)
                .push(button::standard("Add a folder").on_press(Message::Do(Action::Pick(pick::Job::LibraryFolder))))
                .push(button::standard("Look again").on_press(Message::Rescan)),
        );
        col = col.push(text::heading("Titles and pictures"));
        col = col.push(widget::toggler(self.settings.use_tags).label("Read titles, artists and albums from the files".to_string()).on_toggle(|_| Message::Toggle(Switch::UseTags)));
        col = col.push(widget::toggler(self.settings.show_covers).label("Show album pictures".to_string()).on_toggle(|_| Message::Toggle(Switch::ShowCovers)));
        col = col.push(text::heading("Sound output"));
        if self.devices.is_empty() {
            col = col.push(text::caption("No sound outputs found"));
        }
        for (name, desc) in &self.devices {
            let current = *name == self.device;
            col = col.push(button::custom(text::body(desc.clone())).width(Length::Fill).padding([6, 12]).class(lapin_ux_ui::nav_item(current)).on_press(Message::SetDevice(name.clone())));
        }
        col = col.push(text::heading("History"));
        col = col.push(button::standard("Clear what I have played").on_press(Message::ResetPlays));
        scrollable(container(lapin_ux_ui::a11y::group("Settings", col)).max_width(640)).height(Length::Fill).into()
    }

    fn device_panel(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(self.devices.len() + 2).spacing(2).push(text::heading("Output device"));
        if self.devices.is_empty() {
            col = col.push(text::caption("No sound outputs found"));
        }
        for (name, desc) in &self.devices {
            let current = *name == self.device;
            col = col.push(button::custom(text::body(desc.clone())).width(Length::Fill).padding([6, 12]).class(lapin_ux_ui::nav_item(current)).on_press(Message::SetDevice(name.clone())));
        }
        container(lapin_ux_ui::a11y::group("Output device", scrollable(col))).width(Length::Fixed(260.0)).height(Length::Fill).padding(8).into()
    }

    fn queue_panel(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(self.queue.len() + 1).spacing(2).push(text::heading("Queue"));
        if self.queue.is_empty() {
            col = col.push(text::caption("Empty"));
        }
        let queue = std::sync::Arc::new(self.queue.clone());
        for (i, it) in self.queue.iter().enumerate() {
            col = col.push(button::custom(text::body(it.title.clone())).width(Length::Fill).padding([6, 12]).class(lapin_ux_ui::nav_item(self.index == Some(i))).on_press(Message::PlayList(queue.clone(), i)));
        }
        container(scrollable(col)).width(Length::Fixed(260.0)).height(Length::Fill).padding(8).into()
    }
}


impl App {
    /// What the desktop's media controls should show.
    fn mpris_state(&self) -> mpris::State {
        let has_media = self.engine.is_some() && self.index.is_some();
        mpris::State {
            has_media,
            playing: has_media && !self.now.paused,
            title: if self.now.stream_title.is_empty() { self.now.title.clone() } else { self.now.stream_title.clone() },
            artist: self.now.artist.clone(),
            album: self.now.album.clone(),
            dur: self.now.dur,
            pos: self.now.pos,
            volume: if self.muted { 0.0 } else { self.volume },
        }
    }

    fn mpris_cmd(&mut self, c: mpris::Cmd) -> Task<Message> {
        use mpris::Cmd;
        match c {
            Cmd::PlayPause => return self.update_cmd(Message::PlayPause),
            Cmd::Play | Cmd::Pause => {
                let want_paused = c == Cmd::Pause;
                if self.index.is_none() {
                    if !want_paused {
                        return self.update_cmd(Message::PlayPause);
                    }
                } else if self.now.paused != want_paused {
                    return self.update_cmd(Message::PlayPause);
                }
            }
            Cmd::Next => return self.do_action(Action::Next),
            Cmd::Prev => return self.do_action(Action::Prev),
            Cmd::Stop => return self.do_action(Action::Stop),
            Cmd::SeekBy(d) => {
                if let Some(e) = &self.engine {
                    e.seek_by(d);
                }
            }
            Cmd::SeekTo(t) => {
                if let Some(e) = &self.engine {
                    e.seek_to(t);
                    self.now.pos = t;
                }
            }
            Cmd::Raise => return match self.core.main_window_id() { Some(id) => cosmic::iced::window::gain_focus(id), None => Task::none() },
        }
        Task::none()
    }

    fn update_cmd(&mut self, m: Message) -> Task<Message> {
        self.handle(m)
    }
}

impl App {
    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Engine(e) => self.engine_event(e),
            Message::Mpris(c) => return self.mpris_cmd(c),
            Message::Menu(m) => self.menu = if self.menu == Some(m) { None } else { Some(m) },
            Message::Do(a) => {
                self.menu = None;
                return self.do_action(a);
            }
            Message::Show(s) => self.section = s,
            Message::Search(s) => {
                self.search = s;
                if matches!(self.section, Section::ListenNow | Section::MadeForYou) {
                    self.section = Section::Songs;
                }
            }
            Message::PlayList(items, i) => self.play_list(items.as_ref().clone(), i),
            Message::PlayStation(i) => {
                if let Some(s) = self.stations.get(i).cloned() {
                    self.queue = vec![Item { title: s.name.clone(), artist: s.genre.clone(), target: s.url, stream: true, kind: Kind::Audio }];
                    self.fail_streak = 0;
                    self.play_index(0);
                    self.station = Some(i);
                }
            }
            Message::PlayPause => return self.do_action(Action::PlayPause),
            Message::Next => return self.do_action(Action::Next),
            Message::Prev => return self.do_action(Action::Prev),
            Message::Shuffle => self.shuffle = !self.shuffle,
            Message::Repeat => {
                self.repeat = match self.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                }
            }
            Message::SeekDrag(v) => self.drag = Some(v),
            Message::SeekRelease => {
                if let (Some(v), Some(e)) = (self.drag.take(), &self.engine) {
                    e.seek_to(f64::from(v) * self.now.dur);
                    self.now.pos = f64::from(v) * self.now.dur;
                }
            }
            Message::Volume(v) => self.set_volume(f64::from(v)),
            Message::Scanned(id, t, skipped) => {
                if id != self.scan_id {
                    return Task::none();
                }
                self.scanning = false;
                if skipped > 0 {
                    self.notice = Some(format!("{skipped} folder{} or file{} in the library folders could not be read, so some items may be missing.", if skipped == 1 { "" } else { "s" }, if skipped == 1 { "" } else { "s" }));
                }
                let next = self.enrich(&t);
                self.tracks = t;
                return next;
            }
            Message::Tagged(id, t) => {
                // an empty answer means the reader failed: keep the names from the folders
                if id == self.scan_id && !t.is_empty() {
                    self.tracks = t;
                }
            }
            Message::Covers(id, found) => {
                if id == self.scan_id {
                    self.covers = found.into_iter().map(|(dir, png)| (dir, widget::image::Handle::from_path(png))).collect();
                }
            }
            Message::Toggle(sw) => {
                match sw {
                    Switch::UseTags => self.settings.use_tags = !self.settings.use_tags,
                    Switch::ShowCovers => self.settings.show_covers = !self.settings.show_covers,
                }
                self.save_settings();
                // titles and pictures change with these, so look again
                if !self.settings.show_covers {
                    self.covers.clear();
                }
                return self.start_scan();
            }
            Message::RemoveFolder(i) => {
                self.settings.remove_folder(i);
                self.save_settings();
                return self.start_scan();
            }
            Message::Rescan => return self.start_scan(),
            Message::ResetPlays => {
                self.plays.clear();
                if let Err(e) = std::fs::remove_file(plays_path()) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        self.notice = Some("The play history could not be cleared.".into());
                    }
                }
            }
            Message::AskInput(s) => {
                if let Some((_, v)) = &mut self.ask {
                    *v = s;
                }
            }
            Message::Picked(job, r) => {
                self.picked(job, r);
                if std::mem::take(&mut self.scan_wanted) {
                    return self.start_scan();
                }
            }
            Message::AskSubmit => {
                self.submit_ask();
                if std::mem::take(&mut self.scan_wanted) {
                    return self.start_scan();
                }
            }
            Message::AskCancel => self.ask = None,
            Message::StationName(s) => {
                if let Some(f) = &mut self.new_station {
                    f.0 = s;
                }
            }
            Message::StationUrl(s) => {
                if let Some(f) = &mut self.new_station {
                    f.1 = s;
                }
            }
            Message::StationGenre(s) => {
                if let Some(f) = &mut self.new_station {
                    f.2 = s;
                }
            }
            Message::StationSave => {
                if let Some((name, url, genre)) = self.new_station.take() {
                    self.stations.push(Station { name: name.trim().into(), url: url.trim().into(), genre: genre.trim().into(), ..Default::default() });
                    self.save_stations();
                }
            }
            Message::StationCancel => self.new_station = None,
            Message::StationDelete(i) => {
                if i < self.stations.len() {
                    self.stations.remove(i);
                    self.station = match self.station {
                        Some(s) if s == i => None,
                        Some(s) if s > i => Some(s - 1),
                        o => o,
                    };
                    self.save_stations();
                }
            }
            Message::SetDevice(name) => {
                if let Some(e) = &self.engine {
                    e.set("audio-device", json!(name));
                }
                if let Some(dir) = device_path().parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Err(e) = std::fs::write(device_path(), &name) {
                    self.notice = Some(format!("Could not remember the output device ({e})."));
                }
                self.device = name;
            }
            Message::ToggleFavourite(path) => self.toggle_favourite(path),
            Message::StationFavourite(i) => {
                if let Some(s) = self.stations.get_mut(i) {
                    s.favourite = !s.favourite;
                    self.save_stations();
                }
            }
            Message::Shutdown => {
                if let Some(e) = &self.engine {
                    e.command(vec![json!("quit")]);
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            }
        }
        Task::none()
    }
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Vec<String>;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, start: Vec<String>) -> (Self, Task<Message>) {
        let mut app = App {
            core,
            engine: None,
            problem: None,
            notice: None,
            tracks: Vec::new(),
            scanning: true,
            stations: Vec::new(),
            section: Section::ListenNow,
            menu: None,
            search: String::new(),
            queue: Vec::new(),
            index: None,
            station: None,
            show_queue: false,
            hide_video_queue: false,
            scan_wanted: false,
            devices: Vec::new(),
            device: String::from("auto"),
            show_devices: false,
            mini: false,
            shuffle: false,
            repeat: Repeat::Off,
            now: Now::default(),
            volume: 80.0,
            muted: false,
            drag: None,
            ask: None,
            new_station: None,
            about: false,
            start,
            favourites: std::fs::read_to_string(favourites_path()).ok().and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()).unwrap_or_default().into_iter().collect(),
            plays: std::fs::read_to_string(plays_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
            awaiting_start: false,
            fail_streak: 0,
            confirmed_pos: 0.0,
            stations_error: false,
            settings: settings::Settings::load(),
            covers: std::collections::HashMap::new(),
            scan_id: 0,
        };
        match library::load_stations() {
            Ok(list) => app.stations = list,
            Err(why) => {
                app.stations_error = true;
                app.notice = Some(format!("Your saved stations could not be read ({why}). They are left as they are."));
            }
        }
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Media Player".into(), id),
            None => Task::none(),
        };
        let scan = app.start_scan();
        (app, Task::batch([title, scan]))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![lapin_ux_ui::title_bar("applications-multimedia", "Media Player")]
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([engine::subscription().map(Message::Engine), mpris::subscription().map(Message::Mpris)])
    }

    fn on_app_exit(&mut self) -> Option<Message> {
        Some(Message::Shutdown)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        mpris::publish(self.mpris_state());
        task
    }

    fn view(&self) -> Element<'_, Message> {
        if self.mini {
            let back = button::standard("Full player").on_press(Message::Do(Action::ToggleMini));
            let mut page = column::with_capacity(3).push(self.transport());
            if let Some(msg) = self.problem.as_ref().or(self.notice.as_ref()) {
                page = page.push(container(text::body(msg.clone())).padding([4, 16]));
            }
            return container(lapin_ux_ui::a11y::group("Mini player", page.push(container(back).padding([0, 16, 6, 16])))).width(Length::Fill).height(Length::Fill).into();
        }
        let body: Element<'_, Message> = match self.section {
            Section::ListenNow => self.listen_now(),
            Section::Artists => self.shelf(false),
            Section::Albums => self.shelf(true),
            Section::Songs => self.track_rows(self.visible_tracks(Kind::Audio)),
            Section::Favourites => self.track_rows(self.tracks.iter().filter(|t| self.favourites.contains(&t.path.to_string_lossy().into_owned()) && library::matches(t, &self.search)).collect()),
            Section::Videos => {
                let list = self.track_rows(self.visible_tracks(Kind::Video));
                // while a video plays, its playlist sits beside the list (the picture is in mpv's own window)
                if self.inline_queue() {
                    row::with_capacity(2).push(container(list).width(Length::Fill)).push(self.queue_panel()).into()
                } else {
                    list
                }
            }
            Section::Radio => self.radio(),
            Section::MadeForYou => self.made_for_you(),
            Section::Settings => self.settings_page(),
        };
        let mut main = row::with_capacity(3).push(self.sidebar()).push(container(body).width(Length::Fill).height(Length::Fill));
        if self.show_queue {
            main = main.push(self.queue_panel());
        }
        if self.show_devices {
            main = main.push(self.device_panel());
        }
        let mut page = column::with_capacity(6).push(self.menu_bar()).push(self.transport());
        if let Some(msg) = self.problem.as_ref().or(self.notice.as_ref()) {
            page = page.push(container(text::body(msg.clone())).padding([4, 16]));
        }
        if let Some((kind, value)) = &self.ask {
            let ask = row::with_capacity(3)
                .spacing(8)
                .align_y(Alignment::Center)
                .push(text_input::text_input(kind.label(), value).on_input(Message::AskInput).on_submit(|_| Message::AskSubmit).width(Length::Fill))
                .push(button::suggested("OK").on_press(Message::AskSubmit))
                .push(button::standard("Cancel").on_press(Message::AskCancel));
            page = page.push(container(ask).padding([4, 16]));
        }
        if self.about {
            page = page.push(container(text::caption("Lapin Media Player. Plays through mpv (GPL-2.0-or-later) and FFmpeg. Everything stays on this computer.")).padding([4, 16]));
        }
        page = page.push(widget::divider::horizontal::default()).push(main);
        container(lapin_ux_ui::a11y::group("Media Player", page)).class(theme::Container::Background).width(Length::Fill).height(Length::Fill).into()
    }
}

fn main() -> cosmic::iced::Result {
    lapin_ux_core::crash::install("lapin-ux-media-player");
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .log_internal_errors(false)
        .init();
    let start: Vec<String> = std::env::args().skip(1).map(|a| open_target(&a)).collect();
    cosmic::app::run::<App>(
        Settings::default().size(Size::new(1100.0, 720.0)).size_limits(cosmic::iced::Limits::NONE.min_width(720.0).min_height(420.0)),
        start,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_addresses_are_decoded() {
        assert_eq!(open_target("file:///home/a/My%20Song.mp3"), "/home/a/My Song.mp3");
        assert_eq!(open_target("file://localhost/home/a/b.mp3"), "/home/a/b.mp3");
        assert_eq!(open_target("/home/a/100%.mp3"), "/home/a/100%.mp3");
        assert_eq!(open_target("https://example.org/a%20b"), "https://example.org/a%20b");
        // a stray or cut-off escape is left alone
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("%C3%A9"), "\u{e9}");
    }

    #[test]
    fn only_a_leading_tilde_is_the_home_folder() {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
        assert_eq!(expand_tilde("~/Music"), home.join("Music"));
        assert_eq!(expand_tilde("/music/~backup/a.mp3"), PathBuf::from("/music/~backup/a.mp3"));
        assert_eq!(expand_tilde("~other/a.mp3"), PathBuf::from("~other/a.mp3"));
    }

    #[test]
    fn exported_targets_are_absolute() {
        assert_eq!(absolute_target("/a/b.mp3"), "/a/b.mp3");
        assert_eq!(absolute_target("http://x.example/s"), "http://x.example/s");
        let rel = absolute_target("b.mp3");
        assert!(Path::new(&rel).is_absolute() && rel.ends_with("b.mp3"));
    }

    #[test]
    fn network_items_are_marked() {
        assert!(Item::new("a".into(), String::new(), "https://x.example/s".into(), Kind::Audio).stream);
        assert!(!Item::new("a".into(), String::new(), "/home/a/b.mp3".into(), Kind::Audio).stream);
    }
}
