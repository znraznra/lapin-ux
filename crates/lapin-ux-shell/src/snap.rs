// SPDX-License-Identifier: GPL-3.0-only
//! Snap layouts (Super+Z) and Snap assist: pick a space in a layout for the
//! window you were using, then fill the other spaces from your other
//! windows. Needs Lapin UX's compositor build (`lapin-ux-comp`).

use cosmic::app::Task;
use cosmic::iced::{Alignment, Background, Border, Color};
use cosmic::widget::{self, button, column, container, icon, row, text};
use cosmic::{Element, theme};
use lapin_ux_core::snap::{LAYOUTS, Zone, layouts_for, next_zone};

use crate::app::{Message, Shell};
use crate::panels::PanelKind;
use crate::style;
use crate::taskbar_view::{app_icon, app_name};
use crate::wayland::{WaylandRequest, WindowId};

pub const WIDTH: u32 = 620;

#[derive(Debug, Clone, PartialEq)]
pub struct SnapPanel {
    /// The window being snapped first.
    pub target: WindowId,
    pub layout: Option<usize>,
    /// Spaces already filled, and the windows put in them.
    pub filled: Vec<usize>,
    pub placed: Vec<WindowId>,
    /// How many layouts this screen gets.
    pub count: usize,
}

#[derive(Debug, Clone)]
pub enum SnapMsg {
    Open,
    Pick(usize, usize),
    Assist(WindowId),
}

impl Shell {
    fn snap_panel(&mut self) -> Option<&mut SnapPanel> {
        match &mut self.panel {
            Some(crate::panels::Panel { kind: PanelKind::Snap(s), .. }) => Some(s),
            _ => None,
        }
    }

    /// Windows that can fill a space: on this desktop, not minimized, not yet placed.
    fn assist_candidates(&self, skip: &[WindowId]) -> Vec<WindowId> {
        let here = self.desktops.list.iter().find(|d| d.active).map(|d| d.id);
        self.mru
            .iter()
            .copied()
            .filter(|id| !skip.contains(id))
            .filter(|id| self.taskbar.windows().iter().any(|w| w.handle == *id && !w.minimized))
            .filter(|id| match (here, self.desktops.of_window.get(id)) {
                (Some(ws), Some(l)) => l.contains(&ws),
                _ => true,
            })
            .collect()
    }

    pub(crate) fn update_snap(&mut self, msg: SnapMsg) -> Task<Message> {
        match msg {
            SnapMsg::Open => {
                if matches!(self.panel.as_ref().map(|p| &p.kind), Some(PanelKind::Snap(_))) {
                    return self.close_panel();
                }
                if !self.snap_supported {
                    return Task::future(async {
                        crate::start_search::notify(
                            "Snap layouts aren't available",
                            "They need Lapin UX's own build of the COSMIC compositor (lapin-ux-comp).",
                        )
                        .await;
                        cosmic::Action::App(Message::Noop)
                    });
                }
                // The window in use: most recently focused, still open and visible.
                let Some(target) = self.assist_candidates(&[]).into_iter().next() else { return Task::none() };
                let (w, h) = self.desktop.as_ref().map_or((1920, 1080), |d| (d.width as u32, d.height as u32));
                for id in self.assist_candidates(&[target]) {
                    if !self.thumbs.contains_key(&id) {
                        self.request(WaylandRequest::Capture(id));
                    }
                }
                return self.open_panel(PanelKind::Snap(SnapPanel { target, layout: None, filled: Vec::new(), placed: vec![target], count: layouts_for(w, h) }));
            }
            SnapMsg::Pick(layout, zone) => {
                let Some(p) = self.snap_panel() else { return Task::none() };
                let target = p.target;
                p.layout = Some(layout);
                p.filled = vec![zone];
                self.request(WaylandRequest::Snap(target, LAYOUTS[layout][zone]));
                // Nothing left to fill (or nothing to fill it with): done.
                if next_zone(layout, &[zone]).is_none() || self.assist_candidates(&[target]).is_empty() {
                    return self.finish_snap(target);
                }
            }
            SnapMsg::Assist(win) => {
                let Some(p) = self.snap_panel() else { return Task::none() };
                let (Some(layout), target) = (p.layout, p.target) else { return Task::none() };
                let Some(zone) = next_zone(layout, &p.filled) else { return Task::none() };
                p.filled.push(zone);
                p.placed.push(win);
                let (filled, placed) = (p.filled.clone(), p.placed.clone());
                self.request(WaylandRequest::Snap(win, LAYOUTS[layout][zone]));
                if next_zone(layout, &filled).is_none() || self.assist_candidates(&placed).is_empty() {
                    return self.finish_snap(target);
                }
            }
        }
        Task::none()
    }

    /// Close the panel and give the first window the focus back.
    fn finish_snap(&mut self, target: WindowId) -> Task<Message> {
        let close = self.close_panel();
        Task::batch([
            close,
            Task::future(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                cosmic::Action::App(Message::ActivateWindow(target))
            }),
        ])
    }
}

