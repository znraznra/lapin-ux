// SPDX-License-Identifier: GPL-3.0-only
//! "Automatically hide the taskbar": the bar disappears and gives its space
//! to windows; touching the bottom edge brings it back. It stays up while Start, one of its menus or a panel is open.

use std::time::Duration;

use cosmic::app::Task;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::IcedMargin;
use cosmic::iced::platform_specific::shell::commands::layer_surface::{set_exclusive_zone, set_input_zone, set_size, Anchor};
use cosmic::iced::Rectangle;
use cosmic::iced::window;

use crate::app::{Message, Shell};
use crate::config::TaskbarPosition;
use crate::style;

/// Height of the edge that still reacts while hidden.
pub const STRIP: u32 = 2;
const HIDE_DELAY: Duration = Duration::from_millis(600);

impl Shell {
    /// How thick the bar is: its height on a top or bottom bar, its width on a side bar (#41).
    pub(crate) fn bar_thickness(&self) -> u32 {
        if self.vertical() { style::taskbar_width() } else { style::taskbar_height() }
    }

    /// Distance from the bar's edge for Start and panels: clear of the bar even
    /// when the bar doesn't reserve space.
    pub(crate) fn above_bar(&self) -> i32 {
        12 + if self.config.auto_hide { self.bar_thickness() as i32 } else { 0 }
    }

    /// The taskbar is at the top of the screen (#41).
    pub(crate) fn top_bar(&self) -> bool {
        self.config.position == TaskbarPosition::Top
    }

    /// The taskbar runs up and down the left or right edge (#41).
    pub(crate) fn vertical(&self) -> bool {
        self.config.position.vertical()
    }

    /// The anchor of the bar's own surface: the whole edge it sits on.
    pub(crate) fn bar_anchor(&self) -> Anchor {
        match self.config.position {
            TaskbarPosition::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            TaskbarPosition::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            TaskbarPosition::Left => Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
            TaskbarPosition::Right => Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM,
        }
    }

    /// Where Start and panels sit: the screen edge the bar is on (on a side
    /// bar, its bottom corner, next to the tray).
    pub(crate) fn edge(&self) -> Anchor {
        match self.config.position {
            TaskbarPosition::Bottom => Anchor::BOTTOM,
            TaskbarPosition::Top => Anchor::TOP,
            TaskbarPosition::Left => Anchor::LEFT | Anchor::BOTTOM,
            TaskbarPosition::Right => Anchor::RIGHT | Anchor::BOTTOM,
        }
    }

    /// `edge()` plus the right-hand side of a horizontal bar (a side bar's
    /// own side already is the corner).
    pub(crate) fn edge_right(&self) -> Anchor {
        if self.vertical() { self.edge() } else { self.edge() | Anchor::RIGHT }
    }

    /// `edge()` plus the left-hand side of a horizontal bar.
    pub(crate) fn edge_left(&self) -> Anchor {
        if self.vertical() { self.edge() } else { self.edge() | Anchor::LEFT }
    }

    /// A margin of `edge` px on the taskbar's side of the screen.
    pub(crate) fn edge_margin(&self, edge: i32, left: i32, right: i32) -> IcedMargin {
        margin_on_edge(self.config.position, edge, left, right)
    }

    /// Where the sliver that brings back a hidden bar is, inside the bar's surface.
    pub(crate) fn hidden_strip(&self) -> Rectangle {
        let t = self.bar_thickness() as f32;
        let s = STRIP as f32;
        match self.config.position {
            TaskbarPosition::Bottom => Rectangle { x: 0.0, y: t - s, width: 100_000.0, height: s },
            TaskbarPosition::Top => Rectangle { x: 0.0, y: 0.0, width: 100_000.0, height: s },
            TaskbarPosition::Left => Rectangle { x: 0.0, y: 0.0, width: s, height: 100_000.0 },
            TaskbarPosition::Right => Rectangle { x: t - s, y: 0.0, width: s, height: 100_000.0 },
        }
    }

    fn bar_needed(&self, id: window::Id) -> bool {
        let Some(bar) = self.bars.iter().find(|b| b.id == id) else { return false };
        bar.hovered
            || self.start.is_some()
            || self.panel.is_some()
            || self.popup.as_ref().is_some_and(|p| p.parent == id)
            || self.drag.as_ref().is_some_and(|d| d.bar == id)
    }

    /// The bar keeps its full size (resizing a layer surface under the
    /// pointer confuses input routing); hidden, it draws nothing and only
    /// takes the pointer along the bottom edge, so windows underneath get
    /// every other click.
    fn resize_bar(&self, id: window::Id, revealed: bool) -> Task<Message> {
        let zone = if self.config.auto_hide { 0 } else { self.bar_thickness() as i32 };
        let t = self.bar_thickness() as f32;
        let input = if revealed || !self.config.auto_hide {
            if self.vertical() {
                Rectangle { x: 0.0, y: 0.0, width: t, height: 100_000.0 }
            } else {
                Rectangle { x: 0.0, y: 0.0, width: 100_000.0, height: t }
            }
        } else {
            self.hidden_strip()
        };
        Task::batch([set_exclusive_zone(id, zone), set_input_zone(id, Some(vec![input]))])
    }

