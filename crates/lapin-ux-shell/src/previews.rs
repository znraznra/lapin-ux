// SPDX-License-Identifier: GPL-3.0-only
//! Window previews: thumbnails when hovering a taskbar button, and the
//! Alt+Tab switcher. Pictures come from the window-tracking thread
//! (ext-image-copy-capture of each toplevel), captured on demand.

use std::sync::Arc;
use std::time::Duration;

use cosmic::app::Task;
use cosmic::iced::{Alignment, Length, window};
use cosmic::widget::{self, button, column, container, icon, mouse_area, row, text};
use cosmic::Element;

use crate::app::{Message, Popup, PopupKind, Shell, rect_key};
use crate::panels::{Panel, PanelKind};
use crate::style;
use crate::taskbar_view::{app_icon, app_name};
use crate::wayland::{WaylandRequest, WindowId};

pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub handle: widget::image::Handle,
}

/// Taskbar hover state.
pub struct Hover {
    bar: window::Id,
    app: String,
    serial: u64,
    on_button: bool,
    in_popup: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AltTab {
    pub order: Vec<WindowId>,
    pub selected: usize,
    /// Alt was seen held after the switcher got the keyboard.
    saw_alt: bool,
}

#[derive(Debug, Clone)]
pub enum PreviewMsg {
    Thumbnail(WindowId, u32, u32, Arc<Vec<u8>>),
    ButtonHover(window::Id, String, bool),
    PopupHover(bool),
    Show(u64),
    Hide(u64),
    /// Alt+Tab (or Alt+Shift+Tab with `true`).
    AltTab(bool),
    AltTabPick(WindowId),
    /// Alt went up / down while the switcher has the keyboard.
    Alt(window::Id, bool),
    /// Quick Alt+Tab tap: Alt may be released before the switcher appears.
    AltTabCheck,
    /// The pointer left a surface (a button's own "exit" never fires when
    /// the pointer jumps straight off the taskbar).
    SurfaceLeft(window::Id),
}

const SHOW_DELAY: Duration = Duration::from_millis(400);
const HIDE_DELAY: Duration = Duration::from_millis(300);

fn later(d: Duration, m: PreviewMsg) -> Task<Message> {
    Task::future(async move {
        tokio::time::sleep(d).await;
        cosmic::Action::App(Message::Preview(m))
    })
}

impl Shell {
    fn capture(&self, ids: impl IntoIterator<Item = WindowId>) {
        for id in ids {
            self.request(WaylandRequest::Capture(id));
        }
    }

    fn app_windows(&self, app: &str) -> Vec<WindowId> {
        self.taskbar.windows().iter().filter(|w| w.app == app).map(|w| w.handle).collect()
    }

    pub(crate) fn note_activation(&mut self, id: WindowId) {
        self.mru.retain(|w| *w != id);
        self.mru.insert(0, id);
    }

    pub(crate) fn forget_window(&mut self, id: WindowId) {
        self.mru.retain(|w| *w != id);
        self.thumbs.remove(&id);
    }

    fn preview_open(&self) -> Option<&str> {
        match &self.popup {
            Some(Popup { kind: PopupKind::Preview(app), .. }) => Some(app),
            _ => None,
        }
    }

