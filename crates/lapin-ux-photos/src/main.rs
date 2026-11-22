// SPDX-License-Identifier: GPL-3.0-only
//! Photos (#74): an offline picture app. The front end is a dark Gallery (all
//! photos by date, Favorites, and your picture folders); opening a photo, or
//! clicking an image in Files, shows it in a light single-image viewer with a
//! floating toolbar and a filmstrip. Nothing leaves the computer: no cloud, no
//! accounts, no network.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cosmic::app::{Settings, Task};
use cosmic::iced::keyboard::{Event as KeyEvent, Key, key::Named};
use cosmic::iced::widget::{stack, themer};
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
    Scanned(Vec<Photo>),
    Section(Section),
    Thumb(PathBuf, Option<Handle>),
    Open(PathBuf),
    Full(PathBuf, Option<Handle>),
    Back,
    Prev,
    Next,
    ToggleFavorite(PathBuf),
}

struct App {
    core: Core,
    photos: Vec<Photo>,
    scanning: bool,
    section: Section,
    thumbs: HashMap<PathBuf, Handle>,
    /// Tiles asked for already (loaded, loading or failed), so none is decoded twice.
    asked: HashSet<PathBuf>,
    favorites: HashSet<PathBuf>,
    /// The photo shown in the viewer, as a path (the list can change under it).
    viewing: Option<PathBuf>,
    full: Option<(PathBuf, Handle)>,
    /// The picture to open at start, from the command line or Files.
    start: Option<PathBuf>,
}

fn favorites_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_ID).join("favorites.json"))
}

fn load_favorites() -> HashSet<PathBuf> {
    let Some(file) = favorites_file() else { return HashSet::new() };
    std::fs::read_to_string(file)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<PathBuf>>(&s).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

fn save_favorites(set: &HashSet<PathBuf>) {
    let Some(file) = favorites_file() else { return };
    let mut list: Vec<&PathBuf> = set.iter().collect();
    list.sort();
    // A full disk or a read-only home must not end the program (#174).
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&file, serde_json::to_vec_pretty(&list).unwrap_or_default())
    })();
    if let Err(e) = result {
        tracing::warn!("could not save favorites: {e}");
    }
}

fn is_photo(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Every picture under the usual folders, newest first. Hidden folders are skipped,
/// and the walk is bounded so a huge Documents tree cannot stall the app.
fn scan() -> Vec<Photo> {
    let mut found: Vec<Photo> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for section in [Section::Pictures, Section::Downloads, Section::Desktop, Section::Documents] {
        let Some(root) = section.dir() else { continue };
        let mut stack = vec![(root, 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let hidden = entry.file_name().to_string_lossy().starts_with('.');
                let Ok(kind) = entry.file_type() else { continue };
                if kind.is_dir() {
                    if !hidden && depth < MAX_DEPTH {
                        stack.push((path, depth + 1));
                    }
                } else if !hidden && is_photo(&path) && seen.insert(path.clone()) {
                    let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                    found.push(Photo { path, modified });
                }
                if found.len() >= MAX_PHOTOS {
                    break;
                }
            }
        }
    }
    found.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
    found
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

    fn open(&mut self, path: PathBuf) -> Task<Message> {
        self.viewing = Some(path.clone());
        self.full = None;
        let near = self.neighbours(&path, 6);
        let thumbs = self.want_thumbs(near);
        let name = file_name(&path);
        let title = match self.core.main_window_id() {
            Some(id) => self.set_window_title(format!("{name} - Photos"), id),
            None => Task::none(),
        };
        let full = Task::future(async move {
            let key = path.clone();
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

    fn back(&mut self) -> Task<Message> {
        self.viewing = None;
        self.full = None;
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
            section: Section::Gallery,
            thumbs: HashMap::new(),
            asked: HashSet::new(),
            favorites: load_favorites(),
            viewing: None,
            full: None,
            start,
        };
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Photos".into(), id),
            None => Task::none(),
        };
        let scan = Task::future(async {
            let photos = tokio::task::spawn_blocking(scan).await.unwrap_or_default();
            cosmic::Action::App(Message::Scanned(photos))
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
            Message::Scanned(photos) => {
                self.photos = photos;
                self.scanning = false;
                let tiles = self.want_section_thumbs();
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
                return Task::batch([tiles, open]);
            }
            Message::Section(s) => {
                self.section = s;
                return self.want_section_thumbs();
            }
            Message::Thumb(path, handle) => {
                if let Some(h) = handle {
                    self.thumbs.insert(path, h);
                }
            }
            Message::Open(path) => return self.open(path),
            Message::Full(path, handle) => {
                if self.viewing.as_deref() == Some(path.as_path()) {
                    self.full = handle.map(|h| (path, h));
                }
            }
            Message::Back => {
                if self.viewing.is_some() {
                    return self.back();
                }
            }
            Message::Prev => return self.step(-1),
            Message::Next => return self.step(1),
            Message::ToggleFavorite(path) => {
                if !self.favorites.remove(&path) {
                    self.favorites.insert(path);
                }
                save_favorites(&self.favorites);
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        match &self.viewing {
            Some(path) => themer(Some(cosmic::Theme::light()), container(self.viewer(path)).class(theme::Container::Background).width(Length::Fill).height(Length::Fill)).into(),
            None => themer(Some(cosmic::Theme::dark()), container(self.gallery()).class(theme::Container::Background).width(Length::Fill).height(Length::Fill)).into(),
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
            let msg = match self.section {
                Section::Favorites => "No favorites yet. Open a photo and press the star.",
                _ => "No pictures here yet.",
            };
            container(text::body(msg)).center(Length::Fill).into()
        } else {
            let mut col = column::with_capacity(8).spacing(8).padding([8, 16, 16, 8]);
            let mut last = String::new();
            let mut tiles: Vec<Element<'_, Message>> = Vec::new();
            for photo in shown {
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
            None => container(icon::from_name("image-x-generic-symbolic").size(32)).center(side).height(side).into(),
        };
        let mut tile = stack![picture];
        if self.favorites.contains(path) {
            tile = tile.push(
                container(icon::from_name("starred-symbolic").size(16))
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
            _ => container(text::body("Opening...")).center(Length::Fill).into(),
        };

        let list = self.viewer_list();
        let index = list.iter().position(|p| p.path == path);
        let favorite = self.favorites.contains(path);

        let bar = row::with_capacity(6)
            .spacing(4)
            .align_y(Alignment::Center)
            .push(button::icon(icon::from_name("go-previous-symbolic")).name("Back to the gallery").on_press(Message::Back))
            .push(container(text::body(file_name(path))).padding([0, 8]))
            .push(widget::Space::new().width(Length::Fill))
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
