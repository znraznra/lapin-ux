// SPDX-License-Identifier: GPL-3.0-only
//! Photos (#74): an offline picture app. The front end is a Gallery (all photos
//! grouped by date, Favorites, Recents) with a toolbar for the source folder,
//! grid style, grouping, preview size and selection (#203); opening a photo, or
//! clicking an image in Files, shows it in a single-image viewer with a floating
//! toolbar and a filmstrip. Both follow the system theme. Nothing leaves the
//! computer: no cloud, no accounts, no network.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cosmic::app::{Settings, Task};
use cosmic::iced::keyboard::{Event as KeyEvent, Key, Modifiers, key::Named};
use cosmic::iced::widget::stack;
use cosmic::iced::{Alignment, Length, Size, Subscription};
use cosmic::iced::mouse::{Event as MouseEvent, ScrollDelta};
use cosmic::widget::image::Handle;
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text};
use cosmic::{ApplicationExt, Core, Element, theme};
use futures::StreamExt;

const APP_ID: &str = "dev.lapinux.Photos";
/// Longest side of a loaded thumbnail. One size serves every slider step (it is
/// scaled on screen, never decoded again), and the picture keeps its proportions.
const THUMB: u32 = 240;
/// Preview sizes of the slider: the height of a tile in pixels. One step beyond
/// the last is the reel (one large photo with the strip below it).
const SIZES: [u32; 6] = [100, 130, 160, 190, 220, 250];
const REEL: u8 = SIZES.len() as u8;
const DEFAULT_SIZE: u8 = 2;
/// How many tiles are loaded in one go. The grid is not virtualised yet, so this
/// keeps the memory use (about 170 KB per tile) predictable.
const MAX_TILES: usize = 400;
/// Recents shows what was saved or changed within this long.
const RECENT_SECS: u64 = 72 * 3600;
/// Pictures are shown at most this wide or tall. Bigger ones are scaled down once.
const MAX_SIDE: u32 = 3840;
const MAX_PHOTOS: usize = 20_000;
const MAX_DEPTH: usize = 5;
const EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "webp", "bmp", "gif", "jpe"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Gallery,
    Favorites,
    Recents,
    Screenshots,
    Gifs,
    Hidden,
}

impl Section {
    const ALL: [Section; 6] =
        [Section::Gallery, Section::Favorites, Section::Recents, Section::Screenshots, Section::Gifs, Section::Hidden];

    fn label(self) -> &'static str {
        match self {
            Section::Gallery => "Gallery",
            Section::Favorites => "Favorites",
            Section::Recents => "Recents",
            Section::Screenshots => "Screenshots",
            Section::Gifs => "Animated GIFs",
            Section::Hidden => "Hidden",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Gallery => "folder-pictures-symbolic",
            Section::Favorites => "starred-symbolic",
            Section::Recents => "document-open-recent-symbolic",
            Section::Screenshots => "applets-screenshooter-symbolic",
            Section::Gifs => "image-x-generic-symbolic",
            Section::Hidden => "view-conceal-symbolic",
        }
    }
}

/// The picture folders Photos looks in. The source drop-down picks one of them,
/// or all (`None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Folder {
    Pictures,
    Downloads,
    Desktop,
    Documents,
}

impl Folder {
    const ALL: [Folder; 4] = [Folder::Pictures, Folder::Downloads, Folder::Desktop, Folder::Documents];

    fn label(self) -> &'static str {
        match self {
            Folder::Pictures => "Pictures",
            Folder::Downloads => "Downloads",
            Folder::Desktop => "Desktop",
            Folder::Documents => "Documents",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Folder::Pictures => "pictures",
            Folder::Downloads => "downloads",
            Folder::Desktop => "desktop",
            Folder::Documents => "documents",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Folder::Pictures => "folder-pictures-symbolic",
            Folder::Downloads => "folder-download-symbolic",
            Folder::Desktop => "user-desktop-symbolic",
            Folder::Documents => "folder-documents-symbolic",
        }
    }

    fn dir(self) -> Option<PathBuf> {
        match self {
            Folder::Pictures => dirs::picture_dir(),
            Folder::Downloads => dirs::download_dir(),
            Folder::Desktop => dirs::desktop_dir(),
            Folder::Documents => dirs::document_dir(),
        }
    }
}

/// How the grid is divided by headings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Years,
    Months,
    Days,
    All,
}

impl Group {
    const ALL: [Group; 4] = [Group::Years, Group::Months, Group::Days, Group::All];

    fn label(self) -> &'static str {
        match self {
            Group::Years => "Years",
            Group::Months => "Months",
            Group::Days => "Days",
            Group::All => "All photos",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Group::Years => "years",
            Group::Months => "months",
            Group::Days => "days",
            Group::All => "all",
        }
    }

    fn from_key(k: &str) -> Option<Group> {
        Group::ALL.into_iter().find(|g| g.key() == k)
    }
}

/// Which toolbar pop-up menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Menu {
    Source,
    Group,
    More,
}

#[derive(Debug, Clone)]
struct Photo {
    path: PathBuf,
    modified: SystemTime,
}

#[derive(Debug, Clone)]
enum Message {
    Scanned(Scan),
    Section(Section),
    Source(Option<Folder>),
    ToggleSidebar,
    ToggleSquare,
    SetGroup(Group),
    Menu(Option<Menu>),
    /// The preview size slider (0 up to `REEL`).
    Size(u8),
    /// Ctrl and the mouse wheel change the size.
    Wheel(f32),
    Modifiers(Modifiers),
    /// A tile was pressed: opens it, or selects it in select mode or with Ctrl or Shift.
    Tile(PathBuf),
    SelectMode(bool),
    SelectAll,
    FavoriteTargets,
    HideTargets,
    AskDelete,
    CancelDelete,
    ConfirmDelete,
    /// The photos that reached the Recycle Bin, and how many could not.
    Trashed(Vec<PathBuf>, usize),
    /// A photo picked in the reel's filmstrip.
    Reel(PathBuf),
    Refresh,
    /// A thumbnail with its size, so tiles can keep the picture's proportions.
    Thumb(PathBuf, Option<(Handle, (u32, u32))>),
    Open(PathBuf),
    Full(PathBuf, Option<Handle>),
    /// A full-size decode that was skipped because the picture is no longer open.
    Ignore,
    Retry,
    Back,
    Prev,
    Next,
    ToggleFavorite(PathBuf),
}

/// Full-size decodes allowed at once, so paging quickly through a folder queues
/// work instead of decoding every picture in parallel.
static FULL_DECODES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
/// The picture the viewer wants now; queued decodes of any other picture are dropped.
static VIEWING: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