    pub(crate) fn update_preview(&mut self, msg: PreviewMsg) -> Task<Message> {
        match msg {
            PreviewMsg::Thumbnail(id, w, h, rgba) => {
                self.thumbs.insert(
                    id,
                    Thumb { width: w, height: h, handle: widget::image::Handle::from_rgba(w, h, rgba.as_ref().clone()) },
                );
            }
            PreviewMsg::ButtonHover(bar, app, true) => {
                if self.start.is_some() || self.click_suppressed() {
                    return Task::none();
                }
                let windows = self.app_windows(&app);
                if windows.is_empty() {
                    return Task::none();
                }
                self.hover_gen += 1;
                let serial = self.hover_gen;
                let in_popup = self.hover.as_ref().is_some_and(|h| h.app == app && h.in_popup);
                self.hover = Some(Hover { bar, app: app.clone(), serial, on_button: true, in_popup });
                self.capture(windows);
                return match self.preview_open() {
                    Some(open) if open == app => Task::none(),
                    // Moving along the taskbar switches previews at once.
                    Some(_) => self.update_preview(PreviewMsg::Show(serial)),
                    None => later(SHOW_DELAY, PreviewMsg::Show(serial)),
                };
            }
            PreviewMsg::ButtonHover(_, app, false) => {
                if let Some(h) = &mut self.hover
                    && h.app == app
                {
                    h.on_button = false;
                    return later(HIDE_DELAY, PreviewMsg::Hide(h.serial));
                }
            }
            PreviewMsg::PopupHover(inside) => {
                if let Some(h) = &mut self.hover {
                    h.in_popup = inside;
                    if !inside {
                        return later(HIDE_DELAY, PreviewMsg::Hide(h.serial));
                    }
                }
            }
            PreviewMsg::Show(serial) => {
                let Some(h) = &self.hover else { return Task::none() };
                if h.serial != serial || !h.on_button {
                    return Task::none();
                }
                // Don't replace a menu the user opened.
                if self.popup.as_ref().is_some_and(|p| !matches!(p.kind, PopupKind::Preview(_))) {
                    return Task::none();
                }
                let (bar, app) = (h.bar, h.app.clone());
                let key = rect_key(bar, &app);
                return self.open_popup(bar, &key, PopupKind::Preview(app));
            }
            PreviewMsg::Hide(serial) => {
                let Some(h) = &self.hover else { return Task::none() };
                if h.serial != serial || h.on_button || h.in_popup {
                    return Task::none();
                }
                self.hover = None;
                if self.preview_open().is_some() {
                    return self.close_popup();
                }
            }
            PreviewMsg::SurfaceLeft(id) => {
                self.bar_hover(id, false);
                if self.extras.in_corner && self.bars.iter().any(|b| b.id == id) {
                    self.extras.in_corner = false;
                    let _ = self.update_extras(crate::comp_extras::ExtrasMsg::CornerHover(false));
                }
                if let Some(h) = &self.hover
                    && h.bar == id
                {
                    let app = h.app.clone();
                    return self.update_preview(PreviewMsg::ButtonHover(id, app, false));
                }
            }
            PreviewMsg::AltTab(back) => return self.alt_tab(back),
            PreviewMsg::AltTabPick(id) => {
                let close = self.close_panel();
                return Task::batch([close, activate_after_close(id)]);
            }
            PreviewMsg::Alt(id, held) => {
                let Some(Panel { id: pid, kind: PanelKind::AltTab(a) }) = &mut self.panel else { return Task::none() };
                if *pid != id {
                    return Task::none();
                }
                if held {
                    a.saw_alt = true;
                } else if a.saw_alt {
                    // On focus the surface first reports "no modifiers", then
                    // Alt; only a release after Alt was seen means "done".
                    return self.alt_tab_commit();
                }
            }
            PreviewMsg::AltTabCheck => {
                tracing::debug!("alt-tab: checking for a quick tap");
                if let Some(Panel { kind: PanelKind::AltTab(a), .. }) = &self.panel
                    && !a.saw_alt
                {
                    return self.alt_tab_commit();
                }
            }
        }
        Task::none()
    }

    /// Close a hover preview when its button is clicked.
    pub(crate) fn close_preview(&mut self) -> Task<Message> {
        self.hover = None;
        if self.preview_open().is_some() { self.close_popup() } else { Task::none() }
    }

    fn alt_tab(&mut self, back: bool) -> Task<Message> {
        if let Some(Panel { kind: PanelKind::AltTab(a), .. }) = &mut self.panel {
            let n = a.order.len().max(1);
            a.selected = if back { (a.selected + n - 1) % n } else { (a.selected + 1) % n };
            return Task::none();
        }
        let existing: Vec<WindowId> = self.taskbar.windows().iter().map(|w| w.handle).collect();
        let mut order: Vec<WindowId> = self.mru.iter().copied().filter(|w| existing.contains(w)).collect();
        let rest: Vec<WindowId> = existing.iter().copied().filter(|w| !order.contains(w)).collect();
        order.extend(rest);
        if order.is_empty() {
            return Task::none();
        }
        self.capture(order.iter().copied());
        let n = order.len();
        let selected = if n == 1 { 0 } else if back { n - 1 } else { 1 };
        let close = Task::batch([self.close_start(), self.close_popup()]);
        Task::batch([close, self.open_panel(PanelKind::AltTab(AltTab { order, selected, saw_alt: false }))])
    }

    fn alt_tab_commit(&mut self) -> Task<Message> {
        let Some(Panel { kind: PanelKind::AltTab(a), .. }) = &self.panel else { return Task::none() };
        tracing::debug!(selected = a.selected, n = a.order.len(), "alt-tab: switching");
        let target = a.order.get(a.selected).copied();
        let close = self.close_panel();
        match target {
            Some(id) => Task::batch([close, activate_after_close(id)]),
            None => close,
        }
    }

    pub(crate) fn alt_tab_key(&mut self, key: cosmic::iced::keyboard::key::Named) -> Task<Message> {
        use cosmic::iced::keyboard::key::Named;
        match key {
            Named::Tab | Named::ArrowRight => self.alt_tab(false),
            Named::ArrowLeft => self.alt_tab(true),
            Named::Enter => self.alt_tab_commit(),
            Named::Escape => self.close_panel(),
            _ => Task::none(),
        }
    }

