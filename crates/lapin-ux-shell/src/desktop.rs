// SPDX-License-Identifier: GPL-3.0-only
//! Desktop icons: the contents of ~/Desktop on the main monitor, above the
//! wallpaper and below windows. Click selects, double-click opens, drag
//! moves an icon to another grid cell, right-click opens a menu, F2 renames,
//! Delete moves to the trash. Right-click on empty space offers view and
//! sort options, New folder / text document, and settings shortcuts.
//! Ctrl/Shift-click and dragging a box select several icons; Ctrl+X/C/V
//! cut, copy and paste files; icons drag out into apps (and files drag in),
//! onto folders, and onto the Recycle Bin.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

use cosmic::app::Task;
use cosmic::iced::keyboard::key::Named;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{IcedMargin, IcedOutput, SctkLayerSurfaceSettings};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer, destroy_layer_surface, get_layer_surface,
};
use cosmic::iced::clipboard::dnd::DndAction;
use cosmic::iced::{Alignment, Background, Border, Color, Length, Point, Rectangle, window};
use cosmic::widget::{self, column, container, icon, mouse_area, row, text};
use cosmic::{Application, Element, theme};
use lapin_ux_core::desktop::{self as model, Cell, DesktopItem, SortBy};
use cosmic::cctk::wayland_client::protocol::wl_output::WlOutput;

use crate::app::{Message, PopupKind, Shell, rect_key};
use crate::config::{DesktopSlot, IconSize};
use crate::taskbar_view::{menu_divider, menu_item, panel};

static RENAME_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("lapin-ux-desktop-rename"));

const MARGIN: f32 = 8.0;
/// The Recycle Bin's place in the icon list (never a real file name).
const TRASH_NAME: &str = "::recycle-bin";

/// The user's Documents folder (xdg-user-dirs, else ~/Documents).
fn dirs_documents() -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let cfg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    std::fs::read_to_string(cfg.join("user-dirs.dirs"))
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| l.strip_prefix("XDG_DOCUMENTS_DIR=").map(|v| v.trim_matches('"').replace("$HOME", &home.to_string_lossy())))
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("Documents"))
}

fn trash_path() -> PathBuf {
    PathBuf::from("trash:///")
}

fn trash_shown(full: bool) -> Shown {
    Shown {
        item: DesktopItem {
            path: trash_path(),
            file_name: TRASH_NAME.into(),
            is_dir: true,
            launcher: false,
            modified: None,
            size: 0,
        },
        label: "Recycle Bin".into(),
        look: Look::Named(if full { "user-trash-full".into() } else { "user-trash".into() }),
        launch: None,
    }
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/share"))
}

fn trash_count() -> usize {
    std::fs::read_dir(data_home().join("Trash/files")).map(|d| d.count()).unwrap_or(0)
}

