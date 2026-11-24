// SPDX-License-Identifier: GPL-3.0-only
//! Photos (#74): an offline picture app. The front end is a Gallery (all
//! photos by date, Favorites, and your picture folders); opening a photo, or
//! clicking an image in Files, shows it in a single-image viewer with a toolbar
//! and a filmstrip. Both follow the system theme (#200). Nothing leaves the
//! computer: no cloud, no accounts, no network.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cosmic::app::{Settings, Task};
use cosmic::iced::keyboard::{Event as KeyEvent, Key, key::Named};
use cosmic::iced::widget::stack;
use cosmic::iced::{Alignment, Length, Size, Subscription};
use cosmic::widget::image::Handle;
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text};
use cosmic::{ApplicationExt, Core, Element, theme};
use futures::StreamExt;

const APP_ID: &str = "dev.lapinux.Photos";
/// Side of a square tile in the grid and of a filmstrip picture.
const TILE: u32 = 170;
/// How many tiles are loaded in one go. The grid is not virtualised yet, so this
/// keeps the memory use (about 115 KB per tile) predictable.
const MAX_TILES: usize = 400;
/// Pictures are shown at most this wide or tall. Bigger ones are scaled down once.
const MAX_SIDE: u32 = 3840;
const MAX_PHOTOS: usize = 20_000;
const MAX_DEPTH: usize = 5;
const EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "webp", "bmp", "gif", "jpe"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Gallery,
    Favorites,
    Pictures,
    Downloads,
    Desktop,
    Documents,
}

impl Section {
    const ALL: [Section; 6] = [
        Section::Gallery,
        Section::Favorites,
        Section::Pictures,
        Section::Downloads,
        Section::Desktop,
        Section::Documents,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::Gallery => "Gallery",
            Section::Favorites => "Favorites",
            Section::Pictures => "Pictures",
            Section::Downloads => "Downloads",
            Section::Desktop => "Desktop",
            Section::Documents => "Documents",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Gallery => "folder-pictures-symbolic",
            Section::Favorites => "starred-symbolic",
            Section::Pictures => "folder-pictures-symbolic",
            Section::Downloads => "folder-download-symbolic",
            Section::Desktop => "user-desktop-symbolic",
            Section::Documents => "folder-documents-symbolic",
        }
    }

    /// The folder this section shows, if it is one.
    fn dir(self) -> Option<PathBuf> {
        match self {
            Section::Pictures => dirs::picture_dir(),
            Section::Downloads => dirs::download_dir(),
            Section::Desktop => dirs::desktop_dir(),
            Section::Documents => dirs::document_dir(),
            Section::Gallery | Section::Favorites => None,
        }
    }
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
    Thumb(PathBuf, u64, Option<Handle>),
    Open(PathBuf),
    Full(PathBuf, Option<Handle>),
    /// A full-size decode that was skipped because the picture is no longer open.
    Ignore,
    Retry,
    Back,
    /// The "Back to the gallery" button: always leaves the viewer, even with a menu open.
    BackToGallery,
    Prev,
    Next,
    ToggleFavorite(PathBuf),
    /// Open the picture in the snip editor (#78).
    Edit(PathBuf),
    /// Opens or closes the "More actions" menu.
    ToggleMenu,
    StartRename,
    RenameInput(String),
    RenameCommit,
    AskDelete,
    CancelOverlay,
    ConfirmDelete,
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
    thumbs: HashMap<PathBuf, Handle>,
    /// Tiles asked for already (loaded, loading or failed), so none is decoded twice.
    asked: HashSet<PathBuf>,
    /// Which request each tile is waiting for. A result for a name that was
    /// renamed, deleted or asked again since is stale and dropped.
    thumb_epoch: HashMap<PathBuf, u64>,
    next_epoch: u64,
    /// Counts viewer visits, so a late "could not open the folder" from an
    /// earlier photo never shows on the current one.
    visit: u64,
    /// A rename or delete is running; the other is refused until it has finished.
    file_op: bool,
    favorites: HashSet<PathBuf>,
    /// The last favorites change could not be saved, so the stars show what is on disk.
    favorites_unsaved: bool,
    /// The photo shown in the viewer, as a path (the list can change under it).
    viewing: Option<PathBuf>,
    full: Option<(PathBuf, Handle)>,
    /// The picture to open at start, from the command line or Files.
    start: Option<PathBuf>,
    overlay: Option<Overlay>,
    /// What the last rename or delete could not do.
    action_error: Option<String>,
}

