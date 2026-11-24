// SPDX-License-Identifier: GPL-3.0-only
//! Photos (#74): an offline picture app. The front end is a Gallery (all photos
//! grouped by date, Favorites, Recents) with a toolbar for the source folder,
//! grid style, grouping, preview size and selection (#203); opening a photo, or
//! clicking an image in Files, shows it in a single-image viewer with a floating
//! toolbar and a filmstrip. Both follow the system theme (#200). Nothing leaves the
//! computer: no cloud, no accounts, no network.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use cosmic::app::{Settings, Task};
use cosmic::iced::keyboard::{Event as KeyEvent, Key, Modifiers, key::Named};
use cosmic::iced::widget::stack;
use cosmic::iced::{Alignment, Length, Size, Subscription};
use cosmic::iced::mouse::{Event as MouseEvent, ScrollDelta};
use cosmic::widget::image::Handle;
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text};
use cosmic::{ApplicationExt, Core, Element, theme};
use futures::StreamExt;

mod vault;

const APP_ID: &str = "dev.lapinux.Photos";
/// Longest side of a loaded thumbnail. One size serves every slider step (it is
/// scaled on screen, never decoded again), and the picture keeps its proportions.
const THUMB: u32 = 240;
/// Preview sizes of the slider: the height of a tile in pixels. One step beyond
/// the last is the reel (one large photo with the strip below it).
const SIZES: [u32; 6] = [100, 130, 160, 190, 220, 250];
const REEL: u8 = SIZES.len() as u8;
const DEFAULT_SIZE: u8 = 2;
/// How many tiles the grid starts with. Scrolling near the end adds `TILE_STEP`
/// more, so memory (about 170 KB per tile) grows only as far as you look.
const MAX_TILES: usize = 200;
const TILE_STEP: usize = 200;
/// The grid keeps thumbnails for this many tiles either side of the visible
/// stretch (headings make the estimate rough), and re-checks every `KEEP_STEP`.
const KEEP_AROUND: usize = 150;
const KEEP_STEP: usize = 25;
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
    /// A finished scan, with the number of the request that started it.
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
    /// Opens the folder chooser.
    AddFolder,
    FolderPicked(Option<PathBuf>),
    /// The folder chooser itself failed (not the same as cancelling it).
    FolderChooserFailed,
    SourceExtra(PathBuf),
    RemoveFolder,
    /// Starts the slideshow, or stops it when it is running.
    Slideshow,
    SlideshowTick,
    /// Turns the open picture a quarter turn (true: clockwise) and saves it.
    Rotate(bool),
    Rotated(PathBuf, Result<Saved, String>),
    /// Shows or hides the edit buttons in the viewer.
    EditToggle,
    /// Applies an edit to the open picture and saves it as a copy.
    Edit(Edit),
    /// The picture that was edited, and where the result went.
    Edited(PathBuf, Result<Saved, String>),
    /// The grid was scrolled; near the bottom it shows more tiles.
    GridScrolled(f32),
    /// Steps the slideshow speed to the next choice.
    SlideSpeed,
    /// Fills the screen with the window, or goes back.
    Fullscreen,
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
    /// The vault opened (its pictures) or the password was wrong.
    VaultOpened(Result<Vec<vault::Entry>, String>),
    /// A vault job ended: set up, hide, unhide, change or remove.
    VaultDone(Result<VaultOutcome, String>),
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
    Thumb(PathBuf, u64, Option<(Handle, (u32, u32))>),
    Open(PathBuf),
    Full(PathBuf, u64, Option<Handle>),
    /// A full-size decode that was skipped because the picture is no longer open.
    Ignore,
    Retry,
    Back,
    /// The "Back to the gallery" button: always leaves the viewer, even with a menu open.
    BackToGallery,
    Prev,
    Next,
    ToggleFavorite(PathBuf),
    /// Opens or closes the "More actions" menu.
    ToggleMenu,
    StartRename,
    RenameInput(String),
    RenameCommit,
    AskDeleteOne,
    CancelOverlay,
    ConfirmDeleteOne,
    OpenLocation,
    /// Opening the folder failed to start.
    LocationFailed(u64, String),
    /// A rename or delete finished: the old path and what came of it.
    Renamed(PathBuf, Result<PathBuf, String>),
    Deleted(PathBuf, Result<(), String>),
}

/// What the viewer's menu is showing over the picture.
#[derive(Debug, Clone, PartialEq)]
enum Overlay {
    Menu,
    Rename(String),
    ConfirmDelete,
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

const ALBUMS_UNSAVED: &str = "Your albums could not be saved, so the change was not kept.";
static LOCK_ID: std::sync::LazyLock<widget::Id> = std::sync::LazyLock::new(|| widget::Id::new("lapin-ux-photos-lock"));
static NEW_ALBUM_ID: std::sync::LazyLock<widget::Id> = std::sync::LazyLock::new(|| widget::Id::new("lapin-ux-photos-new-album"));
const FAVORITES_UNSAVED: &str = "Your favorites could not be saved, so the change was not kept.";

struct App {
    core: Core,
    photos: Vec<Photo>,
    scanning: bool,
    /// What the last scan could not do, shown above the grid.
    notice: Option<String>,
    /// The viewer's edit buttons are showing.
    edit_open: bool,
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
    /// Pictures with a turn still being written. A second turn waits for it.
    turning: HashSet<PathBuf>,
    /// Bumped whenever a picture's cached tile or full decode is thrown away
    /// (its file changed on disk). Decodes carry the number they started under.
    epoch: u64,
    /// The epoch at which each picture's caches were last thrown away: a decode
    /// that started before it shows the old file and is dropped.
    stale_since: HashMap<PathBuf, u64>,
    /// Folder scans that have not reported yet.
    scans_running: u32,
    /// Pictures moved to the Recycle Bin while a scan was running; that scan may
    /// still list them.
    trashed_meanwhile: HashSet<PathBuf>,
    /// Which request each tile is waiting for. A result for a name that was
    /// renamed, deleted or asked again since is stale and dropped.
    thumb_epoch: HashMap<PathBuf, u64>,
    next_epoch: u64,
    /// Counts viewer visits, so a late "could not open the folder" from an
    /// earlier photo never shows on the current one.
    visit: u64,
    /// A rename or delete is running; the other is refused until it has finished.
    file_op: bool,
    /// Bumped whenever the list of added folders changes: a scan that began
    /// with an older list must not replace the gallery.
    folders_epoch: u32,
    favorites: HashSet<PathBuf>,
    /// Where each photo was found, to its canonical name (`Photo.key`), so the
    /// saved lists can be matched without touching the disk for every tile.
    keys: HashMap<PathBuf, PathBuf>,
    /// The last favorites change could not be saved, so the stars show what is on disk.
    favorites_unsaved: bool,
    /// Photos the person hid. They show only in the Hidden section.
    hidden: HashSet<PathBuf>,
    /// Copies an edit made in this session. Only these are edited in place.
    made: HashMap<PathBuf, FileId>,
    /// The albums: name to the photos in it. Saved in `albums.json`.
    albums: Albums,
    /// The album shown when the section is Album.
    album: Option<String>,
    /// The name being typed for a new album, while the box is open.
    new_album: Option<String>,
    /// Set while the name box renames this album instead of making a new one.
    renaming: Option<String>,
    /// A Hidden vault exists, so a password is set.
    vault_set: bool,
    /// The pictures inside the vault while Hidden is open, newest first.
    vault_photos: Vec<Photo>,
    /// A vault job is running (they take a moment: the password is slow on purpose).
    vault_busy: bool,
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
    /// How many tiles the grid shows right now (grows as you scroll down).
    tile_limit: usize,
    /// Which stretch of the grid was last kept in memory (see `GridScrolled`).
    grid_bucket: usize,
    /// Seconds each photo stays in the slideshow (saved).
    slide_secs: u8,
    /// The window fills the screen.
    fullscreen: bool,
    /// The full-size picture of the viewer or the reel.
    full: Option<(PathBuf, Handle)>,
    /// The picture to open at start, from the command line or Files.
    start: Option<PathBuf>,
    overlay: Option<Overlay>,
    /// What the last rename or delete could not do.
    action_error: Option<String>,
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
fn store_path_text(name: &str) -> String {
    store_file(name).map(|p| p.display().to_string()).unwrap_or_else(|| format!("{name}.json in Photos' config folder"))
}

fn hidden_unreadable_msg() -> String {
    format!("The list of hidden photos could not be read, so no pictures are shown. Fix or remove {}, then start Photos again.", store_path_text("hidden"))
}

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
///
/// The list is synced before the rename. With `strict` the folder is synced
/// afterwards too and a failure there is an error, for callers (a hidden copy)
/// that must not rely on a list a power cut could take back.
fn write_favorites(file: &Path, set: &HashSet<PathBuf>, strict: bool) -> std::io::Result<()> {
    let mut list: Vec<&PathBuf> = set.iter().collect();
    list.sort();
    let bytes = serde_json::to_vec_pretty(&list).map_err(std::io::Error::other)?;
    let dir = file.parent().ok_or_else(|| std::io::Error::other("favorites file has no folder"))?;
    std::fs::create_dir_all(dir)?;
    let stem = file.file_name().and_then(|n| n.to_str()).unwrap_or("list.json");
    let tmp = dir.join(format!("{stem}.{}.tmp", std::process::id()));
    let result = (|| {
        use std::io::Write;
        let mut out = std::fs::File::create(&tmp)?;
        out.write_all(&bytes)?;
        out.sync_all()?;
        drop(out);
        std::fs::rename(&tmp, file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return result;
    }
    if !sync_parent(file) && strict {
        return Err(std::io::Error::other("the list could not be synced to disk"));
    }
    Ok(())
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
    set_in_with(name, paths, on, false)
}

/// `set_in`, optionally requiring the change to be on disk (see `write_favorites`).
fn set_in_with(name: &str, paths: &[PathBuf], on: bool, strict: bool) -> std::io::Result<HashSet<PathBuf>> {
    edit_in(name, strict, |set| {
        for path in paths {
            let key = photo_key(path);
            if on {
                set.insert(key);
            } else {
                set.remove(&key);
            }
        }
    })
}

/// Moves a star to a new name in one locked read-modify-write, so a failure
/// never leaves both names starred or neither.
fn move_favorite(old: &Path, new: &Path) -> std::io::Result<HashSet<PathBuf>> {
    edit_in("favorites", false, |set| {
        let (old_key, new_key) = (photo_key(old), photo_key(new));
        if set.remove(&old_key) || set.remove(old) {
            set.insert(new_key);
        }
    })
}

fn edit_in(name: &str, strict: bool, change: impl FnOnce(&mut HashSet<PathBuf>)) -> std::io::Result<HashSet<PathBuf>> {
    let file = store_file(name).ok_or_else(|| std::io::Error::other("no config folder"))?;
    let dir = file.parent().ok_or_else(|| std::io::Error::other("list file has no folder"))?;
    std::fs::create_dir_all(dir)?;
    let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join(format!("{name}.lock")))?;
    lock.lock()?; // released when `lock` is dropped
    let mut set = read_favorites(&file)?;
    change(&mut set);
    write_favorites(&file, &set, strict)?;
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
    /// Seconds each photo stays in the slideshow.
    slide_secs: u8,
}

/// The slideshow speeds the menu steps through.
const SLIDE_CHOICES: [u8; 4] = [2, 4, 8, 15];

fn prefs_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_ID).join("settings.json"))
}

fn load_prefs() -> Prefs {
    let mut prefs = Prefs { size: DEFAULT_SIZE, square: true, group: Group::Days, sidebar: true, source: None, slide_secs: 4 };
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
    if let Some(n) = v.get("slide_secs").and_then(|x| x.as_u64()) {
        prefs.slide_secs = n.clamp(1, 60) as u8;
    }
    prefs.source = v.get("source").and_then(|x| x.as_str()).and_then(|s| Folder::ALL.into_iter().find(|f| f.key() == s));
    prefs
}