    pub(crate) fn resize_bar_pub(&self, id: window::Id, revealed: bool) -> Task<Message> {
        self.resize_bar(id, revealed)
    }

    /// Hidden bars draw nothing (and get no blur behind them).
    pub(crate) fn bar_visible(&self, id: window::Id) -> bool {
        !self.config.auto_hide || self.bars.iter().any(|b| b.id == id && b.revealed)
    }

    /// After every update: reveal at once when needed; hiding waits a moment.
    pub(crate) fn sync_autohide(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        let ids: Vec<window::Id> = self.bars.iter().map(|b| b.id).collect();
        for id in ids {
            let needed = !self.config.auto_hide || self.bar_needed(id);
            let Some(bar) = self.bars.iter_mut().find(|b| b.id == id) else { continue };
            if needed && !bar.revealed {
                tracing::debug!("taskbar: reveal");
                bar.revealed = true;
                bar.hide_pending = false;
                tasks.push(self.resize_bar(id, true));
            } else if !needed && bar.revealed && !bar.hide_pending {
                bar.hide_pending = true;
                tasks.push(Task::future(async move {
                    tokio::time::sleep(HIDE_DELAY).await;
                    cosmic::Action::App(Message::AutoHide(id))
                }));
            }
        }
        Task::batch(tasks)
    }

    pub(crate) fn auto_hide_now(&mut self, id: window::Id) -> Task<Message> {
        let still_needed = !self.config.auto_hide || self.bar_needed(id);
        let Some(bar) = self.bars.iter_mut().find(|b| b.id == id) else { return Task::none() };
        bar.hide_pending = false;
        if still_needed || !bar.revealed {
            return Task::none();
        }
        bar.revealed = false;
        tracing::debug!("taskbar: hide");
        self.resize_bar(id, false)
    }

    pub(crate) fn bar_hover(&mut self, id: window::Id, inside: bool) {
        if self.bars.iter().any(|b| b.id == id) {
            tracing::debug!(?id, inside, "taskbar hover");
        }
        if let Some(bar) = self.bars.iter_mut().find(|b| b.id == id) {
            bar.hovered = inside;
        }
    }

    /// The taskbar was moved to another edge (#41): re-anchor every bar and
    /// move its strip; open pop-ups and panels close, they would be on the wrong side.
    pub(crate) fn position_changed(&mut self) -> Task<Message> {
        use cosmic::iced::platform_specific::shell::commands::layer_surface::set_anchor;
        let anchor = self.bar_anchor();
        let mut tasks = vec![self.close_start(), self.close_popup(), self.close_panel()];
        let ids: Vec<window::Id> = self.bars.iter().map(|b| b.id).collect();
        for id in ids {
            tasks.push(set_anchor(id, anchor));
            // The bar's thickness is a height on top or bottom, a width on a side.
            tasks.push(if self.vertical() {
                set_size(id, Some(self.bar_thickness()), None)
            } else {
                set_size(id, None, Some(self.bar_thickness()))
            });
            let revealed = self.bars.iter().find(|b| b.id == id).is_none_or(|b| b.revealed);
            tasks.push(self.resize_bar(id, revealed));
        }
        Task::batch(tasks)
    }

    /// The setting was switched: resize every bar to match.
    pub(crate) fn auto_hide_changed(&mut self) -> Task<Message> {
        let ids: Vec<window::Id> = self.bars.iter().map(|b| b.id).collect();
        let mut tasks = Vec::new();
        for id in ids {
            if let Some(bar) = self.bars.iter_mut().find(|b| b.id == id) {
                bar.revealed = true;
                bar.hide_pending = false;
            }
            tasks.push(self.resize_bar(id, true));
        }
        tasks.push(self.sync_autohide());
        Task::batch(tasks)
    }
}

/// `edge` px on the margin facing the bar: top, bottom, or a side. A side
/// bar's Start and panels also sit 12 px off the bottom of the screen.
fn margin_on_edge(position: TaskbarPosition, edge: i32, left: i32, right: i32) -> IcedMargin {
    match position {
        TaskbarPosition::Bottom => IcedMargin { top: 0, right, bottom: edge, left },
        TaskbarPosition::Top => IcedMargin { top: edge, right, bottom: 0, left },
        TaskbarPosition::Left => IcedMargin { top: 0, right: 0, bottom: 12, left: edge },
        TaskbarPosition::Right => IcedMargin { top: 0, right: edge, bottom: 12, left: 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::margin_on_edge;
    use crate::config::TaskbarPosition::*;

    #[test]
    fn margin_follows_the_bar() {
        let b = margin_on_edge(Bottom, 12, 5, 7);
        assert_eq!((b.top, b.bottom, b.left, b.right), (0, 12, 5, 7));
        let t = margin_on_edge(Top, 12, 5, 7);
        assert_eq!((t.top, t.bottom, t.left, t.right), (12, 0, 5, 7));
        let l = margin_on_edge(Left, 60, 5, 7);
        assert_eq!((l.top, l.bottom, l.left, l.right), (0, 12, 60, 0));
        let r = margin_on_edge(Right, 60, 5, 7);
        assert_eq!((r.top, r.bottom, r.left, r.right), (0, 12, 0, 60));
    }
}
