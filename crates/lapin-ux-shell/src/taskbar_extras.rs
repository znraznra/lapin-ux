// SPDX-License-Identifier: GPL-3.0-only
//! Taskbar behaviours beyond click-to-open: drag to reorder pins, the
//! taskbar's own right-click menu, Super+1…9, and tray overflow.

use std::time::{Duration, Instant};

use cosmic::app::Task;
use cosmic::iced::{Point, Rectangle, window};

use crate::app::{Message, PopupKind, Shell, rect_key};
use crate::config::TaskbarAlignment;
use crate::wayland::WaylandRequest;
use lapin_ux_core::taskbar::ClickAction;

/// Pointer events on a taskbar surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointer {
    Down,
    Move(Point),
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarOption {
    Align(TaskbarAlignment),
    ToggleSearch,
    ToggleDesktopButton,
    ToggleAutoHide,
    /// Open the "Show taskbar apps on" submenu.
    DisplaysMenu,
    MultiDisplay(lapin_ux_core::taskbar::MultiDisplay),
    /// Open the "Combine taskbar buttons" / "Smaller buttons" submenus.
    CombineMenu,
    SmallMenu,
    Combine(lapin_ux_core::taskbar::Combine),
    Small(lapin_ux_core::taskbar::SmallButtons),
}

#[derive(Debug, Clone)]
pub struct Drag {
    pub bar: window::Id,
    pub app: String,
    origin: Point,
    pub active: bool,
}

/// Pixels the pointer must travel before a press becomes a drag.
const DRAG_THRESHOLD: f32 = 6.0;
/// How long a dragged file must rest on a button to bring its window up.
const DRAG_HOVER_DELAY: Duration = Duration::from_millis(600);

impl Shell {
    pub(crate) fn bar_pointer(&mut self, surface: window::Id, ev: Pointer) -> Task<Message> {
        if !self.bars.iter().any(|b| b.id == surface) {
            return Task::none();
        }
        match ev {
            Pointer::Move(p) => {
                self.pointer = Some((surface, p));
                // The far corner ("Show desktop"): resting there peeks.
                let width = self.bars.iter().find(|b| b.id == surface).and_then(|b| b.output_width);
                let in_corner = self.config.show_desktop_corner && width.is_some_and(|w| p.x >= w as f32 - 8.0);
                if in_corner != self.extras.in_corner {
                    self.extras.in_corner = in_corner;
                    return self.update_extras(crate::comp_extras::ExtrasMsg::CornerHover(in_corner));
                }
                let Some(drag) = &mut self.drag else { return Task::none() };
                if drag.bar != surface {
                    return Task::none();
                }
                if !drag.active && (p.x - drag.origin.x).hypot(p.y - drag.origin.y) > DRAG_THRESHOLD {
                    drag.active = true;
                    let close = self.close_popup();
                    return close;
                }
                if drag.active {
                    let app = drag.app.clone();
                    let target = self
                        .taskbar
                        .pinned()
                        .iter()
                        .position(|other| {
                            self.rects
                                .get(&rect_key(surface, other))
                                .is_some_and(|r| p.x >= r.x && p.x < r.x + r.width)
                        });
                    if let Some(to) = target {
                        self.taskbar.move_pinned(&app, to);
                    }
                }
                Task::none()
            }
            Pointer::Down => {
                let Some((s, p)) = self.pointer else { return Task::none() };
                if s != surface {
                    return Task::none();
                }
                let hit = self.taskbar.pinned().iter().find(|app| {
                    self.rects.get(&rect_key(surface, app)).is_some_and(|r| r.contains(p))
                });
                self.drag = hit.map(|app| Drag { bar: surface, app: app.clone(), origin: p, active: false });
                Task::none()
            }
            Pointer::Up => {
                if let Some(drag) = self.drag.take()
                    && drag.active
                {
                    // The release also ends a button press; don't treat it as a click.
                    self.suppress_click_until = Instant::now() + Duration::from_millis(250);
                    self.config.pinned_taskbar = self.taskbar.pinned().to_vec();
                    self.persist_config();
                }
                Task::none()
            }
        }
    }

    pub(crate) fn click_suppressed(&self) -> bool {
        Instant::now() < self.suppress_click_until
    }