/// Saves one toolbar preference. The file is read again under a lock and only
/// `key` is changed, so a second window never writes back an older value of
/// something the first one just changed.
fn save_pref(key: &str, value: serde_json::Value) {
    let Some(file) = prefs_file() else { return };
    let result = (|| {
        let dir = file.parent().ok_or_else(|| std::io::Error::other("settings file has no folder"))?;
        std::fs::create_dir_all(dir)?;
        let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("settings.lock"))?;
        lock.lock()?;
        let mut v = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .filter(|v| v.is_object())
            .unwrap_or_else(|| serde_json::json!({}));
        v[key] = value;
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

// ------------------------------------------------------------ Hidden vault

/// What a vault job changed.
#[derive(Debug, Clone, Default)]
struct VaultOutcome {
    /// Everything in the vault afterwards.
    entries: Vec<vault::Entry>,
    /// Pictures that left the library (they went into the vault).
    moved: Vec<PathBuf>,
    /// Some pictures came out of the vault, so the library is scanned again.
    rescan: bool,
    /// The vault now exists (`true`) or is gone (`false`); `None` when unchanged.
    set: Option<bool>,
    /// The plain Hidden list, when the job changed it.
    hidden: Option<HashSet<PathBuf>>,
    note: Option<String>,
}

/// Runs a vault job off the UI thread (the password stretching takes a moment).
fn vault_task(job: impl FnOnce() -> Result<VaultOutcome, String> + Send + 'static) -> Task<Message> {
    Task::future(async move {
        let r = tokio::task::spawn_blocking(job)
            .await
            .unwrap_or_else(|_| Err("The vault job did not finish.".to_string()));
        cosmic::Action::App(Message::VaultDone(r))
    })
}

fn vault_folder() -> Result<&'static Path, String> {
    vault::default_dir().map(|d| d.as_path()).ok_or_else(|| "Photos has no folder to keep the vault in.".to_string())
}

fn vault_open_key() -> Result<vault::Key, String> {
    vault::key().ok_or_else(|| "Open Hidden and type the password first.".to_string())
}

fn some_failed(failed: usize, what: &str) -> Option<String> {
    (failed > 0).then(|| format!("{failed} picture(s) could not be {what}."))
}

fn vault_unlock(password: &str) -> Result<Vec<vault::Entry>, String> {
    let dir = vault_folder()?;
    let key = vault::unlock(dir, password).map_err(|e| e.to_string())?;
    let index = vault::load_index(dir, &key).map_err(|e| e.to_string())?;
    vault::set_key(Some(key));
    Ok(index)
}

/// Makes the vault and moves the pictures already hidden (a plain list) into it.
fn vault_setup(password: &str, plain: Vec<PathBuf>) -> Result<VaultOutcome, String> {
    let dir = vault_folder()?;
    let key = vault::create(dir, password, vault::Cost::DEFAULT).map_err(|e| e.to_string())?;
    vault::set_key(Some(key));
    let mut out = VaultOutcome { set: Some(true), ..Default::default() };
    let mut failed = 0;
    for p in plain.iter().filter(|p| p.is_file()) {
        match vault::add(dir, &key, &mut out.entries, p) {
            Ok(_) => out.moved.push(p.clone()),
            Err(e) => {
                tracing::warn!("could not move {} into the vault: {e}", p.display());
                failed += 1;
            }
        }
    }
    if !out.moved.is_empty() {
        match set_in("hidden", &out.moved, false) {
            Ok(h) => out.hidden = Some(h),
            Err(e) => tracing::warn!("could not update the hidden list: {e}"),
        }
    }
    // The old password file (it only ever hid the list) is no longer needed.
    if let Some(f) = store_file("hidden-lock") {
        let _ = std::fs::remove_file(f);
    }
    out.note = some_failed(failed, "moved into the vault, so they stay hidden as before");
    Ok(out)
}

fn vault_hide(paths: &[PathBuf]) -> Result<VaultOutcome, String> {
    let dir = vault_folder()?;
    let key = vault_open_key()?;
    let mut out = VaultOutcome { entries: vault::load_index(dir, &key).map_err(|e| e.to_string())?, ..Default::default() };
    let mut failed = 0;
    for p in paths {
        match vault::add(dir, &key, &mut out.entries, p) {
            Ok(_) => out.moved.push(p.clone()),
            Err(e) => {
                tracing::warn!("could not move {} into the vault: {e}", p.display());
                failed += 1;
            }
        }
    }
    out.note = some_failed(failed, "moved into the vault");
    Ok(out)
}

fn vault_unhide(paths: &[PathBuf]) -> Result<VaultOutcome, String> {
    let dir = vault_folder()?;
    let key = vault_open_key()?;
    let mut out = VaultOutcome { entries: vault::load_index(dir, &key).map_err(|e| e.to_string())?, rescan: true, ..Default::default() };
    let mut failed = 0;
    for p in paths {
        let Some(id) = p.file_stem().and_then(|s| s.to_str()) else { continue };
        if let Err(e) = vault::restore(dir, &key, &mut out.entries, id) {
            tracing::warn!("could not take a picture out of the vault: {e}");
            failed += 1;
        }
    }
    out.note = some_failed(failed, "taken out of the vault");
    Ok(out)
}

fn vault_change(new_password: &str) -> Result<VaultOutcome, String> {
    let dir = vault_folder()?;
    let key = vault_open_key()?;
    vault::change_password(dir, &key, new_password, vault::Cost::DEFAULT).map_err(|e| e.to_string())?;
    Ok(VaultOutcome {
        entries: vault::load_index(dir, &key).map_err(|e| e.to_string())?,
        note: Some("The password was changed.".to_string()),
        ..Default::default()
    })
}

/// Takes every picture out of the vault, puts them back on the plain Hidden list
/// and deletes the vault. If any picture cannot come out the vault stays.
fn vault_remove() -> Result<VaultOutcome, String> {
    let dir = vault_folder()?;
    let key = vault_open_key()?;
    let mut out = VaultOutcome { entries: vault::load_index(dir, &key).map_err(|e| e.to_string())?, rescan: true, ..Default::default() };
    let ids: Vec<String> = out.entries.iter().map(|e| e.id.clone()).collect();
    let (mut back, mut failed) = (Vec::new(), 0);
    for id in ids {
        match vault::restore(dir, &key, &mut out.entries, &id) {
            Ok(p) => back.push(p),
            Err(e) => {
                tracing::warn!("could not take a picture out of the vault: {e}");
                failed += 1;
            }
        }
    }
    if !back.is_empty() {
        match set_in("hidden", &back, true) {
            Ok(h) => out.hidden = Some(h),
            Err(e) => tracing::warn!("could not update the hidden list: {e}"),
        }
    }
    if failed > 0 {
        out.note = some_failed(failed, "taken out of the vault, so the password stays");
        return Ok(out);
    }
    vault::destroy(dir).map_err(|e| format!("The vault folder could not be removed: {e}"))?;
    vault::set_key(None);
    out.set = Some(false);
    out.note = Some("The password is removed. Your pictures are back in their folders and still hidden.".to_string());
    Ok(out)
}

/// Renames `old` to `new` without ever replacing something already there.
/// A hard link fails if the name is taken (a dangling symlink counts), and the
/// old name is removed only after it worked, so a file that appears at the new
/// name in between is never overwritten. File systems without hard links
/// (FAT, exFAT) fall back to the kernel's atomic no-replace rename.
fn rename_no_replace(old: &Path, new: &Path) -> Result<(), String> {
    const TAKEN: &str = "A file with that name already exists.";
    match std::fs::hard_link(old, new) {
        Ok(()) => std::fs::remove_file(old).map_err(|e| {
            // Undo, so there aren't two names for one photo. If even the undo
            // fails, say so, so the user knows to remove one of the two.
            match std::fs::remove_file(new) {
                Ok(()) => format!("Could not rename: {e}"),
                Err(u) => format!(
                    "Could not finish renaming: {e}. The photo now exists under both names and the copy could not be removed ({u})."
                ),
            }
        }),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(TAKEN.to_string()),
        Err(_) => {
            // No hard links here (FAT, exFAT). The kernel's own no-replace
            // rename is atomic: it fails if the name is taken, however late
            // another process creates it. Where even that is missing, say so
            // rather than risk overwriting.
            use rustix::fs::{renameat_with, RenameFlags, CWD};
            match renameat_with(CWD, old, CWD, new, RenameFlags::NOREPLACE) {
                Ok(()) => Ok(()),
                Err(e) if e == rustix::io::Errno::EXIST => Err(TAKEN.to_string()),
                Err(e) if e == rustix::io::Errno::INVAL || e == rustix::io::Errno::NOSYS || e == rustix::io::Errno::OPNOTSUPP => {
                    Err("This drive cannot rename a photo safely. Copy it under the new name in your file manager instead.".to_string())
                }
                Err(e) => Err(format!("Could not rename: {}", std::io::Error::from(e))),
            }
        }
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
    /// `Photos::folders_epoch` when the scan began.
    folders_epoch: u32,
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
/// `extra` are pictures to include even outside the folders (starred or hidden ones),
/// `folders` are the folders the user added.
fn scan(extra: HashSet<PathBuf>, folders: Vec<PathBuf>, first: Option<PathBuf>) -> Scan {
    let mut found: Vec<Photo> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut unreadable = 0usize;
    let mut truncated = false;
    // The folder the person is looking at is walked first and has its own
    // budget, so a full Pictures folder cannot leave it empty.
    let mut roots: Vec<PathBuf> = Vec::new();
    let has_first = first.is_some();
    if let Some(f) = &first {
        roots.push(f.clone());
    }
    roots.extend(Folder::ALL.into_iter().filter_map(Folder::dir).chain(folders).filter(|r| first.as_ref() != Some(r)));
    // Pictures taken from the ordinary roots, which share `MAX_PHOTOS`.
    let mut budgeted = 0usize;
    'roots: for (i, root) in roots.into_iter().enumerate() {
        let priority = has_first && i == 0;
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
                        if !priority {
                            budgeted += 1;
                        }
                    }
                }
                if !priority && budgeted >= MAX_PHOTOS {
                    truncated = true;
                    break 'roots;
                }
            }
        }
    }
    // Starred and hidden pictures outside the folders above must not vanish.
    for path in extra {
        // Saved names are canonical, so a starred symlink such as
        // `a.jpg -> raw-file` arrives as its target. The extension is then no
        // evidence either way: the content decides.
        // Only a regular file is ever opened: a pipe or device that took a
        // starred file's place would block the sniff and the scan with it.
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if !meta.is_file() {
            continue;
        }
        if !is_photo(&path) && !sniffs_as_photo(&path) {
            continue;
        }
        let key = photo_key(&path);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key.clone());
        found.push(Photo { path, key, modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH) });
    }
    sort_photos(&mut found);
    Scan { folders_epoch: 0, photos: found, unreadable, truncated }
}

/// Opens a picture upright, as the camera meant it (EXIF rotation).
fn decode(path: &Path) -> Option<image::DynamicImage> {
    decode_keeping_exif(path).map(|(img, _)| img)
}