fn set_viewing(path: Option<PathBuf>) {
    *VIEWING.lock().unwrap_or_else(|e| e.into_inner()) = path;
}

fn still_viewing(path: &Path) -> bool {
    VIEWING.lock().unwrap_or_else(|e| e.into_inner()).as_deref() == Some(path)
}

const FAVORITES_UNSAVED: &str = "Your favorites could not be saved, so the change was not kept.";

struct App {
    core: Core,
    photos: Vec<Photo>,
    scanning: bool,
    /// What the last scan could not do, shown above the grid.
    notice: Option<String>,
    /// Some folders could not be read (apart from a scan that only stopped early).
    read_errors: bool,
    /// The picture whose full-size decode failed.
    failed: Option<PathBuf>,
    section: Section,
    /// The folder the lists are limited to (`None`: all of them).
    source: Option<Folder>,
    /// Preview size: an index into `SIZES`, or `REEL`.
    size: u8,
    /// Square tiles, or tiles that keep the picture's proportions.
    square: bool,
    group: Group,
    sidebar: bool,
    /// The toolbar pop-up that is open.
    menu: Option<Menu>,
    thumbs: HashMap<PathBuf, Handle>,
    /// Width and height of each loaded thumbnail.
    dims: HashMap<PathBuf, (u32, u32)>,
    /// Tiles asked for already (loaded, loading or failed), so none is decoded twice.
    asked: HashSet<PathBuf>,
    favorites: HashSet<PathBuf>,
    /// The last favorites change could not be saved, so the stars show what is on disk.
    favorites_unsaved: bool,
    /// Photos the person hid. They show only in the Hidden section.
    hidden: HashSet<PathBuf>,
    selected: HashSet<PathBuf>,
    select_mode: bool,
    /// The photo a Shift-click range starts from.
    anchor: Option<PathBuf>,
    modifiers: Modifiers,
    confirm_delete: bool,
    /// The big photo of the reel (the last slider step).
    reel: Option<PathBuf>,
    /// The photo shown in the viewer, as a path (the list can change under it).
    viewing: Option<PathBuf>,
    /// The full-size picture of the viewer or the reel.
    full: Option<(PathBuf, Handle)>,
    /// The picture to open at start, from the command line or Files.
    start: Option<PathBuf>,
}

fn favorites_file() -> Option<PathBuf> {
    store_file("favorites")
}

/// A saved list of photo paths (`<name>.json` in Photos' own config folder).
fn store_file(name: &str) -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_ID).join(format!("{name}.json")))
}

fn load_set(name: &str) -> HashSet<PathBuf> {
    let Some(file) = store_file(name) else { return HashSet::new() };
    read_favorites(&file).unwrap_or_else(|e| {
        tracing::warn!("could not read {name}: {e}");
        HashSet::new()
    })
}

/// The saved favorites. A missing file is an empty list; a file that cannot be
/// read or understood is an error, so it is never overwritten by accident.
fn read_favorites(file: &Path) -> std::io::Result<HashSet<PathBuf>> {
    match std::fs::read_to_string(file) {
        Ok(s) => serde_json::from_str::<Vec<PathBuf>>(&s)
            .map(|v| v.into_iter().collect())
            .map_err(std::io::Error::other),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(e) => Err(e),
    }
}

fn load_favorites() -> HashSet<PathBuf> {
    let Some(file) = favorites_file() else { return HashSet::new() };
    read_favorites(&file).unwrap_or_else(|e| {
        tracing::warn!("could not read favorites: {e}");
        HashSet::new()
    })
}

/// Writes the list next to the real file and renames it over it, so a full disk
/// or a crash halfway leaves the previous list in place.
fn write_favorites(file: &Path, set: &HashSet<PathBuf>) -> std::io::Result<()> {
    let mut list: Vec<&PathBuf> = set.iter().collect();
    list.sort();
    let bytes = serde_json::to_vec_pretty(&list).map_err(std::io::Error::other)?;
    let dir = file.parent().ok_or_else(|| std::io::Error::other("favorites file has no folder"))?;
    std::fs::create_dir_all(dir)?;
    let stem = file.file_name().and_then(|n| n.to_str()).unwrap_or("list.json");
    let tmp = dir.join(format!("{stem}.{}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, file));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Stars or unstars one photo and returns the list as saved.
fn set_favorite(path: &Path, on: bool) -> std::io::Result<HashSet<PathBuf>> {
    set_favorites(std::slice::from_ref(&path.to_path_buf()), on)
}

/// Stars or unstars photos and returns the list as saved. The list is read
/// again under a lock first, so two Photos windows never undo each other's change.
fn set_favorites(paths: &[PathBuf], on: bool) -> std::io::Result<HashSet<PathBuf>> {
    set_in("favorites", paths, on)
}

/// Adds photos to, or removes them from, the saved list `name`, under a lock.
fn set_in(name: &str, paths: &[PathBuf], on: bool) -> std::io::Result<HashSet<PathBuf>> {
    let file = store_file(name).ok_or_else(|| std::io::Error::other("no config folder"))?;
    let dir = file.parent().ok_or_else(|| std::io::Error::other("list file has no folder"))?;
    std::fs::create_dir_all(dir)?;
    let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join(format!("{name}.lock")))?;
    lock.lock()?; // released when `lock` is dropped
    let mut set = read_favorites(&file)?;
    for path in paths {
        if on {
            set.insert(path.clone());
        } else {
            set.remove(path);
        }
    }
    write_favorites(&file, &set)?;
    Ok(set)
}

/// Sends photos to the Recycle Bin one by one (`gio trash`), so one that cannot
/// be moved does not hold back the rest. Returns what moved and how many did not.
fn move_to_recycle_bin(paths: Vec<PathBuf>) -> (Vec<PathBuf>, usize) {
    let mut done = Vec::new();
    let mut failed = 0;
    for path in paths {
        match std::process::Command::new("gio").arg("trash").arg("--").arg(&path).status() {
            Ok(status) if status.success() => done.push(path),
            _ => failed += 1,
        }
    }
    (done, failed)
}

/// What the toolbar remembers between runs (`settings.json` next to the favorites).
struct Prefs {
    size: u8,
    square: bool,
    group: Group,
    sidebar: bool,
    source: Option<Folder>,
}

fn prefs_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_ID).join("settings.json"))
}