    pub(crate) fn alt_tab_focused(&self) -> Task<Message> {
        later(Duration::from_millis(350), PreviewMsg::AltTabCheck)
    }
}

/// Closing the switcher makes the compositor give the keyboard back to the
/// window that had it before; activate the chosen one just after that, or
/// it would be raised without getting the focus.
fn activate_after_close(id: WindowId) -> Task<Message> {
    Task::future(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cosmic::Action::App(Message::ActivateWindow(id))
    })
}

pub(crate) fn picture_pub<'a>(shell: &'a Shell, id: WindowId, app: &str, max_w: f32, max_h: f32) -> Element<'a, Message> {
    picture(shell, id, app, max_w, max_h)
}

/// The picture for a window: its thumbnail, or its app icon until one arrives.
fn picture<'a>(shell: &'a Shell, id: WindowId, app: &str, max_w: f32, max_h: f32) -> Element<'a, Message> {
    match shell.thumbs.get(&id) {
        Some(t) => {
            let s = (max_w / t.width as f32).min(max_h / t.height as f32).min(1.0);
            container(widget::image(t.handle.clone()).width(t.width as f32 * s).height(t.height as f32 * s))
                .width(max_w)
                .height(max_h)
                .center(Length::Fill)
                .into()
        }
        None => container(icon(app_icon(shell, app)).size(48)).width(max_w).height(max_h).center(Length::Fill).into(),
    }
}

/// Hover preview above a taskbar button: one card per window.
pub fn preview<'a>(shell: &'a Shell, app: &'a str) -> Element<'a, Message> {
    let cards: Vec<Element<'a, Message>> = shell
        .taskbar
        .windows()
        .iter()
        .filter(|w| w.app == app)
        .map(|w| {
            let title = if w.title.is_empty() { app_name(shell, app) } else { w.title.clone() };
            let head = row::with_children(vec![
                icon(app_icon(shell, app)).size(16).into(),
                text::caption(title).width(Length::Fill).height(18).into(),
                button::custom(icon::from_name("window-close-symbolic").size(12))
                    .padding(3)
                    .class(style::flat_button(false))
                    .on_press(Message::CloseWindow(w.handle))
                    .into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center);
            button::custom(column::with_children(vec![head.into(), picture(shell, w.handle, app, 200.0, 120.0)]).spacing(6))
                .width(220)
                .padding(8)
                .class(style::flat_button(w.active))
                .on_press(Message::ActivateWindow(w.handle))
                .into()
        })
        .collect();
    let body = container(row::with_children(cards).spacing(4)).padding(4).class(style::panel_bg());
    mouse_area(body)
        .on_enter(Message::Preview(PreviewMsg::PopupHover(true)))
        .on_exit(Message::Preview(PreviewMsg::PopupHover(false)))
        .into()
}

/// Alt+Tab: windows in most-recently-used order.
pub fn switcher<'a>(shell: &'a Shell, a: &'a AltTab) -> Element<'a, Message> {
    let per_row = 5;
    let mut rows = column::with_capacity(a.order.len() / per_row + 1).spacing(8);
    let windows = shell.taskbar.windows();
    for (r, chunk) in a.order.chunks(per_row).enumerate() {
        let cards: Vec<Element<'a, Message>> = chunk
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let w = windows.iter().find(|w| w.handle == *id)?;
                let title = if w.title.is_empty() { app_name(shell, &w.app) } else { w.title.clone() };
                let selected = r * per_row + i == a.selected;
                Some(
                    button::custom(
                        column::with_children(vec![
                            row::with_children(vec![
                                icon(app_icon(shell, &w.app)).size(16).into(),
                                text::caption(title).width(Length::Fill).height(18).into(),
                            ])
                            .spacing(8)
                            .align_y(Alignment::Center)
                            .into(),
                            picture(shell, w.handle, &w.app, 220.0, 140.0),
                        ])
                        .spacing(6),
                    )
                    .width(240)
                    .padding(8)
                    .class(style::flat_button(selected))
                    .on_press(Message::Preview(PreviewMsg::AltTabPick(*id)))
                    .into(),
                )
            })
            .collect();
        rows = rows.push(row::with_children(cards).spacing(8));
    }
    container(rows).padding(12).into()
}

/// Width of the switcher for `n` windows.
pub fn switcher_width(n: usize) -> u32 {
    let cols = n.clamp(1, 5) as u32;
    cols * 240 + (cols - 1) * 8 + 24
}