/// Like `decode`, and also gives back the picture's EXIF block with the rotation
/// tag reset to "upright" (the pixels already are), so a rewrite of the file can
/// keep the capture date, camera and the rest.
fn decode_keeping_exif(path: &Path) -> Option<(image::DynamicImage, Option<Vec<u8>>)> {
    use image::ImageDecoder;
    // A picture in the Hidden vault is decrypted in memory, never to disk.
    let (mut img, orientation, exif) = if vault::is_vault_path(path) {
        let reader = image::ImageReader::new(std::io::Cursor::new(vault::read_path(path)?)).with_guessed_format().ok()?;
        let mut decoder = reader.into_decoder().ok()?;
        let orientation = decoder.orientation().ok();
        let mut exif = decoder.exif_metadata().ok().flatten();
        if let Some(exif) = exif.as_mut() {
            let _ = image::metadata::Orientation::remove_from_exif_chunk(exif);
        }
        (image::DynamicImage::from_decoder(decoder).ok()?, orientation, exif)
    } else {
        let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
        let mut decoder = reader.into_decoder().ok()?;
        let orientation = decoder.orientation().ok();
        let mut exif = decoder.exif_metadata().ok().flatten();
        if let Some(exif) = exif.as_mut() {
            let _ = image::metadata::Orientation::remove_from_exif_chunk(exif);
        }
        (image::DynamicImage::from_decoder(decoder).ok()?, orientation, exif)
    };
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Some((img, exif))
}

/// Writes a picture into an already open file in the format `ext` names,
/// carrying `exif` along where the format can hold it (JPEG, PNG, WebP).
fn save_with_exif(
    img: &image::DynamicImage,
    file: &std::fs::File,
    ext: &str,
    exif: Option<Vec<u8>>,
) -> image::ImageResult<()> {
    use image::ImageEncoder;
    let mut out = std::io::BufWriter::new(file);
    let format = match ext {
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        "png" => image::ImageFormat::Png,
        "webp" => image::ImageFormat::WebP,
        _ => image::ImageFormat::Bmp,
    };
    let result = match (exif, format) {
        (Some(exif), image::ImageFormat::Jpeg) => {
            let mut enc = image::codecs::jpeg::JpegEncoder::new(&mut out);
            let _ = enc.set_exif_metadata(exif);
            // JPEG has no alpha.
            if img.color().has_alpha() {
                image::DynamicImage::ImageRgb8(img.to_rgb8()).write_with_encoder(enc)
            } else {
                img.write_with_encoder(enc)
            }
        }
        (Some(exif), image::ImageFormat::Png) => {
            let mut enc = image::codecs::png::PngEncoder::new(&mut out);
            let _ = enc.set_exif_metadata(exif);
            img.write_with_encoder(enc)
        }
        (Some(exif), image::ImageFormat::WebP) => {
            let mut enc = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
            let _ = enc.set_exif_metadata(exif);
            img.write_with_encoder(enc)
        }
        _ => img.write_to(&mut out, format),
    };
    result?;
    std::io::Write::flush(&mut out)?;
    Ok(())
}

/// Creates a new file beside `path` that nothing else can have made first
/// (O_EXCL, which also refuses a symlink planted at the name), so a leftover
/// or hostile entry is never written through (CWE-59).
fn create_temp_beside(path: &Path) -> Option<(PathBuf, std::fs::File)> {
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    create_temp_beside_seeded(path, seed)
}

/// The seed is a parameter so a test can name the first candidate.
    use std::os::unix::fs::OpenOptionsExt;
fn create_temp_beside_seeded(path: &Path, seed: u128) -> Option<(PathBuf, std::fs::File)> {
    let name = path.file_name().and_then(|n| n.to_str())?;
    for attempt in 0..8u32 {
        let tmp = path.with_file_name(format!(".{name}.{}.{seed:x}.{attempt}.tmp", std::process::id()));
        // Owner-only while it is being written (CWE-378); the caller puts the
        // picture's own permissions back before the rename.
        match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp) {
            Ok(file) => return Some((tmp, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// What a rotate or an edit left on disk. `durable` is false when the new file
/// is in place but the folder could not be flushed, so a crash right now could
/// still bring the old state back. The picture is kept either way.
#[derive(Debug, Clone)]
struct Saved {
    path: PathBuf,
    durable: bool,
    /// Which file is at `path` now (see `file_id`), so a copy made here can be
    /// told apart from a different file that later takes its name.
    id: Option<FileId>,
}

/// Device and inode of a file: who it is, not what it is called.
type FileId = (u64, u64);

fn file_id(path: &Path) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

const NOT_DURABLE: &str = "Saved, but the disk did not confirm it yet. Keep the computer on for a moment.";
const COPY_FAILED: &str = "The copy could not be saved. Is the folder read-only?";
const TURN_FAILED: &str = "The picture could not be saved. Is the folder read-only?";
const MOVING: &str = "This picture is a moving one or its file type does not match its name, so it was left as it is.";

/// Makes a rename or a new name in `path`'s folder durable.
fn sync_parent(path: &Path) -> bool {
    match path.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(dir) => std::fs::File::open(dir).and_then(|d| d.sync_all()).is_ok(),
        None => true,
    }
}

/// True only when the decoder itself says the PNG or WebP is a still picture.
/// An animation, and a file the decoder cannot inspect, both count as "not
/// known to be still", since a rewrite would keep one frame or fail halfway.
fn is_still(path: &Path, format: image::ImageFormat) -> bool {
    let Ok(file) = std::fs::File::open(path) else { return false };
    let reader = std::io::BufReader::new(file);
    match format {
        image::ImageFormat::Png => {
            image::codecs::png::PngDecoder::new(reader).and_then(|d| d.is_apng()).is_ok_and(|animated| !animated)
        }
        image::ImageFormat::WebP => {
            image::codecs::webp::WebPDecoder::new(reader).is_ok_and(|d| !d.has_animation())
        }
        _ => true,
    }
}

/// Whether a picture may be rewritten as one still image: what is inside the
/// file is the format its name says, and it is not an animation. A rewrite
/// reads the file by its content, so without this a renamed or animated file
/// would silently lose its frames or change type.
fn transformable(path: &Path, ext: &str) -> bool {
    use std::io::Read;
    let mut head = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(64 * 1024).read_to_end(&mut head));
    if read.is_err() {
        return false;
    }
    let Ok(guess) = image::guess_format(&head) else { return false };
    if guess != image_format(ext) {
        return false;
    }
    is_still(path, guess)
}

/// One rotate or edit at a time on this account, across Photos windows, so two
/// of them never rewrite the same picture at once. Released when dropped.
fn transform_lock_in(dir: &Path) -> std::io::Result<std::fs::File> {
    std::fs::create_dir_all(dir)?;
    let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("transform.lock"))?;
    lock.lock()?;
    Ok(lock)
}

fn transform_lock() -> std::io::Result<std::fs::File> {
    let dir = store_file("transform")
        .and_then(|f| f.parent().map(Path::to_path_buf))
        .ok_or_else(|| std::io::Error::other("no config folder"))?;
    transform_lock_in(&dir)
}

/// The name a picture will have in the saved lists once it exists: its folder
/// is resolved now, so the name matches before and after the file is there.
fn future_key(path: &Path) -> PathBuf {
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    match (path.parent().and_then(|d| d.canonicalize().ok()), path.file_name()) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

/// Turns the picture a quarter turn and writes it back. The file is saved
/// upright (the camera's rotation tag is baked in and dropped), through a
/// temporary file so a failure leaves the original alone. JPEG is saved again,
/// so it is compressed once more. A symlink is followed: the file it points to
/// is the one that is replaced, and the link stays a link.
fn rotate_file(path: &Path, clockwise: bool) -> Result<Saved, String> {
    let real = path.canonicalize().map_err(|_| "The picture could not be read.".to_string())?;
    let path = real.as_path();
    // The format comes from the file's content, not its name, so a link
    // without an extension or a ".jpe" file is turned the way it is stored.
    let head = {
        use std::io::Read;
        let mut buf = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(64 * 1024).read_to_end(&mut buf))
            .map_err(|_| "The picture could not be read.".to_string())?;
        buf
    };
    let ext = match image::guess_format(&head) {
        Ok(image::ImageFormat::Jpeg) => "jpg",
        Ok(image::ImageFormat::WebP) => "webp",
        Ok(image::ImageFormat::Bmp) => "bmp",
        // An animated PNG would lose every frame but the first.
        Ok(image::ImageFormat::Png) if !head.windows(4).any(|w| w == b"acTL") => "png",
        _ => return Err("This kind of picture cannot be turned.".into()),
    };
    if !transformable(path, ext) {
        return Err(MOVING.into());
    }
    let (img, exif) = decode_keeping_exif(path).ok_or("The picture could not be read.")?;
    let turned = if clockwise { img.rotate90() } else { img.rotate270() };
    let (tmp, file) = create_temp_beside(path).ok_or(TURN_FAILED)?;
    let result = save_with_exif(&turned, &file, ext, exif).map_err(|e| e.to_string()).and_then(|()| {
        if let Ok(meta) = std::fs::metadata(path) {
            file.set_permissions(meta.permissions()).map_err(|e| e.to_string())?;
        }
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &real).map_err(|e| e.to_string())
    });
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(|_| TURN_FAILED.to_string())?;
    // The new picture is in place. A folder that cannot be flushed is told to
    // the person, not turned into a failure that hides a saved file.
    Ok(Saved { id: file_id(&real), durable: sync_parent(&real), path: real.clone() })
}

fn image_format(ext: &str) -> image::ImageFormat {
    match ext {
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        "png" => image::ImageFormat::Png,
        "webp" => image::ImageFormat::WebP,
        _ => image::ImageFormat::Bmp,
    }
}

fn write_image(img: &image::DynamicImage, file: &std::fs::File, format: image::ImageFormat) -> Result<(), String> {
    let mut out = std::io::BufWriter::new(file);
    img.write_to(&mut out, format)
        .map_err(|e| e.to_string())
        .and_then(|()| std::io::Write::flush(&mut out).map_err(|e| e.to_string()))
}

/// What the viewer's edit buttons can do. Every edit is saved as a copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edit {
    Brighter,
    Darker,
    MoreContrast,
    LessContrast,
    BlackWhite,
    CropSquare,
    Crop169,
    Crop43,
}

/// Applies one edit. Crops keep the middle of the picture.
fn edit_image(img: image::DynamicImage, edit: Edit) -> image::DynamicImage {
    let crop = |img: image::DynamicImage, rw: u32, rh: u32| {
        let (w, h) = (img.width(), img.height());
        // Largest rectangle of the wanted shape that fits.
        let (nw, nh) = if u64::from(w) * u64::from(rh) > u64::from(h) * u64::from(rw) {
            ((u64::from(h) * u64::from(rw) / u64::from(rh)) as u32, h)
        } else {
            (w, (u64::from(w) * u64::from(rh) / u64::from(rw)) as u32)
        };
        let (nw, nh) = (nw.max(1), nh.max(1));
        img.crop_imm((w - nw) / 2, (h - nh) / 2, nw, nh)
    };
    match edit {
        Edit::Brighter => img.brighten(20),
        Edit::Darker => img.brighten(-20),
        Edit::MoreContrast => img.adjust_contrast(15.0),
        Edit::LessContrast => img.adjust_contrast(-15.0),
        Edit::BlackWhite => image::DynamicImage::ImageRgb8(img.grayscale().to_rgb8()),
        Edit::CropSquare => crop(img, 1, 1),
        Edit::Crop169 => crop(img, 16, 9),
        Edit::Crop43 => crop(img, 4, 3),
    }
}

/// Where an edit is saved. A copy this session made earlier (`in_place`) is
/// edited in place; any other picture gets a new "name (edited).ext" beside
/// it, never over an existing file, whatever the picture is called.
fn edit_target(path: &Path, in_place: bool, taken: impl Fn(&Path) -> bool) -> PathBuf {
    if in_place {
        return path.to_path_buf();
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("picture");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem} (edited).{ext}") } else { format!("{stem} (edited {n}).{ext}") };
        let candidate = path.with_file_name(name);
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Puts a finished temporary file at `target` as a new name. It never replaces
/// a file: the hard link fails if the name is taken, which a rename would not.
/// A folder that cannot link (FAT, some network shares) falls back to a rename
/// after one more look.
fn publish_new(tmp: &Path, target: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(tmp, target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e),
        Err(_) if target.exists() => Err(std::io::ErrorKind::AlreadyExists.into()),
        Err(_) => std::fs::rename(tmp, target),
    }
}