fn load_prefs() -> Prefs {
    let mut prefs = Prefs { size: DEFAULT_SIZE, square: true, group: Group::Days, sidebar: true, source: None };
    let Some(file) = prefs_file() else { return prefs };
    let Ok(text) = std::fs::read_to_string(&file) else { return prefs };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        tracing::warn!("could not understand {}", file.display());
        return prefs;
    };
    if let Some(n) = v.get("size").and_then(|x| x.as_u64()) {
        prefs.size = n.min(u64::from(REEL)) as u8;
    }
    if let Some(b) = v.get("square").and_then(|x| x.as_bool()) {
        prefs.square = b;
    }
    if let Some(g) = v.get("group").and_then(|x| x.as_str()).and_then(Group::from_key) {
        prefs.group = g;
    }
    if let Some(b) = v.get("sidebar").and_then(|x| x.as_bool()) {
        prefs.sidebar = b;
    }
    prefs.source = v.get("source").and_then(|x| x.as_str()).and_then(|s| Folder::ALL.into_iter().find(|f| f.key() == s));
    prefs
}

fn save_prefs(prefs: &Prefs) {
    let Some(file) = prefs_file() else { return };
    let v = serde_json::json!({
        "size": prefs.size,
        "square": prefs.square,
        "group": prefs.group.key(),
        "sidebar": prefs.sidebar,
        "source": prefs.source.map(|f| f.key()),
    });
    let result = (|| {
        let dir = file.parent().ok_or_else(|| std::io::Error::other("settings file has no folder"))?;
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("settings.json.{}.tmp", std::process::id()));
        let written = std::fs::write(&tmp, serde_json::to_vec_pretty(&v).map_err(std::io::Error::other)?)
            .and_then(|()| std::fs::rename(&tmp, &file));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written
    })();
    if let Err(e) = result {
        tracing::warn!("could not save Photos settings: {e}");
    }
}

fn is_photo(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// What a scan found, and what it could not look at.
#[derive(Debug, Clone, Default)]
struct Scan {
    photos: Vec<Photo>,
    /// Folders or entries that could not be read (permissions, a vanished drive).
    unreadable: usize,
    /// The folder walk stopped at `MAX_PHOTOS`. Starred pictures elsewhere are
    /// still added on top: they are the person's own choice and a short list.
    truncated: bool,
}

fn sort_photos(photos: &mut [Photo]) {
    photos.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
}

/// Every picture under the usual folders, newest first, plus the starred pictures
/// that live elsewhere. Hidden folders are skipped, and the walk is bounded so a
/// huge Documents tree cannot stall the app.
fn scan(favorites: HashSet<PathBuf>) -> Scan {
    let mut found: Vec<Photo> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut unreadable = 0usize;
    let mut truncated = false;
    'walk: for folder in Folder::ALL {
        let Some(root) = folder.dir() else { continue };
        let mut stack = vec![(root, 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                // A folder that is not there is not a problem; one we may not open is.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    tracing::warn!("could not read {}: {e}", dir.display());
                    unreadable += 1;
                    continue;
                }
            };
            for entry in entries {
                let Ok(entry) = entry else {
                    unreadable += 1;
                    continue;
                };
                let path = entry.path();
                let hidden = entry.file_name().to_string_lossy().starts_with('.');
                let Ok(kind) = entry.file_type() else {
                    unreadable += 1;
                    continue;
                };
                if kind.is_dir() {
                    if !hidden && depth < MAX_DEPTH {
                        stack.push((path, depth + 1));
                    }
                } else if !hidden && is_photo(&path) && seen.insert(path.clone()) {
                    let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                    found.push(Photo { path, modified });
                }
                if found.len() >= MAX_PHOTOS {
                    truncated = true;
                    break 'walk;
                }
            }
        }
    }
    // Starred pictures outside the folders above still belong in Favorites.
    for path in favorites {
        if seen.contains(&path) || !is_photo(&path) {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.is_file() {
                found.push(Photo { path, modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH) });
            }
        }
    }
    sort_photos(&mut found);
    Scan { photos: found, unreadable, truncated }
}

/// Opens a picture upright, as the camera meant it (EXIF rotation).
fn decode(path: &Path) -> Option<image::DynamicImage> {
    use image::ImageDecoder;
    let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut img = image::DynamicImage::from_decoder(decoder).ok()?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Some(img)
}

fn handle(img: image::DynamicImage) -> Handle {
    let rgba = img.to_rgba8();
    Handle::from_rgba(rgba.width(), rgba.height(), rgba.into_raw())
}

/// A small copy that keeps the picture's proportions, and its size.
fn load_thumb(path: PathBuf) -> Option<(Handle, (u32, u32))> {
    let img = decode(&path)?.thumbnail(THUMB, THUMB);
    let dims = (img.width(), img.height());
    Some((handle(img), dims))
}

fn load_full(path: PathBuf) -> Option<Handle> {
    let mut img = decode(&path)?;
    if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        img = img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    Some(handle(img))
}

fn date_heading(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    let day = DateTime::<Local>::from(t).date_naive();
    let today = Local::now().date_naive();
    if day == today {
        "Today".into()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".into()
    } else {
        day.format("%A, %-d %B %Y").to_string()
    }
}

fn group_heading(t: SystemTime, group: Group) -> String {
    use chrono::{DateTime, Local};
    let when = DateTime::<Local>::from(t);
    match group {
        Group::Years => when.format("%Y").to_string(),
        Group::Months => when.format("%B %Y").to_string(),
        Group::Days => date_heading(t),
        Group::All => String::new(),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Screen captures, by their folder or file name ("Screenshots", "Screenshot from ...").
fn is_screenshot(path: &Path) -> bool {
    let in_folder = path
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_lowercase().starts_with("screenshot"));
    let named = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| {
            let n = n.to_lowercase();
            n.starts_with("screenshot") || n.starts_with("screen shot") || n.starts_with("screen_shot")
        });
    in_folder || named
}

fn is_gif(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("gif"))
}

impl App {
    fn prefs(&self) -> Prefs {
        Prefs { size: self.size, square: self.square, group: self.group, sidebar: self.sidebar, source: self.source }
    }

    fn save(&self) {
        save_prefs(&self.prefs());
    }

    /// The photos the current section and source show, newest first.
    fn shown(&self) -> Vec<&Photo> {
        let root = self.source.and_then(Folder::dir);
        let now = SystemTime::now();
        self.photos
            .iter()
            .filter(|p| {
                let is_hidden = self.hidden.contains(&p.path);
                if is_hidden != (self.section == Section::Hidden) {
                    return false;
                }
                let in_section = match self.section {
                    Section::Gallery | Section::Hidden => true,
                    Section::Screenshots => is_screenshot(&p.path),
                    Section::Gifs => is_gif(&p.path),
                    Section::Favorites => self.favorites.contains(&p.path),
                    // A date in the future (a wrong camera clock) still counts as recent.
                    Section::Recents => now.duration_since(p.modified).map_or(true, |d| d.as_secs() <= RECENT_SECS),
                };
                in_section && root.as_ref().map_or(true, |r| p.path.starts_with(r))
            })
            .collect()
    }

