// SPDX-License-Identifier: GPL-3.0-only
//! Cascading submenus: a second popup opened beside an item of the current
//! menu (like the benchmark's "View ›" on the desktop), on hover or click.

use std::sync::LazyLock;

use cosmic::app::Task;
use cosmic::iced::platform_specific::runtime::wayland::CornerRadius;
use cosmic::iced::platform_specific::runtime::wayland::popup::{SctkPopupSettings, SctkPositioner};
use cosmic::iced::platform_specific::shell::commands::popup::destroy_popup;
use cosmic::iced::{Alignment, Length, Limits, Rectangle, window};
use cosmic::surface::action::LiveSettings;
use cosmic::widget::{self, button, container, icon, mouse_area, row, text};
use cosmic::Element;
use cosmic::cctk::sctk::reexports::protocols::xdg::shell::client::xdg_positioner::{Anchor as PAnchor, Gravity};

use crate::app::{Message, Popup, PopupKind, Shell, rect_key};
use crate::style;

pub(crate) static SUB_AUTOSIZE: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("lapin-ux-submenu-autosize"));

impl Shell {
    /// Open `kind` as a submenu beside the item tracked as `item` in the
    /// current popup. Re-opening the same submenu is a no-op.
    pub(crate) fn open_submenu(&mut self, item: &str, kind: PopupKind) -> Task<Message> {
        let Some(parent) = self.popup.as_ref().map(|p| p.id) else { return Task::none() };
        if self.sub.as_ref().is_some_and(|s| s.kind == kind) {
            return Task::none();
        }
        let close = self.close_submenu();
        let r = self.rects.get(&rect_key(parent, item)).copied().unwrap_or(Rectangle { x: 0.0, y: 0.0, width: 1.0, height: 1.0 });
        let id = window::Id::unique();
        let settings = SctkPopupSettings {
            parent,
            id,
            positioner: SctkPositioner {
                size: None,
                size_limits: Limits::NONE.min_width(1.0).min_height(1.0).max_width(440.0).max_height(720.0),
                anchor_rect: Rectangle { x: r.x as i32, y: r.y as i32, width: (r.width as i32).max(1), height: (r.height as i32).max(1) },
                anchor: PAnchor::TopRight,
                gravity: Gravity::BottomRight,
                // slide_y | flip_x | resize_y
                constraint_adjustment: 2 | 4 | 32,
                offset: (2, -4),
                reactive: true,
            },
            parent_size: None,
            grab: true,
            close_with_children: false,
            input_zone: None,
        };
        self.sub = Some(Popup { id, parent, kind });
        let rad = style::PANEL_RADIUS as u32;
        let create = cosmic::surface::surface_task(cosmic::surface::action::app_popup(
            move |_: &Shell| LiveSettings {
                corners: Some(CornerRadius { top_left: rad, top_right: rad, bottom_left: rad, bottom_right: rad }),
                ..Default::default()
            },
            move |_: &mut Shell| settings.clone(),
            None,
        ));
        Task::batch([close, create])
    }

    pub(crate) fn close_submenu(&mut self) -> Task<Message> {
        match self.sub.take() {
            Some(p) => destroy_popup(p.id),
            None => Task::none(),
        }
    }
}

/// What sits in a menu row's left column.
#[derive(Clone, Copy)]
pub enum Lead<'a> {
    /// No icon: the label starts at the left edge (no empty gap).
    #[allow(dead_code)]
    None,
    /// An empty mark column (an unticked choice next to ticked ones).
    Blank,
    Icon(&'a str),
    /// `✓` for a toggle, `•` for the chosen option.
    Mark(&'static str),
}

/// A menu row in the benchmark's classic style: a fixed left column (icon,
/// check mark or bullet), the label, and `›` at the far right for a submenu.
pub fn entry<'a>(lead: Lead<'a>, label: impl Into<String>, trailing: Option<&'static str>, msg: Option<Message>) -> Element<'a, Message> {
    let lead_kind = lead;
    let lead: Element<'a, Message> = match lead {
        Lead::Icon(n) => icon::from_name(n).size(16).into(),
        Lead::Mark(m) => container(text::body(m)).width(16).align_x(Alignment::Center).into(),
        // A blank in a list of choices keeps the column for the mark.
        Lead::Blank => widget::Space::new().width(16).height(16).into(),
        // No icon: the label starts at the edge, no empty gap.
        Lead::None => widget::Space::new().width(0).height(16).into(),
    };
    let gap = if matches!(lead_kind, Lead::None) { 0 } else { 12 };
    let mut r = row::with_capacity(3).spacing(gap).align_y(Alignment::Center).push(lead).push(text::body(label.into()).width(Length::Fill));
    if let Some(t) = trailing {
        r = r.push(container(text::body(t)).width(16).align_x(Alignment::Center));
    }
    button::custom(r)
        .width(Length::Fill)
        .padding([7, 12])
        .class(style::flat_button(false))
        .on_press_maybe(msg)
        .into()
}

/// A row that opens a submenu on hover (and on click), tracked so the
/// submenu can be placed beside it.
pub fn parent_entry<'a>(shell: &'a Shell, popup: window::Id, key: &'static str, lead: Lead<'a>, label: &'static str, open: Message) -> Element<'a, Message> {
    let row = mouse_area(entry(lead, label, Some("\u{203A}"), Some(open.clone()))).on_enter(open);
    crate::taskbar_view::track(shell, rect_key(popup, key), row)
}

/// A plain row in a menu that has submenus: hovering it closes any open one.
pub fn leaf<'a>(el: Element<'a, Message>) -> Element<'a, Message> {
    mouse_area(el).on_enter(Message::CloseSubmenu).into()
}
