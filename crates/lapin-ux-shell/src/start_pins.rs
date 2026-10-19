// SPDX-License-Identifier: GPL-3.0-only
//! Start's pinned grid beyond apps: files, folders and websites, folders
//! of apps (drag one app onto another), dragging to reorder, and pinning
//! by dropping a file or link onto the grid.

use cosmic::app::Task;
use cosmic::iced::{Alignment, Length, Point};
use cosmic::widget::{self, button, column, container, grid, icon, mouse_area, row, text, text_input};
use cosmic::Element;
use lapin_ux_core::pins::{self, Drop, Pin};

use crate::app::{Message, PopupKind, Shell, StartMenu, rect_key};
use crate::start_view::PINNED_COLUMNS;
use crate::style;
use crate::taskbar_extras::Pointer;
use crate::taskbar_view::{app_icon, app_name};

pub const PINNED_MAX: usize = PINNED_COLUMNS * 3;
const DRAG_THRESHOLD: f32 = 6.0;

#[derive(Debug, Clone)]
pub struct PinDrag {
    pub from: usize,
    origin: Point,
    pub active: bool,
    pub target: Option<Drop>,
}

#[derive(Debug, Clone)]
pub enum PinMsg {
    /// Open pin at this (stored) index.
    Open(usize),
    Context(usize),
    Unpin(usize),
    /// Rename the open folder.
    FolderName(String),
    CloseFolder,
    /// Move an app out of the open folder.
    TakeOut(String),
    PinPath(std::path::PathBuf),
    /// Files or links dropped on the pinned grid.
    Dropped(Option<crate::dnd::Uris>),
}

impl Shell {
    fn stored_pins(&self) -> Vec<Pin> {
        pins::decode_all(&self.config.pinned_start)
    }

    fn save_pins(&mut self, list: &[Pin]) {
        self.config.pinned_start = pins::encode_all(list);
        self.persist_config();
    }

