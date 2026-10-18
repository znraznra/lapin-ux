// SPDX-License-Identifier: GPL-3.0-only
//! Desktop icons: the contents of ~/Desktop on the main monitor, above the
//! wallpaper and below windows. Click selects, double-click opens, drag
//! moves an icon to another grid cell, right-click opens a menu, F2 renames,
//! Delete moves to the trash. Right-click on empty space offers view and
//! sort options, New folder / text document, and settings shortcuts.

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
use cosmic::iced::{Alignment, Background, Border, Color, Length, Point, Rectangle, window};
use cosmic::widget::{self, column, container, icon, mouse_area, row, text, text_input};
use cosmic::{Element, theme};
use lapin_ux_core::desktop::{self as model, Cell, DesktopItem, SortBy};
use cosmic::cctk::wayland_client::protocol::wl_output::WlOutput;

use crate::app::{Message, PopupKind, Shell, rect_key};
use crate::config::{DesktopSlot, IconSize};
use crate::taskbar_view::{menu_check, menu_divider, menu_item, panel};

static RENAME_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("lapin-ux-desktop-rename"));

const MARGIN: f32 = 8.0;
const DRAG_THRESHOLD: f32 = 6.0;

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
    /// The small text box shown over an icon's label while renaming.
    pub rename_surface: Option<window::Id>,
    rename_cell: Cell,
    /// A right-click on an icon also reaches the desktop behind it; this
    /// keeps the second one from replacing the icon's menu.
    item_context_at: std::time::Instant,
}

struct Drag {
    path: PathBuf,
    origin: Point,
    active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuPage {
    Main,
    View,
    Sort,
    New,
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
    Settings(&'static str),
    Terminal,
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
            height: size.1.saturating_sub(crate::style::TASKBAR_HEIGHT) as f32,
            dir,
            items: Vec::new(),
            stamp: None,
            selected: HashSet::new(),
            rename: None,
            drag: None,
            pointer: Point::ORIGIN,
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
            d.height = size.1.saturating_sub(crate::style::TASKBAR_HEIGHT) as f32;
        }
    }

    fn reload_desktop(&mut self) {
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
        let paths: HashSet<&PathBuf> = d.items.iter().map(|s| &s.item.path).collect();
        d.selected.retain(|p| paths.contains(p));
    }

    /// Called every second: pick up files added or removed by other apps.
    pub(crate) fn desktop_tick(&mut self) {
        let Some(d) = &self.desktop else { return };
        if d.drag.is_some() || d.rename.is_some() {
            return;
        }
        if std::fs::metadata(&d.dir).and_then(|m| m.modified()).ok() != d.stamp {
            self.reload_desktop();
        }
    }

