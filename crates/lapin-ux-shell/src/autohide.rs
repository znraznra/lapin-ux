// SPDX-License-Identifier: GPL-3.0-only
//! "Automatically hide the taskbar": the bar disappears and gives its space
//! to windows; touching the bottom edge brings it back. It stays up while Start, one of its menus or a panel is open.

use std::time::Duration;

use cosmic::app::Task;
use cosmic::iced::platform_specific::shell::commands::layer_surface::{set_exclusive_zone, set_input_zone};
use cosmic::iced::window;

use crate::app::{Message, Shell};
use crate::style;

/// Height of the edge that still reacts while hidden.
pub const STRIP: u32 = 2;
const HIDE_DELAY: Duration = Duration::from_millis(600);

impl Shell {
    /// Bottom margin for Start and panels: above the bar even when the bar
    /// doesn't reserve space.
    pub(crate) fn above_bar(&self) -> i32 {
        12 + if self.config.auto_hide { style::TASKBAR_HEIGHT as i32 } else { 0 }
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
        let zone = if self.config.auto_hide { 0 } else { style::TASKBAR_HEIGHT as i32 };
        let h = style::TASKBAR_HEIGHT as f32;
        let input = if revealed || !self.config.auto_hide {
            cosmic::iced::Rectangle { x: 0.0, y: 0.0, width: 100_000.0, height: h }
        } else {
            cosmic::iced::Rectangle { x: 0.0, y: h - STRIP as f32, width: 100_000.0, height: STRIP as f32 }
        };
        Task::batch([set_exclusive_zone(id, zone), set_input_zone(id, Some(vec![input]))])
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