    /// Pins shown in Start (with their stored index): apps that aren't
    /// installed and files that are gone are hidden.
    pub(crate) fn visible_pins(&self) -> Vec<(usize, Pin)> {
        self.stored_pins()
            .into_iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let p = match p {
                    Pin::App(ref a) if self.catalog.get(a).is_none() => return None,
                    Pin::Path(ref path) if !path.exists() => return None,
                    Pin::Folder { name, apps } => {
                        let apps: Vec<String> = apps.into_iter().filter(|a| self.catalog.get(a).is_some()).collect();
                        if apps.is_empty() {
                            return None;
                        }
                        Pin::Folder { name, apps }
                    }
                    p => p,
                };
                Some((i, p))
            })
            .take(PINNED_MAX)
            .collect()
    }

    /// Apps of the folder open in Start, if any.
    pub(crate) fn open_folder(&self) -> Option<(usize, String, Vec<String>)> {
        let i = self.start.as_ref()?.folder?;
        self.visible_pins().into_iter().find_map(|(j, p)| match p {
            Pin::Folder { name, apps } if j == i => Some((i, name, apps)),
            _ => None,
        })
    }

    pub(crate) fn pin_app(&mut self, app: String) {
        let mut list = self.stored_pins();
        if !pins::has_app(&list, &app) {
            list.push(Pin::App(app));
            self.save_pins(&list);
        }
    }

    pub(crate) fn unpin_app(&mut self, app: &str) {
        let mut list = self.stored_pins();
        pins::remove_app(&mut list, app);
        self.save_pins(&list);
    }

    pub(crate) fn update_pins(&mut self, msg: PinMsg) -> Task<Message> {
        match msg {
            PinMsg::Open(i) => {
                if self.click_suppressed() {
                    return Task::none();
                }
                let Some(pin) = self.stored_pins().into_iter().nth(i) else { return Task::none() };
                return match pin {
                    Pin::App(id) => self.launch(&id),
                    Pin::Path(path) => cosmic::Application::update(self, Message::OpenFile(path)),
                    Pin::Site { url, .. } => {
                        let close = self.close_start();
                        Task::batch([close, crate::start_search::open_with_default(url.into())])
                    }
                    Pin::Folder { .. } => {
                        if let Some(s) = &mut self.start {
                            s.folder = Some(i);
                            s.selected = None;
                        }
                        Task::none()
                    }
                };
            }
            PinMsg::Context(i) => {
                self.note_item_context();
                let Some(s) = &self.start else { return Task::none() };
                let (parent, key) = (s.id, rect_key(s.id, &format!("pin:{i}")));
                return self.open_popup(parent, &key, PopupKind::PinContext(i));
            }
            PinMsg::Unpin(i) => {
                let mut list = self.stored_pins();
                if i < list.len() {
                    list.remove(i);
                    self.save_pins(&list);
                }
                return self.close_popup();
            }
            PinMsg::FolderName(name) => {
                let Some(i) = self.start.as_ref().and_then(|s| s.folder) else { return Task::none() };
                let mut list = self.stored_pins();
                if let Some(Pin::Folder { name: n, .. }) = list.get_mut(i) {
                    *n = name;
                    self.save_pins(&list);
                }
            }
            PinMsg::CloseFolder => {
                if let Some(s) = &mut self.start {
                    s.folder = None;
                    s.selected = None;
                }
            }
            PinMsg::TakeOut(app) => {
                let Some(i) = self.start.as_ref().and_then(|s| s.folder) else { return self.close_popup() };
                let mut list = self.stored_pins();
                pins::take_out(&mut list, i, &app);
                self.save_pins(&list);
                // The folder may have dissolved.
                if !matches!(list.get(i), Some(Pin::Folder { .. }))
                    && let Some(s) = &mut self.start
                {
                    s.folder = None;
                }
                return self.close_popup();
            }
            PinMsg::PinPath(path) => {
                let mut list = self.stored_pins();
                if !list.contains(&Pin::Path(path.clone())) {
                    list.push(Pin::Path(path));
                    self.save_pins(&list);
                }
                return self.close_popup();
            }
            PinMsg::Dropped(uris) => {
                let mut list = self.stored_pins();
                for pin in uris.into_iter().flat_map(|u| u.0).filter_map(|u| pins::pin_for_uri(&u)) {
                    if !list.contains(&pin) {
                        list.push(pin);
                    }
                }
                self.save_pins(&list);
            }
        }
        Task::none()
    }

    /// Pointer events on the Start surface: drag pinned tiles.
    pub(crate) fn start_pointer(&mut self, ev: Pointer) -> Task<Message> {
        let Some(start) = &self.start else { return Task::none() };
        let sid = start.id;
        match ev {
            Pointer::Move(p) => {
                self.pointer = Some((sid, p));
                let Some(drag) = self.start.as_ref().and_then(|s| s.pin_drag.clone()) else { return Task::none() };
                let mut drag = drag;
                if !drag.active && (p.x - drag.origin.x).hypot(p.y - drag.origin.y) > DRAG_THRESHOLD {
                    drag.active = true;
                }
                if drag.active {
                    drag.target = self.drop_target(sid, p);
                }
                if let Some(s) = &mut self.start {
                    s.pin_drag = Some(drag);
                }
            }
            Pointer::Down => {
                if start.folder.is_some() || !start.query.trim().is_empty() || start.page != crate::app::StartPage::Home {
                    return Task::none();
                }
                let Some((s, p)) = self.pointer else { return Task::none() };
                if s != sid {
                    return Task::none();
                }
                let hit = self
                    .visible_pins()
                    .into_iter()
                    .find(|(i, _)| self.rects.get(&rect_key(sid, &format!("pin:{i}"))).is_some_and(|r| r.contains(p)));
                let drag = hit.map(|(from, _)| PinDrag { from, origin: p, active: false, target: None });
                if let Some(s) = &mut self.start {
                    s.pin_drag = drag;
                }
            }
            Pointer::Up => {
                let Some(drag) = self.start.as_mut().and_then(|s| s.pin_drag.take()) else { return Task::none() };
                if drag.active {
                    self.suppress_click_until = std::time::Instant::now() + std::time::Duration::from_millis(250);
                    if let Some(to) = drag.target {
                        let mut list = self.stored_pins();
                        if pins::drop_pin(&mut list, drag.from, to) {
                            self.save_pins(&list);
                        }
                    }
                }
            }
        }
        Task::none()
    }

    /// Onto a tile's middle: folder; near its left/right edge: move there.
    fn drop_target(&self, sid: cosmic::iced::window::Id, p: Point) -> Option<Drop> {
        let visible = self.visible_pins();
        for (n, (i, _)) in visible.iter().enumerate() {
            let Some(r) = self.rects.get(&rect_key(sid, &format!("pin:{i}"))) else { continue };
            if !r.contains(p) {
                continue;
            }
            let rel = (p.x - r.x) / r.width.max(1.0);
            let after = visible.get(n + 1).map_or(*i + 1, |(j, _)| *j);
            return Some(if rel < 0.25 {
                Drop::Before(*i)
            } else if rel > 0.75 {
                Drop::Before(after)
            } else {
                Drop::Onto(*i)
            });
        }
        None
    }
}