    /// Super+N: open, focus, minimize or cycle the N-th taskbar app.
    pub(crate) fn taskbar_item(&mut self, n: usize) -> Task<Message> {
        let Some(button) = self.taskbar.buttons().into_iter().nth(n.saturating_sub(1)) else {
            return Task::none();
        };
        let close = Task::batch([self.close_start(), self.close_popup(), self.close_panel()]);
        let next = match self.taskbar.click(&button.app) {
            ClickAction::Launch(id) => return Task::batch([close, self.launch(&id)]),
            ClickAction::Activate(h) => WaylandRequest::Activate(h),
            ClickAction::Minimize(h) => WaylandRequest::Minimize(h),
            ClickAction::ShowPicker => {
                // Several windows: each press brings the next one forward.
                let wins = &button.windows;
                let cur = wins.iter().position(|w| w.active);
                let pick = cur.map_or(0, |i| (i + 1) % wins.len());
                WaylandRequest::Activate(wins[pick].handle)
            }
        };
        self.request(next);
        close
    }

    /// Dragging a file onto a running app's button brings its window up
    /// after a moment, so the file can be dropped into it.
    pub(crate) fn drag_hover(&mut self, app: String, inside: bool) -> Task<Message> {
        if !inside {
            if self.drag_hover.as_ref().is_some_and(|(a, _)| *a == app) {
                self.drag_hover = None;
            }
            return Task::none();
        }
        self.drag_serial += 1;
        let serial = self.drag_serial;
        self.drag_hover = Some((app, serial));
        Task::future(async move {
            tokio::time::sleep(DRAG_HOVER_DELAY).await;
            cosmic::Action::App(Message::DragHoverFire(serial))
        })
    }

    pub(crate) fn drag_hover_fire(&mut self, serial: u64) -> Task<Message> {
        let Some((app, s)) = self.drag_hover.take() else { return Task::none() };
        if s != serial {
            self.drag_hover = Some((app, s));
            return Task::none();
        }
        let Some(button) = self.taskbar.buttons().into_iter().find(|b| b.app == app) else { return Task::none() };
        let pick = button.windows.iter().find(|w| w.active).or_else(|| button.windows.first());
        if let Some(w) = pick {
            self.request(WaylandRequest::Activate(w.handle));
        }
        Task::none()
    }

    pub(crate) fn note_item_context(&mut self) {
        self.item_context_at = Instant::now();
    }

    /// Right-click on empty taskbar space. A right-click on a button
    /// bubbles here too, right after the button's own menu opened.
    pub(crate) fn taskbar_context(&mut self, bar: window::Id) -> Task<Message> {
        if self.item_context_at.elapsed() < Duration::from_millis(150) {
            return Task::none();
        }
        let at = match self.pointer {
            Some((s, p)) if s == bar => p,
            _ => Point::new(0.0, 0.0),
        };
        let key = rect_key(bar, "pointer");
        self.rects.insert(key.clone(), Rectangle { x: at.x, y: at.y, width: 1.0, height: 1.0 });
        self.open_popup(bar, &key, PopupKind::TaskbarMenu)
    }

    pub(crate) fn taskbar_option(&mut self, opt: TaskbarOption) -> Task<Message> {
        match opt {
            TaskbarOption::Align(a) => self.config.alignment = a,
            TaskbarOption::ToggleSearch => self.config.show_search = !self.config.show_search,
            TaskbarOption::ToggleDesktopButton => self.config.show_desktop_corner = !self.config.show_desktop_corner,
            TaskbarOption::DisplaysMenu => return self.open_submenu("tb-displays", PopupKind::TaskbarDisplays),
            TaskbarOption::MultiDisplay(m) => self.config.multi_display = m,
            TaskbarOption::CombineMenu => return self.open_submenu("tb-combine", PopupKind::TaskbarCombine),
            TaskbarOption::SmallMenu => return self.open_submenu("tb-small", PopupKind::TaskbarSmall),
            TaskbarOption::Combine(c) => self.config.combine_buttons = c,
            TaskbarOption::Small(s) => self.config.small_buttons = s,
            TaskbarOption::ToggleAutoHide => {
                self.config.auto_hide = !self.config.auto_hide;
                self.persist_config();
                let close = self.close_popup();
                return Task::batch([close, self.auto_hide_changed()]);
            }
        }
        self.persist_config();
        self.close_popup()
    }

    /// Tray icons are inline when their stable id is listed in the config.
    pub(crate) fn tray_inline(&self, entry: &crate::app::TrayEntry) -> bool {
        entry.id.as_ref().is_some_and(|id| self.config.tray_inline.contains(id))
    }

    pub(crate) fn tray_pin(&mut self, id: String, inline: bool) -> Task<Message> {
        self.config.tray_inline.retain(|i| i != &id);
        if inline {
            self.config.tray_inline.push(id);
        }
        self.persist_config();
        self.close_popup()
    }

    pub(crate) fn tray_overflow(&mut self, bar: window::Id) -> Task<Message> {
        let key = rect_key(bar, "tray-overflow");
        self.open_popup(bar, &key, PopupKind::TrayOverflow)
    }
}
