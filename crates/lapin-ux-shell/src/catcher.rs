// SPDX-License-Identifier: GPL-3.0-only
//! "Click outside closes it" for Start and the Win+X / Run panels.
//!
//! These take the keyboard exclusively, and cosmic-comp never moves focus
//! away from an exclusive layer surface, so they never get "unfocused" when
//! the user clicks elsewhere. While one is open, a transparent surface covers
//! the screen (except the taskbar) underneath it and turns any click into
//! "close". Menus sit on the Overlay layer so they are always above it.

use cosmic::app::Task;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{IcedOutput, SctkLayerSurfaceSettings};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer, destroy_layer_surface, get_layer_surface,
};
use cosmic::iced::{Length, window};
use cosmic::widget::mouse_area;
use cosmic::Element;

use crate::app::{Message, Shell};

impl Shell {
    pub(crate) fn sync_catcher(&mut self) -> Task<Message> {
        let wanted = self.start.is_some() || self.panel.is_some();
        match (wanted, self.catcher) {
            (true, None) => {
                let id = window::Id::unique();
                self.catcher = Some(id);
                tracing::debug!(?id, "click catcher on");
                if *crate::redraw_debug::ON {
                    crate::redraw_debug::surface("click catcher on", id);
                }
                get_layer_surface(SctkLayerSurfaceSettings {
                    id,
                    layer: Layer::Top,
                    keyboard_interactivity: KeyboardInteractivity::None,
                    anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                    output: IcedOutput::Active,
                    namespace: "lapin-ux-dismiss".into(),
                    size: Some((None, None)),
                    // Respect the taskbar's reserved space so it stays clickable.
                    exclusive_zone: 0,
                    ..Default::default()
                })
            }
            (false, Some(id)) => {
                self.catcher = None;
                destroy_layer_surface(id)
            }
            _ => Task::none(),
        }
    }

    pub(crate) fn catcher_click(&mut self) -> Task<Message> {
        tracing::debug!("click outside menu");
        Task::batch([self.close_popup(), self.close_start(), self.close_panel()])
    }
}

pub fn view<'a>() -> Element<'a, Message> {
    mouse_area(cosmic::widget::Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(Message::CatcherClick)
        .on_right_press(Message::CatcherClick)
        .on_middle_press(Message::CatcherClick)
        .into()
}