    /// Loads the tiles of `paths` that were not asked for yet, four at a time.
    fn want_thumbs(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        let todo: Vec<PathBuf> = paths.into_iter().filter(|p| self.asked.insert(p.clone())).collect();
        if todo.is_empty() {
            return Task::none();
        }
        Task::stream(
            futures::stream::iter(todo)
                .map(|p| async move {
                    let key = p.clone();
                    let h = tokio::task::spawn_blocking(move || load_thumb(p)).await.ok().flatten();
                    cosmic::Action::App(Message::Thumb(key, h))
                })
                .buffer_unordered(4),
        )
    }

    fn want_section_thumbs(&mut self) -> Task<Message> {
        let paths: Vec<PathBuf> = self.shown().into_iter().take(MAX_TILES).map(|p| p.path.clone()).collect();
        self.want_thumbs(paths)
    }

    fn scan_task(&self) -> Task<Message> {
        let starred = self.favorites.clone();
        Task::future(async move {
            let found = tokio::task::spawn_blocking(move || scan(starred)).await.unwrap_or_default();
            cosmic::Action::App(Message::Scanned(found))
        })
    }

    /// Decodes `path` at full size for the viewer or the reel, and drops any
    /// queued decode of another picture.
    fn want_full(&mut self, path: PathBuf) -> Task<Message> {
        set_viewing(Some(path.clone()));
        Task::future(async move {
            let key = path.clone();
            let Ok(_permit) = FULL_DECODES.acquire().await else {
                return cosmic::Action::App(Message::Full(key, None));
            };
            // Paged past or closed while waiting for a turn: skip the decode.
            if !still_viewing(&key) {
                return cosmic::Action::App(Message::Ignore);
            }
            let h = tokio::task::spawn_blocking(move || load_full(path)).await.ok().flatten();
            cosmic::Action::App(Message::Full(key, h))
        })
    }

    fn open(&mut self, path: PathBuf) -> Task<Message> {
        self.viewing = Some(path.clone());
        self.full = None;
        self.failed = None;
        let near = self.neighbours(&path, 6);
        let thumbs = self.want_thumbs(near);
        let name = file_name(&path);
        let title = match self.core.main_window_id() {
            Some(id) => self.set_window_title(format!("{name} - Photos"), id),
            None => Task::none(),
        };
        let full = self.want_full(path);
        Task::batch([thumbs, full, title])
    }

    /// Keeps the reel's big photo valid: it follows the section, the source and
    /// deletions, and it only exists at the last slider step.
    fn sync_reel(&mut self) -> Task<Message> {
        if self.size != REEL {
            if self.reel.take().is_some() && self.viewing.is_none() {
                set_viewing(None);
                self.full = None;
                self.failed = None;
            }
            return Task::none();
        }
        let list: Vec<PathBuf> = self.shown().into_iter().map(|p| p.path.clone()).collect();
        let target = self.reel.clone().filter(|r| list.contains(r)).or_else(|| list.first().cloned());
        let Some(target) = target else {
            self.reel = None;
            if self.viewing.is_none() {
                self.full = None;
            }
            return Task::none();
        };
        if self.viewing.is_some() {
            self.reel = Some(target);
            return Task::none();
        }
        if self.reel.as_ref() == Some(&target) && self.full.as_ref().is_some_and(|(p, _)| *p == target) {
            return Task::none();
        }
        self.pick_reel(target)
    }

    fn pick_reel(&mut self, path: PathBuf) -> Task<Message> {
        self.reel = Some(path.clone());
        self.full = None;
        self.failed = None;
        self.want_full(path)
    }

    /// The thumbnails of the current list, and the reel after a change of list.
    fn refresh_view(&mut self) -> Task<Message> {
        let tiles = self.want_section_thumbs();
        let reel = self.sync_reel();
        Task::batch([tiles, reel])
    }

    fn clear_selection(&mut self) {
        self.selected.clear();
        self.select_mode = false;
        self.anchor = None;
        self.confirm_delete = false;
    }

    /// Up to `n` photos on each side of `path` in the current list, `path` included.
    fn neighbours(&self, path: &Path, n: usize) -> Vec<PathBuf> {
        let list = self.viewer_list();
        let Some(i) = list.iter().position(|p| p.path == path) else { return vec![path.to_path_buf()] };
        let from = i.saturating_sub(n);
        let to = (i + n + 1).min(list.len());
        list[from..to].iter().map(|p| p.path.clone()).collect()
    }

    /// What Left and Right walk through. A photo opened from outside the folders
    /// (from Files) is not in the list, so it is walked as a list of one.
    fn viewer_list(&self) -> Vec<&Photo> {
        self.shown()
    }

    /// Left and Right: the viewer's photo, or the reel's big photo in the gallery.
    fn step(&mut self, delta: isize) -> Task<Message> {
        let Some(current) = self.viewing.clone().or_else(|| self.reel.clone()) else { return Task::none() };
        let list = self.viewer_list();
        let Some(i) = list.iter().position(|p| p.path == current) else { return Task::none() };
        let j = i as isize + delta;
        if j < 0 || j as usize >= list.len() {
            return Task::none();
        }
        let next = list[j as usize].path.clone();
        if self.viewing.is_some() { self.open(next) } else { self.pick_reel(next) }
    }