/// 0 = empty, 1 = already filled, 2 = the space being offered.
fn zone_style(state: u8) -> theme::Container<'static> {
    theme::Container::custom(move |t| {
        let c = t.cosmic();
        let accent: Color = c.accent_color().into();
        let fg: Color = c.on_bg_color().into();
        let bg = match state {
            2 => accent,
            1 => Color { a: 0.35, ..fg },
            _ => Color { a: 0.08, ..fg },
        };
        container::Style {
            background: Some(Background::Color(bg)),
            border: Border { color: Color { a: 0.35, ..fg }, width: 1.0, radius: 4.0.into() },
            ..Default::default()
        }
    })
}

const MINI_W: f32 = 168.0;
const MINI_H: f32 = 96.0;

/// A small picture of a layout; each space is a button (or shows filled /
/// next in Snap assist).
fn mini(layout: usize, zones: &[Zone], filled: Option<(&[usize], Option<usize>)>) -> Element<'static, Message> {
    let mut layers: Vec<Element<'static, Message>> = Vec::new();
    for (i, z) in zones.iter().enumerate() {
        let (x, y, w, h) = (z[0] * MINI_W + 2.0, z[1] * MINI_H + 2.0, z[2] * MINI_W - 4.0, z[3] * MINI_H - 4.0);
        let cell: Element<'static, Message> = match filled {
            None => button::custom(widget::Space::new().width(w).height(h))
                .padding(0)
                .class(zone_button())
                .on_press(Message::Snap(SnapMsg::Pick(layout, i)))
                .into(),
            Some((done, next)) => container(widget::Space::new().width(w).height(h))
                .class(zone_style(if next == Some(i) { 2 } else if done.contains(&i) { 1 } else { 0 }))
                .into(),
        };
        layers.push(container(cell).padding(cosmic::iced::Padding { top: y, left: x, right: 0.0, bottom: 0.0 }).into());
    }
    container(cosmic::iced::widget::Stack::with_children(layers).width(MINI_W).height(MINI_H)).into()
}

/// A space in a layout: tinted, accent on hover.
fn zone_button() -> theme::Button {
    let look = |hover: bool| {
        move |t: &cosmic::Theme| {
            let c = t.cosmic();
            let fg: Color = c.on_bg_color().into();
            let accent: Color = c.accent_color().into();
            button::Style {
                background: Some(Background::Color(if hover { accent } else { Color { a: 0.12, ..fg } })),
                border_radius: 4.0.into(),
                border_width: 1.0,
                border_color: Color { a: 0.35, ..fg },
                ..Default::default()
            }
        }
    };
    theme::Button::Custom {
        active: Box::new(move |_f, t| look(false)(t)),
        disabled: Box::new(move |t| look(false)(t)),
        hovered: Box::new(move |_f, t| look(true)(t)),
        pressed: Box::new(move |_f, t| look(true)(t)),
    }
}

pub fn view<'a>(shell: &'a Shell, p: &'a SnapPanel) -> Element<'a, Message> {
    let body: Element<'a, Message> = match p.layout {
        None => {
            let mut rows = column::with_capacity(2).spacing(14);
            let mut it = (0..p.count).peekable();
            while it.peek().is_some() {
                let r: Vec<Element<'a, Message>> = it.by_ref().take(3).map(|l| mini(l, LAYOUTS[l], None)).collect();
                rows = rows.push(row::with_children(r).spacing(18));
            }
            column::with_children(vec![text::heading("Snap layouts").into(), rows.into(), text::caption("Pick a space for the current window.").class(style::dim_text()).into()])
                .spacing(12)
                .into()
        }
        Some(layout) => {
            let next = next_zone(layout, &p.filled);
            let mut grid = column::with_capacity(4).spacing(12);
            let cards: Vec<Element<'a, Message>> = shell
                .assist_candidates(&p.placed)
                .into_iter()
                .filter_map(|id| {
                    let w = shell.taskbar.windows().iter().find(|w| w.handle == id)?;
                    let title = if w.title.is_empty() { app_name(shell, &w.app) } else { w.title.clone() };
                    let card = column::with_children(vec![
                        row::with_children(vec![icon(app_icon(shell, &w.app)).size(16).into(), text::caption(title).width(150).height(18).wrapping(cosmic::iced::widget::text::Wrapping::None).into()])
                            .spacing(6)
                            .align_y(Alignment::Center)
                            .into(),
                        crate::previews::picture_pub(shell, id, &w.app, 176.0, 104.0),
                    ])
                    .spacing(6)
                    .width(176);
                    Some(button::custom(card).padding(6).class(style::flat_button(false)).on_press(Message::Snap(SnapMsg::Assist(id))).into())
                })
                .collect();
            let mut it = cards.into_iter().peekable();
            while it.peek().is_some() {
                grid = grid.push(row::with_children(it.by_ref().take(3).collect::<Vec<_>>()).spacing(8));
            }
            column::with_children(vec![
                row::with_children(vec![
                    mini(layout, LAYOUTS[layout], Some((&p.filled, next))),
                    column::with_children(vec![
                        text::heading("Snap assist").into(),
                        text::body("Choose a window for the highlighted space.").into(),
                        text::caption("Press Esc to leave it empty.").class(style::dim_text()).into(),
                    ])
                    .spacing(4)
                    .into(),
                ])
                .spacing(16)
                .align_y(Alignment::Center)
                .into(),
                grid.into(),
            ])
            .spacing(16)
            .into()
        }
    };
    container(body).padding(18).into()
}