/// How an icon is drawn.
#[derive(Debug, Clone)]
enum Look {
    Named(String),
    Image(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Shown {
    pub item: DesktopItem,
    label: String,
    look: Look,
    /// For launchers: the catalog id it matches, or its own Exec line.
    launch: Option<Launch>,
}

#[derive(Debug, Clone)]
enum Launch {
    App(String),
    Exec { exec: String, terminal: bool, trusted: bool },
}

pub struct DesktopView {
    pub id: window::Id,
    pub output: WlOutput,
    pub width: f32,
    pub height: f32,
    pub dir: PathBuf,
    items: Vec<Shown>,
    stamp: Option<SystemTime>,
    pub selected: HashSet<PathBuf>,
    rename: Option<(PathBuf, String)>,
    drag: Option<Drag>,
    pointer: Point,
    /// Box selection: where the drag started.
    band: Option<Point>,
    /// Shift-click selects from here.
    anchor: Option<PathBuf>,
    /// A click on an icon that's part of a multi-selection: it becomes the
    /// only selected icon on release, unless the click turned into a drag.
    press_keep: Option<PathBuf>,
    /// Where a drag is hovering over the desktop.
    drop_at: Option<Point>,
    trash_full: bool,
    trash_stamp: Option<SystemTime>,
    /// The small text box shown over an icon's label while renaming.
    pub rename_surface: Option<window::Id>,
    rename_cell: Cell,
    /// A right-click on an icon also reaches the desktop behind it; this
    /// keeps the second one from replacing the icon's menu.
    item_context_at: std::time::Instant,
}

/// One of our icons being dragged (drag-and-drop).
struct Drag {
    path: PathBuf,
    from: Cell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuPage {
    Main,
    View,
    Sort,
    New,
    /// An icon's "Open with" list.
    OpenWith,
    /// An icon's "Send to" list.
    SendTo,
    /// "Show more options": the classic, longer icon menu.
    More,
}

#[derive(Debug, Clone)]
pub enum DesktopMsg {
    Press(PathBuf),
    Open(PathBuf),
    /// Right-click; `Some` on an icon.
    Context(Option<PathBuf>),
    Menu(MenuPage),
    Empty,
    Move(Point),
    Release,
    /// Drag-and-drop from an icon started / ended.
    DragStart(PathBuf),
    DragEnd,
    DropMotion(Point),
    DropLeave,
    Dropped(Option<crate::dnd::Uris>, DndAction),
    /// Ctrl + a letter.
    Ctrl(String),
    Cut,
    Copy,
    /// Paste finished: new files, and where to put them.
    Pasted(Vec<PathBuf>, Option<Cell>),
    EmptyTrashAsk,
    EmptyTrash,
    Key(Named),
    RenameStart,
    /// Open the rename box (delayed until a menu has closed).
    ShowRenameBox,
    RenameInput(String),
    RenameCommit,
    Trash,
    CopyPath,
    AllowLaunching,
    NewFolder,
    NewText,
    Refresh,
    Sort(SortBy),
    Size(IconSize),
    ToggleIcons,
    ToggleAutoArrange,
    Paste,
    Settings(&'static str),
    Terminal,
    Properties,
    Compress,
    OpenWithApp(String),
    CreateShortcut,
    SendToDocuments,
    ToggleSpecial(&'static str),
}

/// Extra desktop icons (Desktop icon settings), in Windows' order.
pub const SPECIAL: [(&str, &str, &str); 4] = [
    ("this-pc", "This PC", "computer"),
    ("user", "", "user-home"),
    ("network", "Network", "network-workgroup"),
    ("control", "Control Panel", "preferences-system"),
];

fn special_path(id: &str) -> PathBuf {
    PathBuf::from(format!("lapin-desktop:///{id}"))
}

fn special_id(path: &Path) -> Option<&str> {
    path.to_str()?.strip_prefix("lapin-desktop:///")
}

/// Apps that can open `path` (its MIME type from the extension).
pub fn open_with_apps(shell: &Shell, path: &Path) -> Vec<String> {
    let mime = if path.is_dir() { "inode/directory".to_string() } else { mime_guess::from_path(path).first_raw().unwrap_or("text/plain").to_string() };
    let mut ids: Vec<String> = shell
        .catalog
        .apps()
        .iter()
        .filter(|a| a.data.mime_types.iter().any(|m| m.essence_str() == mime))
        .map(|a| a.data.id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

impl IconSize {
    /// Cell width, cell height, icon size.
    fn metrics(self) -> (f32, f32, u16) {
        match self {
            IconSize::Small => (76.0, 76.0, 32),
            IconSize::Medium => (92.0, 100.0, 48),
            IconSize::Large => (120.0, 132.0, 72),
        }
    }
}

fn is_image(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
}

impl Shell {
    fn grid(&self) -> (u32, u32) {
        let Some(d) = &self.desktop else { return (1, 1) };
        let (cw, ch, _) = self.config.desktop_icon_size.metrics();
        (((d.width - 2.0 * MARGIN) / cw).floor().max(1.0) as u32, ((d.height - 2.0 * MARGIN) / ch).floor().max(1.0) as u32)
    }

    /// Create the desktop surface on the first monitor.
    pub(crate) fn create_desktop(&mut self, output: WlOutput, size: (u32, u32)) -> Task<Message> {
        if self.desktop.is_some() {
            return Task::none();
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        let dir = model::desktop_dir(&home, &config);
        let _ = std::fs::create_dir_all(&dir);
        let id = window::Id::unique();
        self.desktop = Some(DesktopView {
            id,
            output: output.clone(),
            width: size.0 as f32,
            // The taskbar reserves its height; the surface gets the rest.
            height: size.1.saturating_sub(crate::style::taskbar_height()) as f32,
            dir,
            items: Vec::new(),
            stamp: None,
            selected: HashSet::new(),
            rename: None,
            drag: None,
            pointer: Point::ORIGIN,
            band: None,
            anchor: None,
            press_keep: None,
            drop_at: None,
            trash_full: false,
            trash_stamp: None,
            rename_surface: None,
            rename_cell: (0, 0),
            item_context_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
        });
        self.reload_desktop();
        get_layer_surface(SctkLayerSurfaceSettings {
            id,
            layer: Layer::Bottom,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            output: IcedOutput::Output(output),
            namespace: "lapin-ux-desktop".into(),
            size: Some((None, None)),
            exclusive_zone: 0,
            ..Default::default()
        })
    }

    pub(crate) fn destroy_desktop(&mut self) -> Task<Message> {
        match self.desktop.take() {
            Some(d) => destroy_layer_surface(d.id),
            None => Task::none(),
        }
    }

    pub(crate) fn desktop_resized(&mut self, output: &WlOutput, size: (u32, u32)) {
        if let Some(d) = &mut self.desktop
            && &d.output == output
        {
            d.width = size.0 as f32;
            d.height = size.1.saturating_sub(crate::style::taskbar_height()) as f32;
        }
    }

    pub(crate) fn reload_desktop(&mut self) {
        let sort = self.config.desktop_sort;
        let locales = cosmic::desktop::fde::get_languages_from_env();
        let Some(d) = &mut self.desktop else { return };
        d.stamp = std::fs::metadata(&d.dir).and_then(|m| m.modified()).ok();
        let items = model::list(&d.dir, sort);
        d.items = items
            .into_iter()
            .map(|item| {
                if item.launcher
                    && let Ok(de) = cosmic::desktop::fde::DesktopEntry::from_path(&item.path, Some(&locales))
                {
                    let id = item.file_name.trim_end_matches(".desktop").to_string();
                    let launch = if self.catalog.get(&id).is_some() {
                        Some(Launch::App(id))
                    } else {
                        de.exec().map(|e| Launch::Exec {
                            exec: e.to_string(),
                            terminal: de.terminal(),
                            trusted: is_executable(&item.path),
                        })
                    };
                    return Shown {
                        label: de.name(&locales).map(|n| n.to_string()).unwrap_or_else(|| item.file_name.clone()),
                        look: Look::Named(de.icon().unwrap_or("application-x-executable").to_string()),
                        launch,
                        item,
                    };
                }
                let look = if item.is_dir {
                    Look::Named("folder".into())
                } else if is_image(&item.file_name)
                    // Thumbnails only for real, reasonably sized pictures.
                    && std::fs::metadata(&item.path).is_ok_and(|m| m.len() > 0 && m.len() < 32 << 20)
                {
                    Look::Image(item.path.clone())
                } else {
                    Look::Named(crate::start_view::mime_icon(&item.file_name).into())
                };
                Shown { label: item.file_name.clone(), look, launch: None, item }
            })
            .collect();
        // The Recycle Bin comes first, like the benchmark's.
        d.trash_full = trash_count() > 0;
        d.trash_stamp = std::fs::metadata(data_home().join("Trash/files")).and_then(|m| m.modified()).ok();
        // Desktop icon settings: This PC, the user's folder, Network, Recycle Bin, Control Panel.
        let mut specials = Vec::new();
        let user = std::env::var("USER").unwrap_or_else(|_| "User".into());
        for (id, label, icon) in SPECIAL {
            if id == "control" && !self.config.hide_recycle_bin {
                specials.push(trash_shown(d.trash_full));
            }
            if self.config.desktop_special.iter().any(|s| s == id) {
                specials.push(Shown {
                    item: DesktopItem {
                        path: special_path(id),
                        file_name: format!("::{id}"),
                        is_dir: true,
                        launcher: false,
                        modified: None,
                        size: 0,
                    },
                    label: if id == "user" { user.clone() } else { label.to_string() },
                    look: Look::Named(icon.into()),
                    launch: None,
                });
            }
        }
        for (i, s) in specials.into_iter().enumerate() {
            d.items.insert(i, s);
        }
        let paths: HashSet<&PathBuf> = d.items.iter().map(|s| &s.item.path).collect();
        d.selected.retain(|p| paths.contains(p));
    }

    /// Called every second: pick up files added or removed by other apps.
    pub(crate) fn desktop_tick(&mut self) {
        let Some(d) = &self.desktop else { return };
        if d.drag.is_some() || d.rename.is_some() {
            return;
        }
        let trash = std::fs::metadata(data_home().join("Trash/files")).and_then(|m| m.modified()).ok();
        if std::fs::metadata(&d.dir).and_then(|m| m.modified()).ok() != d.stamp || trash != d.trash_stamp {
            self.reload_desktop();
        }
    }

    fn saved_cells(&self) -> HashMap<String, Cell> {
        if self.config.desktop_auto_arrange {
            return HashMap::new();
        }
        self.state.desktop_cells.iter().map(|s| (s.name.clone(), (s.col, s.row))).collect()
    }

    fn cell_at(&self, p: Point) -> Cell {
        let (cw, ch, _) = self.config.desktop_icon_size.metrics();
        let (cols, rows) = self.grid();
        (
            (((p.x - MARGIN) / cw).floor().max(0.0) as u32).min(cols - 1),
            (((p.y - MARGIN) / ch).floor().max(0.0) as u32).min(rows - 1),
        )
    }

    fn open_item(&mut self, path: &Path) -> Task<Message> {
        if path == trash_path() {
            return open_trash();
        }
        if let Some(id) = special_id(path) {
            let files = crate::config::find_app(&self.catalog, crate::config::Role::Files);
            let open_in_files = |target: &str| match &files {
                Some(app) => Message::OpenWith(app.clone(), PathBuf::from(target)),
                None => Message::OpenWith(String::new(), PathBuf::from(target)),
            };
            return match id {
                "this-pc" => self.update(open_in_files("/")),
                "user" => self.update(open_in_files(&std::env::var("HOME").unwrap_or_else(|_| "/".into()))),
                "network" => self.update(open_in_files("network:///")),
                _ => match ["dev.lapinux.ControlPanel"].iter().find(|a| self.catalog.get(a).is_some()) {
                    Some(app) => self.launch(app),
                    None => self.open_settings(""),
                },
            };
        }
        let Some(shown) = self.desktop.as_ref().and_then(|d| d.items.iter().find(|s| s.item.path == path)).cloned() else {
            return Task::none();
        };
        match shown.launch {
            Some(Launch::App(id)) => self.launch(&id),
            Some(Launch::Exec { exec, terminal, trusted: true }) => {
                let id = shown.item.file_name.trim_end_matches(".desktop").to_string();
                Task::future(async move {
                    crate::catalog::launch(exec, id, terminal, None).await;
                    cosmic::Action::App(Message::Noop)
                })
            }
            Some(Launch::Exec { trusted: false, .. }) => Task::future(async move {
                crate::start_search::notify(
                    "This launcher isn't trusted",
                    "Right-click it and choose \u{201C}Allow launching\u{201D} if you trust where it came from.",
                )
                .await;
                cosmic::Action::App(Message::Noop)
            }),
            None => Task::future(async move {
                let mut c = std::process::Command::new("xdg-open");
                c.arg(shown.item.path);
                let _ = cosmic::process::spawn(c).await;
                cosmic::Action::App(Message::Noop)
            }),
        }
    }

    /// Selected files (the Recycle Bin isn't a file).
    fn selection(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> =
            self.desktop.as_ref().map(|d| d.selected.iter().filter(|p| **p != trash_path()).cloned().collect()).unwrap_or_default();
        v.sort();
        v
    }

    /// Icons in grid order (down each column, then across) with their cells.
    fn placed(&self) -> Vec<(PathBuf, Cell)> {
        let Some(d) = &self.desktop else { return Vec::new() };
        let items: Vec<DesktopItem> = d.items.iter().map(|s| s.item.clone()).collect();
        let (cols, rows) = self.grid();
        let mut v: Vec<(PathBuf, Cell)> =
            model::arrange(&items, &self.saved_cells(), cols, rows).into_iter().map(|(i, c)| (items[i].path.clone(), c)).collect();
        v.sort_by_key(|(_, (c, r))| (*c, *r));
        v
    }

    fn item_at_cell(&self, cell: Cell) -> Option<PathBuf> {
        self.placed().into_iter().find(|(_, c)| *c == cell).map(|(p, _)| p)
    }

    /// Pin every icon where it is now, so moving one doesn't reshuffle the rest.
    fn pin_cells(&mut self) {
        let Some(d) = &self.desktop else { return };
        let names: HashMap<PathBuf, String> = d.items.iter().map(|s| (s.item.path.clone(), s.item.file_name.clone())).collect();
        self.state.desktop_cells =
            self.placed().into_iter().filter_map(|(p, (c, r))| Some(DesktopSlot { name: names.get(&p)?.clone(), col: c, row: r })).collect();
    }

    /// Paste files into the desktop (copy, or move when cut), starting at `cell`.
    fn paste_into_desktop(&mut self, paths: Vec<PathBuf>, cut: bool, cell: Option<Cell>) -> Task<Message> {
        let Some(dir) = self.desktop.as_ref().map(|d| d.dir.clone()) else { return Task::none() };
        Task::future(async move {
            let done = tokio::task::spawn_blocking(move || {
                let mut out = Vec::new();
                for p in paths {
                    match model::paste_one(&p, &dir, cut) {
                        Ok(t) => out.push(t),
                        Err(e) => tracing::warn!("paste {p:?}: {e}"),
                    }
                }
                out
            })
            .await
            .unwrap_or_default();
            cosmic::Action::App(Message::Desktop(DesktopMsg::Pasted(done, cell)))
        })
    }

    /// Rename in a small Top-layer box over the icon's label. The desktop
    /// itself sits on the Bottom layer, where the compositor never hands it
    /// the keyboard on its own; a newly mapped Top-layer surface with
    /// on-demand keyboard gets focus at once, and clicking anywhere else
    /// unfocuses it, which commits the new name (like the benchmark).
    fn start_rename(&mut self, path: PathBuf, after_menu: bool) -> Task<Message> {
        if path == trash_path() {
            return Task::none();
        }
        let items: Vec<DesktopItem> = self.desktop.as_ref().map(|d| d.items.iter().map(|s| s.item.clone()).collect()).unwrap_or_default();
        let (cols, rows) = self.grid();
        let saved = self.saved_cells();
        let Some(d) = &mut self.desktop else { return Task::none() };
        let Some(&(_, (c, r))) = model::arrange(&items, &saved, cols, rows).iter().find(|(i, _)| items[*i].path == path) else {
            return Task::none();
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        d.selected = HashSet::from([path.clone()]);
        d.rename = Some((path, name));
        d.rename_cell = (c, r);
        if after_menu {
            // Closing the menu returns the keyboard to the desktop; open the
            // box once that has happened so it keeps the focus.
            return Task::future(async {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                cosmic::Action::App(Message::Desktop(DesktopMsg::ShowRenameBox))
            });
        }
        self.show_rename_box()
    }

    fn show_rename_box(&mut self) -> Task<Message> {
        let (cw, ch, isz) = self.config.desktop_icon_size.metrics();
        let Some(d) = &mut self.desktop else { return Task::none() };
        if d.rename.is_none() {
            return Task::none();
        }
        let (c, r) = d.rename_cell;
        let mut tasks = Vec::new();
        if let Some(old) = d.rename_surface.take() {
            tasks.push(destroy_layer_surface(old));
        }
        let id = window::Id::unique();
        d.rename_surface = Some(id);
        let width = (cw + 40.0) as u32;
        let left = (MARGIN + c as f32 * cw + cw / 2.0) as i32 - width as i32 / 2;
        let top = (MARGIN + r as f32 * ch + 2.0 + 6.0 + isz as f32 + 2.0) as i32;
        tasks.push(get_layer_surface(SctkLayerSurfaceSettings {
            id,
            layer: Layer::Top,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            anchor: Anchor::TOP | Anchor::LEFT,
            output: IcedOutput::Output(d.output.clone()),
            namespace: "lapin-ux-desktop-rename".into(),
            margin: IcedMargin { top, right: 0, bottom: 0, left: left.max(0) },
            size: Some((Some(width), Some(34))),
            exclusive_zone: -1,
            ..Default::default()
        }));
        // The text field is focused once the box gets the keyboard
        // (see `rename_focused`); the widget doesn't exist before that.
        Task::batch(tasks)
    }

    pub(crate) fn rename_focused(&self) -> Task<Message> {
        // Select the name without the extension, like the benchmark.
        Task::batch([
            cosmic::iced::runtime::widget::operation::focus(RENAME_ID.clone()),
            cosmic::iced::runtime::widget::operation::select_all(RENAME_ID.clone()),
        ])
    }

    fn create_new(&mut self, folder: bool) -> Task<Message> {
        let close = self.close_popup();
        let Some(d) = &self.desktop else { return close };
        let name = if folder { model::unique_name(&d.dir, "New folder", "") } else { model::unique_name(&d.dir, "New Text Document", "txt") };
        let path = d.dir.join(&name);
        let res = if folder { std::fs::create_dir(&path) } else { std::fs::write(&path, b"") };
        if let Err(e) = res {
            tracing::warn!("create {path:?}: {e}");
            return close;
        }
        // Put it where the user right-clicked, like the benchmark.
        let cell = self.cell_at(self.desktop.as_ref().map_or(Point::ORIGIN, |d| d.pointer));
        self.remember_cell(&name, cell);
        self.reload_desktop();
        Task::batch([close, self.start_rename(path, true)])
    }

    fn remember_cell(&mut self, name: &str, (col, row): Cell) {
        // Whoever was in that cell moves to the next free one on reload.
        self.state.desktop_cells.retain(|s| s.name != name && (s.col, s.row) != (col, row));
        self.state.desktop_cells.push(DesktopSlot { name: name.to_string(), col, row });
        self.persist_state();
    }

    pub(crate) fn update_desktop(&mut self, msg: DesktopMsg) -> Task<Message> {
        if !matches!(msg, DesktopMsg::Move(_)) {
            tracing::debug!(?msg, "desktop");
        }
        let was_renaming = self.desktop.as_ref().is_some_and(|d| d.rename.is_some());
        let task = self.update_desktop_inner(msg);
        // Close the rename box once the rename is over.
        let close = match &mut self.desktop {
            Some(d) if was_renaming && d.rename.is_none() => d.rename_surface.take().map(destroy_layer_surface),
            _ => None,
        };
        Task::batch([task, close.unwrap_or_else(Task::none)])
    }

    fn update_desktop_inner(&mut self, msg: DesktopMsg) -> Task<Message> {
        let Some(d) = &mut self.desktop else { return Task::none() };
        match msg {
            DesktopMsg::Press(path) => {
                // Clicking elsewhere keeps a pending rename, then carries on.
                let commit = if d.rename.as_ref().is_some_and(|(p, _)| p != &path) {
                    self.update_desktop(DesktopMsg::RenameCommit)
                } else {
                    Task::none()
                };
                let (ctrl, shift) = (self.mods.control(), self.mods.shift());
                let order: Vec<PathBuf> = if shift { self.placed().into_iter().map(|(p, _)| p).collect() } else { Vec::new() };
                let Some(d) = &mut self.desktop else { return commit };
                d.press_keep = None;
                if ctrl {
                    // Ctrl-click adds or removes one icon.
                    if !d.selected.remove(&path) {
                        d.selected.insert(path.clone());
                    }
                    d.anchor = Some(path);
                } else if shift && d.anchor.is_some() {
                    // Shift-click selects everything from the last click.
                    let anchor = d.anchor.clone().unwrap_or_default();
                    let (i, j) = (order.iter().position(|p| *p == anchor), order.iter().position(|p| *p == path));
                    if let (Some(i), Some(j)) = (i, j) {
                        d.selected = order[i.min(j)..=i.max(j)].iter().cloned().collect();
                    }
                } else if d.selected.contains(&path) && d.selected.len() > 1 {
                    // Keep the group for dragging; a plain click narrows it on release.
                    d.press_keep = Some(path.clone());
                    d.anchor = Some(path);
                } else {
                    d.selected = HashSet::from([path.clone()]);
                    d.anchor = Some(path);
                }
                return Task::batch([commit, self.close_popup()]);
            }
            DesktopMsg::Empty => {
                let commit = if d.rename.is_some() { self.update_desktop(DesktopMsg::RenameCommit) } else { Task::none() };
                let ctrl = self.mods.control();
                if let Some(d) = &mut self.desktop {
                    if !ctrl {
                        d.selected.clear();
                    }
                    // Dragging from empty space draws a selection box.
                    d.band = Some(d.pointer);
                }
                return Task::batch([commit, self.close_popup()]);
            }
            DesktopMsg::ShowRenameBox => return self.show_rename_box(),
            DesktopMsg::Move(p) => {
                d.pointer = p;
                if let Some(o) = d.band {
                    // Live box selection.
                    let rect = band_rect(o, p);
                    let (cw, ch, _) = self.config.desktop_icon_size.metrics();
                    let hits: Vec<PathBuf> = self
                        .placed()
                        .into_iter()
                        .filter(|(_, (c, r))| {
                            let cell = Rectangle { x: MARGIN + *c as f32 * cw + 2.0, y: MARGIN + *r as f32 * ch + 2.0, width: cw - 4.0, height: ch - 4.0 };
                            cell.intersects(&rect)
                        })
                        .map(|(p, _)| p)
                        .collect();
                    if (rect.width > 3.0 || rect.height > 3.0)
                        && let Some(d) = &mut self.desktop
                    {
                        d.selected = hits.into_iter().collect();
                    }
                }
            }
            DesktopMsg::Release => {
                d.band = None;
                if let Some(p) = d.press_keep.take()
                    && d.drag.is_none()
                {
                    d.selected = HashSet::from([p]);
                }
            }
            DesktopMsg::DragStart(path) => {
                d.press_keep = None;
                let from = self.placed().into_iter().find(|(p, _)| *p == path).map_or((0, 0), |(_, c)| c);
                if let Some(d) = &mut self.desktop {
                    if !d.selected.contains(&path) {
                        d.selected = HashSet::from([path.clone()]);
                    }
                    d.drag = Some(Drag { path, from });
                }
            }
            DesktopMsg::DragEnd => {
                d.drag = None;
                d.drop_at = None;
            }
            DesktopMsg::DropMotion(p) => {
                d.drop_at = Some(p);
                d.pointer = p;
            }
            DesktopMsg::DropLeave => d.drop_at = None,
            DesktopMsg::Dropped(uris, action) => {
                let at = d.drop_at.take().unwrap_or(d.pointer);
                let ours = d.drag.take();
                let dir = d.dir.clone();
                let cell = self.cell_at(at);
                let uris = uris.map(|u| u.0).unwrap_or_default();
                let paths: Vec<PathBuf> = uris.iter().filter_map(|u| lapin_ux_core::recent::file_uri_to_path(u)).collect();
                let links: Vec<String> = uris.iter().filter(|u| u.starts_with("http://") || u.starts_with("https://")).cloned().collect();
                let target = self.item_at_cell(cell).filter(|t| !paths.contains(t));
                // Onto the Recycle Bin: delete.
                if target.as_deref() == Some(trash_path().as_path()) {
                    if let Some(d) = &mut self.desktop {
                        d.selected = paths.iter().cloned().collect();
                    }
                    return self.update_desktop(DesktopMsg::Trash);
                }
                // Onto a folder: move (or copy) into it.
                if let Some(folder) = target.filter(|t| t.is_dir()) {
                    let cut = action != DndAction::Copy;
                    return Task::future(async move {
                        let _ = tokio::task::spawn_blocking(move || {
                            for p in paths {
                                if let Err(e) = model::paste_one(&p, &folder, cut) {
                                    tracing::warn!("drop into folder: {e}");
                                }
                            }
                        })
                        .await;
                        cosmic::Action::App(Message::Desktop(DesktopMsg::Refresh))
                    });
                }
                // Our own icons: move them on the grid, keeping their layout.
                if let Some(drag) = ours
                    && paths.iter().all(|p| p.parent() == Some(dir.as_path()))
                {
                    self.move_icons(&paths, drag, cell);
                    return Task::none();
                }
                // Links from a browser become shortcuts.
                for url in links {
                    if let Some(lapin_ux_core::pins::Pin::Site { name, url }) = lapin_ux_core::pins::site(&url) {
                        let file = dir.join(model::unique_name(&dir, &name, "desktop"));
                        let _ = std::fs::write(&file, format!("[Desktop Entry]\nType=Link\nName={name}\nURL={url}\nIcon=web-browser\n"));
                        self.remember_cell(&file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), cell);
                    }
                }
                // Files from another app: copy here, or move if that's what was asked.
                let outside: Vec<PathBuf> = paths.into_iter().filter(|p| p.parent() != Some(dir.as_path())).collect();
                if outside.is_empty() {
                    self.reload_desktop();
                    return Task::none();
                }
                return self.paste_into_desktop(outside, action == DndAction::Move, Some(cell));
            }
            DesktopMsg::Pasted(paths, cell) => {
                if let Some(cell) = cell {
                    self.pin_cells();
                    let (cols, rows) = self.grid();
                    for (n, p) in paths.iter().enumerate() {
                        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let c = step_cell(cell, n as u32, rows, cols);
                        self.remember_cell(&name, c);
                    }
                }
                self.reload_desktop();
                if let Some(d) = &mut self.desktop {
                    d.selected = paths.into_iter().collect();
                }
            }
            DesktopMsg::Ctrl(k) => match k.as_str() {
                "a" => d.selected = d.items.iter().map(|s| s.item.path.clone()).collect(),
                "c" => return self.update_desktop(DesktopMsg::Copy),
                "x" => return self.update_desktop(DesktopMsg::Cut),
                "v" => return self.update_desktop(DesktopMsg::Paste),
                _ => {}
            },
            DesktopMsg::Cut | DesktopMsg::Copy => {
                let paths = self.selection();
                if !paths.is_empty() {
                    self.set_clipboard_files(model::FileClip { cut: matches!(msg, DesktopMsg::Cut), paths });
                }
                return self.close_popup();
            }
            DesktopMsg::EmptyTrashAsk => {
                let Some(id) = self.desktop.as_ref().map(|d| d.id) else { return Task::none() };
                let key = rect_key(id, "pointer");
                return self.open_popup(id, &key, PopupKind::ConfirmEmptyTrash);
            }
            DesktopMsg::EmptyTrash => {
                lapin_ux_core::sound::play("trash-empty");
                let close = self.close_popup();
                let trash = data_home().join("Trash");
                return Task::batch([
                    close,
                    Task::future(async move {
                        let _ = tokio::task::spawn_blocking(move || {
                            for sub in ["files", "info", "expunged"] {
                                for e in std::fs::read_dir(trash.join(sub)).into_iter().flatten().flatten() {
                                    let p = e.path();
                                    let _ = if p.is_dir() && !p.is_symlink() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                                }
                            }
                        })
                        .await;
                        cosmic::Action::App(Message::Desktop(DesktopMsg::Refresh))
                    }),
                ]);
            }
            DesktopMsg::Open(path) => {
                d.drag = None;
                return self.open_item(&path);
            }
            DesktopMsg::Context(on) => {
                if on.is_some() {
                    d.item_context_at = std::time::Instant::now();
                } else if d.item_context_at.elapsed() < std::time::Duration::from_millis(150) {
                    return Task::none();
                }
                let id = d.id;
                if let Some(p) = &on
                    && !d.selected.contains(p)
                {
                    d.selected = HashSet::from([p.clone()]);
                }
                if on.is_none() {
                    d.selected.clear();
                }
                let key = rect_key(id, "pointer");
                self.rects.insert(key.clone(), Rectangle { x: d.pointer.x, y: d.pointer.y, width: 1.0, height: 1.0 });
                return self.open_popup(id, &key, PopupKind::DesktopMenu(on, MenuPage::Main));
            }
            DesktopMsg::Menu(page) => {
                // The icon the open menu belongs to, if any.
                let on = match self.popup.as_ref().map(|p| &p.kind) {
                    Some(PopupKind::DesktopMenu(on, _)) => on.clone(),
                    _ => None,
                };
                let key = match page {
                    MenuPage::Main => return self.close_submenu(),
                    MenuPage::View => "desk-view",
                    MenuPage::Sort => "desk-sort",
                    MenuPage::New => "desk-new",
                    MenuPage::OpenWith => "desk-openwith",
                    MenuPage::SendTo => "desk-sendto",
                    MenuPage::More => {
                        // Swap the open menu for the classic one in place.
                        let close = self.close_submenu();
                        if let Some(p) = self.popup.as_mut() {
                            p.kind = PopupKind::DesktopMenu(on, MenuPage::More);
                        }
                        return close;
                    }
                };
                let on = if matches!(page, MenuPage::OpenWith | MenuPage::SendTo) { on } else { None };
                return self.open_submenu(key, PopupKind::DesktopMenu(on, page));
            }
            DesktopMsg::Properties => {
                let target = self.selection().into_iter().next();
                let close = self.close_popup();
                return match target {
                    Some(p) => Task::batch([close, self.update(Message::Props(crate::properties::PropsMsg::Open(p)))]),
                    None => close,
                };
            }
            DesktopMsg::OpenWithApp(app) => {
                let Some(target) = self.selection().into_iter().next() else { return self.close_popup() };
                let close = self.close_popup();
                return Task::batch([close, self.update(Message::OpenWith(app, target))]);
            }
            DesktopMsg::Compress => {
                let dir = d.dir.clone();
                let files = self.selection();
                let close = self.close_popup();
                let Some(first) = files.first().cloned() else { return close };
                let stem = first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Archive".into());
                let name = model::unique_name(&dir, &stem, "zip");
                let zip = async move {
                    let mut c = tokio::process::Command::new("bsdtar");
                    c.current_dir(&dir).arg("-a").arg("-cf").arg(&name);
                    for f in &files {
                        if let Some(n) = f.file_name() {
                            c.arg(n);
                        }
                    }
                    match c.status().await {
                        Ok(s) if s.success() => {}
                        _ => crate::start_search::notify("Couldn't compress", "bsdtar (libarchive) failed or isn't installed.").await,
                    }
                    cosmic::Action::App(Message::Desktop(DesktopMsg::Refresh))
                };
                return Task::batch([close, Task::future(zip)]);
            }
            DesktopMsg::CreateShortcut => {
                let dir = d.dir.clone();
                let close = self.close_popup();
                for p in self.selection() {
                    let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
                    let (stem, ext) = match name.rsplit_once('.') {
                        Some((s, e)) if !s.is_empty() && !p.is_dir() => (s.to_string(), e.to_string()),
                        _ => (name.clone(), String::new()),
                    };
                    let base = format!("{stem} - Shortcut");
                    let link = dir.join(model::unique_name(&dir, &base, &ext));
                    if let Err(e) = std::os::unix::fs::symlink(&p, &link) {
                        tracing::warn!("shortcut {p:?}: {e}");
                    }
                }
                self.reload_desktop();
                return close;
            }
            DesktopMsg::SendToDocuments => {
                let close = self.close_popup();
                let docs = dirs_documents();
                for p in self.selection() {
                    if let Err(e) = model::paste_one(&p, &docs, false) {
                        tracing::warn!("send to Documents {p:?}: {e}");
                    }
                }
                return close;
            }
            DesktopMsg::ToggleSpecial(id) => {
                if id == "recycle-bin" {
                    self.config.hide_recycle_bin = !self.config.hide_recycle_bin;
                } else if self.config.desktop_special.iter().any(|s| s == id) {
                    self.config.desktop_special.retain(|s| s != id);
                } else {
                    self.config.desktop_special.push(id.to_string());
                }
                self.persist_config();
                self.reload_desktop();
                return self.close_popup();
            }
            DesktopMsg::ToggleAutoArrange => {
                self.config.desktop_auto_arrange = !self.config.desktop_auto_arrange;
                self.persist_config();
                return self.close_popup();
            }
            DesktopMsg::Paste => return self.paste_files(),
            DesktopMsg::Key(key) => match key {
                Named::Escape => {
                    if d.rename.take().is_none() {
                        d.selected.clear();
                    }
                }
                Named::Enter if d.rename.is_some() => return self.update_desktop(DesktopMsg::RenameCommit),
                // Alt+Enter: Properties.
                Named::Enter if self.mods.alt() => return self.update_desktop(DesktopMsg::Properties),
                Named::Enter => {
                    let sel = self.selection();
                    return Task::batch(sel.iter().map(|p| self.open_item(p)).collect::<Vec<_>>());
                }
                Named::Delete if d.rename.is_none() => return self.update_desktop(DesktopMsg::Trash),
                Named::F2 if d.selected.len() == 1 => {
                    let p = d.selected.iter().next().cloned().unwrap_or_default();
                    return self.start_rename(p, false);
                }
                Named::F5 => return self.update_desktop(DesktopMsg::Refresh),
                _ => {}
            },
            DesktopMsg::RenameStart => {
                let close = self.close_popup();
                let Some(p) = self.selection().into_iter().next() else { return close };
                return Task::batch([close, self.start_rename(p, true)]);
            }
            DesktopMsg::RenameInput(s) => {
                if let Some((_, name)) = &mut d.rename {
                    *name = s;
                }
            }
            DesktopMsg::RenameCommit => {
                let Some((path, new)) = d.rename.take() else { return Task::none() };
                let old = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let new = new.trim().to_string();
                if new == old {
                    return Task::none();
                }
                if let Some(problem) = model::rename_problem(&d.dir, &old, &new) {
                    return Task::future(async move {
                        crate::start_search::notify("Couldn't rename", problem).await;
                        cosmic::Action::App(Message::Noop)
                    });
                }
                let target = d.dir.join(&new);
                if let Err(e) = std::fs::rename(&path, &target) {
                    tracing::warn!("rename: {e}");
                } else {
                    d.selected = HashSet::from([target]);
                    if let Some(slot) = self.state.desktop_cells.iter_mut().find(|s| s.name == old) {
                        slot.name = new;
                        self.persist_state();
                    }
                }
                self.reload_desktop();
            }
            DesktopMsg::Trash => {
                let close = self.close_popup();
                let data = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/share"));
                let mut fallback = Vec::new();
                for p in self.selection() {
                    if let Err(e) = model::trash(&p, &data) {
                        tracing::debug!("trash {p:?}: {e}; trying gio");
                        fallback.push(p);
                    }
                }
                self.reload_desktop();
                if fallback.is_empty() {
                    return close;
                }
                return Task::batch([
                    close,
                    Task::future(async move {
                        let ok = tokio::process::Command::new("gio").arg("trash").args(&fallback).status().await.is_ok_and(|s| s.success());
                        if !ok {
                            crate::start_search::notify("Couldn't move to the trash", "").await;
                        }
                        cosmic::Action::App(Message::Desktop(DesktopMsg::Refresh))
                    }),
                ]);
            }
            DesktopMsg::CopyPath => {
                let close = self.close_popup();
                let text = self.selection().iter().map(|p| format!("\"{}\"", p.display())).collect::<Vec<_>>().join("\n");
                return Task::batch([close, cosmic::iced::clipboard::write(text)]);
            }
            DesktopMsg::AllowLaunching => {
                for p in self.selection() {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(m) = std::fs::metadata(&p) {
                        let mut perm = m.permissions();
                        perm.set_mode(perm.mode() | 0o100);
                        let _ = std::fs::set_permissions(&p, perm);
                    }
                }
                self.reload_desktop();
                return self.close_popup();
            }
            DesktopMsg::NewFolder => return self.create_new(true),
            DesktopMsg::NewText => return self.create_new(false),
            DesktopMsg::Refresh => {
                self.reload_desktop();
                return self.close_popup();
            }
            DesktopMsg::Sort(s) => {
                self.config.desktop_sort = s;
                self.persist_config();
                // Sorting arranges everything afresh, like the benchmark.
                self.state.desktop_cells.clear();
                self.persist_state();
                self.reload_desktop();
                return self.close_popup();
            }
            DesktopMsg::Size(s) => {
                self.config.desktop_icon_size = s;
                self.persist_config();
                return self.close_popup();
            }
            DesktopMsg::ToggleIcons => {
                self.config.hide_desktop_icons = !self.config.hide_desktop_icons;
                self.persist_config();
                return self.close_popup();
            }
            DesktopMsg::Settings(page) => {
                let close = self.close_popup();
                return Task::batch([close, self.open_settings(page)]);
            }
            DesktopMsg::Terminal => {
                let dir = d.dir.clone();
                let close = self.close_popup();
                let Some(exec) = crate::config::find_app(&self.catalog, crate::config::Role::Terminal)
                    .and_then(|id| self.catalog.get(&id))
                    .and_then(|a| a.data.exec.clone())
                else {
                    return close;
                };
                let program: Vec<&str> = exec.split_whitespace().filter(|w| !w.starts_with('%')).collect();
                let mut c = std::process::Command::new(program[0]);
                c.args(&program[1..]).current_dir(dir);
                return Task::batch([
                    close,
                    Task::future(async move {
                        let _ = cosmic::process::spawn(c).await;
                        cosmic::Action::App(Message::Noop)
                    }),
                ]);
            }
        }
        Task::none()
    }
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

// ---- view --------------------------------------------------------------------

fn cell_style(selected: bool, target: bool) -> theme::Container<'static> {
    theme::Container::custom(move |t| {
        let accent: Color = t.cosmic().accent_color().into();
        let (bg, border) = match (selected, target) {
            (_, true) => (Color { a: 0.15, ..Color::WHITE }, Color { a: 0.6, ..Color::WHITE }),
            (true, _) => (Color { a: 0.35, ..accent }, Color { a: 0.8, ..accent }),
            _ => (Color::TRANSPARENT, Color::TRANSPARENT),
        };
        container::Style {
            background: Some(Background::Color(bg)),
            border: Border { color: border, width: 1.0, radius: 4.0.into() },
            text_color: Some(Color::WHITE),
            ..Default::default()
        }
    })
}

/// White label with a dark shadow so it reads on any wallpaper.
fn label<'a>(s: String, width: f32) -> Element<'a, Message> {
    let shadow = text::body(s.clone())
        .width(width)
        .align_x(Alignment::Center)
        .class(theme::Text::Custom(|_| cosmic::iced::widget::text::Style { color: Some(Color { a: 0.85, ..Color::BLACK }), ..Default::default() }));
    let front = text::body(s)
        .width(width)
        .align_x(Alignment::Center)
        .class(theme::Text::Custom(|_| cosmic::iced::widget::text::Style { color: Some(Color::WHITE), ..Default::default() }));
    cosmic::iced::widget::stack![container(shadow).padding([1, 0, 0, 1]), front].into()
}

fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}\u{2026}", s.chars().take(max - 1).collect::<String>()) }
}

pub fn view(shell: &Shell) -> Element<'_, Message> {
    let Some(d) = &shell.desktop else { return widget::Space::new().into() };
    let (cw, ch, isz) = shell.config.desktop_icon_size.metrics();
    let (cols, rows) = shell.grid();
    let mut by_cell: HashMap<Cell, usize> = HashMap::new();
    if !shell.config.hide_desktop_icons {
        let items: Vec<DesktopItem> = d.items.iter().map(|s| s.item.clone()).collect();
        for (i, cell) in model::arrange(&items, &shell.saved_cells(), cols, rows) {
            by_cell.entry(cell).or_insert(i);
        }
    }
    let dragging = d.drag.as_ref();
    let target = d.drop_at.map(|p| shell.cell_at(p));

    let mut grid = row::with_capacity(cols as usize);
    for c in 0..cols {
        let mut col = column::with_capacity(rows as usize);
        for r in 0..rows {
            let is_target = target == Some((c, r));
            let cell: Element<'_, Message> = match by_cell.get(&(c, r)).map(|&i| &d.items[i]) {
                Some(s) => {
                    let selected = d.selected.contains(&s.item.path);
                    let picture: Element<'_, Message> = match &s.look {
                        Look::Image(p) => widget::image(widget::image::Handle::from_path(p))
                            .width(isz as f32)
                            .height(isz as f32)
                            .content_fit(cosmic::iced::ContentFit::Contain)
                            .into(),
                        Look::Named(n) => icon(crate::icons::full(n))
                        .size(isz)
                        .into(),
                    };
                    // While renaming, the label is drawn by the rename box above it.
                    let renaming = d.rename.as_ref().is_some_and(|(p, _)| p == &s.item.path);
                    let caption: Element<'_, Message> = if renaming {
                        widget::Space::new().height(20).into()
                    } else {
                        label(if selected { short(&s.label, 60) } else { short(&s.label, 22) }, cw - 8.0)
                    };
                    let body = column::with_children(vec![picture, caption]).spacing(4).align_x(Alignment::Center);
                    let dim = dragging.is_some_and(|g| g.path == s.item.path);
                    let boxed = container(body)
                        .width(cw - 4.0)
                        .height(ch - 4.0)
                        .padding([6, 2])
                        .align_x(Alignment::Center)
                        .class(cell_style(selected && !dim, is_target));
                    let path = s.item.path.clone();
                    let area = mouse_area(boxed)
                        .on_press(Message::Desktop(DesktopMsg::Press(path.clone())))
                        .on_double_click(Message::Desktop(DesktopMsg::Open(path.clone())))
                        .on_right_press(Message::Desktop(DesktopMsg::Context(Some(path.clone()))));
                    if path == trash_path() {
                        area.into()
                    } else {
                        // Drag out: the whole selection goes, as files.
                        let files: Vec<PathBuf> = if d.selected.contains(&path) {
                            d.selected.iter().filter(|p| **p != trash_path()).cloned().collect()
                        } else {
                            vec![path.clone()]
                        };
                        let look = s.look.clone();
                        cosmic::widget::dnd_source::<Message, crate::dnd::FileDrag>(area)
                            .drag_threshold(6.0)
                            .drag_content(move || crate::dnd::FileDrag(files.clone()))
                            .drag_icon(move |offset| {
                                let handle = match &look {
                                    Look::Image(p) => icon::from_path(p.clone()),
                                    Look::Named(n) => crate::icons::full(n),
                                };
                                let el: Element<'static, ()> = icon(handle).size(isz).into();
                                (el, cosmic::iced::advanced::widget::tree::State::None, offset)
                            })
                            .on_start(Some(Message::Desktop(DesktopMsg::DragStart(path))))
                            .on_cancel(Some(Message::Desktop(DesktopMsg::DragEnd)))
                            .on_finish(Some(Message::Desktop(DesktopMsg::DragEnd)))
                            .into()
                    }
                }
                None => container(widget::Space::new())
                    .width(cw - 4.0)
                    .height(ch - 4.0)
                    .class(cell_style(false, is_target))
                    .into(),
            };
            col = col.push(container(cell).width(cw).height(ch).padding(2));
        }
        grid = grid.push(col);
    }
    let mut layers: Vec<Element<'_, Message>> = vec![container(grid).padding(MARGIN as u16).width(Length::Fill).height(Length::Fill).into()];
    // The selection box.
    if let Some(o) = d.band {
        let r = band_rect(o, d.pointer);
        if r.width > 3.0 || r.height > 3.0 {
            layers.push(
                container(
                    container(widget::Space::new().width(r.width).height(r.height)).class(theme::Container::custom(|t| {
                        let a: Color = t.cosmic().accent_color().into();
                        container::Style {
                            background: Some(Background::Color(Color { a: 0.25, ..a })),
                            border: Border { color: Color { a: 0.9, ..a }, width: 1.0, radius: 0.0.into() },
                            ..Default::default()
                        }
                    })),
                )
                .padding(cosmic::iced::Padding { top: r.y, left: r.x, right: 0.0, bottom: 0.0 })
                .into(),
            );
        }
    }
    let area = mouse_area(cosmic::iced::widget::Stack::with_children(layers).width(Length::Fill).height(Length::Fill))
        .on_press(Message::Desktop(DesktopMsg::Empty))
        .on_right_press(Message::Desktop(DesktopMsg::Context(None)));
    // Files dropped here (from apps or our own icons).
    cosmic::widget::dnd_destination::dnd_destination_for_data::<crate::dnd::Uris, Message>(area, |uris, action| {
        Message::Desktop(DesktopMsg::Dropped(uris, action))
    })
    .on_motion(|x, y| Message::Desktop(DesktopMsg::DropMotion(Point::new(x as f32, y as f32))))
    .on_enter(|x, y, _| Message::Desktop(DesktopMsg::DropMotion(Point::new(x as f32, y as f32))))
    .on_leave(|| Message::Desktop(DesktopMsg::DropLeave))
    .into()
}

/// The rename box (its own surface, see `start_rename`).
pub fn rename_view(shell: &Shell) -> Element<'_, Message> {
    let Some((_, name)) = shell.desktop.as_ref().and_then(|d| d.rename.as_ref()) else { return widget::Space::new().into() };
    // Centred under the icon, like the label it replaces.
    container(
        cosmic::iced::widget::text_input("", name)
            .id(RENAME_ID.clone())
            .on_input(|v| Message::Desktop(DesktopMsg::RenameInput(v)))
            .on_submit(Message::Desktop(DesktopMsg::RenameCommit))
            .align_x(cosmic::iced::alignment::Horizontal::Center)
            .padding([3, 4])
            .size(14)
            .width(Length::Fill),
    )
    .padding(1)
    .into()
}

/// Right-click menus for the desktop and its icons.
pub fn menu<'a>(shell: &'a Shell, on: &'a Option<PathBuf>, page: &MenuPage) -> Element<'a, Message> {
    use crate::submenu::{Lead, entry, leaf, parent_entry};
    let m = |x: DesktopMsg| Message::Desktop(x);
    let pick = |on: bool| if on { Lead::Mark("\u{2022}") } else { Lead::Blank };
    let tick = |on: bool| if on { Lead::Mark("\u{2713}") } else { Lead::Blank };
    let items: Vec<Element<'a, Message>> = match (on, page) {
        (Some(path), _) if *path == trash_path() => vec![
            menu_item("folder-open-symbolic", "Open", m(DesktopMsg::Open(path.clone()))),
            menu_divider(),
            menu_item(
                "user-trash-full-symbolic",
                "Empty Recycle Bin",
                m(DesktopMsg::EmptyTrashAsk),
            ),
        ],
        (Some(path), _) if special_id(path).is_some() => vec![menu_item("folder-open-symbolic", "Open", m(DesktopMsg::Open(path.clone())))],
        (Some(path), MenuPage::OpenWith) => {
            let mut v: Vec<Element<'a, Message>> = open_with_apps(shell, path)
                .into_iter()
                .map(|id| {
                    let name = crate::taskbar_view::app_name(shell, &id);
                    crate::taskbar_view::menu_item_with(crate::taskbar_view::app_icon(shell, &id), name, m(DesktopMsg::OpenWithApp(id)))
                })
                .collect();
            if v.is_empty() {
                v.push(container(text::caption("No apps found").class(crate::style::dim_text())).padding([8, 12]).into());
            }
            v.push(crate::taskbar_view::menu_divider());
            v.push(menu_item(
                "application-x-executable-symbolic",
                "Choose another app",
                Message::OpenWithPick(crate::open_with::OpenWithMsg::Open(path.clone())),
            ));
            v
        }
        (Some(_), MenuPage::SendTo) => vec![
            menu_item("package-x-generic-symbolic", "Compressed (zipped) folder", m(DesktopMsg::Compress)),
            menu_item("folder-documents-symbolic", "Documents", m(DesktopMsg::SendToDocuments)),
        ],
        // "Show more options": the classic menu.
        (Some(path), MenuPage::More) => {
            let pid = shell.popup.as_ref().map_or(window::Id::NONE, |p| p.id);
            vec![
                leaf(entry(Lead::None, "Open", None, Some(m(DesktopMsg::Open(path.clone()))))),
                parent_entry(shell, pid, "desk-openwith", Lead::None, "Open with", m(DesktopMsg::Menu(MenuPage::OpenWith))),
                leaf(menu_divider()),
                parent_entry(shell, pid, "desk-sendto", Lead::None, "Send to", m(DesktopMsg::Menu(MenuPage::SendTo))),
                leaf(menu_divider()),
                leaf(entry(Lead::None, "Cut", None, Some(m(DesktopMsg::Cut)))),
                leaf(entry(Lead::None, "Copy", None, Some(m(DesktopMsg::Copy)))),
                leaf(menu_divider()),
                leaf(entry(Lead::None, "Create shortcut", None, Some(m(DesktopMsg::CreateShortcut)))),
                leaf(entry(Lead::None, "Delete", None, Some(m(DesktopMsg::Trash)))),
                leaf(entry(Lead::None, "Rename", None, Some(m(DesktopMsg::RenameStart)))),
                leaf(menu_divider()),
                leaf(entry(Lead::None, "Properties", None, Some(m(DesktopMsg::Properties)))),
            ]
        }
        (Some(path), _) => {
            let pid = shell.popup.as_ref().map_or(window::Id::NONE, |p| p.id);
            let launcher = shell
                .desktop
                .as_ref()
                .and_then(|d| d.items.iter().find(|s| &s.item.path == path))
                .and_then(|s| s.launch.as_ref());
            let mut v = vec![leaf(menu_item("document-open-symbolic", "Open", m(DesktopMsg::Open(path.clone()))))];
            if launcher.is_none() {
                v.push(parent_entry(shell, pid, "desk-openwith", Lead::Icon("document-open-symbolic"), "Open with", m(DesktopMsg::Menu(MenuPage::OpenWith))));
            }
            if matches!(launcher, Some(Launch::Exec { trusted: false, .. })) {
                v.push(leaf(menu_item("emblem-ok-symbolic", "Allow launching", m(DesktopMsg::AllowLaunching))));
            }
            v.extend([
                leaf(menu_divider()),
                leaf(menu_item("edit-cut-symbolic", "Cut", m(DesktopMsg::Cut))),
                leaf(menu_item("edit-copy-symbolic", "Copy", m(DesktopMsg::Copy))),
                leaf(menu_divider()),
                leaf(menu_item("package-x-generic-symbolic", "Compress to ZIP file", m(DesktopMsg::Compress))),
                leaf(menu_item("edit-copy-symbolic", "Copy as path", m(DesktopMsg::CopyPath))),
                leaf(menu_item("edit-symbolic", "Rename", m(DesktopMsg::RenameStart))),
                leaf(menu_item("user-trash-symbolic", "Delete", m(DesktopMsg::Trash))),
                leaf(menu_item("document-properties-symbolic", "Properties", m(DesktopMsg::Properties))),
                leaf(menu_divider()),
                leaf(menu_item("view-more-symbolic", "Show more options", m(DesktopMsg::Menu(MenuPage::More)))),
            ]);
            v
        }
        // The desktop menu, laid out like the benchmark's classic one:
        // submenus cascade to the side with their arrow at the far right.
        (None, MenuPage::Main) => {
            let pid = shell.popup.as_ref().map_or(window::Id::NONE, |p| p.id);
            vec![
                parent_entry(shell, pid, "desk-view", Lead::Icon("view-grid-symbolic"), "View", m(DesktopMsg::Menu(MenuPage::View))),
                parent_entry(shell, pid, "desk-sort", Lead::Icon("view-sort-ascending-symbolic"), "Sort by", m(DesktopMsg::Menu(MenuPage::Sort))),
                leaf(entry(Lead::Icon("view-refresh-symbolic"), "Refresh", None, Some(m(DesktopMsg::Refresh)))),
                leaf(menu_divider()),
                leaf(entry(Lead::Icon("edit-paste-symbolic"), "Paste", None, shell.can_paste_files().then(|| m(DesktopMsg::Paste)))),
                leaf(menu_divider()),
                parent_entry(shell, pid, "desk-new", Lead::Icon("list-add-symbolic"), "New", m(DesktopMsg::Menu(MenuPage::New))),
                leaf(menu_divider()),
                leaf(entry(Lead::Icon("preferences-desktop-display-symbolic"), "Display settings", None, Some(m(DesktopMsg::Settings("displays"))))),
                leaf(entry(Lead::Icon("preferences-desktop-wallpaper-symbolic"), "Personalize", None, Some(m(DesktopMsg::Settings("lapin-personalization"))))),
                leaf(entry(Lead::Icon("utilities-terminal-symbolic"), "Open in Terminal", None, Some(m(DesktopMsg::Terminal)))),
            ]
        }
        (None, MenuPage::View) => {
            let s = shell.config.desktop_icon_size;
            vec![
                entry(pick(s == IconSize::Large), "Large icons", None, Some(m(DesktopMsg::Size(IconSize::Large)))),
                entry(pick(s == IconSize::Medium), "Medium icons", None, Some(m(DesktopMsg::Size(IconSize::Medium)))),
                entry(pick(s == IconSize::Small), "Small icons", None, Some(m(DesktopMsg::Size(IconSize::Small)))),
                menu_divider(),
                entry(tick(shell.config.desktop_auto_arrange), "Auto arrange icons", None, Some(m(DesktopMsg::ToggleAutoArrange))),
                entry(tick(true), "Align icons to grid", None, None),
                menu_divider(),
                entry(tick(!shell.config.hide_desktop_icons), "Show desktop icons", None, Some(m(DesktopMsg::ToggleIcons))),
                menu_divider(),
                entry(tick(shell.config.desktop_special.iter().any(|s| s == "this-pc")), "This PC", None, Some(m(DesktopMsg::ToggleSpecial("this-pc")))),
                entry(tick(shell.config.desktop_special.iter().any(|s| s == "user")), "User's files", None, Some(m(DesktopMsg::ToggleSpecial("user")))),
                entry(tick(shell.config.desktop_special.iter().any(|s| s == "network")), "Network", None, Some(m(DesktopMsg::ToggleSpecial("network")))),
                entry(tick(!shell.config.hide_recycle_bin), "Recycle Bin", None, Some(m(DesktopMsg::ToggleSpecial("recycle-bin")))),
                entry(tick(shell.config.desktop_special.iter().any(|s| s == "control")), "Control Panel", None, Some(m(DesktopMsg::ToggleSpecial("control")))),
            ]
        }
        (None, MenuPage::Sort) => {
            let s = shell.config.desktop_sort;
            vec![
                entry(pick(s == SortBy::Name), "Name", None, Some(m(DesktopMsg::Sort(SortBy::Name)))),
                entry(pick(s == SortBy::Size), "Size", None, Some(m(DesktopMsg::Sort(SortBy::Size)))),
                entry(pick(s == SortBy::Type), "Item type", None, Some(m(DesktopMsg::Sort(SortBy::Type)))),
                entry(pick(s == SortBy::Modified), "Date modified", None, Some(m(DesktopMsg::Sort(SortBy::Modified)))),
            ]
        }
        (None, MenuPage::OpenWith | MenuPage::SendTo | MenuPage::More) => Vec::new(),
        (None, MenuPage::New) => vec![
            entry(Lead::Icon("folder-new-symbolic"), "Folder", None, Some(m(DesktopMsg::NewFolder))),
            menu_divider(),
            entry(Lead::Icon("text-x-generic-symbolic"), "Text Document", None, Some(m(DesktopMsg::NewText))),
        ],
    };
    let width = if matches!(page, MenuPage::Main) && on.is_none() { 250.0 } else { 220.0 };
    panel(column::with_children(items).spacing(1), width)
}