fn favorites_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_ID).join("favorites.json"))
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
    let tmp = dir.join(format!("favorites.json.{}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, file));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Stars or unstars one photo and returns the list as saved. The list is read
/// again under a lock first, so two Photos windows never undo each other's change.
fn set_favorite(path: &Path, on: bool) -> std::io::Result<HashSet<PathBuf>> {
    edit_favorites(|set| {
        if on {
            set.insert(path.to_path_buf());
        } else {
            set.remove(path);
        }
    })
}

/// Moves a star to a new name in one locked read-modify-write, so a failure
/// never leaves both names starred or neither.
fn move_favorite(old: &Path, new: &Path) -> std::io::Result<HashSet<PathBuf>> {
    edit_favorites(|set| {
        if set.remove(old) {
            set.insert(new.to_path_buf());
        }
    })
}

fn edit_favorites(change: impl FnOnce(&mut HashSet<PathBuf>)) -> std::io::Result<HashSet<PathBuf>> {
    let file = favorites_file().ok_or_else(|| std::io::Error::other("no config folder"))?;
    let dir = file.parent().ok_or_else(|| std::io::Error::other("favorites file has no folder"))?;
    std::fs::create_dir_all(dir)?;
    let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("favorites.lock"))?;
    lock.lock()?; // released when `lock` is dropped
    let mut set = read_favorites(&file)?;
    change(&mut set);
    write_favorites(&file, &set)?;
    Ok(set)
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
    'walk: for section in [Section::Pictures, Section::Downloads, Section::Desktop, Section::Documents] {
        let Some(root) = section.dir() else { continue };
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

fn load_thumb(path: PathBuf) -> Option<Handle> {
    let img = decode(&path)?;
    Some(handle(img.resize_to_fill(TILE, TILE, image::imageops::FilterType::Triangle)))
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

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl App {
    /// The photos the current section shows, newest first.
    fn shown(&self) -> Vec<&Photo> {
        let root = self.section.dir();
        self.photos
            .iter()
            .filter(|p| match self.section {
                Section::Gallery => true,
                Section::Favorites => self.favorites.contains(&p.path),
                _ => root.as_ref().is_some_and(|r| p.path.starts_with(r)),
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
        let paths: Vec<PathBuf> = self.shown().into_iter().take(MAX_TILES).map(|p| p.path.clone()).collect();
        self.want_thumbs(paths)
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
        let full = Task::future(async move {
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
        });
        Task::batch([thumbs, full, title])
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

    fn step(&mut self, delta: isize) -> Task<Message> {
        let Some(current) = self.viewing.clone() else { return Task::none() };
        let list = self.viewer_list();
        let Some(i) = list.iter().position(|p| p.path == current) else { return Task::none() };
        let j = i as isize + delta;
        if j < 0 || j as usize >= list.len() {
            return Task::none();
        }
        let next = list[j as usize].path.clone();
        self.open(next)
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
        self.visit += 1;
        self.overlay = None;
        self.action_error = None;
        self.viewing = None;
        set_viewing(None);
        self.full = None;
        self.failed = None;
        match self.core.main_window_id() {
            Some(id) => self.set_window_title("Photos".into(), id),
            None => Task::none(),
        }
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
        let mut app = App {
            core,
            photos: Vec::new(),
            scanning: true,
            notice: None,
            read_errors: false,
            favorites_unsaved: false,
            failed: None,
            section: Section::Gallery,
            thumbs: HashMap::new(),
            asked: HashSet::new(),
            thumb_epoch: HashMap::new(),
            next_epoch: 0,
            visit: 0,
            file_op: false,
            favorites: load_favorites(),
            viewing: None,
            full: None,
            start,
            overlay: None,
            action_error: None,
        };
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Photos".into(), id),
            None => Task::none(),
        };
        let starred = app.favorites.clone();
        let scan = Task::future(async move {
            let found = tokio::task::spawn_blocking(move || scan(starred)).await.unwrap_or_default();
            cosmic::Action::App(Message::Scanned(found))
        });
        (app, Task::batch([title, scan]))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![lapin_ux_ui::title_bar("folder-pictures", "Photos")]
    }

    fn subscription(&self) -> Subscription<Message> {
        cosmic::iced::event::listen_with(|event, status, _| {
            if status != cosmic::iced::event::Status::Ignored {
                return None;
            }
            match event {
                cosmic::iced::Event::Keyboard(KeyEvent::KeyPressed { key: Key::Named(named), .. }) => match named {
                    Named::ArrowLeft => Some(Message::Prev),
                    Named::ArrowRight => Some(Message::Next),
                    Named::Escape => Some(Message::Back),
                    _ => None,
                },
                _ => None,
            }
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
                let tiles = self.want_section_thumbs();
                // A picture handed over by Files opens straight in the viewer.
                let open = match self.start.take() {
                    Some(p) if is_photo(&p) && p.is_file() => {
                        let p = p.canonicalize().unwrap_or(p);
                        if !self.photos.iter().any(|x| x.path == p) {
                            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                            self.photos.insert(0, Photo { path: p.clone(), modified });
                        }
                        self.open(p)
                    }
                    _ => Task::none(),
                };
                return Task::batch([tiles, open]);
            }
            Message::Section(s) => {
                self.section = s;
                return self.want_section_thumbs();
            }
            Message::Thumb(path, epoch, handle) => {
                if self.thumb_epoch.get(&path) != Some(&epoch) {
                    return Task::none();
                }
                if let Some(h) = handle {
                    self.thumbs.insert(path, h);
                }
            }
            Message::Open(path) => return self.open(path),
            Message::Full(path, handle) => {
                if self.viewing.as_deref() == Some(path.as_path()) {
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
            }
            Message::Back => {
                // Escape closes the menu first, then the viewer.
                if self.overlay.take().is_some() {
                    return Task::none();
                }
                if self.viewing.is_some() {
                    return self.back();
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
            Message::AskDelete => self.overlay = Some(Overlay::ConfirmDelete),
            Message::CancelOverlay => self.overlay = None,
            Message::ConfirmDelete => {
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
            Message::Edit(path) => {
                // The editor is its own app, so Photos never holds the picture open for it.
                if std::process::Command::new("lapin-ux-snip-editor").arg(&path).spawn().is_err() {
                    self.notice = Some("The editor could not be opened.".into());
                }
            }
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
        match &self.viewing {
            Some(path) => container(self.viewer(path)).class(theme::Container::Background).width(Length::Fill).height(Length::Fill).into(),
            None => container(self.gallery()).class(theme::Container::Background).width(Length::Fill).height(Length::Fill).into(),
        }
    }
}

impl App {
    fn gallery(&self) -> Element<'_, Message> {
        let mut nav = column::with_capacity(Section::ALL.len() + 2).spacing(2);
        for s in Section::ALL {
            // The folders only appear when they exist.
            if s.dir().is_some_and(|d| !d.is_dir()) {
                continue;
            }
            if s == Section::Pictures {
                nav = nav.push(widget::divider::horizontal::default());
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
            let mut last = String::new();
            let mut tiles: Vec<Element<'_, Message>> = Vec::new();
            for photo in shown.into_iter().take(MAX_TILES) {
                let heading = date_heading(photo.modified);
                if heading != last {
                    if !tiles.is_empty() {
                        col = col.push(widget::flex_row(std::mem::take(&mut tiles)).column_spacing(4).row_spacing(4));
                    }
                    col = col.push(container(text::heading(heading.clone())).padding([12, 0, 4, 0]));
                    last = heading;
                }
                tiles.push(self.tile(&photo.path));
            }
            if !tiles.is_empty() {
                col = col.push(widget::flex_row(tiles).column_spacing(4).row_spacing(4));
            }
            scrollable(col).width(Length::Fill).height(Length::Fill).into()
        };

        lapin_ux_ui::a11y::group("Gallery", row::with_capacity(2).push(sidebar).push(body).height(Length::Fill))
    }

    fn tile(&self, path: &Path) -> Element<'_, Message> {
        let side = Length::Fixed(TILE as f32);
        let picture: Element<'_, Message> = match self.thumbs.get(path) {
            Some(h) => widget::image(h.clone()).content_fit(cosmic::iced::ContentFit::Cover).width(side).height(side).into(),
            // A neutral backdrop, so an unloaded tile reads in light and dark.
            None => container(icon::from_name("image-x-generic-symbolic").size(32))
                .center(side)
                .height(side)
                .class(theme::Container::Card)
                .into(),
        };
        let mut tile = stack![picture];
        if self.favorites.contains(path) {
            tile = tile.push(
                container(container(icon::from_name("starred-symbolic").size(16)).padding(3).class(theme::Container::Card))
                    .padding(6)
                    .align_x(Alignment::End)
                    .align_y(Alignment::End)
                    .width(side)
                    .height(side),
            );
        }
        button::custom(tile)
            .padding(0)
            .class(theme::Button::Image)
            .name(file_name(path))
            .on_press(Message::Open(path.to_path_buf()))
            .into()
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

        let mut bar = row::with_capacity(8)
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
        if let Some(n) = &self.notice {
            bar = bar.push(text::caption(n.clone()));
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
                button::icon(icon::from_name("document-edit-symbolic"))
                    .name("Edit")
                    .on_press(Message::Edit(path.to_path_buf())),
            )
            .push(
                button::icon(icon::from_name(if favorite { "starred-symbolic" } else { "non-starred-symbolic" }))
                    .name(if favorite { "Remove from favorites" } else { "Add to favorites" })
                    .on_press(Message::ToggleFavorite(path.to_path_buf())),
            );
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
                    .push(button::destructive("Delete").name("Delete").on_press(Message::AskDelete))
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
                    .push(button::destructive("Delete").name("Delete").on_press(Message::ConfirmDelete))
                    .push(button::standard("Cancel").name("Cancel").on_press(Message::CancelOverlay))
                    .into(),
            ),
        };

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