    fn saved_cells(&self) -> HashMap<String, Cell> {
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

    fn selection(&self) -> Vec<PathBuf> {
        self.desktop.as_ref().map(|d| d.selected.iter().cloned().collect()).unwrap_or_default()
    }

    /// Rename in a small Top-layer box over the icon's label. The desktop
    /// itself sits on the Bottom layer, where the compositor never hands it
    /// the keyboard on its own; a newly mapped Top-layer surface with
    /// on-demand keyboard gets focus at once, and clicking anywhere else
    /// unfocuses it, which commits the new name (like the benchmark).
    fn start_rename(&mut self, path: PathBuf, after_menu: bool) -> Task<Message> {
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
        Task::batch([text_input::focus(RENAME_ID.clone()), text_input::select_all(RENAME_ID.clone())])
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
                let Some(d) = &mut self.desktop else { return commit };
                d.selected = HashSet::from([path.clone()]);
                d.drag = Some(Drag { path, origin: d.pointer, active: false });
                return Task::batch([commit, self.close_popup()]);
            }
            DesktopMsg::Empty => {
                let commit = if d.rename.is_some() { self.update_desktop(DesktopMsg::RenameCommit) } else { Task::none() };
                if let Some(d) = &mut self.desktop {
                    d.selected.clear();
                }
                return Task::batch([commit, self.close_popup()]);
            }
            DesktopMsg::ShowRenameBox => return self.show_rename_box(),
            DesktopMsg::Move(p) => {
                d.pointer = p;
                if let Some(drag) = &mut d.drag
                    && !drag.active
                    && (p.x - drag.origin.x).hypot(p.y - drag.origin.y) > DRAG_THRESHOLD
                {
                    drag.active = true;
                }
            }
            DesktopMsg::Release => {
                let Some(drag) = d.drag.take() else { return Task::none() };
                if drag.active {
                    let cell = self.cell_at(self.desktop.as_ref().map_or(Point::ORIGIN, |d| d.pointer));
                    let name = drag.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    // Dropping on another icon swaps places.
                    let items: Vec<DesktopItem> = self.desktop.as_ref().map(|d| d.items.iter().map(|s| s.item.clone()).collect()).unwrap_or_default();
                    let (cols, rows) = self.grid();
                    let placed = model::arrange(&items, &self.saved_cells(), cols, rows);
                    let from = placed.iter().find(|(i, _)| items[*i].path == drag.path).map(|(_, c)| *c);
                    let other = placed.iter().find(|(_, c)| *c == cell).map(|(i, _)| items[*i].file_name.clone());
                    // Pin every icon's current cell so the move doesn't reshuffle the rest.
                    self.state.desktop_cells = placed
                        .iter()
                        .map(|(i, (c, r))| DesktopSlot { name: items[*i].file_name.clone(), col: *c, row: *r })
                        .collect();
                    if let (Some(other), Some(from)) = (other, from)
                        && other != name
                    {
                        self.remember_cell(&other, from);
                    }
                    self.remember_cell(&name, cell);
                }
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
                if let Some(p) = &mut self.popup
                    && let PopupKind::DesktopMenu(_, pg) = &mut p.kind
                {
                    *pg = page;
                }
            }
            DesktopMsg::Key(key) => match key {
                Named::Escape => {
                    if d.rename.take().is_none() {
                        d.selected.clear();
                    }
                }
                Named::Enter if d.rename.is_some() => return self.update_desktop(DesktopMsg::RenameCommit),
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
    let dragging = d.drag.as_ref().filter(|g| g.active);
    let target = dragging.map(|_| shell.cell_at(d.pointer));

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
                        Look::Named(n) => icon(
                            icon::from_name(n.clone())
                                .fallback(Some(icon::IconFallback::Names(vec!["text-x-generic".into()])))
                                .handle(),
                        )
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
                    mouse_area(boxed)
                        .on_press(Message::Desktop(DesktopMsg::Press(path.clone())))
                        .on_double_click(Message::Desktop(DesktopMsg::Open(path.clone())))
                        .on_right_press(Message::Desktop(DesktopMsg::Context(Some(path))))
                        .into()
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
    let area = mouse_area(container(grid).padding(MARGIN as u16).width(Length::Fill).height(Length::Fill))
        .on_press(Message::Desktop(DesktopMsg::Empty))
        .on_right_press(Message::Desktop(DesktopMsg::Context(None)))
        .on_release(Message::Desktop(DesktopMsg::Release))
        .on_move(|p| Message::Desktop(DesktopMsg::Move(p)));
    area.into()
}

/// The rename box (its own surface, see `start_rename`).
pub fn rename_view(shell: &Shell) -> Element<'_, Message> {
    let Some((_, name)) = shell.desktop.as_ref().and_then(|d| d.rename.as_ref()) else { return widget::Space::new().into() };
    container(
        text_input::text_input("", name)
            .id(RENAME_ID.clone())
            .on_input(|v| Message::Desktop(DesktopMsg::RenameInput(v)))
            .on_submit(|_| Message::Desktop(DesktopMsg::RenameCommit))
            .width(Length::Fill),
    )
    .padding(1)
    .into()
}

/// Right-click menus for the desktop and its icons.
pub fn menu<'a>(shell: &'a Shell, on: &'a Option<PathBuf>, page: &MenuPage) -> Element<'a, Message> {
    let m = |x: DesktopMsg| Message::Desktop(x);
    let back = || menu_item("go-previous-symbolic", "Back", m(DesktopMsg::Menu(MenuPage::Main)));
    let items: Vec<Element<'a, Message>> = match (on, page) {
        (Some(path), _) => {
            let launcher = shell
                .desktop
                .as_ref()
                .and_then(|d| d.items.iter().find(|s| &s.item.path == path))
                .and_then(|s| s.launch.as_ref());
            let mut v = vec![menu_item("document-open-symbolic", "Open", m(DesktopMsg::Open(path.clone())))];
            if matches!(launcher, Some(Launch::Exec { trusted: false, .. })) {
                v.push(menu_item("emblem-ok-symbolic", "Allow launching", m(DesktopMsg::AllowLaunching)));
            }
            v.extend([
                menu_divider(),
                menu_item("edit-copy-symbolic", "Copy as path", m(DesktopMsg::CopyPath)),
                menu_item("edit-symbolic", "Rename", m(DesktopMsg::RenameStart)),
                menu_item("user-trash-symbolic", "Delete", m(DesktopMsg::Trash)),
            ]);
            v
        }
        (None, MenuPage::Main) => vec![
            menu_item("view-grid-symbolic", "View \u{203A}", m(DesktopMsg::Menu(MenuPage::View))),
            menu_item("view-sort-ascending-symbolic", "Sort by \u{203A}", m(DesktopMsg::Menu(MenuPage::Sort))),
            menu_item("view-refresh-symbolic", "Refresh", m(DesktopMsg::Refresh)),
            menu_divider(),
            menu_item("list-add-symbolic", "New \u{203A}", m(DesktopMsg::Menu(MenuPage::New))),
            menu_divider(),
            menu_item("preferences-desktop-display-symbolic", "Display settings", m(DesktopMsg::Settings("displays"))),
            menu_item("preferences-desktop-wallpaper-symbolic", "Personalize", m(DesktopMsg::Settings("wallpaper"))),
            menu_item("utilities-terminal-symbolic", "Open in Terminal", m(DesktopMsg::Terminal)),
        ],
        (None, MenuPage::View) => {
            let s = shell.config.desktop_icon_size;
            vec![
                back(),
                menu_divider(),
                menu_check("Large icons", s == IconSize::Large, m(DesktopMsg::Size(IconSize::Large))),
                menu_check("Medium icons", s == IconSize::Medium, m(DesktopMsg::Size(IconSize::Medium))),
                menu_check("Small icons", s == IconSize::Small, m(DesktopMsg::Size(IconSize::Small))),
                menu_divider(),
                menu_check("Show desktop icons", !shell.config.hide_desktop_icons, m(DesktopMsg::ToggleIcons)),
            ]
        }
        (None, MenuPage::Sort) => {
            let s = shell.config.desktop_sort;
            vec![
                back(),
                menu_divider(),
                menu_check("Name", s == SortBy::Name, m(DesktopMsg::Sort(SortBy::Name))),
                menu_check("Date modified", s == SortBy::Modified, m(DesktopMsg::Sort(SortBy::Modified))),
                menu_check("Item type", s == SortBy::Type, m(DesktopMsg::Sort(SortBy::Type))),
            ]
        }
        (None, MenuPage::New) => vec![
            back(),
            menu_divider(),
            menu_item("folder-new-symbolic", "Folder", m(DesktopMsg::NewFolder)),
            menu_item("text-x-generic-symbolic", "Text Document", m(DesktopMsg::NewText)),
        ],
    };
    panel(column::with_children(items).spacing(2), 240.0)
}
