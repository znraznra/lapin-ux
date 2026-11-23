// SPDX-License-Identifier: GPL-3.0-only
//! Lapin Media Player (#222): music library, radio and video, on an mpv engine.
//! It follows the light or dark choice of the session like every other Lapin
//! window. Nothing is fetched from the network until the user plays an
//! address or a station they added themselves.

mod engine;
mod library;

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
    Videos,
    Radio,
    MadeForYou,
}

impl Section {
    const ALL: [Section; 7] = [
        Section::ListenNow,
        Section::Artists,
        Section::Albums,
        Section::Songs,
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
            Section::Videos => "Videos",
            Section::Radio => "Radio",
            Section::MadeForYou => "Made for You",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Section::ListenNow => "media-playback-start-symbolic",
            Section::Artists => "avatar-default-symbolic",
            Section::Albums => "media-optical-symbolic",
            Section::Songs => "audio-x-generic-symbolic",
            Section::Videos => "video-x-generic-symbolic",
            Section::Radio => "network-wireless-symbolic",
            Section::MadeForYou => "starred-symbolic",
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
            MenuId::File => &[("Open...", Prompt(Ask::Open)), ("Quit", Quit)],
            MenuId::Edit => &[("Add radio station...", AddStation), ("Import stations...", Prompt(Ask::ImportStations)), ("Export stations...", Prompt(Ask::ExportStations))],
            MenuId::Media => &[("Play / Pause", PlayPause), ("Stop", Stop), ("Next", Next), ("Previous", Prev)],
            MenuId::View => &[("Show queue", ToggleQueue), ("Songs", Show(Section::Songs)), ("Videos", Show(Section::Videos)), ("Radio", Show(Section::Radio))],
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
            MenuId::Playlist => &[("Open playlist...", Prompt(Ask::ImportPlaylist)), ("Save queue as playlist...", Prompt(Ask::ExportPlaylist)), ("Clear queue", ClearQueue)],
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
}

impl Ask {
    fn label(self) -> &'static str {
        match self {
            Ask::Open => "File path or address to play",
            Ask::ImportPlaylist => "Playlist file (.m3u, .m3u8, .pls)",
            Ask::ExportPlaylist => "Save the queue as (.m3u)",
            Ask::ImportStations => "Stations file (.m3u or .json)",
            Ask::ExportStations => "Save stations as (.json or .m3u)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Prompt(Ask),
    Quit,
    AddStation,
    PlayPause,
    Stop,
    Next,
    Prev,
    ToggleQueue,
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
    #[allow(dead_code)]
    kind: Kind,
}

impl From<&Track> for Item {
    fn from(t: &Track) -> Self {
        Item { title: t.title.clone(), artist: t.artist.clone(), target: t.path.to_string_lossy().into_owned(), kind: t.kind }
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
    Menu(MenuId),
    Do(Action),
    Show(Section),
    Search(String),
    PlayList(Vec<Item>, usize),
    PlayStation(usize),
    PlayPause,
    Next,
    Prev,
    Shuffle,
    Repeat,
    SeekDrag(f32),
    SeekRelease,
    Volume(f32),
    Scanned(Vec<Track>),
    AskInput(String),
    AskSubmit,
    AskCancel,
    StationName(String),
    StationUrl(String),
    StationGenre(String),
    StationSave,
    StationDelete(usize),
    StationFavourite(usize),
    StationCancel,
    Shutdown,
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
}

fn fmt_time(s: f64) -> String {
    let s = s.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn plays_path() -> PathBuf {
    engine::config_dir().join("plays.json")
}

fn pseudo_random(n: usize) -> usize {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(7);
    (t ^ (t >> 7)).wrapping_mul(2654435761) % n.max(1)
}

fn run_scan() -> Vec<Track> {
    let mut out = Vec::new();
    if let Some(d) = dirs::audio_dir() {
        out.extend(library::scan(&d));
    }
    if let Some(d) = dirs::video_dir() {
        out.extend(library::scan(&d));
    }
    out
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
        engine.load(&item.target);
        *self.plays.entry(item.target).or_insert(0) += 1;
        if let Some(dir) = plays_path().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(plays_path(), serde_json::to_string(&self.plays).unwrap_or_default());
    }

    fn play_list(&mut self, items: Vec<Item>, at: usize) {
        self.station = None;
        self.queue = items;
        self.play_index(at);
    }

    fn step(&mut self, forward: bool, ended: bool) {
        let n = self.queue.len();
        let Some(i) = self.index else { return };
        if n == 0 {
            return;
        }
        if ended && self.repeat == Repeat::One {
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
            Some(j) => self.play_index(j),
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
                self.engine = Some(eng);
                self.problem = None;
                for target in std::mem::take(&mut self.start) {
                    let p = Path::new(&target);
                    let item = Item { title: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(target.clone()), artist: String::new(), kind: library::kind_of(p).unwrap_or(Kind::Audio), target };
                    self.queue.push(item);
                }
                if !self.queue.is_empty() && self.index.is_none() {
                    self.play_index(0);
                }
            }
            E::Pause(p) => self.now.paused = p,
            E::Time(t) => self.now.pos = t,
            E::Duration(d) => self.now.dur = d,
            E::Volume(v) => self.volume = v,
            E::Mute(m) => self.muted = m,
            E::Speed(_) => {}
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
            E::HasVideo(v) => self.now.has_video = v,
            E::Loaded => self.notice = None,
            E::Ended { error: Some(why) } => {
                self.notice = Some(match self.station {
                    Some(i) => format!("{} could not be played ({why}). Check the address in Radio.", self.stations.get(i).map(|s| s.name.as_str()).unwrap_or("The station")),
                    None => format!("{} could not be played ({why}).", self.now.title),
                });
                if self.station.is_none() {
                    self.step(true, true);
                }
            }
            E::Ended { error: None } => self.step(true, true),
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
            Action::Quit => {
                return cosmic::iced::exit();
            }
            Action::AddStation => {
                self.section = Section::Radio;
                self.new_station = Some(Default::default());
            }
            Action::PlayPause => send(&|e| e.toggle("pause")),
            Action::Stop => {
                send(&|e| e.stop());
                self.index = None;
                self.station = None;
                self.now = Now::default();
            }
            Action::Next => self.step(true, false),
            Action::Prev => {
                if self.now.pos > 3.0 {
                    send(&|e| e.seek_to(0.0));
                } else {
                    self.step(false, false)
                }
            }
            Action::ToggleQueue => self.show_queue = !self.show_queue,
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

    fn set_volume(&mut self, v: f64) {
        self.volume = v;
        if let Some(e) = &self.engine {
            e.set("volume", json!(v));
        }
    }

    fn submit_ask(&mut self) {
        let Some((kind, value)) = self.ask.take() else { return };
        let value = value.trim().to_string();
        if value.is_empty() {
            return;
        }
        let path = PathBuf::from(value.replacen('~', &dirs::home_dir().unwrap_or_default().to_string_lossy(), 1).trim_start_matches("file://").to_string());
        match kind {
            Ask::Open => {
                if library::valid_stream_url(&value) {
                    self.station = None;
                    self.play_list(vec![Item { title: value.clone(), artist: String::new(), target: value, kind: Kind::Audio }], 0);
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
                    self.play_list(vec![Item { title, artist: String::new(), target: path.to_string_lossy().into_owned(), kind }], 0);
                } else {
                    self.notice = Some(format!("{value} was not found."));
                }
            }
            Ask::ImportPlaylist => match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let base = path.parent().unwrap_or(Path::new("."));
                    let items: Vec<Item> = library::parse_playlist(&text, base)
                        .into_iter()
                        .map(|t| Item { title: Path::new(&t).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(t.clone()), artist: String::new(), kind: library::kind_of(Path::new(&t)).unwrap_or(Kind::Audio), target: t })
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
                let list: Vec<(String, String)> = self.queue.iter().map(|i| (i.title.clone(), i.target.clone())).collect();
                self.notice = Some(match std::fs::write(&path, library::write_m3u(&list)) {
                    Ok(()) => format!("Saved {}.", path.display()),
                    Err(e) => format!("The playlist could not be saved: {e}"),
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
                    serde_json::to_string_pretty(&self.stations).unwrap_or_default()
                };
                self.notice = Some(match std::fs::write(&path, body) {
                    Ok(()) => format!("Saved {}.", path.display()),
                    Err(e) => format!("The stations could not be saved: {e}"),
                });
            }
        }
    }

    fn save_stations(&mut self) {
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
        let volume = slider(0.0..=100.0f32, self.volume.min(100.0) as f32, Message::Volume).name("Volume").class(lapin_ux_ui::slider()).width(Length::Fixed(110.0));
        let vol_icon = if self.muted || self.volume <= 0.0 { "audio-volume-muted-symbolic" } else { "audio-volume-high-symbolic" };
        let right = row::with_capacity(3)
            .spacing(4)
            .align_y(Alignment::Center)
            .push(icon_btn(vol_icon, "Mute", Message::Do(Action::Mute)))
            .push(volume)
            .push(icon_btn("view-list-symbolic", "Queue", Message::Do(Action::ToggleQueue)).selected(self.show_queue));
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
        container(lapin_ux_ui::a11y::group("Library", nav)).width(Length::Fixed(230.0)).padding(8).into()
    }

    fn visible_tracks(&self, kind: Kind) -> Vec<&Track> {
        self.tracks.iter().filter(|t| t.kind == kind && library::matches(t, &self.search)).collect()
    }

    fn track_rows(&self, tracks: Vec<&Track>) -> Element<'_, Message> {
        if tracks.is_empty() {
            return self.empty(if self.scanning { "Looking for your music and videos..." } else { "Nothing here yet. Put music in your Music folder or videos in Videos, or use File > Open." });
        }
        let items: Vec<Item> = tracks.iter().map(|t| Item::from(*t)).collect();
        let mut col = column::with_capacity(tracks.len()).spacing(2);
        for (i, t) in tracks.iter().enumerate() {
            let sub = if t.artist.is_empty() { String::new() } else { format!("{} - {}", t.artist, t.album).trim_end_matches(" - ").to_string() };
            let content = row::with_capacity(3)
                .spacing(12)
                .align_y(Alignment::Center)
                .push(icon::from_name(if t.kind == Kind::Video { "video-x-generic-symbolic" } else { "audio-x-generic-symbolic" }).size(16))
                .push(text::body(t.title.clone()).width(Length::FillPortion(3)))
                .push(text::caption(sub).width(Length::FillPortion(2)));
            col = col.push(button::custom(content).width(Length::Fill).padding([6, 12]).class(lapin_ux_ui::nav_item(self.queue.get(self.index.unwrap_or(usize::MAX)).is_some_and(|q| q.target == items[i].target))).on_press(Message::PlayList(items.clone(), i)));
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
            let face = column::with_capacity(3)
                .spacing(6)
                .align_x(Alignment::Center)
                .push(container(icon::from_name(if by_album { "media-optical-symbolic" } else { "avatar-default-symbolic" }).size(64)).center_x(Length::Fixed(150.0)).padding([24, 0]).class(theme::Container::Card))
                .push(text::body(name).width(Length::Fixed(150.0)))
                .push(text::caption(sub).width(Length::Fixed(150.0)));
            cards.push(button::custom(face).padding(4).class(theme::Button::Text).on_press(Message::PlayList(items, 0)).into());
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
        col = col.push(button::suggested("Play the most played").on_press(Message::PlayList(most, 0)));
        if let Some(top) = played.first().map(|t| t.artist.clone()).filter(|a| !a.is_empty()) {
            let mix: Vec<Item> = self.tracks.iter().filter(|t| t.kind == Kind::Audio && t.artist == top).map(Item::from).collect();
            col = col.push(text::title3(format!("More from {top}")));
            col = col.push(button::standard("Play this mix").on_press(Message::PlayList(mix, 0)));
        }
        scrollable(col).height(Length::Fill).into()
    }

    fn queue_panel(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(self.queue.len() + 1).spacing(2).push(text::heading("Queue"));
        if self.queue.is_empty() {
            col = col.push(text::caption("Empty"));
        }
        for (i, it) in self.queue.iter().enumerate() {
            col = col.push(button::custom(text::body(it.title.clone())).width(Length::Fill).padding([6, 12]).class(lapin_ux_ui::nav_item(self.index == Some(i))).on_press(Message::PlayList(self.queue.clone(), i)));
        }
        container(scrollable(col)).width(Length::Fixed(260.0)).height(Length::Fill).padding(8).into()
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
            stations: library::load_stations(),
            section: Section::ListenNow,
            menu: None,
            search: String::new(),
            queue: Vec::new(),
            index: None,
            station: None,
            show_queue: false,
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
            plays: std::fs::read_to_string(plays_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
        };
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Media Player".into(), id),
            None => Task::none(),
        };
        let scan = Task::future(async {
            let found = tokio::task::spawn_blocking(run_scan).await.unwrap_or_default();
            cosmic::Action::App(Message::Scanned(found))
        });
        (app, Task::batch([title, scan]))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![lapin_ux_ui::title_bar("applications-multimedia", "Media Player")]
    }

    fn subscription(&self) -> Subscription<Message> {
        engine::subscription().map(Message::Engine)
    }

    fn on_app_exit(&mut self) -> Option<Message> {
        Some(Message::Shutdown)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Engine(e) => self.engine_event(e),
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
            Message::PlayList(items, i) => self.play_list(items, i),
            Message::PlayStation(i) => {
                if let Some(s) = self.stations.get(i).cloned() {
                    self.queue = vec![Item { title: s.name.clone(), artist: s.genre.clone(), target: s.url, kind: Kind::Audio }];
                    self.play_index(0);
                    self.station = Some(i);
                }
            }
            Message::PlayPause => {
                if self.index.is_none() && self.engine.is_some() && !self.queue.is_empty() {
                    self.play_index(0);
                } else if let Some(e) = &self.engine {
                    e.toggle("pause");
                }
            }
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
            Message::Scanned(t) => {
                self.tracks = t;
                self.scanning = false;
            }
            Message::AskInput(s) => {
                if let Some((_, v)) = &mut self.ask {
                    *v = s;
                }
            }
            Message::AskSubmit => self.submit_ask(),
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

    fn view(&self) -> Element<'_, Message> {
        let body: Element<'_, Message> = match self.section {
            Section::ListenNow => self.listen_now(),
            Section::Artists => self.shelf(false),
            Section::Albums => self.shelf(true),
            Section::Songs => self.track_rows(self.visible_tracks(Kind::Audio)),
            Section::Videos => self.track_rows(self.visible_tracks(Kind::Video)),
            Section::Radio => self.radio(),
            Section::MadeForYou => self.made_for_you(),
        };
        let mut main = row::with_capacity(3).push(self.sidebar()).push(container(body).width(Length::Fill).height(Length::Fill));
        if self.show_queue {
            main = main.push(self.queue_panel());
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
    let start: Vec<String> = std::env::args().skip(1).map(|a| a.strip_prefix("file://").map(str::to_string).unwrap_or(a)).collect();
    cosmic::app::run::<App>(
        Settings::default().size(Size::new(1100.0, 720.0)).size_limits(cosmic::iced::Limits::NONE.min_width(720.0).min_height(420.0)),
        start,
    )
}
