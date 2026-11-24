// SPDX-License-Identifier: GPL-3.0-only
//! Photos (#74): an offline picture app. The front end is a Gallery (all photos
//! grouped by date, Favorites, Recents) with a toolbar for the source folder,
//! grid style, grouping, preview size and selection (#203); opening a photo, or
//! clicking an image in Files, shows it in a single-image viewer with a floating
//! toolbar and a filmstrip. Both follow the system theme. Nothing leaves the
//! computer: no cloud, no accounts, no network.

use std::collections::{BTreeMap, HashMap, HashSet};
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
    /// One album; which one is `App::album`.
    Album,
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
            Section::Album => "Album",
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
            Section::Album => "folder-symbolic",
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
    AddToAlbum,
}

#[derive(Debug, Clone)]
struct Photo {
    path: PathBuf,
    /// The canonical name of the picture (see `photo_key`). The saved list of
    /// hidden photos is keyed on it, so two paths to one file are one photo.
    key: PathBuf,
    modified: SystemTime,
}

#[derive(Debug, Clone)]
enum Message {
    Scanned(u64, Scan),
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
    /// Opens the folder chooser.
    AddFolder,
    FolderPicked(Option<PathBuf>),
    SourceExtra(PathBuf),
    RemoveFolder,
    /// Starts the slideshow, or stops it when it is running.
    Slideshow,
    SlideshowTick,
    /// Opens an album.
    Album(String),
    AddToAlbum(String),
    RemoveFromAlbum,
    DeleteAlbum,
    RenameAlbum,
    LockText(String),
    LockSubmit,
    LockCancel,
    /// Opens the box for a new Hidden password.
    SetLock,
    RemoveLock,
    /// Shows the box for naming a new album.
    NewAlbum,
    NewAlbumName(String),
    CreateAlbum,
    CancelNewAlbum,
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

/// How long each photo stays in the slideshow.
const SLIDE_TIME: std::time::Duration = std::time::Duration::from_secs(4);
const ALBUMS_UNSAVED: &str = "Your albums could not be saved, so the change was not kept.";
static LOCK_ID: std::sync::LazyLock<widget::Id> = std::sync::LazyLock::new(|| widget::Id::new("lapin-ux-photos-lock"));
static NEW_ALBUM_ID: std::sync::LazyLock<widget::Id> = std::sync::LazyLock::new(|| widget::Id::new("lapin-ux-photos-new-album"));
const FAVORITES_UNSAVED: &str = "Your favorites could not be saved, so the change was not kept.";

struct App {
    core: Core,
    photos: Vec<Photo>,
    scanning: bool,
    /// Counts the scans started; a result from an older one is ignored.
    scan_gen: u64,
    /// Photos moved to the Recycle Bin here, dropped from a scan that was already running.
    trashed: HashSet<PathBuf>,
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
    /// Where each photo was found, to its canonical name (`Photo.key`), so the
    /// saved lists can be matched without touching the disk for every tile.
    keys: HashMap<PathBuf, PathBuf>,
    /// The last favorites change could not be saved, so the stars show what is on disk.
    favorites_unsaved: bool,
    /// Photos the person hid. They show only in the Hidden section.
    hidden: HashSet<PathBuf>,
    /// The albums: name to the photos in it. Saved in `albums.json`.
    albums: Albums,
    /// The album shown when the section is Album.
    album: Option<String>,
    /// The name being typed for a new album, while the box is open.
    new_album: Option<String>,
    /// Set while the name box renames this album instead of making a new one.
    renaming: Option<String>,
    /// The Hidden password (salt and hash), if one is set.
    lock: Option<Lock>,
    /// The password file exists but could not be read or understood. Hidden
    /// then stays locked, since there is no way to tell what the password is.
    lock_unreadable: bool,
    /// The list of hidden photos could not be read. No pictures are shown at
    /// all, because the ones that were hidden cannot be told apart.
    hidden_unreadable: bool,
    /// Hidden was unlocked since the person last opened it.
    hidden_open: bool,
    /// Set while the box for a new password is open.
    setting_lock: bool,
    lock_text: String,
    lock_msg: Option<String>,
    albums_unsaved: bool,
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
    /// Folders the person added, saved in `folders.json`.
    extra: Vec<PathBuf>,
    /// One of `extra` picked in the source drop-down.
    source_extra: Option<PathBuf>,
    /// The slideshow is running: the viewer moves on by itself.
    slideshow: bool,
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

/// A saved list that guards something. Only a missing file means an empty
/// list; a file that cannot be read or understood is an error, so the caller
/// can keep what the list protects out of sight instead of showing it.
fn try_load_set(name: &str) -> std::io::Result<HashSet<PathBuf>> {
    let Some(file) = store_file(name) else { return Ok(HashSet::new()) };
    read_favorites(&file)
}

/// The one name a picture goes by in the saved lists, whatever path it was
/// reached through (symlink, `..`, a second folder that points at the same
/// place). A picture that cannot be resolved keeps the path it was found at.
fn photo_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The saved favorites. A missing file is an empty list; a file that cannot be
/// read or understood is an error, so it is never overwritten by accident.
fn read_favorites(file: &Path) -> std::io::Result<HashSet<PathBuf>> {
    match std::fs::read_to_string(file) {
        // Every entry is read under its canonical name, so a list saved under
        // another spelling of a path (symlink, `..`) still matches the picture.
        Ok(s) => serde_json::from_str::<Vec<PathBuf>>(&s)
            .map(|v| v.iter().map(|p| photo_key(p)).collect())
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
        let key = photo_key(path);
        if on {
            set.insert(key);
        } else {
            set.remove(&key);
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

/// True when the first bytes of the file are one of the supported picture
/// formats, whatever the file is called.
fn sniffs_as_photo(path: &Path) -> bool {
    use image::ImageFormat as F;
    let Ok(reader) = image::ImageReader::open(path).and_then(|r| r.with_guessed_format()) else { return false };
    matches!(reader.format(), Some(F::Jpeg | F::Png | F::WebP | F::Bmp | F::Gif))
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
fn scan(favorites: HashSet<PathBuf>, extra: Vec<PathBuf>) -> Scan {
    let mut found: Vec<Photo> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut unreadable = 0usize;
    let mut truncated = false;
    let roots: Vec<PathBuf> = Folder::ALL.into_iter().filter_map(Folder::dir).chain(extra).collect();
    'walk: for root in roots {
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
                } else if !hidden && is_photo(&path) {
                    let key = photo_key(&path);
                    if seen.insert(key.clone()) {
                        let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                        found.push(Photo { path, key, modified });
                    }
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
        // Saved favorites are canonical names, so a starred symlink such as
        // `a.jpg -> raw-file` arrives as its target. The extension is then no
        // evidence either way: the content decides.
        if !is_photo(&path) && !sniffs_as_photo(&path) {
            continue;
        }
        let key = photo_key(&path);
        if seen.contains(&key) {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.is_file() {
                seen.insert(key.clone());
                found.push(Photo { path, key, modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH) });
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

/// The Hidden password: a random salt and a slow hash of salt plus password,
/// both as hex. This keeps the section out of casual sight; the pictures are
/// not encrypted.
#[derive(Clone, PartialEq)]
struct Lock {
    salt: String,
    hash: String,
}

impl Lock {
    fn matches(&self, password: &str) -> bool {
        let Some(salt) = from_hex(&self.salt) else { return false };
        let want = to_hex(&stretch(&salt, password));
        // Compare every byte so the time does not say how much matched.
        want.len() == self.hash.len() && want.bytes().zip(self.hash.bytes()).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
    }
}

const LOCK_ROUNDS: u32 = 20_000;

const LOCK_UNREADABLE: &str = "Hidden stays locked because its password file could not be read. Fix or remove hidden-lock.json in ~/.config/dev.lapinux.Photos, then start Photos again.";
const LOCK_CHANGED: &str = "The password was changed in another window. Type the current one to open Hidden.";
const HIDDEN_UNREADABLE: &str = "The list of hidden photos could not be read, so no pictures are shown. Fix or remove hidden.json in ~/.config/dev.lapinux.Photos, then start Photos again.";

fn stretch(salt: &[u8], password: &str) -> [u8; 32] {
    let mut data = salt.to_vec();
    data.extend_from_slice(password.as_bytes());
    let mut h = sha256(&data);
    for _ in 0..LOCK_ROUNDS {
        let mut next = h.to_vec();
        next.extend_from_slice(salt);
        h = sha256(&next);
    }
    h
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn make_lock(password: &str) -> std::io::Result<Lock> {
    use std::io::Read;
    let mut salt = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut salt)?;
    Ok(Lock { salt: to_hex(&salt), hash: to_hex(&stretch(&salt, password)) })
}

/// The saved password. Only a missing file means "no password"; a file that
/// cannot be read or understood is an error, so Hidden stays locked rather
/// than opening for lack of a password it could not load.
fn load_lock() -> std::io::Result<Option<Lock>> {
    let Some(file) = store_file("hidden-lock") else { return Ok(None) };
    let s = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let bad = || std::io::Error::other("the password file is not understood");
    let v: serde_json::Value = serde_json::from_str(&s).map_err(|_| bad())?;
    let field = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string).ok_or_else(bad);
    Ok(Some(Lock { salt: field("salt")?, hash: field("hash")? }))
}

fn save_lock(lock: &Lock) -> std::io::Result<()> {
    let file = store_file("hidden-lock").ok_or_else(|| std::io::Error::other("no config folder"))?;
    let dir = file.parent().ok_or_else(|| std::io::Error::other("no folder"))?;
    std::fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec(&serde_json::json!({ "salt": lock.salt, "hash": lock.hash })).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!("hidden-lock.json.{}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &file));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn remove_lock() -> std::io::Result<()> {
    let Some(file) = store_file("hidden-lock") else { return Ok(()) };
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// SHA-256 (FIPS 180-4), small enough to keep here instead of a new crate.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, b) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

fn load_folders() -> Vec<PathBuf> {
    let Some(file) = store_file("folders") else { return Vec::new() };
    match read_folders(&file) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("could not read folders: {e}");
            Vec::new()
        }
    }
}

fn read_folders(file: &Path) -> std::io::Result<Vec<PathBuf>> {
    match std::fs::read_to_string(file) {
        Ok(s) => serde_json::from_str(&s).map_err(std::io::Error::other),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Changes the added folders and returns them as saved, with the same lock and
/// atomic swap as the albums.
fn edit_folders(change: impl FnOnce(&mut Vec<PathBuf>)) -> std::io::Result<Vec<PathBuf>> {
    let file = store_file("folders").ok_or_else(|| std::io::Error::other("no config folder"))?;
    let dir = file.parent().ok_or_else(|| std::io::Error::other("folders file has no folder"))?;
    std::fs::create_dir_all(dir)?;
    let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("folders.lock"))?;
    lock.lock()?;
    let mut list = read_folders(&file)?;
    change(&mut list);
    let bytes = serde_json::to_vec_pretty(&list).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!("folders.json.{}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &file));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| list)
}

/// Album name to the photos in it, in the order they were added.
type Albums = BTreeMap<String, Vec<PathBuf>>;

fn read_albums(file: &Path) -> std::io::Result<Albums> {
    match std::fs::read_to_string(file) {
        Ok(s) => serde_json::from_str::<Albums>(&s).map_err(std::io::Error::other).map(|albums| {
            // Photos are listed under their canonical names, once each.
            albums
                .into_iter()
                .map(|(name, list)| {
                    let mut seen = HashSet::new();
                    let list: Vec<PathBuf> = list.iter().map(|p| photo_key(p)).filter(|k| seen.insert(k.clone())).collect();
                    (name, list)
                })
                .collect()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Albums::new()),
        Err(e) => Err(e),
    }
}

fn load_albums() -> Albums {
    let Some(file) = store_file("albums") else { return Albums::new() };
    read_albums(&file).unwrap_or_else(|e| {
        tracing::warn!("could not read albums: {e}");
        Albums::new()
    })
}

/// Changes the albums and returns them as saved. The file is read again under a
/// lock first and replaced atomically, like the favorites.
fn edit_albums(change: impl FnOnce(&mut Albums)) -> std::io::Result<Albums> {
    let file = store_file("albums").ok_or_else(|| std::io::Error::other("no config folder"))?;
    let dir = file.parent().ok_or_else(|| std::io::Error::other("albums file has no folder"))?;
    std::fs::create_dir_all(dir)?;
    let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("albums.lock"))?;
    lock.lock()?;
    let mut albums = read_albums(&file)?;
    change(&mut albums);
    let bytes = serde_json::to_vec_pretty(&albums).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!("albums.json.{}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &file));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| albums)
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
        if self.hidden_locked() || self.hidden_unreadable {
            return Vec::new();
        }
        let root = self.source_extra.clone().or_else(|| self.source.and_then(Folder::dir));
        // A photo reached through a symlink belongs to the folder it really lives in.
        let root_key = root.as_deref().map(photo_key);
        let now = SystemTime::now();
        self.photos
            .iter()
            .filter(|p| {
                let is_hidden = self.is_hidden(p);
                if is_hidden != (self.section == Section::Hidden) {
                    return false;
                }
                let in_section = match self.section {
                    Section::Gallery | Section::Hidden => true,
                    Section::Screenshots => is_screenshot(&p.path),
                    Section::Gifs => is_gif(&p.path),
                    Section::Favorites => self.favorites.contains(&p.key),
                    Section::Album => self
                        .album
                        .as_ref()
                        .and_then(|a| self.albums.get(a))
                        .is_some_and(|list| list.contains(&p.key)),
                    // A date in the future (a wrong camera clock) still counts as recent.
                    Section::Recents => now.duration_since(p.modified).map_or(true, |d| d.as_secs() <= RECENT_SECS),
                };
                in_section
                    && root.as_ref().map_or(true, |r| {
                        p.path.starts_with(r) || root_key.as_ref().is_some_and(|k| p.key.starts_with(k))
                    })
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

    fn scan_task(&mut self) -> Task<Message> {
        self.scan_gen += 1;
        let generation = self.scan_gen;
        let starred = self.favorites.clone();
        let extra = self.extra.clone();
        Task::future(async move {
            let found = tokio::task::spawn_blocking(move || scan(starred, extra)).await.unwrap_or_default();
            cosmic::Action::App(Message::Scanned(generation, found))
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

    /// The Hidden section is showing and its password has not been entered yet.
    fn hidden_locked(&self) -> bool {
        self.section == Section::Hidden && (self.lock.is_some() || self.lock_unreadable) && !self.hidden_open
    }

    /// Whether the person hid this picture. Entries saved under another
    /// spelling of the path still count.
    fn is_hidden(&self, p: &Photo) -> bool {
        self.hidden.contains(&p.key) || self.hidden.contains(&p.path)
    }

    /// Reads the saved password again, because another Photos window may have
    /// set, changed or removed it since this one started. A change closes
    /// Hidden, so a window that was unlocked with the old password does not
    /// stay open under the new one.
    fn sync_lock(&mut self) {
        let saved = load_lock();
        let (lock, unreadable) = match &saved {
            Ok(l) => (l.clone(), false),
            Err(e) => {
                tracing::warn!("could not read the Hidden password: {e}");
                (None, true)
            }
        };
        if lock != self.lock || unreadable != self.lock_unreadable {
            self.lock = lock;
            self.lock_unreadable = unreadable;
            self.hidden_open = false;
            self.setting_lock = false;
            // Nothing picked in Hidden, and no half-asked delete, survives the
            // lock changing under it.
            self.clear_selection();
        }
    }

    /// The canonical name of the photo found at `path`.
    fn key_of(&self, path: &Path) -> PathBuf {
        self.keys.get(path).cloned().unwrap_or_else(|| photo_key(path))
    }

    fn is_favorite(&self, path: &Path) -> bool {
        self.favorites.contains(&self.key_of(path))
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
        self.slideshow = false;
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
        let saved_lock = load_lock();
        if let Err(e) = &saved_lock {
            tracing::warn!("could not read the Hidden password: {e}");
        }
        let hidden_set = try_load_set("hidden");
        let hidden_unreadable = hidden_set.is_err();
        if let Err(e) = &hidden_set {
            tracing::warn!("could not read the hidden photos: {e}");
        }
        let hidden_set = hidden_set.ok();
        let mut app = App {
            core,
            photos: Vec::new(),
            scanning: true,
            scan_gen: 0,
            trashed: HashSet::new(),
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
            keys: HashMap::new(),
            hidden: hidden_set.unwrap_or_default(),
            hidden_unreadable,
            albums: load_albums(),
            album: None,
            new_album: None,
            renaming: None,
            lock: saved_lock.as_ref().ok().cloned().flatten(),
            lock_unreadable: saved_lock.is_err(),
            hidden_open: false,
            setting_lock: false,
            lock_text: String::new(),
            lock_msg: None,
            albums_unsaved: false,
            selected: HashSet::new(),
            select_mode: false,
            anchor: None,
            modifiers: Modifiers::empty(),
            confirm_delete: false,
            reel: None,
            viewing: None,
            extra: load_folders(),
            source_extra: None,
            slideshow: false,
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
        let tick = if self.slideshow {
            cosmic::iced::time::every(SLIDE_TIME).map(|_| Message::SlideshowTick)
        } else {
            Subscription::none()
        };
        Subscription::batch([tick, cosmic::iced::event::listen_with(|event, status, _| match event {
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
        })])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Scanned(generation, _) if generation != self.scan_gen => {}
            Message::Scanned(_, mut found) => {
                let gone = std::mem::take(&mut self.trashed);
                found.photos.retain(|p| !gone.contains(&p.path));
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
                self.keys = self.photos.iter().map(|p| (p.path.clone(), p.key.clone())).collect();
                self.scanning = false;
                let live: HashSet<&PathBuf> = self.photos.iter().map(|p| &p.path).collect();
                self.selected.retain(|p| live.contains(p));
                // A picture handed over by Files opens straight in the viewer.
                let open = match self.start.take() {
                    Some(p) if is_photo(&p) => {
                        let p = p.canonicalize().unwrap_or(p);
                        let key = photo_key(&p);
                        if !self.photos.iter().any(|x| x.key == key) {
                            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                            self.photos.insert(0, Photo { path: p.clone(), key, modified });
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
                self.hidden_open = false;
                self.sync_lock();
                self.setting_lock = false;
                self.lock_text.clear();
                self.lock_msg = None;
                self.album = None;
                self.menu = None;
                self.clear_selection();
                return self.refresh_view();
            }
            Message::Source(folder) => {
                self.source = folder;
                self.source_extra = None;
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
                    // The tiles loaded for this section (at most `MAX_TILES`), never
                    // a photo the list does not show, so nothing unseen is batch-deleted.
                    self.selected = self.shown().into_iter().take(MAX_TILES).map(|p| p.path.clone()).collect();
                    self.select_mode = true;
                    self.confirm_delete = false;
                }
            }
            Message::FavoriteTargets => {
                let paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
                if paths.is_empty() {
                    return Task::none();
                }
                let on = !paths.iter().all(|p| self.is_favorite(p));
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
            Message::Album(name) => {
                self.section = Section::Album;
                self.album = Some(name);
                self.menu = None;
                self.clear_selection();
                return self.refresh_view();
            }
            Message::AddToAlbum(name) => {
                let paths: Vec<PathBuf> = self.selected.iter().map(|p| self.key_of(p)).collect();
                self.menu = None;
                if paths.is_empty() {
                    return Task::none();
                }
                match edit_albums(|a| {
                    let list = a.entry(name).or_default();
                    for p in paths {
                        if !list.contains(&p) {
                            list.push(p);
                        }
                    }
                }) {
                    Ok(saved) => {
                        self.albums = saved;
                        self.albums_unsaved = false;
                        self.clear_selection();
                    }
                    Err(e) => {
                        tracing::warn!("could not save albums: {e}");
                        self.albums_unsaved = true;
                    }
                }
            }
            Message::RemoveFromAlbum => {
                let (Some(name), false) = (self.album.clone(), self.selected.is_empty()) else { return Task::none() };
                let gone: HashSet<PathBuf> = self.selected.iter().map(|p| self.key_of(p)).collect();
                match edit_albums(|a| {
                    if let Some(list) = a.get_mut(&name) {
                        list.retain(|p| !gone.contains(p));
                    }
                }) {
                    Ok(saved) => {
                        self.albums = saved;
                        self.albums_unsaved = false;
                        self.clear_selection();
                    }
                    Err(e) => {
                        tracing::warn!("could not save albums: {e}");
                        self.albums_unsaved = true;
                    }
                }
                return self.refresh_view();
            }
            Message::DeleteAlbum => {
                let Some(name) = self.album.clone() else { return Task::none() };
                self.menu = None;
                match edit_albums(|a| {
                    a.remove(&name);
                }) {
                    Ok(saved) => {
                        self.albums = saved;
                        self.albums_unsaved = false;
                        self.section = Section::Gallery;
                        self.album = None;
                        self.clear_selection();
                    }
                    Err(e) => {
                        tracing::warn!("could not save albums: {e}");
                        self.albums_unsaved = true;
                    }
                }
                return self.refresh_view();
            }
            Message::NewAlbum => {
                self.menu = None;
                self.renaming = None;
                self.new_album = Some(String::new());
                return widget::text_input::focus(NEW_ALBUM_ID.clone());
            }
            Message::NewAlbumName(s) => self.new_album = Some(s),
            Message::CancelNewAlbum => {
                self.new_album = None;
                self.renaming = None;
            }
            Message::LockText(s) => self.lock_text = s,
            Message::LockCancel => {
                self.setting_lock = false;
                self.lock_text.clear();
                self.lock_msg = None;
            }
            Message::SetLock => {
                self.menu = None;
                // Another window may have locked Hidden since this one opened it.
                self.sync_lock();
                if self.hidden_locked() {
                    return self.refresh_view();
                }
                self.setting_lock = true;
                self.lock_text.clear();
                self.lock_msg = None;
                return widget::text_input::focus(LOCK_ID.clone());
            }
            Message::RemoveLock => {
                self.menu = None;
                self.sync_lock();
                if self.hidden_locked() {
                    return self.refresh_view();
                }
                match remove_lock() {
                    Ok(()) => {
                        self.lock = None;
                        self.lock_msg = None;
                    }
                    Err(e) => {
                        tracing::warn!("could not remove the Hidden password: {e}");
                        self.lock_msg = Some("The password could not be removed.".to_string());
                    }
                }
            }
            Message::LockSubmit => {
                let text = std::mem::take(&mut self.lock_text);
                if self.setting_lock {
                    if text.is_empty() {
                        return Task::none();
                    }
                    // Never write over a password another window set or changed
                    // meanwhile, or one that could not be read.
                    let before = (self.lock.clone(), self.lock_unreadable);
                    self.sync_lock();
                    if self.lock_unreadable || before != (self.lock.clone(), self.lock_unreadable) {
                        self.lock_msg = Some(if self.lock_unreadable { LOCK_UNREADABLE } else { LOCK_CHANGED }.to_string());
                        return self.refresh_view();
                    }
                    match make_lock(&text).and_then(|l| save_lock(&l).map(|()| l)) {
                        Ok(l) => {
                            self.lock = Some(l);
                            self.hidden_open = true;
                            self.setting_lock = false;
                            self.lock_msg = None;
                        }
                        Err(e) => {
                            tracing::warn!("could not save the Hidden password: {e}");
                            self.lock_msg = Some("The password could not be saved.".to_string());
                        }
                    }
                } else if {
                    // Check against the saved password, not the one this window
                    // loaded: it may have been changed in another window.
                    self.sync_lock();
                    self.lock_unreadable
                } {
                    self.lock_msg = Some(LOCK_UNREADABLE.to_string());
                } else if self.lock.is_none() {
                    // The password was removed in another window.
                    self.lock_msg = None;
                    return self.refresh_view();
                } else if self.lock.as_ref().is_some_and(|l| l.matches(&text)) {
                    self.hidden_open = true;
                    self.lock_msg = None;
                    return self.refresh_view();
                } else {
                    self.lock_msg = Some("That is not the password.".to_string());
                    return widget::text_input::focus(LOCK_ID.clone());
                }
            }
            Message::RenameAlbum => {
                let Some(name) = self.album.clone() else { return Task::none() };
                self.menu = None;
                self.new_album = Some(name.clone());
                self.renaming = Some(name);
                return widget::text_input::focus(NEW_ALBUM_ID.clone());
            }
            Message::CreateAlbum => {
                let name = self.new_album.as_deref().map(|s| s.trim().to_string()).unwrap_or_default();
                if name.is_empty() {
                    return Task::none();
                }
                if let Some(old) = self.renaming.clone() {
                    if old == name {
                        self.new_album = None;
                        self.renaming = None;
                        return Task::none();
                    }
                    let to = name.clone();
                    let mut conflict: Option<&str> = None;
                    match edit_albums(|a| {
                        if a.contains_key(&to) {
                            conflict = Some("An album with this name already exists.");
                        } else if let Some(list) = a.remove(&old) {
                            a.insert(to, list);
                        } else {
                            conflict = Some("This album no longer exists, so it was not renamed.");
                        }
                    }) {
                        Ok(saved) if conflict.is_some() => {
                            self.albums = saved;
                            self.notice = conflict.map(str::to_string);
                        }
                        Ok(saved) => {
                            self.albums = saved;
                            self.albums_unsaved = false;
                            self.new_album = None;
                            self.renaming = None;
                            self.notice = None;
                            self.album = Some(name);
                        }
                        Err(e) => {
                            tracing::warn!("could not save albums: {e}");
                            self.albums_unsaved = true;
                        }
                    }
                    return Task::none();
                }
                let paths: Vec<PathBuf> = self.selected.iter().map(|p| self.key_of(p)).collect();
                let had = !paths.is_empty();
                let open = name.clone();
                match edit_albums(|a| {
                    let list = a.entry(name).or_default();
                    for p in paths {
                        if !list.contains(&p) {
                            list.push(p);
                        }
                    }
                }) {
                    Ok(saved) => {
                        self.albums = saved;
                        self.albums_unsaved = false;
                        self.new_album = None;
                        if had {
                            self.clear_selection();
                        }
                        self.section = Section::Album;
                        self.album = Some(open);
                        return self.refresh_view();
                    }
                    Err(e) => {
                        tracing::warn!("could not save albums: {e}");
                        self.albums_unsaved = true;
                    }
                }
            }
            Message::AddFolder => {
                self.menu = None;
                return Task::future(async {
                    use cosmic::dialog::file_chooser;
                    let picked = match file_chooser::open::Dialog::new().title("Add a folder of pictures").open_folder().await {
                        Ok(r) => r.url().to_file_path().ok(),
                        Err(file_chooser::Error::Cancelled) => None,
                        Err(e) => {
                            tracing::warn!("folder chooser failed: {e:?}");
                            None
                        }
                    };
                    cosmic::Action::App(Message::FolderPicked(picked))
                });
            }
            Message::FolderPicked(None) => {}
            Message::FolderPicked(Some(dir)) => {
                if !dir.is_dir() {
                    return Task::none();
                }
                match edit_folders(|l| {
                    if !l.contains(&dir) {
                        l.push(dir.clone());
                    }
                }) {
                    Ok(saved) => {
                        self.extra = saved;
                        self.source = None;
                        self.source_extra = Some(dir);
                        self.clear_selection();
                        self.notice = None;
                        self.scanning = true;
                        return self.scan_task();
                    }
                    Err(e) => {
                        tracing::warn!("could not save folders: {e}");
                        self.notice = Some("The folder could not be added, so Photos kept showing what it was showing.".to_string());
                    }
                }
            }
            Message::SourceExtra(dir) => {
                self.source = None;
                self.source_extra = Some(dir);
                self.menu = None;
                self.clear_selection();
                return self.refresh_view();
            }
            Message::RemoveFolder => {
                let Some(dir) = self.source_extra.clone() else { return Task::none() };
                self.menu = None;
                match edit_folders(|l| l.retain(|d| d != &dir)) {
                    Ok(saved) => {
                        self.extra = saved;
                        self.source_extra = None;
                        self.clear_selection();
                        self.scanning = true;
                        return self.scan_task();
                    }
                    Err(e) => {
                        tracing::warn!("could not save folders: {e}");
                        self.notice = Some("The folder could not be removed.".to_string());
                    }
                }
            }
            Message::HideTargets => {
                // A locked Hidden section shows nothing, so nothing in it can be picked.
                if self.selected.is_empty() || self.hidden_locked() {
                    return Task::none();
                }
                let hide = self.section != Section::Hidden;
                // The list holds each picture under its canonical name. To unhide, the
                // spelling the picture was found at goes too, for entries saved before.
                let mut paths: Vec<PathBuf> = Vec::new();
                for p in self.photos.iter().filter(|p| self.selected.contains(&p.path)) {
                    paths.push(p.key.clone());
                    if !hide {
                        paths.push(p.path.clone());
                    }
                }
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
                if self.viewing.is_none() && !self.selected.is_empty() && !self.hidden_locked() {
                    self.confirm_delete = true;
                }
            }
            Message::CancelDelete => self.confirm_delete = false,
            Message::ConfirmDelete => {
                self.confirm_delete = false;
                if self.hidden_locked() {
                    return Task::none();
                }
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
                self.trashed.extend(gone.iter().cloned());
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
                } else if self.setting_lock {
                    self.setting_lock = false;
                    self.lock_text.clear();
                } else if self.new_album.is_some() {
                    self.new_album = None;
                    self.renaming = None;
                } else if self.confirm_delete {
                    self.confirm_delete = false;
                } else if self.select_mode || !self.selected.is_empty() {
                    self.clear_selection();
                }
            }
            Message::Slideshow => {
                self.menu = None;
                if self.slideshow {
                    self.slideshow = false;
                } else {
                    let list = self.viewer_list();
                    let start = self
                        .viewing
                        .clone()
                        .filter(|v| list.iter().any(|p| &p.path == v))
                        .or_else(|| self.selected.iter().find_map(|s| list.iter().find(|p| &p.path == s).map(|p| p.path.clone())))
                        .or_else(|| list.first().map(|p| p.path.clone()));
                    let Some(start) = start else { return Task::none() };
                    if self.viewing.as_ref().is_some_and(|v| v != &start) {
                        self.notice = Some("This picture is not in the gallery, so there is no slideshow to play from it.".to_string());
                        return Task::none();
                    }
                    self.slideshow = true;
                    return self.open(start);
                }
            }
            Message::SlideshowTick => {
                if !self.slideshow || self.viewing.is_none() {
                    return Task::none();
                }
                let list = self.viewer_list();
                let Some(current) = self.viewing.as_ref() else { return Task::none() };
                let Some(i) = list.iter().position(|p| &p.path == current) else {
                    self.slideshow = false;
                    return Task::none();
                };
                let next = list[(i + 1) % list.len()].path.clone();
                // A single photo has nothing to move on to.
                if &next != current {
                    return self.open(next);
                }
            }
            Message::Prev => return self.step(-1),
            Message::Next => return self.step(1),
            Message::ToggleFavorite(path) => {
                let on = !self.is_favorite(&path);
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
        for dir in &self.extra {
            let name = dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned());
            sources.push(Self::menu_item(&name, Some("folder-symbolic"), self.source_extra.as_ref() == Some(dir), Message::SourceExtra(dir.clone())));
        }
        sources.push(Self::menu_item("Add folder...", Some("folder-new-symbolic"), false, Message::AddFolder));
        for f in Folder::ALL {
            if f.dir().is_some_and(|d| d.is_dir()) {
                sources.push(Self::menu_item(f.label(), Some(f.icon()), self.source == Some(f), Message::Source(Some(f))));
            }
        }
        bar = bar.push(self.dropdown(
            Menu::Source,
            "folder-symbolic",
            match (&self.source_extra, self.source) {
                (Some(d), _) => d.file_name().map_or_else(|| d.display().to_string(), |n| n.to_string_lossy().into_owned()),
                (None, s) => s.map_or("All folders", Folder::label).to_string(),
            },
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
            let all_starred = n > 0 && self.selected.iter().all(|p| self.is_favorite(p));
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
            let mut album_items: Vec<Element<'static, Message>> = self
                .albums
                .keys()
                .map(|n| Self::menu_item(n, Some("folder-symbolic"), false, Message::AddToAlbum(n.clone())))
                .collect();
            album_items.push(Self::menu_item("New album...", Some("list-add-symbolic"), false, Message::NewAlbum));
            let add = self.dropdown(Menu::AddToAlbum, "folder-new-symbolic", "Add to album".to_string(), album_items);
            bar = bar.push(star).push(hide);
            if n > 0 {
                bar = bar.push(add);
            }
            if self.section == Section::Album && n > 0 {
                bar = bar.push(button::icon(icon::from_name("list-remove-symbolic")).name("Remove from this album").on_press(Message::RemoveFromAlbum));
            }
            bar = bar.push(bin);
            bar = bar.push(button::standard("Done").on_press(Message::SelectMode(false)));
        } else {
            bar = bar.push(button::standard("Select").on_press(Message::SelectMode(true)));
        }

        let mut more = vec![Self::menu_item("Slideshow", Some("media-playback-start-symbolic"), false, Message::Slideshow), Self::menu_item("Look for new pictures", Some("view-refresh-symbolic"), false, Message::Refresh)];
        if self.section == Section::Hidden && !self.hidden_locked() {
            if self.lock.is_some() {
                more.push(Self::menu_item("Change the password", Some("system-lock-screen-symbolic"), false, Message::SetLock));
                more.push(Self::menu_item("Remove the password", Some("list-remove-symbolic"), false, Message::RemoveLock));
            } else {
                more.push(Self::menu_item("Set a password", Some("system-lock-screen-symbolic"), false, Message::SetLock));
            }
        }
        if self.source_extra.is_some() {
            more.push(Self::menu_item("Stop looking in this folder", Some("list-remove-symbolic"), false, Message::RemoveFolder));
        }
        if self.section == Section::Album {
            more.push(Self::menu_item("Rename this album", Some("document-edit-symbolic"), false, Message::RenameAlbum));
            more.push(Self::menu_item("Delete this album", Some("user-trash-symbolic"), false, Message::DeleteAlbum));
        }
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

    /// Shown in place of the pictures while Hidden is locked.
    fn lock_screen(&self) -> Element<'_, Message> {
        let mut unlock = button::suggested("Unlock");
        if !self.lock_text.is_empty() {
            unlock = unlock.on_press(Message::LockSubmit);
        }
        if self.lock_unreadable {
            let col = column::with_capacity(2)
                .spacing(12)
                .align_x(Alignment::Center)
                .push(icon::from_name("system-lock-screen-symbolic").size(48))
                .push(text::body(LOCK_UNREADABLE));
            return container(col).center(Length::Fill).padding(24).into();
        }
        let mut col = column::with_capacity(4)
            .spacing(12)
            .align_x(Alignment::Center)
            .push(icon::from_name("system-lock-screen-symbolic").size(48))
            .push(text::body("Hidden is locked. Type its password to look."))
            .push(
                widget::secure_input("Password", &self.lock_text, None, true)
                    .id(LOCK_ID.clone())
                    .on_input(Message::LockText)
                    .on_submit(|_| Message::LockSubmit)
                    .width(Length::Fixed(260.0)),
            )
            .push(unlock);
        if let Some(msg) = &self.lock_msg {
            col = col.push(text::caption(msg.as_str()));
        }
        container(col).center(Length::Fill).into()
    }

    /// The box for a new Hidden password.
    fn set_lock_bar(&self) -> Element<'_, Message> {
        let mut save = button::suggested("Save");
        if !self.lock_text.is_empty() {
            save = save.on_press(Message::LockSubmit);
        }
        let mut col = column::with_capacity(2).spacing(4).push(
            row::with_capacity(5)
                .spacing(12)
                .align_y(Alignment::Center)
                .push(text::body("Choose a password for Hidden"))
                .push(
                    widget::secure_input("New password", &self.lock_text, None, true)
                        .id(LOCK_ID.clone())
                        .on_input(Message::LockText)
                        .on_submit(|_| Message::LockSubmit)
                        .width(Length::Fixed(260.0)),
                )
                .push(widget::Space::new().width(Length::Fill))
                .push(button::standard("Cancel").on_press(Message::LockCancel))
                .push(save),
        );
        col = col.push(text::caption("This keeps Hidden out of sight on this computer. It does not encrypt the pictures."));
        if let Some(msg) = &self.lock_msg {
            col = col.push(text::caption(msg.as_str()));
        }
        container(col).padding([8, 16]).class(theme::Container::Card).width(Length::Fill).into()
    }

    /// The box for naming a new album. The selected photos, if any, go into it.
    fn new_album_bar(&self) -> Element<'_, Message> {
        let name = self.new_album.as_deref().unwrap_or_default();
        let n = self.selected.len();
        let renaming = self.renaming.is_some();
        let exists = self.albums.contains_key(name.trim()) && self.renaming.as_deref() != Some(name.trim());
        let prompt = match n {
            _ if renaming => "Rename this album".to_string(),
            0 => "Name the new album".to_string(),
            1 => "Name the new album for 1 photo".to_string(),
            n => format!("Name the new album for {n} photos"),
        };
        let mut create = button::suggested(if renaming { "Rename" } else { "Create" });
        if !name.trim().is_empty() {
            create = create.on_press(Message::CreateAlbum);
        }
        let mut col = column::with_capacity(2).spacing(4).push(
            row::with_capacity(5)
                .spacing(12)
                .align_y(Alignment::Center)
                .push(text::body(prompt))
                .push(
                    widget::text_input("Album name", name)
                        .id(NEW_ALBUM_ID.clone())
                        .on_input(Message::NewAlbumName)
                        .on_submit(|_| Message::CreateAlbum)
                        .width(Length::Fixed(260.0)),
                )
                .push(widget::Space::new().width(Length::Fill))
                .push(button::standard("Cancel").on_press(Message::CancelNewAlbum))
                .push(create),
        );
        if exists && renaming {
            col = col.push(text::caption("An album with this name exists. Pick another name."));
        } else if exists {
            col = col.push(text::caption("An album with this name exists. The photos will be added to it."));
        }
        if self.albums_unsaved {
            col = col.push(text::caption(ALBUMS_UNSAVED));
        }
        container(col).padding([8, 16]).class(theme::Container::Card).width(Length::Fill).into()
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
        nav = nav.push(container(text::caption("Albums")).padding([8, 12, 2, 12]));
        for name in self.albums.keys() {
            let selected = self.section == Section::Album && self.album.as_deref() == Some(name.as_str());
            let content = row::with_capacity(2)
                .spacing(12)
                .align_y(Alignment::Center)
                .push(icon::from_name("folder-symbolic").size(16))
                .push(text::body(name.clone()));
            nav = nav.push(
                button::custom(content)
                    .width(Length::Fill)
                    .padding([8, 12])
                    .class(lapin_ux_ui::nav_item(selected))
                    .on_press(Message::Album(name.clone())),
            );
        }
        nav = nav.push(
            button::custom(
                row::with_capacity(2)
                    .spacing(12)
                    .align_y(Alignment::Center)
                    .push(icon::from_name("list-add-symbolic").size(16))
                    .push(text::body("New album")),
            )
            .width(Length::Fill)
            .padding([8, 12])
            .class(lapin_ux_ui::nav_item(false))
            .on_press(Message::NewAlbum),
        );
        let sidebar = container(scrollable(nav)).width(Length::Fixed(220.0)).padding(8);

        let shown = self.shown();
        let body: Element<'_, Message> = if self.hidden_unreadable {
            container(text::body(HIDDEN_UNREADABLE)).center(Length::Fill).padding(24).into()
        } else if self.hidden_locked() {
            self.lock_screen()
        } else if self.scanning {
            container(text::body("Looking for your pictures...")).center(Length::Fill).into()
        } else if shown.is_empty() {
            let msg = match (self.section, self.read_errors) {
                (Section::Favorites, _) => "No favorites yet. Open a photo and press the star.",
                (Section::Recents, _) => "Nothing new in the last three days.",
                (Section::Screenshots, _) => "No screenshots found.",
                (Section::Gifs, _) => "No animated GIFs found.",
                (Section::Album, _) => "This album is empty. Select photos in the gallery and choose Add to album.",
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
        if self.new_album.is_some() {
            page = page.push(self.new_album_bar());
        }
        if self.setting_lock {
            page = page.push(self.set_lock_bar());
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
        let mut col = column::with_capacity(3).spacing(8).padding([8, 16, 16, 8]);
        if let Some(notice) = &self.notice {
            col = col.push(text::caption(notice.as_str()));
        }
        col.push(big)
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
        if self.is_favorite(path) {
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
        let favorite = self.is_favorite(path);

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
                button::icon(icon::from_name(if self.slideshow { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" }))
                    .name(if self.slideshow { "Stop the slideshow" } else { "Start the slideshow" })
                    .on_press(Message::Slideshow),
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

#[cfg(test)]
mod lock_tests {
    use super::*;

    #[test]
    fn sha256_known_answers() {
        assert_eq!(to_hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(to_hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn password_matches_only_itself() {
        let lock = Lock { salt: to_hex(&[7u8; 16]), hash: to_hex(&stretch(&[7u8; 16], "kitten")) };
        assert!(lock.matches("kitten"));
        assert!(!lock.matches("Kitten"));
        assert!(!lock.matches(""));
    }

    #[test]
    fn saved_lists_match_by_canonical_name() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("lapin-photos-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real")).unwrap();
        std::fs::write(dir.join("real/a.png"), b"x").unwrap();
        symlink(dir.join("real"), dir.join("alias")).unwrap();
        let direct = dir.join("real/a.png");
        let aliased = dir.join("alias/a.png");
        assert_ne!(direct, aliased);
        assert_eq!(photo_key(&direct), photo_key(&aliased));

        let favorites = dir.join("favorites.json");
        std::fs::write(&favorites, serde_json::to_string(&vec![&aliased]).unwrap()).unwrap();
        assert!(read_favorites(&favorites).unwrap().contains(&photo_key(&direct)));

        let albums = dir.join("albums.json");
        std::fs::write(&albums, serde_json::to_string(&BTreeMap::from([("x", vec![&aliased, &direct])])).unwrap()).unwrap();
        assert_eq!(read_albums(&albums).unwrap()["x"], vec![photo_key(&direct)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn starred_symlink_to_extensionless_picture_is_kept() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("lapin-photos-sniff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        std::fs::write(dir.join("raw-file"), png).unwrap();
        std::fs::write(dir.join("notes"), b"just text").unwrap();
        symlink(dir.join("raw-file"), dir.join("a.jpg")).unwrap();
        // The saved list holds the canonical name, which has no extension.
        let key = photo_key(&dir.join("a.jpg"));
        assert!(!is_photo(&key));
        assert!(sniffs_as_photo(&key));
        assert!(!sniffs_as_photo(&dir.join("notes")));
        assert!(!sniffs_as_photo(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