fn tracked<'a>(shell: &'a Shell, key: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    match &shell.tracker {
        Some(t) => t.container(key, content).into(),
        None => content.into(),
    }
}

fn pin_icon(shell: &Shell, pin: &Pin, size: u16) -> Element<'static, Message> {
    match pin {
        Pin::App(a) => icon(app_icon(shell, a)).size(size).into(),
        Pin::Path(p) => {
            icon(crate::icons::for_file(&pin.label(), p.is_dir())).size(size).into()
        }
        Pin::Site { .. } => icon(crate::icons::full("web-browser")).size(size).into(),
        Pin::Folder { apps, .. } => {
            // Up to four app icons, 2×2.
            let mini: Vec<Element<'static, Message>> =
                apps.iter().take(4).map(|a| icon(app_icon(shell, a)).size(size / 2 - 3).into()).collect();
            let mut rows = column::with_capacity(2).spacing(2);
            let mut it = mini.into_iter();
            for _ in 0..2 {
                let r: Vec<Element<'static, Message>> = it.by_ref().take(2).collect();
                if !r.is_empty() {
                    rows = rows.push(row::with_children(r).spacing(2));
                }
            }
            container(rows).padding(2).width(size + 4).height(size + 4).class(style::folder_tile()).into()
        }
    }
}

fn pin_label(shell: &Shell, pin: &Pin) -> String {
    match pin {
        Pin::App(a) => app_name(shell, a),
        p => p.label(),
    }
}

/// One tile of the pinned grid.
fn pin_tile<'a>(shell: &'a Shell, start: &'a StartMenu, i: usize, pin: &Pin, selected: bool) -> Element<'a, Message> {
    let drag = start.pin_drag.as_ref().filter(|d| d.active);
    let lit = selected
        || drag.is_some_and(|d| d.from == i || d.target == Some(Drop::Onto(i)));
    let b = button::custom(
        column::with_children(vec![
            pin_icon(shell, pin, 32),
            text::caption(pin_label(shell, pin)).align_x(Alignment::Center).width(Length::Fill).height(32).into(),
        ])
        .spacing(6)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(88.0))
    .padding([12, 4, 4, 4])
    .class(style::flat_button(lit))
    .on_press(Message::Pins(PinMsg::Open(i)));
    let area = match pin {
        // Apps keep the full app menu (pin to taskbar, uninstall…).
        Pin::App(a) => mouse_area(b).on_right_press(Message::StartContext(a.clone())),
        _ => mouse_area(b).on_right_press(Message::Pins(PinMsg::Context(i))),
    };
    // Apps are also tracked by id, where their context menu opens.
    let inner = match pin {
        Pin::App(a) => tracked(shell, rect_key(start.id, a), area),
        _ => area.into(),
    };
    tracked(shell, rect_key(start.id, &format!("pin:{i}")), inner)
}