    fn back(&mut self) -> Task<Message> {
        self.viewing = None;
        self.full = None;
        self.failed = None;
        let reel = match self.reel.clone() {
            Some(p) if self.size == REEL => self.want_full(p),
            _ => {
                set_viewing(None);
                Task::none()
            }
        };
        let title = match self.core.main_window_id() {
            Some(id) => self.set_window_title("Photos".into(), id),
            None => Task::none(),
        };
        Task::batch([title, reel])
    }
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Option<PathBuf>;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, start: Option<PathBuf>) -> (Self, Task<Message>) {
        let prefs = load_prefs();
        let mut app = App {
            core,
            photos: Vec::new(),
            scanning: true,
            notice: None,
            read_errors: false,
            favorites_unsaved: false,
            failed: None,
            section: Section::Gallery,
            source: prefs.source,
            size: prefs.size,
            square: prefs.square,
            group: prefs.group,
            sidebar: prefs.sidebar,
            menu: None,
            thumbs: HashMap::new(),
            dims: HashMap::new(),
            asked: HashSet::new(),
            favorites: load_favorites(),
            hidden: load_set("hidden"),
            selected: HashSet::new(),
            select_mode: false,
            anchor: None,
            modifiers: Modifiers::empty(),
            confirm_delete: false,
            reel: None,
            viewing: None,
            full: None,
            start,
        };
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Photos".into(), id),
            None => Task::none(),
        };
        let scan = app.scan_task();
        (app, Task::batch([title, scan]))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![lapin_ux_ui::title_bar("folder-pictures", "Photos")]
    }

    fn subscription(&self) -> Subscription<Message> {
        cosmic::iced::event::listen_with(|event, status, _| match event {
            cosmic::iced::Event::Keyboard(KeyEvent::ModifiersChanged(m)) => Some(Message::Modifiers(m)),
            cosmic::iced::Event::Mouse(MouseEvent::WheelScrolled { delta }) => {
                let y = match delta {
                    ScrollDelta::Lines { y, .. } | ScrollDelta::Pixels { y, .. } => y,
                };
                Some(Message::Wheel(y))
            }
            cosmic::iced::Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. })
                if status == cosmic::iced::event::Status::Ignored =>
            {
                match key {
                    Key::Named(Named::ArrowLeft) => Some(Message::Prev),
                    Key::Named(Named::ArrowRight) => Some(Message::Next),
                    Key::Named(Named::Escape) => Some(Message::Back),
                    Key::Named(Named::Delete) => Some(Message::AskDelete),
                    Key::Character(c) if modifiers.control() && c.as_str().eq_ignore_ascii_case("a") => {
                        Some(Message::SelectAll)
                    }
                    _ => None,
                }
            }
            _ => None,
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Scanned(found) => {
                self.read_errors = found.unreadable > 0;
                self.notice = match (found.unreadable, found.truncated) {
                    (0, false) => None,
                    (0, true) => Some(format!("Showing the first {MAX_PHOTOS} pictures found in your folders.")),
                    (n, false) => Some(format!("{n} folder(s) could not be read, so some pictures may be missing.")),
                    (n, true) => Some(format!(
                        "Showing the first {MAX_PHOTOS} pictures found in your folders, and {n} folder(s) could not be read."
                    )),
                };
                self.photos = found.photos;
                self.scanning = false;
                let live: HashSet<&PathBuf> = self.photos.iter().map(|p| &p.path).collect();
                self.selected.retain(|p| live.contains(p));
                // A picture handed over by Files opens straight in the viewer.
                let open = match self.start.take() {
                    Some(p) if is_photo(&p) => {
                        let p = p.canonicalize().unwrap_or(p);
                        if !self.photos.iter().any(|x| x.path == p) {
                            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                            self.photos.insert(0, Photo { path: p.clone(), modified });
                        }
                        self.open(p)
                    }
                    _ => Task::none(),
                };
                let view = self.refresh_view();
                return Task::batch([view, open]);
            }
            Message::Section(s) => {
                self.section = s;
                self.menu = None;
                self.clear_selection();
                return self.refresh_view();
            }
            Message::Source(folder) => {
                self.source = folder;
                self.menu = None;
                self.clear_selection();
                self.save();
                return self.refresh_view();
            }
            Message::ToggleSidebar => {
                self.sidebar = !self.sidebar;
                self.save();
            }
            Message::ToggleSquare => {
                self.square = !self.square;
                self.save();
            }
            Message::SetGroup(g) => {
                self.group = g;
                self.menu = None;
                self.save();
            }
            Message::Menu(menu) => self.menu = menu,
            Message::Size(n) => {
                self.size = n.min(REEL);
                self.save();
                return self.sync_reel();
            }
            Message::Wheel(y) => {
                if self.modifiers.control() && self.viewing.is_none() && y != 0.0 {
                    let next = if y > 0.0 { self.size.saturating_add(1).min(REEL) } else { self.size.saturating_sub(1) };
                    if next != self.size {
                        return self.update(Message::Size(next));
                    }
                }
            }
            Message::Modifiers(m) => self.modifiers = m,
            Message::Tile(path) => {
                if self.modifiers.shift() {
                    let list: Vec<PathBuf> = self.shown().into_iter().map(|p| p.path.clone()).collect();
                    let from = self.anchor.as_ref().and_then(|a| list.iter().position(|p| p == a));
                    let to = list.iter().position(|p| *p == path);
                    if let (Some(from), Some(to)) = (from, to) {
                        let (lo, hi) = (from.min(to), from.max(to));
                        self.selected.extend(list[lo..=hi].iter().cloned());
                    } else {
                        self.selected.insert(path.clone());
                        self.anchor = Some(path);
                    }
                    self.select_mode = true;
                } else if self.modifiers.control() || self.select_mode {
                    if !self.selected.remove(&path) {
                        self.selected.insert(path.clone());
                    }
                    self.anchor = Some(path);
                    self.select_mode = true;
                    if self.selected.is_empty() && !self.modifiers.control() {
                        self.select_mode = true;
                    }
                } else {
                    return self.open(path);
                }
                self.confirm_delete = false;
            }
            Message::SelectMode(on) => {
                if on {
                    self.select_mode = true;
                } else {
                    self.clear_selection();
                }
            }
            Message::SelectAll => {
                if self.viewing.is_none() {
                    self.selected = self.shown().into_iter().map(|p| p.path.clone()).collect();
                    self.select_mode = true;
                    self.confirm_delete = false;
                }
            }
            Message::FavoriteTargets => {
                let paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
                if paths.is_empty() {
                    return Task::none();
                }
                let on = !paths.iter().all(|p| self.favorites.contains(p));
                match set_favorites(&paths, on) {
                    Ok(saved) => {
                        self.favorites = saved;
                        self.favorites_unsaved = false;
                    }
                    Err(e) => {
                        tracing::warn!("could not save favorites: {e}");
                        self.favorites_unsaved = true;
                    }
                }
                return self.refresh_view();
            }
            Message::HideTargets => {
                let paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
                if paths.is_empty() {
                    return Task::none();
                }
                let hide = self.section != Section::Hidden;
                match set_in("hidden", &paths, hide) {
                    Ok(saved) => {
                        self.hidden = saved;
                        self.notice = None;
                        self.clear_selection();
                    }
                    Err(e) => {
                        tracing::warn!("could not save hidden photos: {e}");
                        self.notice = Some("Your hidden photos could not be saved, so the change was not kept.".into());
                    }
                }
                return self.refresh_view();
            }
            Message::AskDelete => {
                if self.viewing.is_none() && !self.selected.is_empty() {
                    self.confirm_delete = true;
                }
            }
            Message::CancelDelete => self.confirm_delete = false,
            Message::ConfirmDelete => {
                self.confirm_delete = false;
                let paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
                if paths.is_empty() {
                    return Task::none();
                }
                return Task::future(async move {
                    let (done, failed) = tokio::task::spawn_blocking(move || move_to_recycle_bin(paths))
                        .await
                        .unwrap_or_default();
                    cosmic::Action::App(Message::Trashed(done, failed))
                });
            }
            Message::Trashed(done, failed) => {
                let gone: HashSet<PathBuf> = done.into_iter().collect();
                self.photos.retain(|p| !gone.contains(&p.path));
                self.selected.retain(|p| !gone.contains(p));
                for p in &gone {
                    self.thumbs.remove(p);
                    self.dims.remove(p);
                    self.asked.remove(p);
                }
                if self.selected.is_empty() {
                    self.select_mode = false;
                    self.anchor = None;
                }
                self.notice = (failed > 0).then(|| format!("{failed} photo(s) could not be moved to the Recycle Bin."));
                return self.refresh_view();
            }
            Message::Reel(path) => {
                if self.size == REEL {
                    return self.pick_reel(path);
                }
            }
            Message::Refresh => {
                self.menu = None;
                self.scanning = true;
                return self.scan_task();
            }
            Message::Thumb(path, loaded) => {
                if let Some((h, dims)) = loaded {
                    self.dims.insert(path.clone(), dims);
                    self.thumbs.insert(path, h);
                }
            }
            Message::Open(path) => return self.open(path),
            Message::Full(path, handle) => {
                let wanted = match &self.viewing {
                    Some(v) => *v == path,
                    None => self.reel.as_ref() == Some(&path),
                };
                if wanted {
                    match handle {
                        Some(h) => {
                            self.failed = None;
                            self.full = Some((path, h));
                        }
                        None => {
                            self.full = None;
                            self.failed = Some(path);
                        }
                    }
                }
            }
            Message::Ignore => {}
            Message::Retry => {
                if let Some(path) = self.viewing.clone() {
                    return self.open(path);
                }
                if let Some(path) = self.reel.clone() {
                    return self.pick_reel(path);
                }
            }
            Message::Back => {
                if self.viewing.is_some() {
                    return self.back();
                }
                if self.menu.is_some() {
                    self.menu = None;
                } else if self.confirm_delete {
                    self.confirm_delete = false;
                } else if self.select_mode || !self.selected.is_empty() {
                    self.clear_selection();
                }
            }
            Message::Prev => return self.step(-1),
            Message::Next => return self.step(1),
            Message::ToggleFavorite(path) => {
                let on = !self.favorites.contains(&path);
                // A full disk or a read-only home must not end the program (#174).
                match set_favorite(&path, on) {
                    // The saved list already holds changes made in other windows.
                    Ok(saved) => {
                        self.favorites = saved;
                        self.favorites_unsaved = false;
                    }
                    // Keep the saved state: a star that only exists until the next restart would mislead.
                    Err(e) => {
                        tracing::warn!("could not save favorites: {e}");
                        self.favorites_unsaved = true;
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let body: Element<'_, Message> = match &self.viewing {
            Some(path) => self.viewer(path),
            None => self.gallery(),
        };
        container(body).class(theme::Container::Background).width(Length::Fill).height(Length::Fill).into()
    }
}

impl App {
    /// One entry of a toolbar drop-down.
    fn menu_item(label: &str, icon_name: Option<&str>, selected: bool, msg: Message) -> Element<'static, Message> {
        let mut content = row::with_capacity(2).spacing(12).align_y(Alignment::Center);
        if let Some(name) = icon_name {
            content = content.push(icon::from_name(name).size(16));
        }
        content = content.push(text::body(label.to_string()));
        button::custom(content)
            .width(Length::Fill)
            .padding([6, 12])
            .class(lapin_ux_ui::nav_item(selected))
            .on_press(msg)
            .into()
    }

    /// A drop-down button that opens `items` below itself.
    fn dropdown(
        &self,
        which: Menu,
        icon_name: &str,
        label: String,
        items: Vec<Element<'static, Message>>,
    ) -> Element<'_, Message> {
        let open = self.menu == Some(which);
        let content = row::with_capacity(3)
            .spacing(8)
            .align_y(Alignment::Center)
            .push(icon::from_name(icon_name).size(16))
            .push(text::body(label))
            .push(icon::from_name("pan-down-symbolic").size(12));
        let trigger = button::custom(content)
            .padding([6, 10])
            .class(lapin_ux_ui::nav_item(open))
            .on_press(Message::Menu(if open { None } else { Some(which) }));
        if !open {
            return trigger.into();
        }
        let list = container(column::with_children(items).spacing(2).width(Length::Fixed(200.0)))
            .padding(4)
            .class(theme::Container::Card);
        widget::popover(trigger)
            .popup(list)
            .position(widget::popover::Position::Bottom)
            .on_close(Message::Menu(None))
            .into()
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let mut bar = row::with_capacity(10).spacing(8).align_y(Alignment::Center).padding([8, 12]);
        bar = bar.push(
            button::icon(icon::from_name("sidebar-show-symbolic"))
                .name(if self.sidebar { "Hide the side bar" } else { "Show the side bar" })
                .selected(self.sidebar)
                .on_press(Message::ToggleSidebar),
        );

        let mut sources = vec![Self::menu_item("All folders", Some("folder-symbolic"), self.source.is_none(), Message::Source(None))];
        for f in Folder::ALL {
            if f.dir().is_some_and(|d| d.is_dir()) {
                sources.push(Self::menu_item(f.label(), Some(f.icon()), self.source == Some(f), Message::Source(Some(f))));
            }
        }
        bar = bar.push(self.dropdown(
            Menu::Source,
            "folder-symbolic",
            self.source.map_or("All folders", Folder::label).to_string(),
            sources,
        ));

        let groups = Group::ALL.into_iter().map(|g| Self::menu_item(g.label(), None, self.group == g, Message::SetGroup(g))).collect();
        bar = bar.push(self.dropdown(Menu::Group, "view-list-symbolic", self.group.label().to_string(), groups));

        bar = bar.push(
            button::icon(icon::from_name(if self.square { "view-grid-symbolic" } else { "view-continuous-symbolic" }))
                .name(if self.square { "Show photos in their own proportions" } else { "Show photos as squares" })
                .on_press(Message::ToggleSquare),
        );
        bar = bar.push(
            row::with_capacity(3)
                .spacing(6)
                .align_y(Alignment::Center)
                .push(icon::from_name("image-x-generic-symbolic").size(12))
                .push(widget::slider(0..=REEL, self.size, Message::Size).width(Length::Fixed(140.0)))
                .push(icon::from_name("image-x-generic-symbolic").size(20)),
        );
        bar = bar.push(widget::Space::new().width(Length::Fill));

        if self.select_mode || !self.selected.is_empty() {
            let n = self.selected.len();
            bar = bar.push(text::body(match n {
                0 => "Select photos".to_string(),
                1 => "1 selected".to_string(),
                n => format!("{n} selected"),
            }));
            bar = bar.push(button::standard("Select all").on_press(Message::SelectAll));
            let all_starred = n > 0 && self.selected.iter().all(|p| self.favorites.contains(p));
            let mut star = button::icon(icon::from_name(if all_starred { "starred-symbolic" } else { "non-starred-symbolic" }))
                .name(if all_starred { "Remove from favorites" } else { "Add to favorites" });
            let mut bin = button::icon(icon::from_name("user-trash-symbolic")).name("Move to the Recycle Bin");
            if n > 0 {
                star = star.on_press(Message::FavoriteTargets);
                bin = bin.on_press(Message::AskDelete);
            }
            let hiding = self.section != Section::Hidden;
            let mut hide = button::icon(icon::from_name(if hiding { "view-conceal-symbolic" } else { "view-reveal-symbolic" }))
                .name(if hiding { "Hide" } else { "Unhide" });
            if n > 0 {
                hide = hide.on_press(Message::HideTargets);
            }
            bar = bar.push(star).push(hide).push(bin);
            bar = bar.push(button::standard("Done").on_press(Message::SelectMode(false)));
        } else {
            bar = bar.push(button::standard("Select").on_press(Message::SelectMode(true)));
        }

        let more = vec![Self::menu_item("Look for new pictures", Some("view-refresh-symbolic"), false, Message::Refresh)];
        let more_open = self.menu == Some(Menu::More);
        let more_btn = button::icon(icon::from_name("view-more-symbolic"))
            .name("More")
            .selected(more_open)
            .on_press(Message::Menu(if more_open { None } else { Some(Menu::More) }));
        bar = bar.push(if more_open {
            let list = container(column::with_children(more).width(Length::Fixed(220.0))).padding(4).class(theme::Container::Card);
            Element::from(
                widget::popover(more_btn)
                    .popup(list)
                    .position(widget::popover::Position::Bottom)
                    .on_close(Message::Menu(None)),
            )
        } else {
            more_btn.into()
        });
        bar.into()
    }

    fn delete_bar(&self) -> Element<'_, Message> {
        let n = self.selected.len();
        let what = if n == 1 { "1 photo".to_string() } else { format!("{n} photos") };
        container(
            row::with_capacity(4)
                .spacing(12)
                .align_y(Alignment::Center)
                .push(text::body(format!("Move {what} to the Recycle Bin?")))
                .push(widget::Space::new().width(Length::Fill))
                .push(button::standard("Cancel").on_press(Message::CancelDelete))
                .push(button::destructive("Move").on_press(Message::ConfirmDelete)),
        )
        .padding([8, 16])
        .class(theme::Container::Card)
        .width(Length::Fill)
        .into()
    }

    fn gallery(&self) -> Element<'_, Message> {
        let mut nav = column::with_capacity(Section::ALL.len()).spacing(2);
        for s in Section::ALL {
            if s == Section::Screenshots {
                nav = nav.push(container(text::caption("Media types")).padding([8, 12, 2, 12]));
            }
            let content = row::with_capacity(2)
                .spacing(12)
                .align_y(Alignment::Center)
                .push(icon::from_name(s.icon()).size(16))
                .push(text::body(s.label()));
            nav = nav.push(
                button::custom(content)
                    .width(Length::Fill)
                    .padding([8, 12])
                    .class(lapin_ux_ui::nav_item(self.section == s))
                    .on_press(Message::Section(s)),
            );
        }
        let sidebar = container(nav).width(Length::Fixed(220.0)).padding(8);

        let shown = self.shown();
        let body: Element<'_, Message> = if self.scanning {
            container(text::body("Looking for your pictures...")).center(Length::Fill).into()
        } else if shown.is_empty() {
            let msg = match (self.section, self.read_errors) {
                (Section::Favorites, _) => "No favorites yet. Open a photo and press the star.",
                (Section::Recents, _) => "Nothing new in the last three days.",
                (Section::Screenshots, _) => "No screenshots found.",
                (Section::Gifs, _) => "No animated GIFs found.",
                (Section::Hidden, _) => "No hidden photos. Select photos and choose Hide to keep them out of sight.",
                (_, true) => "Some folders could not be read, so no pictures were found here.",
                _ => "No pictures here yet.",
            };
            let mut col = column::with_capacity(3).spacing(8).align_x(Alignment::Center);
            if self.favorites_unsaved {
                col = col.push(text::caption(FAVORITES_UNSAVED));
            }
            if let Some(notice) = &self.notice {
                col = col.push(text::caption(notice.as_str()));
            }
            container(col.push(text::body(msg))).center(Length::Fill).into()
        } else if self.size == REEL {
            self.reel_view(&shown)
        } else {
            let mut col = column::with_capacity(8).spacing(8).padding([8, 16, 16, 8]);
            if self.favorites_unsaved {
                col = col.push(text::caption(FAVORITES_UNSAVED));
            }
            if let Some(notice) = &self.notice {
                col = col.push(text::caption(notice.as_str()));
            }
            if shown.len() > MAX_TILES {
                col = col.push(text::caption(format!("Showing the newest {MAX_TILES} of {} pictures.", shown.len())));
            }
            let mut last: Option<String> = None;
            let mut tiles: Vec<Element<'_, Message>> = Vec::new();
            for photo in shown.into_iter().take(MAX_TILES) {
                let heading = group_heading(photo.modified, self.group);
                if last.as_ref() != Some(&heading) {
                    if !tiles.is_empty() {
                        col = col.push(widget::flex_row(std::mem::take(&mut tiles)).column_spacing(4).row_spacing(4));
                    }
                    if !heading.is_empty() {
                        col = col.push(container(text::heading(heading.clone())).padding([12, 0, 4, 0]));
                    }
                    last = Some(heading);
                }
                tiles.push(self.tile(&photo.path, false));
            }
            if !tiles.is_empty() {
                col = col.push(widget::flex_row(tiles).column_spacing(4).row_spacing(4));
            }
            scrollable(col).width(Length::Fill).height(Length::Fill).into()
        };

        let mut page = column::with_capacity(3).push(self.toolbar());
        if self.confirm_delete && !self.selected.is_empty() {
            page = page.push(self.delete_bar());
        }
        let mut main = row::with_capacity(2).height(Length::Fill);
        if self.sidebar {
            main = main.push(sidebar);
        }
        page = page.push(main.push(body));
        lapin_ux_ui::a11y::group("Gallery", page.height(Length::Fill))
    }

    /// The last slider step: one large photo, with the strip of the others below.
    fn reel_view(&self, shown: &[&Photo]) -> Element<'_, Message> {
        let big: Element<'_, Message> = match (&self.full, &self.reel) {
            (Some((p, h)), Some(r)) if p == r => button::custom(
                container(widget::image(h.clone()).content_fit(cosmic::iced::ContentFit::Contain))
                    .center(Length::Fill),
            )
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .class(theme::Button::Image)
            .name(file_name(r))
            .on_press(Message::Open(r.clone()))
            .into(),
            _ if self.failed.is_some() && self.failed == self.reel => container(
                column::with_capacity(2)
                    .spacing(8)
                    .align_x(Alignment::Center)
                    .push(text::body("This picture could not be opened."))
                    .push(button::standard("Try again").on_press(Message::Retry)),
            )
            .center(Length::Fill)
            .into(),
            _ => container(text::body("Opening...")).center(Length::Fill).into(),
        };
        let mut strip = row::with_capacity(shown.len().min(MAX_TILES)).spacing(4);
        for photo in shown.iter().take(MAX_TILES) {
            strip = strip.push(self.tile(&photo.path, true));
        }
        column::with_capacity(2)
            .spacing(8)
            .padding([8, 16, 16, 8])
            .push(big)
            .push(scrollable::horizontal(strip).width(Length::Fill))
            .into()
    }

    fn tile(&self, path: &Path, strip: bool) -> Element<'_, Message> {
        let h = if strip { 80.0 } else { SIZES[usize::from(self.size).min(SIZES.len() - 1)] as f32 };
        let w = if self.square || strip && !self.dims.contains_key(path) {
            h
        } else {
            let (dw, dh) = self.dims.get(path).copied().unwrap_or((4, 3));
            (h * dw as f32 / dh.max(1) as f32).clamp(h * 0.6, h * 2.0)
        };
        let (w, h) = (Length::Fixed(w), Length::Fixed(h));
        let picture: Element<'_, Message> = match self.thumbs.get(path) {
            Some(handle) => widget::image(handle.clone()).content_fit(cosmic::iced::ContentFit::Cover).width(w).height(h).into(),
            None => container(icon::from_name("image-x-generic-symbolic").size(32)).center(w).height(h).into(),
        };
        let mut tile = stack![picture];
        if self.favorites.contains(path) {
            tile = tile.push(
                container(icon::from_name("starred-symbolic").size(16))
                    .padding(6)
                    .align_x(Alignment::End)
                    .align_y(Alignment::End)
                    .width(w)
                    .height(h),
            );
        }
        let selected = self.selected.contains(path);
        if selected {
            tile = tile.push(
                container(icon::from_name("object-select-symbolic").size(20))
                    .padding(6)
                    .align_x(Alignment::Start)
                    .align_y(Alignment::Start)
                    .width(w)
                    .height(h),
            );
        }
        let msg = if strip && !self.select_mode && !self.modifiers.shift() && !self.modifiers.control() {
            Message::Reel(path.to_path_buf())
        } else {
            Message::Tile(path.to_path_buf())
        };
        button::custom(tile).padding(0).selected(selected).class(theme::Button::Image).name(file_name(path)).on_press(msg).into()
    }

    fn viewer(&self, path: &Path) -> Element<'_, Message> {
        let picture: Element<'_, Message> = match &self.full {
            Some((p, h)) if p == path => widget::image(h.clone())
                .content_fit(cosmic::iced::ContentFit::Contain)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            _ if self.failed.as_deref() == Some(path) => container(
                column::with_capacity(2)
                    .spacing(12)
                    .align_x(Alignment::Center)
                    .push(text::body("This picture could not be opened."))
                    .push(button::standard("Try again").on_press(Message::Retry)),
            )
            .center(Length::Fill)
            .into(),
            _ => container(text::body("Opening...")).center(Length::Fill).into(),
        };

        let list = self.viewer_list();
        let index = list.iter().position(|p| p.path == path);
        let favorite = self.favorites.contains(path);

        let mut bar = row::with_capacity(7)
            .spacing(4)
            .align_y(Alignment::Center)
            .push(button::icon(icon::from_name("go-previous-symbolic")).name("Back to the gallery").on_press(Message::Back))
            .push(container(text::body(file_name(path))).padding([0, 8]))
            .push(widget::Space::new().width(Length::Fill));
        if self.favorites_unsaved {
            bar = bar.push(text::caption(FAVORITES_UNSAVED));
        }
        let bar = bar
            .push(
                button::icon(icon::from_name("go-previous-symbolic"))
                    .name("Previous photo")
                    .on_press_maybe(index.filter(|i| *i > 0).map(|_| Message::Prev)),
            )
            .push(
                button::icon(icon::from_name("go-next-symbolic"))
                    .name("Next photo")
                    .on_press_maybe(index.filter(|i| i + 1 < list.len()).map(|_| Message::Next)),
            )
            .push(
                button::icon(icon::from_name(if favorite { "starred-symbolic" } else { "non-starred-symbolic" }))
                    .name(if favorite { "Remove from favorites" } else { "Add to favorites" })
                    .on_press(Message::ToggleFavorite(path.to_path_buf())),
            );
        let toolbar = container(container(bar).padding([4, 8]).class(theme::Container::Card))
            .padding(12)
            .width(Length::Fill)
            .align_y(Alignment::Start);

        let mut strip = row::with_capacity(13).spacing(6).align_y(Alignment::Center);
        for p in self.neighbours(path, 6) {
            let side = Length::Fixed(56.0);
            let thumb: Element<'_, Message> = match self.thumbs.get(&p) {
                Some(h) => widget::image(h.clone()).content_fit(cosmic::iced::ContentFit::Cover).width(side).height(side).into(),
                None => container(text::caption("")).width(side).height(side).into(),
            };
            let current = p == path;
            strip = strip.push(
                button::custom(thumb)
                    .padding(if current { 2 } else { 0 })
                    .class(theme::Button::Image)
                    .selected(current)
                    .name(file_name(&p))
                    .on_press(Message::Open(p)),
            );
        }
        let filmstrip = container(container(strip).padding(6).class(theme::Container::Card))
            .padding(12)
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::End)
            .height(Length::Fill);

        lapin_ux_ui::a11y::group("Photo viewer", stack![picture, toolbar, filmstrip])
    }
}

fn main() -> cosmic::iced::Result {
    lapin_ux_core::crash::install("lapin-ux-photos");
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .log_internal_errors(false) // #174: a full disk must not end the program
        .init();
    let start = std::env::args_os().nth(1).map(PathBuf::from);
    cosmic::app::run::<App>(
        Settings::default()
            .size(Size::new(1100.0, 720.0))
            .size_limits(cosmic::iced::Limits::NONE.min_width(640.0).min_height(420.0)),
        start,
    )
}