impl Shell {
    /// Files are on the clipboard (copied or cut in a file manager or here).
    pub(crate) fn can_paste_files(&self) -> bool {
        self.clip.files.is_some()
    }

    pub(crate) fn paste_files(&mut self) -> Task<Message> {
        let close = self.close_popup();
        let Some(clip) = self.clip.files.clone() else { return close };
        let cell = self.desktop.as_ref().map(|d| d.pointer).map(|p| self.cell_at(p));
        // A cut is used up by pasting it.
        if clip.cut {
            self.clip.files = None;
        }
        Task::batch([close, self.paste_into_desktop(clip.paths, clip.cut, cell)])
    }

    /// Move dragged icons so the one grabbed lands on `to`; the others keep
    /// their places relative to it.
    fn move_icons(&mut self, paths: &[PathBuf], drag: Drag, to: Cell) {
        let (cols, rows) = self.grid();
        let (dc, dr) = (to.0 as i64 - drag.from.0 as i64, to.1 as i64 - drag.from.1 as i64);
        if (dc, dr) == (0, 0) {
            return;
        }
        let placed = self.placed();
        self.pin_cells();
        let names: HashMap<PathBuf, String> =
            self.desktop.as_ref().map(|d| d.items.iter().map(|s| (s.item.path.clone(), s.item.file_name.clone())).collect()).unwrap_or_default();
        // A single icon dropped on another swaps with it.
        if paths.len() == 1
            && let Some(other) = placed.iter().find(|(p, c)| *c == to && *p != paths[0]).map(|(p, _)| p.clone())
            && let Some(n) = names.get(&other)
        {
            self.remember_cell(n, drag.from);
        }
        for p in paths.iter().chain(std::iter::once(&drag.path)) {
            let Some((_, (c, r))) = placed.iter().find(|(q, _)| q == p) else { continue };
            let nc = (*c as i64 + dc).clamp(0, cols as i64 - 1) as u32;
            let nr = (*r as i64 + dr).clamp(0, rows as i64 - 1) as u32;
            if let Some(n) = names.get(p) {
                self.remember_cell(n, (nc, nr));
            }
        }
    }
}

fn band_rect(a: Point, b: Point) -> Rectangle {
    Rectangle { x: a.x.min(b.x), y: a.y.min(b.y), width: (a.x - b.x).abs(), height: (a.y - b.y).abs() }
}

/// The n-th cell after `start`, down the column then across.
fn step_cell(start: Cell, n: u32, rows: u32, cols: u32) -> Cell {
    let idx = start.0 * rows + start.1 + n;
    ((idx / rows).min(cols.saturating_sub(1)), idx % rows)
}

/// Open the trash in the file manager.
fn open_trash() -> Task<Message> {
    Task::future(async {
        // Most file managers take trash:///; COSMIC Files too.
        let mut ok = false;
        for (cmd, args) in [("cosmic-files", vec!["trash:///"]), ("gio", vec!["open", "trash:///"]), ("xdg-open", vec!["trash:///"])] {
            if tokio::process::Command::new(cmd).args(&args).spawn().is_ok() {
                ok = true;
                break;
            }
        }
        if !ok {
            tracing::warn!("no file manager to open the Recycle Bin");
        }
        cosmic::Action::App(Message::Noop)
    })
}