/// The pinned grid, or the open folder.
pub fn pinned_block<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    if let Some((_, name, apps)) = shell.open_folder() {
        return folder_view(shell, start, name, apps);
    }
    let pins = shell.visible_pins();
    let content: Element<'_, Message> = if pins.is_empty() {
        text::body("Right-click an app in All apps to pin it here.").class(style::dim_text()).into()
    } else {
        let mut g = grid().column_spacing(4).row_spacing(8);
        for (n, (i, pin)) in pins.iter().enumerate() {
            if n > 0 && n % PINNED_COLUMNS == 0 {
                g = g.insert_row();
            }
            g = g.push(pin_tile(shell, start, *i, pin, start.selected == Some(n)));
        }
        container(g).center_x(Length::Fill).into()
    };
    // Drop a file, folder or link here to pin it.
    cosmic::widget::dnd_destination::dnd_destination_for_data::<crate::dnd::Uris, Message>(
        container(content).width(Length::Fill),
        |uris, _| Message::Pins(PinMsg::Dropped(uris)),
    )
    .into()
}

fn folder_view<'a>(shell: &'a Shell, start: &'a StartMenu, name: String, apps: Vec<String>) -> Element<'a, Message> {
    let mut g = grid().column_spacing(4).row_spacing(8);
    for (n, app) in apps.iter().enumerate() {
        if n > 0 && n % 4 == 0 {
            g = g.insert_row();
        }
        let b = button::custom(
            column::with_children(vec![
                icon(app_icon(shell, app)).size(32).into(),
                text::caption(app_name(shell, app)).align_x(Alignment::Center).width(Length::Fill).height(32).into(),
            ])
            .spacing(6)
            .align_x(Alignment::Center),
        )
        .width(Length::Fixed(88.0))
        .padding([12, 4, 4, 4])
        .class(style::flat_button(start.selected == Some(n)))
        .on_press(Message::Launch(app.clone()));
        g = g.push(tracked(shell, rect_key(start.id, app), mouse_area(b).on_right_press(Message::StartContext(app.clone()))));
    }
    let title = text_input::text_input("Name this folder", name)
        .on_input(|s| Message::Pins(PinMsg::FolderName(s)))
        .width(Length::Fixed(240.0));
    let card = container(
        column::with_children(vec![
            row::with_children(vec![
                widget::Space::new().width(Length::Fill).into(),
                title.into(),
                widget::Space::new().width(Length::Fill).into(),
                button::icon(icon::from_name("window-close-symbolic").size(16))
                    .on_press(Message::Pins(PinMsg::CloseFolder))
                    .into(),
            ])
            .align_y(Alignment::Center)
            .into(),
            container(g).center_x(Length::Fill).into(),
        ])
        .spacing(16),
    )
    .padding(16)
    .max_width(420)
    .class(style::folder_card());
    container(card).center_x(Length::Fill).into()
}

/// Right-click menu for a non-app pin.
pub fn context<'a>(shell: &'a Shell, i: usize) -> Element<'a, Message> {
    use crate::taskbar_view::{menu_item, panel};
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    match pins::decode_all(&shell.config.pinned_start).get(i) {
        Some(Pin::Path(p)) => {
            items.push(menu_item("document-open-symbolic", "Open", Message::Pins(PinMsg::Open(i))));
            items.push(menu_item("folder-open-symbolic", "Open file location", Message::ShowInFolder(p.clone())));
        }
        Some(Pin::Site { .. }) => items.push(menu_item("web-browser-symbolic", "Open", Message::Pins(PinMsg::Open(i)))),
        Some(Pin::Folder { .. }) => items.push(menu_item("folder-open-symbolic", "Open", Message::Pins(PinMsg::Open(i)))),
        _ => {}
    }
    items.push(menu_item("view-pin-symbolic", "Unpin from Start", Message::Pins(PinMsg::Unpin(i))));
    panel(column::with_children(items).spacing(2), 240.0)
}