/// Edits the picture and saves the result as a copy (see `edit_target`),
/// through a temporary file. The original is never changed.
///
/// `register` is called with the name the copy will have, after the picture is
/// written to the temporary file and before the copy gets its name. If it
/// fails nothing is published, so a copy of a hidden picture is never listed
/// before it is hidden (CWE-200). It returns true when it added an entry that
/// was not there. If the name is taken meanwhile, that entry is taken back with
/// `unregister` and the next free name is registered and tried; the same when
/// publishing fails, so the list never keeps a name that was not used.
fn edit_file(
    path: &Path,
    edit: Edit,
    in_place: bool,
    register: &dyn Fn(&Path) -> Result<bool, String>,
    unregister: &dyn Fn(&Path),
) -> Result<Saved, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp") {
        return Err("This kind of picture cannot be edited.".into());
    }
    if !transformable(path, &ext) {
        return Err(MOVING.into());
    }
    let img = decode(path).ok_or("The picture could not be read.")?;
    let edited = edit_image(img, edit);
    let mut target = edit_target(path, in_place, |p| p.exists());
    let (tmp, file) = create_temp_beside(&target).ok_or(COPY_FAILED)?;
    let result = write_image(&edited, &file, image_format(&ext)).and_then(|()| {
        if let Ok(meta) = std::fs::metadata(path) {
            let _ = file.set_permissions(meta.permissions());
        }
        file.sync_all().map_err(|e| e.to_string())?;
        for _ in 0..8 {
            let key = future_key(&target);
            let added = register(&key).map_err(|_| "hidden".to_string())?;
            let published = if target == path {
                // A copy made earlier by an edit is edited in place.
                std::fs::rename(&tmp, &target)
            } else {
                publish_new(&tmp, &target)
            };
            match published {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if added {
                        unregister(&key);
                    }
                    if e.kind() == std::io::ErrorKind::AlreadyExists && target != path {
                        target = edit_target(path, false, |p| p.exists());
                    } else {
                        return Err(e.to_string());
                    }
                }
            }
        }
        Err("no free name".to_string())
    });
    drop(file);
    // The temporary name is only ever a stand-in: after a rename it is gone,
    // after a link it is one of two names for the same file.
    if result.is_err() || target != path {
        let _ = std::fs::remove_file(&tmp);
    }
    match result {
        Ok(()) => Ok(Saved { id: file_id(&target), durable: sync_parent(&target), path: target }),
        Err(e) if e == "hidden" => {
            Err("The copy was not made, because it could not be added to your hidden photos.".to_string())
        }
        Err(_) => Err(COPY_FAILED.to_string()),
    }
}

/// `edit_file` for the app: one transform at a time, and a copy of a hidden
/// picture is put on the hidden list before it exists. The list is read under
/// the lock; if it cannot be read, nothing is made.
///
/// `made` is the identity of the copy this session made at `path`, if it is
/// one. Only when the file there is still that very file is it edited in
/// place; a different file that took the name gets a copy of its own.
fn edit_locked(path: &Path, edit: Edit, made: Option<FileId>) -> Result<Saved, String> {
    let _lock = transform_lock().map_err(|_| COPY_FAILED.to_string())?;
    let hidden = try_load_set("hidden")
        .map_err(|_| "Your hidden photos could not be read, so the picture was left as it is.".to_string())?;
    let hide = hidden.contains(&photo_key(path)) || hidden.contains(path);
    let in_place = made.is_some() && file_id(path) == made;
    edit_file(
        path,
        edit,
        in_place,
        &|key| {
            if !hide {
                return Ok(false);
            }
            // Strict: the entry must be on disk before the copy exists.
            let key = key.to_path_buf();
            let had = try_load_set("hidden").map_err(|e| e.to_string())?.contains(&key);
            set_in_with("hidden", std::slice::from_ref(&key), true, true).map_err(|e| e.to_string())?;
            Ok(!had)
        },
        &|key| {
            if let Err(e) = set_in("hidden", std::slice::from_ref(&key.to_path_buf()), false) {
                tracing::warn!("could not take an unused name off the hidden list: {e}");
            }
        },
    )
}

/// `rotate_file` for the app, one transform at a time.
fn rotate_locked(path: &Path, clockwise: bool) -> Result<Saved, String> {
    let _lock = transform_lock().map_err(|_| TURN_FAILED.to_string())?;
    rotate_file(path, clockwise)
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
    if let Some(name) = vault::original_name(path) {
        return name;
    }
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
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

/// Adds `paths` to `list` in order, skipping any already in it, in one pass.
fn add_unique(list: &mut Vec<PathBuf>, paths: Vec<PathBuf>) {
    let mut have: HashSet<PathBuf> = list.iter().cloned().collect();
    for p in paths {
        if have.insert(p.clone()) {
            list.push(p);
        }
    }
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
    /// The photos the current section and source show, newest first.
    fn shown(&self) -> Vec<&Photo> {
        if self.hidden_locked() || self.hidden_unreadable {
            return Vec::new();
        }
        if self.vault_view() {
            return self.vault_photos.iter().collect();
        }
        let root = self.source_extra.clone().or_else(|| self.source.and_then(Folder::dir));
        // A photo reached through a symlink belongs to the folder it really lives in.
        let root_key = root.as_deref().map(photo_key);
        // A named folder this account does not have shows nothing, not everything.
        if self.source.is_some() && self.source_extra.is_none() && root.is_none() {
            return Vec::new();
        }
        let now = SystemTime::now();
        // Looked up once, so a large album costs one hash lookup per photo.
        let in_album: HashSet<&PathBuf> = match (self.section, self.album.as_ref()) {
            (Section::Album, Some(a)) => self.albums.get(a).map(|l| l.iter().collect()).unwrap_or_default(),
            _ => HashSet::new(),
        };
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
                    Section::Album => in_album.contains(&p.key),
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
        let todo: Vec<(PathBuf, u64)> = todo
            .into_iter()
            .map(|p| {
                self.next_epoch += 1;
                self.thumb_epoch.insert(p.clone(), self.next_epoch);
                (p, self.next_epoch)
            })
            .collect();
        if todo.is_empty() {
            return Task::none();
        }
        Task::stream(
            futures::stream::iter(todo)
                .map(|(p, epoch)| async move {
                    let key = p.clone();
                    let h = tokio::task::spawn_blocking(move || load_thumb(p)).await.ok().flatten();
                    cosmic::Action::App(Message::Thumb(key, epoch, h))
                })
                .buffer_unordered(4),
        )
    }

    fn want_section_thumbs(&mut self) -> Task<Message> {
        let paths: Vec<PathBuf> = self.shown().into_iter().take(self.tile_limit).map(|p| p.path.clone()).collect();
        self.want_thumbs(paths)
    }

    fn scan_task(&mut self) -> Task<Message> {
        self.scans_running += 1;
        let folders = self.extra.clone();
        let mut extra: HashSet<PathBuf> = self.favorites.union(&self.hidden).cloned().collect();
        // Album members outside the scanned folders must come back after a restart too.
        extra.extend(self.albums.values().flatten().cloned());
        let folders_epoch = self.folders_epoch;
        let first = self.source_extra.clone();
        Task::future(async move {
            let mut found = tokio::task::spawn_blocking(move || scan(extra, folders, first)).await.unwrap_or_default();
            found.folders_epoch = folders_epoch;
            cosmic::Action::App(Message::Scanned(found))
        })
    }

    /// Decodes `path` at full size for the viewer or the reel, and drops any
    /// queued decode of another picture.
    fn want_full(&mut self, path: PathBuf) -> Task<Message> {
        set_viewing(Some(path.clone()));
        let epoch = self.epoch;
        Task::future(async move {
            let key = path.clone();
            let Ok(_permit) = FULL_DECODES.acquire().await else {
                return cosmic::Action::App(Message::Full(key, epoch, None));
            };
            // Paged past or closed while waiting for a turn: skip the decode.
            if !still_viewing(&key) {
                return cosmic::Action::App(Message::Ignore);
            }
            let h = tokio::task::spawn_blocking(move || load_full(path)).await.ok().flatten();
            cosmic::Action::App(Message::Full(key, epoch, h))
        })
    }

    fn open(&mut self, path: PathBuf) -> Task<Message> {
        self.visit += 1;
        self.viewing = Some(path.clone());
        set_viewing(Some(path.clone()));
        self.overlay = None;
        self.action_error = None;
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
                set_viewing(None);
                self.full = None;
                self.failed = None;
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

    /// Hidden is showing and its pictures live in the encrypted vault.
    fn vault_view(&self) -> bool {
        self.section == Section::Hidden && self.vault_set
    }

    /// Shows the vault's pictures (newest first) and remembers their file names.
    fn install_vault(&mut self, mut entries: Vec<vault::Entry>) {
        vault::set_names(&entries);
        let Some(dir) = vault::default_dir() else { return };
        entries.sort_by(|a, b| b.modified.cmp(&a.modified));
        let fresh: Vec<Photo> = entries
            .iter()
            .map(|e| Photo {
                path: vault::file_of(dir, &e.id),
                key: vault::file_of(dir, &e.id),
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(e.modified),
            })
            .collect();
        let live: HashSet<&PathBuf> = fresh.iter().map(|p| &p.path).collect();
        for p in self.vault_photos.iter().filter(|p| !live.contains(&p.path)) {
            self.thumbs.remove(&p.path);
            self.dims.remove(&p.path);
            self.asked.remove(&p.path);
        }
        self.vault_photos = fresh;
    }

    /// Hides the vault's pictures again and forgets their thumbnails.
    fn close_vault(&mut self) {
        for p in self.vault_photos.drain(..) {
            self.thumbs.remove(&p.path);
            self.dims.remove(&p.path);
            self.asked.remove(&p.path);
        }
        self.hidden_open = false;
    }

    /// The Hidden section is showing and its password has not been entered yet.
    fn hidden_locked(&self) -> bool {
        self.section == Section::Hidden && self.vault_set && !self.hidden_open
    }

    /// Whether the person hid this picture. Entries saved under another
    /// spelling of the path still count.
    fn is_hidden(&self, p: &Photo) -> bool {
        self.hidden.contains(&p.key) || self.hidden.contains(&p.path)
    }

    /// Reads the Hidden list again: another Photos window may have hidden or
    /// unhidden pictures since this one started.
    fn sync_lock(&mut self) {
        // Another window may have hidden or unhidden pictures, or left the file
        // unreadable, and a decision about what may open must not rest on the
        // list read at start.
        match try_load_set("hidden") {
            Ok(set) => {
                let changed = set != self.hidden;
                self.hidden = set;
                self.hidden_unreadable = false;
                if changed {
                    // Another window may have hidden a selected photo: it is no
                    // longer shown, so it must not stay a target.
                    self.trim_selection();
                }
            }
            Err(e) => {
                tracing::warn!("could not read the hidden photos: {e}");
                if !self.hidden_unreadable {
                    self.clear_selection();
                }
                self.hidden_unreadable = true;
            }
        }
        self.trim_selection();
    }

    /// The canonical name of the photo found at `path`.
    fn key_of(&self, path: &Path) -> PathBuf {
        self.keys.get(path).cloned().unwrap_or_else(|| photo_key(path))
    }

    fn is_favorite(&self, path: &Path) -> bool {
        self.favorites.contains(&self.key_of(path))
    }

    /// The selected pictures that are in the current list, so a bulk action never
    /// touches one that is no longer shown (unstarred in Favorites, say).
    fn targets(&self) -> Vec<PathBuf> {
        let visible: HashSet<&PathBuf> = self.shown().into_iter().map(|p| &p.path).collect();
        self.selected.iter().filter(|p| visible.contains(p)).cloned().collect()
    }

    /// Drops from the selection what the current list no longer shows.
    fn trim_selection(&mut self) {
        let keep: HashSet<PathBuf> = self.targets().into_iter().collect();
        self.selected.retain(|p| keep.contains(p));
        if self.selected.is_empty() {
            self.select_mode = false;
            self.anchor = None;
            self.confirm_delete = false;
        }
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

    /// A renamed photo keeps its place in the list, its thumbnail and its star.
    fn after_rename(&mut self, old: PathBuf, new: PathBuf) -> Task<Message> {
        for p in &mut self.photos {
            if p.path == old {
                p.path = new.clone();
            }
        }
        // Carry the "asked" marker over only together with a cached thumbnail.
        // If the old decode is still pending, its result is filed under the old
        // name, so the new name must ask for its own.
        let asked_before = self.asked.remove(&old);
        self.thumb_epoch.remove(&old);
        let mut refetch = Task::none();
        match self.thumbs.remove(&old) {
            Some(h) => {
                self.thumbs.insert(new.clone(), h);
                if asked_before {
                    self.asked.insert(new.clone());
                }
            }
            None if asked_before => refetch = self.want_thumbs(vec![new.clone()]),
            None => {}
        }
        // Another window may have starred this photo since this one last read
        // the file, so ask the saved set, not the cached one.
        match move_favorite(&old, &new) {
            Ok(saved) => {
                self.favorites = saved;
                self.favorites_unsaved = false;
            }
            Err(e) => {
                tracing::warn!("could not move the favorite: {e}");
                // Report it even when the cached set lacks the old name: another
                // window may have starred the photo, and the saved star then
                // still sits on the old path.
                if self.favorites.remove(&old) {
                    self.favorites.insert(new.clone());
                }
                self.favorites_unsaved = true;
                self.notice = Some("The photo was renamed, but its star could not be moved to the new name.".into());
            }
        }
        if self.viewing.as_deref() == Some(old.as_path()) {
            return Task::batch([refetch, self.open(new)]);
        }
        refetch
    }

    /// A deleted photo moves the viewer to the next one, or the one before it,
    /// or back to the gallery when it was the only one.
    fn after_delete(&mut self, path: PathBuf) -> Task<Message> {
        let next = {
            let list = self.viewer_list();
            list.iter().position(|p| p.path == path).and_then(|i| {
                list.get(i + 1).or_else(|| i.checked_sub(1).and_then(|j| list.get(j))).map(|p| p.path.clone())
            })
        };
        self.photos.retain(|p| p.path != path);
        // The list shifted, so a photo that now falls inside the tile limit needs its thumbnail.
        let more = self.want_section_thumbs();
        self.thumbs.remove(&path);
        self.asked.remove(&path);
        self.thumb_epoch.remove(&path);
        if self.favorites.remove(&path) {
            if let Err(e) = set_favorite(&path, false) {
                tracing::warn!("could not drop the favorite: {e}");
            }
        }
        if self.viewing.as_deref() == Some(path.as_path()) {
            let moved = match next {
                Some(n) => self.open(n),
                None => self.back(),
            };
            return Task::batch([moved, more]);
        }
        more
    }

    fn back(&mut self) -> Task<Message> {
        self.slideshow = false;
        self.edit_open = false;
        self.visit += 1;
        self.overlay = None;
        self.action_error = None;
        self.viewing = None;
        self.full = None;
        self.failed = None;
        // The reel may have changed while the viewer was open (a favorite was
        // removed, a picture trashed), so check it against the list again.
        let reel = if self.size == REEL {
            self.sync_reel()
        } else {
            set_viewing(None);
            Task::none()
        };
        let title = match self.core.main_window_id() {
            Some(id) => self.set_window_title("Photos".into(), id),
            None => Task::none(),
        };
        let windowed = match self.core.main_window_id() {
            Some(id) if self.fullscreen => {
                self.fullscreen = false;
                cosmic::iced::window::set_mode(id, cosmic::iced::window::Mode::Windowed)
            }
            _ => Task::none(),
        };
        Task::batch([title, reel, windowed])
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
            notice: None,
            edit_open: false,
            read_errors: false,
            favorites_unsaved: false,
            failed: None,
            section: Section::Gallery,
            source: prefs.source,
            slide_secs: prefs.slide_secs,
            fullscreen: false,
            size: prefs.size,
            square: prefs.square,
            group: prefs.group,
            sidebar: prefs.sidebar,
            menu: None,
            thumbs: HashMap::new(),
            dims: HashMap::new(),
            asked: HashSet::new(),
            turning: HashSet::new(),
            epoch: 0,
            stale_since: HashMap::new(),
            scans_running: 0,
            trashed_meanwhile: HashSet::new(),
            thumb_epoch: HashMap::new(),
            next_epoch: 0,
            visit: 0,
            file_op: false,
            folders_epoch: 0,
            favorites: load_favorites(),
            keys: HashMap::new(),
            hidden: hidden_set.unwrap_or_default(),
            made: HashMap::new(),
            hidden_unreadable,
            albums: load_albums(),
            album: None,
            new_album: None,
            renaming: None,
            vault_set: vault::default_dir().is_some_and(|d| vault::exists(d)),
            vault_photos: Vec::new(),
            vault_busy: false,
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
            tile_limit: MAX_TILES,
            grid_bucket: usize::MAX,
            full: None,
            start,
            overlay: None,
            action_error: None,
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
            cosmic::iced::time::every(std::time::Duration::from_secs(u64::from(self.slide_secs))).map(|_| Message::SlideshowTick)
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
            Message::Scanned(found) => {
                // A scan that began before the list of added folders changed still
                // lists a removed folder or misses a new one: the newer scan follows.
                if found.folders_epoch != self.folders_epoch {
                    self.scans_running = self.scans_running.saturating_sub(1);
                    if self.scans_running == 0 {
                        self.trashed_meanwhile.clear();
                    }
                    return Task::none();
                }
                self.read_errors = found.unreadable > 0;
                self.notice = match (found.unreadable, found.truncated) {
                    (0, false) => None,
                    (0, true) => Some(format!("Showing the first {MAX_PHOTOS} pictures found in your folders.")),
                    (n, false) => Some(format!("{n} folder(s) could not be read, so some pictures may be missing.")),
                    (n, true) => Some(format!(
                        "Showing the first {MAX_PHOTOS} pictures found in your folders, and {n} folder(s) could not be read."
                    )),
                };
                self.scans_running = self.scans_running.saturating_sub(1);
                let mut photos = found.photos;
                // Pictures trashed while this scan ran must not come back.
                if !self.trashed_meanwhile.is_empty() {
                    photos.retain(|p| !self.trashed_meanwhile.contains(&p.path));
                }
                if self.scans_running == 0 {
                    self.trashed_meanwhile.clear();
                }
                // A file that changed on disk needs its tile and full decode again.
                let old: HashMap<&PathBuf, SystemTime> = self.photos.iter().map(|p| (&p.path, p.modified)).collect();
                let changed: Vec<PathBuf> = photos
                    .iter()
                    .filter(|p| old.get(&p.path).is_some_and(|m| *m != p.modified))
                    .map(|p| p.path.clone())
                    .collect();
                drop(old);
                let mut redo = Task::none();
                if !changed.is_empty() {
                    self.epoch += 1;
                    for p in &changed {
                        self.thumbs.remove(p);
                        self.dims.remove(p);
                        self.asked.remove(p);
                        self.thumb_epoch.remove(p);
                        self.stale_since.insert(p.clone(), self.epoch);
                    }
                    if self.full.as_ref().is_some_and(|(p, _)| changed.contains(p)) {
                        self.full = None;
                    }
                    if let Some(p) = self.viewing.clone().filter(|p| changed.contains(p)) {
                        self.failed = None;
                        redo = self.want_full(p);
                    } else if let Some(p) = self.reel.clone().filter(|p| changed.contains(p)) {
                        if self.viewing.is_none() && self.size == REEL {
                            self.failed = None;
                            redo = self.want_full(p);
                        }
                    }
                }
                self.photos = photos;
                self.keys = self.photos.iter().map(|p| (p.path.clone(), p.key.clone())).collect();
                self.scanning = false;
                let live: HashSet<&PathBuf> = self.photos.iter().map(|p| &p.path).collect();
                self.selected.retain(|p| live.contains(p));
                // A picture handed over by Files opens straight in the viewer.
                // Read the saved password and Hidden list now, so a picture that
                // is hidden never opens without the password.
                self.sync_lock();
                let open = match self.start.take() {
                    Some(p) if is_photo(&p) && p.is_file() => {
                        let p = p.canonicalize().unwrap_or(p);
                        let key = photo_key(&p);
                        let hidden_now = self.is_hidden(&Photo { path: p.clone(), key: key.clone(), modified: SystemTime::UNIX_EPOCH });
                        if self.hidden_unreadable || (hidden_now && self.vault_set && !self.hidden_open) {
                            // Hidden and locked, or the lists could not be read:
                            // show the gallery instead of the picture.
                            Task::none()
                        } else {
                            // Open the entry the scan already has for this file, so
                            // the viewer and the lists see one photo.
                            let path = match self.photos.iter().find(|x| x.key == key) {
                                Some(x) => x.path.clone(),
                                None => {
                                    let modified = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                                    self.photos.insert(0, Photo { path: p.clone(), key, modified });
                                    p
                                }
                            };
                            self.open(path)
                        }
                    }
                    _ => Task::none(),
                };
                let view = self.refresh_view();
                return Task::batch([view, open, redo]);
            }
            Message::Section(s) => {
                self.tile_limit = MAX_TILES;
                self.grid_bucket = usize::MAX;
                self.section = s;
                self.close_vault();
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
                self.tile_limit = MAX_TILES;
                self.grid_bucket = usize::MAX;
                self.source = folder;
                self.source_extra = None;
                self.menu = None;
                return self.scan_task();
            }
            Message::ToggleSidebar => {
                self.sidebar = !self.sidebar;
                save_pref("sidebar", serde_json::json!(self.sidebar));
            }
            Message::ToggleSquare => {
                self.square = !self.square;
                save_pref("square", serde_json::json!(self.square));
            }
            Message::SetGroup(g) => {
                self.group = g;
                self.menu = None;
                save_pref("group", serde_json::json!(g.key()));
            }
            Message::Menu(menu) => self.menu = menu,
            Message::Size(n) => {
                self.size = n.min(REEL);
                save_pref("size", serde_json::json!(self.size));
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
                    // The tiles loaded so far (`tile_limit`, which grows as you scroll),
                    // never a photo the list does not show, so nothing unseen is
                    // batch-deleted.
                    self.selected = self.shown().into_iter().take(self.tile_limit).map(|p| p.path.clone()).collect();
                    self.select_mode = true;
                    self.confirm_delete = false;
                }
            }
            Message::FavoriteTargets => {
                if self.vault_view() {
                    return Task::none();
                }
                let paths = self.targets();
                if paths.is_empty() {
                    return Task::none();
                }
                let on = !paths.iter().all(|p| self.is_favorite(p));
                match set_favorites(&paths, on) {
                    Ok(saved) => {
                        self.favorites = saved;
                        self.favorites_unsaved = false;
                        // In Favorites an unstarred photo leaves the list.
                        self.trim_selection();
                    }
                    Err(e) => {
                        tracing::warn!("could not save favorites: {e}");
                        self.favorites_unsaved = true;
                    }
                }
                self.trim_selection();
                return self.refresh_view();
            }
            Message::Album(name) => {
                self.tile_limit = MAX_TILES;
                self.section = Section::Album;
                self.album = Some(name);
                self.menu = None;
                self.clear_selection();
                return self.refresh_view();
            }
            Message::AddToAlbum(name) => {
                if self.vault_view() {
                    return Task::none();
                }
                self.sync_lock();
                let paths: Vec<PathBuf> = self.selected.iter().map(|p| self.key_of(p)).collect();
                self.menu = None;
                if paths.is_empty() {
                    return Task::none();
                }
                // Only an album that still exists in the saved file gets the photos: another
                // window may have deleted it, and this must not bring it back.
                let mut found = false;
                match edit_albums(|a| {
                    if let Some(list) = a.get_mut(&name) {
                        found = true;
                        add_unique(list, paths);
                    }
                }) {
                    Ok(saved) => {
                        self.albums = saved;
                        self.albums_unsaved = false;
                        if found {
                            self.clear_selection();
                        } else {
                            self.notice = Some(format!("The album \"{name}\" no longer exists. Your selection is kept."));
                        }
                    }
                    Err(e) => {
                        tracing::warn!("could not save albums: {e}");
                        self.albums_unsaved = true;
                        self.notice = Some(format!(
                            "The photos could not be added to \"{name}\", because Photos could not save the album. Your selection is kept."
                        ));
                    }
                }
            }
            Message::RemoveFromAlbum => {
                self.sync_lock();
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
                        self.tile_limit = MAX_TILES;
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
                if self.vault_busy || !self.vault_set || vault::key().is_none() {
                    return Task::none();
                }
                self.vault_busy = true;
                self.notice = Some("Taking your pictures out of the vault...".to_string());
                return vault_task(vault_remove);
            }
            Message::LockSubmit => {
                if self.vault_busy {
                    return Task::none();
                }
                let text = std::mem::take(&mut self.lock_text);
                if text.is_empty() {
                    return Task::none();
                }
                self.lock_msg = None;
                if self.setting_lock {
                    if self.vault_set {
                        // Changing the password: the vault must be open.
                        if vault::key().is_none() {
                            return Task::none();
                        }
                        self.vault_busy = true;
                        return vault_task(move || vault_change(&text));
                    }
                    let plain: Vec<PathBuf> = self.hidden.iter().cloned().collect();
                    self.vault_busy = true;
                    self.lock_msg = Some("Setting up the vault...".to_string());
                    return vault_task(move || vault_setup(&text, plain));
                }
                self.vault_busy = true;
                self.lock_msg = Some("Checking the password...".to_string());
                return Task::future(async move {
                    let r = tokio::task::spawn_blocking(move || vault_unlock(&text))
                        .await
                        .unwrap_or_else(|_| Err("The vault could not be opened.".to_string()));
                    cosmic::Action::App(Message::VaultOpened(r))
                });
            }
            Message::VaultOpened(opened) => {
                self.vault_busy = false;
                match opened {
                    Ok(entries) if self.section == Section::Hidden => {
                        self.hidden_open = true;
                        self.lock_msg = None;
                        self.install_vault(entries);
                        return self.refresh_view();
                    }
                    // The person left Hidden while the password was being checked.
                    Ok(_) => self.lock_msg = None,
                    Err(e) => {
                        self.lock_msg = Some(e);
                        return widget::text_input::focus(LOCK_ID.clone());
                    }
                }
            }
            Message::VaultDone(done) => {
                self.vault_busy = false;
                let o = match done {
                    Ok(o) => o,
                    Err(e) => {
                        if self.setting_lock {
                            self.lock_msg = Some(e);
                        } else {
                            self.notice = Some(e);
                        }
                        return Task::none();
                    }
                };
                if let Some(set) = o.set {
                    self.vault_set = set;
                }
                if let Some(h) = o.hidden {
                    self.hidden = h;
                }
                self.setting_lock = false;
                self.lock_text.clear();
                self.lock_msg = None;
                self.notice = o.note;
                let gone: HashSet<PathBuf> = o.moved.into_iter().collect();
                self.photos.retain(|p| !gone.contains(&p.path));
                for p in &gone {
                    self.thumbs.remove(p);
                    self.dims.remove(p);
                    self.asked.remove(p);
                }
                if !self.vault_set {
                    vault::set_key(None);
                    self.close_vault();
                    self.vault_photos.clear();
                } else if self.hidden_open || o.set == Some(true) {
                    self.hidden_open = true;
                    self.install_vault(o.entries);
                }
                self.clear_selection();
                let view = self.refresh_view();
                if o.rescan {
                    self.scanning = true;
                    return Task::batch([view, self.scan_task()]);
                }
                return view;
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
                    // Checked again inside the lock: another window may have made or
                    // removed an album since this window loaded its list.
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
                    add_unique(a.entry(name).or_default(), paths);
                }) {
                    Ok(saved) => {
                        self.albums = saved;
                        self.albums_unsaved = false;
                        self.new_album = None;
                        if had {
                            self.clear_selection();
                        }
                        self.tile_limit = MAX_TILES;
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
                    match file_chooser::open::Dialog::new().title("Add a folder of pictures").open_folder().await {
                        Ok(r) => cosmic::Action::App(Message::FolderPicked(r.url().to_file_path().ok())),
                        Err(file_chooser::Error::Cancelled) => cosmic::Action::App(Message::FolderPicked(None)),
                        Err(e) => {
                            tracing::warn!("folder chooser failed: {e:?}");
                            cosmic::Action::App(Message::FolderChooserFailed)
                        }
                    }
                });
            }
            Message::FolderPicked(None) => {}
            Message::FolderChooserFailed => {
                self.notice = Some("The folder chooser could not be opened, so no folder was added.".to_string());
            }
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
                        self.folders_epoch = self.folders_epoch.wrapping_add(1);
                        self.source = None;
                        self.source_extra = Some(dir);
                        // The new folder shows all of its pictures, not what an old
                        // section filter or selection left behind.
                        self.section = Section::Gallery;
                        self.album = None;
                        self.clear_selection();
                        self.tile_limit = MAX_TILES;
                        self.grid_bucket = usize::MAX;
                        self.notice = None;
                        // Same reset as picking a source: nothing chosen in the old
                        // view may be batch-deleted from the new one.
                        self.clear_selection();
                        self.tile_limit = MAX_TILES;
                        self.grid_bucket = usize::MAX;
                        self.scanning = true;
                        return self.scan_task();
                    }
                    Err(e) => {
                        tracing::warn!("could not save folders: {e}");
                        self.notice = Some("The folder could not be added, because Photos could not save it. Nothing changed.".to_string());
                    }
                }
            }
            Message::SourceExtra(dir) => {
                self.tile_limit = MAX_TILES;
                self.grid_bucket = usize::MAX;
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
                        self.folders_epoch = self.folders_epoch.wrapping_add(1);
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
                // The password may have changed in another window: read it again
                // (which clears the selection if so) before acting on the selection.
                self.sync_lock();
                let targets: HashSet<PathBuf> = self.targets().into_iter().collect();
                // A locked Hidden section shows nothing, so nothing in it can be picked.
                if targets.is_empty() || self.hidden_locked() {
                    return Task::none();
                }
                let hide = self.section != Section::Hidden;
                if self.vault_set {
                    let paths: Vec<PathBuf> = targets.iter().cloned().collect();
                    if self.vault_busy {
                        return Task::none();
                    }
                    if hide {
                        if vault::key().is_none() {
                            self.notice = Some("Open Hidden and type the password first, then hide.".to_string());
                            return Task::none();
                        }
                        self.notice = Some("Moving the pictures into the vault...".to_string());
                        self.vault_busy = true;
                        return vault_task(move || vault_hide(&paths));
                    }
                    if !paths.iter().all(|p| vault::is_vault_path(p)) {
                        return Task::none();
                    }
                    self.notice = Some("Taking the pictures out of the vault...".to_string());
                    self.vault_busy = true;
                    return vault_task(move || vault_unhide(&paths));
                }
                // The list holds each picture under its canonical name. To unhide, the
                // spelling the picture was found at goes too, for entries saved before.
                let mut paths: Vec<PathBuf> = Vec::new();
                for p in self.photos.iter().filter(|p| targets.contains(&p.path)) {
                    paths.push(p.key.clone());
                    if !hide {
                        paths.push(p.path.clone());
                    }
                }
                match set_in("hidden", &paths, hide) {
                    Ok(saved) => {
                        self.hidden = saved;
                        self.hidden_unreadable = false;
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
                self.sync_lock();
                if self.viewing.is_none() && !self.hidden_locked() {
                    // Count only what the list still shows, so the question matches the action.
                    self.trim_selection();
                    if !self.selected.is_empty() {
                        self.confirm_delete = true;
                    }
                }
            }
            Message::CancelDelete => self.confirm_delete = false,
            Message::ConfirmDelete => {
                self.confirm_delete = false;
                if self.vault_view() {
                    return Task::none();
                }
                self.sync_lock();
                if self.hidden_locked() {
                    return Task::none();
                }
                let paths = self.targets();
                if paths.is_empty() {
                    return Task::none();
                }
                if paths.len() != self.selected.len() {
                    // The list changed while the question was open: ask again instead of
                    // moving a different set than the one the person confirmed.
                    self.trim_selection();
                    self.notice = Some("The selection changed, so nothing was moved. Choose Delete again to confirm.".into());
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
                if self.scans_running > 0 {
                    self.trashed_meanwhile.extend(gone.iter().cloned());
                }
                self.photos.retain(|p| !gone.contains(&p.path));
                self.selected.retain(|p| !gone.contains(p));
                for p in &gone {
                    self.thumbs.remove(p);
                    self.dims.remove(p);
                    self.asked.remove(p);
                    self.thumb_epoch.remove(p);
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
            Message::Thumb(path, epoch, loaded) => {
                // A decode that began before the file changed, or for a name
                // renamed, deleted or asked again since, is dropped.
                if self.thumb_epoch.get(&path) != Some(&epoch) {
                    return Task::none();
                }
                if let Some((h, dims)) = loaded {
                    self.dims.insert(path.clone(), dims);
                    self.thumbs.insert(path, h);
                }
            }
            Message::Open(path) => return self.open(path),
            Message::Full(path, epoch, handle) => {
                let stale = self.stale_since.get(&path).is_some_and(|s| epoch < *s);
                let wanted = !stale
                    && match &self.viewing {
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
                // Escape closes the menu first, then the viewer.
                if self.overlay.take().is_some() {
                    return Task::none();
                }
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
                    // A photo open from outside the section plays as a list of one.
                    let start = self
                        .viewing
                        .clone()
                        .or_else(|| list.iter().find(|p| self.selected.contains(&p.path)).map(|p| p.path.clone()))
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
            Message::SlideSpeed => {
                self.menu = None;
                let at = SLIDE_CHOICES.iter().position(|s| *s == self.slide_secs);
                self.slide_secs = SLIDE_CHOICES[at.map_or(0, |i| (i + 1) % SLIDE_CHOICES.len())];
                save_pref("slide_secs", serde_json::json!(self.slide_secs));
            }
            Message::Fullscreen => {
                self.menu = None;
                self.fullscreen = !self.fullscreen;
                let mode = if self.fullscreen { cosmic::iced::window::Mode::Fullscreen } else { cosmic::iced::window::Mode::Windowed };
                let Some(id) = self.core.main_window_id() else { return Task::none() };
                return cosmic::iced::window::set_mode(id, mode);
            }
            Message::GridScrolled(y) => {
                let all: Vec<PathBuf> = self.shown().into_iter().map(|p| p.path.clone()).collect();
                if y > 0.8 && self.tile_limit < all.len() {
                    self.tile_limit += TILE_STEP;
                }
                // Only the pictures near the visible stretch keep a thumbnail in
                // memory; the rest are dropped and decoded again on the way back.
                let shown = all.len().min(self.tile_limit);
                let centre = (y.clamp(0.0, 1.0) * shown as f32) as usize;
                let bucket = centre / KEEP_STEP;
                if bucket == self.grid_bucket {
                    return Task::none();
                }
                self.grid_bucket = bucket;
                let lo = centre.saturating_sub(KEEP_AROUND);
                let hi = centre + KEEP_AROUND;
                let mut near = Vec::new();
                for (i, p) in all.iter().take(shown).enumerate() {
                    if i >= lo && i <= hi {
                        near.push(p.clone());
                    } else if self.thumbs.remove(p).is_some() {
                        self.asked.remove(p);
                    }
                }
                return self.want_thumbs(near);
            }
            Message::Rotate(clockwise) => {
                let Some(path) = self.viewing.clone().filter(|p| !vault::is_vault_path(p)) else { return Task::none() };
                self.slideshow = false;
                self.notice = None;
                // One turn per picture at a time: a second press while the first
                // is being written would decode the old file and undo half of it.
                if !self.turning.insert(path.clone()) {
                    return Task::none();
                }
                return Task::future(async move {
                    let p = path.clone();
                    let done = tokio::task::spawn_blocking(move || rotate_locked(&p, clockwise))
                        .await
                        .unwrap_or_else(|_| Err("The picture could not be turned.".into()));
                    cosmic::Action::App(Message::Rotated(path, done))
                });
            }
            Message::EditToggle => {
                self.edit_open = !self.edit_open;
                self.slideshow = false;
            }
            Message::Edit(edit) => {
                let Some(path) = self.viewing.clone().filter(|p| !vault::is_vault_path(p)) else { return Task::none() };
                self.slideshow = false;
                self.notice = None;
                // Only a copy this session made is edited in place. A picture
                // that merely has "(edited)" in its name gets a new copy.
                let made = self.made.get(&photo_key(&path)).copied();
                return Task::future(async move {
                    let source = path.clone();
                    let done = tokio::task::spawn_blocking(move || edit_locked(&path, edit, made))
                        .await
                        .unwrap_or_else(|_| Err("The picture could not be edited.".into()));
                    cosmic::Action::App(Message::Edited(source, done))
                });
            }
            Message::Edited(source, done) => match done {
                Ok(saved) => {
                    self.notice = (!saved.durable).then(|| NOT_DURABLE.to_string());
                    let target = saved.path;
                    let key = photo_key(&target);
                    // The copy of a hidden picture was put on the hidden list
                    // before it was published (CWE-200), so there is no window
                    // in which it is listed in the open. Here the open window
                    // only catches up.
                    if self.hidden.contains(&photo_key(&source)) || self.hidden.contains(&source) {
                        self.hidden.insert(key.clone());
                    }
                    if let Some(id) = saved.id {
                        self.made.insert(key.clone(), id);
                    }
                    // A copy that is already listed was edited in place.
                    self.asked.remove(&target);
                    self.thumbs.remove(&target);
                    self.dims.remove(&target);
                    if !self.photos.iter().any(|p| p.path == target) {
                        let modified = std::fs::metadata(&target).and_then(|m| m.modified()).unwrap_or_else(|_| SystemTime::now());
                        self.photos.insert(0, Photo { path: target.clone(), key, modified });
                    }
                    let view = self.refresh_view();
                    let open = self.open(target);
                    return Task::batch([view, open]);
                }
                Err(e) => self.notice = Some(e),
            },
            Message::Rotated(path, done) => {
                self.turning.remove(&path);
                match done {
                Ok(saved) => {
                    self.notice = if saved.durable { None } else { Some(NOT_DURABLE.into()) };
                    // A turn swaps the file, so a copy made here has a new identity.
                    if let Some(id) = saved.id {
                        let key = photo_key(&path);
                        if self.made.contains_key(&key) {
                            self.made.insert(key, id);
                        }
                    }
                    tracing::debug!("turned {}", saved.path.display());
                    // A decode that started before the turn shows the old file: drop it.
                    self.epoch += 1;
                    self.thumb_epoch.remove(&path);
                    self.stale_since.insert(path.clone(), self.epoch);
                    if self.full.as_ref().is_some_and(|(p, _)| p == &path) {
                        self.full = None;
                    }
                    self.asked.remove(&path);
                    self.thumbs.remove(&path);
                    self.dims.remove(&path);
                    if self.viewing.as_ref() == Some(&path) {
                        return self.open(path);
                    }
                    return self.want_section_thumbs();
                }
                Err(e) => self.notice = Some(e),
                }
            }
            Message::SlideshowTick => {
                if !self.slideshow || self.viewing.is_none() {
                    return Task::none();
                }
                let list = self.viewer_list();
                let Some(current) = self.viewing.as_ref() else { return Task::none() };
                // Outside the section the open photo is the whole list: nothing to move on to.
                let Some(i) = list.iter().position(|p| &p.path == current) else { return Task::none() };
                let next = list[(i + 1) % list.len()].path.clone();
                // A single photo has nothing to move on to.
                if &next != current {
                    return self.open(next);
                }
            }
            Message::BackToGallery => {
                if self.viewing.is_some() {
                    return self.back();
                }
            }
            Message::ToggleMenu => {
                self.action_error = None;
                self.overlay = if self.overlay.is_some() { None } else { Some(Overlay::Menu) };
            }
            Message::StartRename => {
                if let Some(p) = &self.viewing {
                    self.overlay = Some(Overlay::Rename(file_name(p)));
                }
            }
            Message::RenameInput(t) => {
                if let Some(Overlay::Rename(cur)) = &mut self.overlay {
                    *cur = t;
                }
            }
            Message::RenameCommit => {
                let (Some(old), Some(Overlay::Rename(name))) = (self.viewing.clone(), self.overlay.clone()) else {
                    return Task::none();
                };
                let name = name.trim().to_string();
                if name.is_empty() || name.contains('/') || name == "." || name == ".." {
                    self.action_error = Some("Type a file name without slashes.".into());
                    return Task::none();
                }
                if name.starts_with('.') {
                    self.action_error = Some("Start the name with a letter or number. Names that begin with a dot are hidden from the gallery.".into());
                    return Task::none();
                }
                if !is_photo(Path::new(&name)) {
                    self.action_error = Some("Keep a picture extension such as .jpg or .png.".into());
                    return Task::none();
                }
                let new = old.with_file_name(&name);
                if new == old {
                    self.overlay = None;
                    return Task::none();
                }
                if self.file_op {
                    return Task::none();
                }
                self.file_op = true;
                self.action_error = None;
                return Task::future(async move {
                    let key = old.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        rename_no_replace(&old, &new).map(|()| new)
                    })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                    cosmic::Action::App(Message::Renamed(key, result))
                });
            }
            Message::Renamed(old, Err(e)) => {
                self.file_op = false;
                // Always a notice, never the viewer's action error: that one
                // is cleared by opening another photo, the menu or going back,
                // and a failed rename can mean a leftover second name. The
                // viewer shows notices too.
                let _ = old;
                self.notice = Some(e);
            }
            Message::Renamed(old, Ok(new)) => {
                self.file_op = false;
                return self.after_rename(old, new);
            }
            Message::AskDeleteOne => self.overlay = Some(Overlay::ConfirmDelete),
            Message::CancelOverlay => self.overlay = None,
            Message::ConfirmDeleteOne => {
                let Some(path) = self.viewing.clone() else { return Task::none() };
                self.overlay = None;
                // One file operation at a time: a rename in flight would
                // leave this path stale under the delete.
                if self.file_op {
                    return Task::none();
                }
                self.file_op = true;
                self.action_error = None;
                return Task::future(async move {
                    let key = path.clone();
                    // To the Recycle Bin, never a permanent delete.
                    let out = tokio::process::Command::new("gio").arg("trash").arg("--").arg(&path).output().await;
                    let result = match out {
                        Ok(o) if o.status.success() => Ok(()),
                        Ok(o) => Err(format!("Could not move it to the Recycle Bin: {}", String::from_utf8_lossy(&o.stderr).trim())),
                        Err(e) => Err(format!("Could not move it to the Recycle Bin: {e}")),
                    };
                    cosmic::Action::App(Message::Deleted(key, result))
                });
            }
            Message::Deleted(path, Err(e)) => {
                self.file_op = false;
                if self.viewing.as_deref() == Some(path.as_path()) {
                    self.action_error = Some(e);
                }
            }
            Message::Deleted(path, Ok(())) => {
                self.file_op = false;
                return self.after_delete(path);
            }
            Message::OpenLocation => {
                self.overlay = None;
                if let Some(dir) = self.viewing.as_deref().and_then(Path::parent).map(Path::to_path_buf) {
                    // Waited for in a task, so the finished helper is reaped and
                    // never lingers as a zombie while Photos stays open.
                    let visit = self.visit;
                    return Task::future(async move {
                        match tokio::process::Command::new("xdg-open").arg(&dir).status().await {
                            Ok(status) if status.success() => cosmic::Action::App(Message::LocationFailed(visit, String::new())),
                            Ok(status) => cosmic::Action::App(Message::LocationFailed(visit, format!("Could not open the folder ({status})."))),
                            Err(e) => cosmic::Action::App(Message::LocationFailed(visit, format!("Could not open the folder: {e}"))),
                        }
                    });
                }
            }
            Message::LocationFailed(visit, e) => {
                if visit == self.visit && !e.is_empty() {
                    self.action_error = Some(e);
                }
            }
            Message::Prev => return self.step(-1),
            Message::Next => return self.step(1),
            Message::ToggleFavorite(path) => {
                if vault::is_vault_path(&path) {
                    return Task::none();
                }
                // The key is read fresh, not from the scan: a link retargeted since
                // then must star (and unstar) the file it points to now.
                let key = photo_key(&path);
                let on = !self.favorites.contains(&key);
                // A full disk or a read-only home must not end the program (#174).
                match set_favorite(&key, on) {
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
            // Pictures inside the vault can only come out again: no favorites,
            // albums or Recycle Bin there.
            let in_vault = self.vault_view();
            if !in_vault {
                bar = bar.push(star);
            }
            bar = bar.push(hide);
            if n > 0 && !in_vault {
                bar = bar.push(add);
            }
            if self.section == Section::Album && n > 0 {
                bar = bar.push(button::icon(icon::from_name("list-remove-symbolic")).name("Remove from this album").on_press(Message::RemoveFromAlbum));
            }
            if !in_vault {
                bar = bar.push(bin);
            }
            bar = bar.push(button::standard("Done").on_press(Message::SelectMode(false)));
        } else {
            bar = bar.push(button::standard("Select").on_press(Message::SelectMode(true)));
        }

        let mut more = vec![Self::menu_item("Slideshow", Some("media-playback-start-symbolic"), false, Message::Slideshow), Self::menu_item("Look for new pictures", Some("view-refresh-symbolic"), false, Message::Refresh)];
        more.push(Self::menu_item(&format!("Slideshow speed: {} seconds", self.slide_secs), Some("preferences-system-time-symbolic"), false, Message::SlideSpeed));
        if self.section == Section::Hidden && !self.hidden_locked() {
            if self.vault_set {
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
        let mut col = column::with_capacity(4)
            .spacing(12)
            .align_x(Alignment::Center)
            .push(icon::from_name("system-lock-screen-symbolic").size(48));
        col = col
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
        col = col.push(text::caption(if self.vault_set { "The new password replaces the old one. If you forget it, the pictures cannot be opened again." } else { "Pictures you hide are encrypted with this password. If you forget it they cannot be opened again, and nobody can recover them for you." }));
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
            container(text::body(hidden_unreadable_msg())).center(Length::Fill).padding(24).into()
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
            let mut col = column::with_capacity(4).spacing(8).align_x(Alignment::Center);
            if self.favorites_unsaved {
                col = col.push(text::caption(FAVORITES_UNSAVED));
            }
            if self.albums_unsaved && self.new_album.is_none() {
                col = col.push(text::caption(ALBUMS_UNSAVED));
            }
            if let Some(notice) = &self.notice {
                col = col.push(text::caption(notice.as_str()));
            }
            container(col.push(text::body(msg))).center(Length::Fill).into()
        } else if self.size == REEL {
            // The reel has no grid to carry the status lines, so they go above it.
            let mut col = column::with_capacity(3).spacing(8).padding([8, 16, 0, 8]);
            if self.favorites_unsaved {
                col = col.push(text::caption(FAVORITES_UNSAVED));
            }
            if self.albums_unsaved && self.new_album.is_none() {
                col = col.push(container(text::caption(ALBUMS_UNSAVED)).padding([8, 16, 0, 8]));
            }
            if let Some(notice) = &self.notice {
                col = col.push(text::caption(notice.as_str()));
            }
            col.push(self.reel_view(&shown)).height(Length::Fill).into()
        } else {
            let mut col = column::with_capacity(8).spacing(8).padding([8, 16, 16, 8]);
            if self.favorites_unsaved {
                col = col.push(text::caption(FAVORITES_UNSAVED));
            }
            if self.albums_unsaved && self.new_album.is_none() {
                col = col.push(text::caption(ALBUMS_UNSAVED));
            }
            if let Some(notice) = &self.notice {
                col = col.push(text::caption(notice.as_str()));
            }
            let mut last: Option<String> = None;
            let mut tiles: Vec<Element<'_, Message>> = Vec::new();
            for photo in shown.into_iter().take(self.tile_limit) {
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
            scrollable(col).width(Length::Fill).height(Length::Fill).on_scroll(|v| Message::GridScrolled(v.relative_offset().y)).into()
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
        let mut strip = row::with_capacity(shown.len().min(self.tile_limit)).spacing(4);
        for photo in shown.iter().take(self.tile_limit) {
            strip = strip.push(self.tile(&photo.path, true));
        }
        let mut col = column::with_capacity(4).spacing(8).padding([8, 16, 16, 8]);
        if self.favorites_unsaved {
            col = col.push(text::caption(FAVORITES_UNSAVED));
        }
        col = col.push_maybe(self.notice.as_deref().map(text::caption));
        col.push(big)
            .push(
                scrollable::horizontal(strip)
                    .width(Length::Fill)
                    // Near the end of the strip, load the next tiles like the grid does.
                    .on_scroll(|v| Message::GridScrolled(v.relative_offset().x)),
            )
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
            Some(handle) => {
                // Square tiles crop to fill. Otherwise the tile follows the
                // picture's shape, and where the width is capped (a very wide or
                // very tall picture) the whole picture stays visible.
                let fit = if self.square || strip && !self.dims.contains_key(path) {
                    cosmic::iced::ContentFit::Cover
                } else {
                    cosmic::iced::ContentFit::Contain
                };
                widget::image(handle.clone()).content_fit(fit).width(w).height(h).into()
            }
            // A neutral backdrop, so an unloaded tile reads in light and dark.
            None => container(icon::from_name("image-x-generic-symbolic").size(32))
                .center(w)
                .height(h)
                .class(theme::Container::Card)
                .into(),
        };
        let mut tile = stack![picture];
        if self.is_favorite(path) {
            tile = tile.push(
                container(container(icon::from_name("starred-symbolic").size(16)).padding(3).class(theme::Container::Card))
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
        let vaulted = vault::is_vault_path(path);

        let mut bar = row::with_capacity(7)
            .spacing(4)
            .align_y(Alignment::Center)
            .push(
                button::standard("Back")
                    .leading_icon(icon::from_name("go-previous-symbolic"))
                    .name("Back to the gallery")
                    .on_press(Message::BackToGallery),
            )
            .push(container(text::body(file_name(path))).padding([0, 8]))
            .push(widget::Space::new().width(Length::Fill));
        if self.favorites_unsaved {
            bar = bar.push(text::caption(FAVORITES_UNSAVED));
        }
        let bar = bar
            .push(
                button::icon(icon::from_name("view-more-symbolic"))
                    .name("More actions")
                    .on_press(Message::ToggleMenu),
            )
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
            .push_maybe((!vaulted).then(|| {
                button::icon(icon::from_name("object-rotate-left-symbolic"))
                    .name("Turn left")
                    .on_press_maybe((!self.turning.contains(path)).then_some(Message::Rotate(false)))
            }))
            .push_maybe((!vaulted).then(|| {
                button::icon(icon::from_name("object-rotate-right-symbolic"))
                    .name("Turn right")
                    .on_press_maybe((!self.turning.contains(path)).then_some(Message::Rotate(true)))
            }))
            .push_maybe((!vaulted).then(|| {
                button::icon(icon::from_name("document-edit-symbolic"))
                    .name(if self.edit_open { "Close the edit buttons" } else { "Edit" })
                    .on_press(Message::EditToggle)
            }))
            .push(
                button::icon(icon::from_name(if self.fullscreen { "view-restore-symbolic" } else { "view-fullscreen-symbolic" }))
                    .name(if self.fullscreen { "Leave full screen" } else { "Full screen" })
                    .on_press(Message::Fullscreen),
            )
            .push_maybe((!vaulted).then(|| {
                button::icon(icon::from_name(if favorite { "starred-symbolic" } else { "non-starred-symbolic" }))
                    .name(if favorite { "Remove from favorites" } else { "Add to favorites" })
                    .on_press(Message::ToggleFavorite(path.to_path_buf()))
            }));
        let toolbar = container(container(bar).padding([4, 8]).class(theme::Container::Card))
            .padding([12, 12, 0, 12])
            .width(Length::Fill);

        // The menu and its follow-up questions sit in their own row under the bar.
        let panel: Option<Element<'_, Message>> = match &self.overlay {
            None => None,
            Some(Overlay::Menu) => Some(
                row::with_capacity(3)
                    .spacing(8)
                    .push(button::standard("Rename").name("Rename").on_press(Message::StartRename))
                    .push(button::destructive("Delete").name("Delete").on_press(Message::AskDeleteOne))
                    .push(button::standard("Open file location").name("Open file location").on_press(Message::OpenLocation))
                    .into(),
            ),
            Some(Overlay::Rename(name)) => Some(
                row::with_capacity(3)
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .push(
                        widget::text_input("File name", name.clone())
                            .on_input(Message::RenameInput)
                            .on_submit(|_| Message::RenameCommit)
                            .width(Length::Fixed(320.0)),
                    )
                    .push(button::suggested("Rename").name("Rename").on_press(Message::RenameCommit))
                    .push(button::standard("Cancel").name("Cancel").on_press(Message::CancelOverlay))
                    .into(),
            ),
            Some(Overlay::ConfirmDelete) => Some(
                row::with_capacity(3)
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .push(text::body(format!("Move {} to the Recycle Bin?", file_name(path))))
                    .push(button::destructive("Delete").name("Delete").on_press(Message::ConfirmDeleteOne))
                    .push(button::standard("Cancel").name("Cancel").on_press(Message::CancelOverlay))
                    .into(),
            ),
        };

        let edit_panel: Option<Element<'_, Message>> = (self.edit_open && !vaulted).then(|| {
            let panel = row::with_capacity(9)
                .spacing(6)
                .align_y(Alignment::Center)
                .push(button::standard("Brighter").on_press(Message::Edit(Edit::Brighter)))
                .push(button::standard("Darker").on_press(Message::Edit(Edit::Darker)))
                .push(button::standard("More contrast").on_press(Message::Edit(Edit::MoreContrast)))
                .push(button::standard("Less contrast").on_press(Message::Edit(Edit::LessContrast)))
                .push(button::standard("Black and white").on_press(Message::Edit(Edit::BlackWhite)))
                .push(button::standard("Crop square").on_press(Message::Edit(Edit::CropSquare)))
                .push(button::standard("Crop 16:9").on_press(Message::Edit(Edit::Crop169)))
                .push(button::standard("Crop 4:3").on_press(Message::Edit(Edit::Crop43)))
                .push(text::caption("Saved as a copy. The original stays as it is."));
            container(panel).padding([4, 8]).class(theme::Container::Card).into()
        });

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
            .align_x(Alignment::Center);

        // Bars take their own space, so the picture is fitted between them and
        // never drawn under them.
        let mut page = column::with_capacity(6).push(toolbar);
        if let Some(panel) = panel {
            page = page.push(container(panel).padding([8, 12, 0, 12]));
        }
        if let Some(edit) = edit_panel {
            page = page.push(container(edit).padding([8, 12, 0, 12]));
        }
        if let Some(e) = &self.action_error {
            page = page.push(container(text::caption(e.clone())).padding([4, 12, 0, 12]));
        }
        // Rename warnings (a leftover second name, a star that did not move)
        // stay visible in the viewer too, also after another photo is opened.
        if let Some(n) = &self.notice {
            page = page.push(container(text::caption(n.clone())).padding([4, 12, 0, 12]));
        }
        let page = page
            .push(container(picture).padding(8).width(Length::Fill).height(Length::Fill))
            .push(filmstrip);
        lapin_ux_ui::a11y::group("Photo viewer", page)
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
mod edit_tests {
    use super::*;

    fn grey(w: u32, h: u32, v: u8) -> image::DynamicImage {
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([v, v, v])))
    }

    #[test]
    fn crops_keep_the_middle_and_the_shape() {
        let sq = edit_image(grey(400, 200, 100), Edit::CropSquare);
        assert_eq!((sq.width(), sq.height()), (200, 200));
        let wide = edit_image(grey(300, 300, 100), Edit::Crop169);
        assert_eq!((wide.width(), wide.height()), (300, 168));
        let tall = edit_image(grey(100, 400, 100), Edit::Crop43);
        assert_eq!((tall.width(), tall.height()), (100, 75));
        let tiny = edit_image(grey(1, 1, 100), Edit::Crop169);
        assert!(tiny.width() >= 1 && tiny.height() >= 1);
    }

    #[test]
    fn brightness_moves_the_right_way() {
        let up = edit_image(grey(2, 2, 100), Edit::Brighter).to_rgb8();
        let down = edit_image(grey(2, 2, 100), Edit::Darker).to_rgb8();
        assert!(up.get_pixel(0, 0)[0] > 100 && down.get_pixel(0, 0)[0] < 100);
    }

    #[test]
    fn copies_never_replace_other_files() {
        let p = Path::new("/a/b/cat.jpg");
        assert_eq!(edit_target(p, false, |_| false), Path::new("/a/b/cat (edited).jpg"));
        let taken = |c: &Path| c == Path::new("/a/b/cat (edited).jpg");
        assert_eq!(edit_target(p, false, taken), Path::new("/a/b/cat (edited 2).jpg"));
        // Only a copy this session made is edited in place.
        let copy = Path::new("/a/b/cat (edited).jpg");
        assert_eq!(edit_target(copy, true, |_| true), copy);
        // An imported file that merely has such a name is never overwritten.
        assert_eq!(edit_target(copy, false, |c| c == copy), Path::new("/a/b/cat (edited) (edited).jpg"));
        let odd = Path::new("/a/b/cat (edited x).jpg");
        assert_eq!(edit_target(odd, false, |_| false), Path::new("/a/b/cat (edited x) (edited).jpg"));
    }
}

