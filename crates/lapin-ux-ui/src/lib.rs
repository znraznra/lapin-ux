// SPDX-License-Identifier: GPL-3.0-only
//! Windows-style widget styles shared by the Lapin apps.
//!
//! The theme keeps COSMIC's largest corner radius fully round, because that
//! radius draws toggle switches, slider thumbs and progress bars, which are
//! round in Windows 11. libcosmic also uses it for ordinary push buttons,
//! which in Windows have small 4 px corners, so the Lapin apps give their
//! buttons these classes instead.

use cosmic::theme::Button;
use cosmic::widget::button::{Catalog, Style};

/// Corner radius of Windows 11 push buttons.
pub const BUTTON_RADIUS: f32 = 4.0;

fn square(mut s: Style) -> Style {
    s.border_radius = BUTTON_RADIUS.into();
    s
}

/// `kind` drawn with 4 px corners.
pub fn win(kind: fn() -> Button) -> Button {
    Button::Custom {
        active: Box::new(move |f, t| square(t.active(f, false, &kind()))),
        disabled: Box::new(move |t| square(t.disabled(&kind()))),
        hovered: Box::new(move |f, t| square(t.hovered(f, false, &kind()))),
        pressed: Box::new(move |f, t| square(t.pressed(f, false, &kind()))),
    }
}

pub fn standard() -> Button {
    win(|| Button::Standard)
}
pub fn suggested() -> Button {
    win(|| Button::Suggested)
}
pub fn destructive() -> Button {
    win(|| Button::Destructive)
}
pub fn text() -> Button {
    win(|| Button::Text)
}
pub fn icon() -> Button {
    win(|| Button::Icon)
}

/// Drop-in replacements for `cosmic::widget::button::{standard, suggested,
/// destructive, text, icon}` with Windows' 4 px corners.
pub mod button {
    use std::borrow::Cow;

    use cosmic::widget::button::{self, IconButton, TextButton};
    use cosmic::widget::icon::Handle;

    pub fn standard<'a, M>(label: impl Into<Cow<'a, str>>) -> TextButton<'a, M> {
        button::standard(label).class(super::standard())
    }
    pub fn suggested<'a, M>(label: impl Into<Cow<'a, str>>) -> TextButton<'a, M> {
        button::suggested(label).class(super::suggested())
    }
    pub fn destructive<'a, M>(label: impl Into<Cow<'a, str>>) -> TextButton<'a, M> {
        button::destructive(label).class(super::destructive())
    }
    pub fn text<'a, M>(label: impl Into<Cow<'a, str>>) -> TextButton<'a, M> {
        button::text(label).class(super::text())
    }
    pub fn icon<'a, M>(handle: impl Into<Handle>) -> IconButton<'a, M> {
        button::icon(handle).class(super::icon())
    }

    /// A fixed-width push button with its label centred both ways (libcosmic
    /// text buttons keep the label at the left when given a width). Windows
    /// dialog buttons look like this.
    pub fn sized<'a, M: Clone + 'a>(
        label: impl Into<Cow<'a, str>> + 'a,
        width: f32,
        class: cosmic::theme::Button,
    ) -> cosmic::widget::Button<'a, M> {
        use cosmic::iced::Length;
        button::custom(
            cosmic::widget::container(cosmic::widget::text::body(label))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fixed(width))
        .height(Length::Fixed(32.0))
        .padding(0)
        .class(class)
    }
}

/// Sliders with a round thumb. libcosmic draws the thumb with the theme's
/// medium radius, which Lapin keeps at 8 px for windows and panels, so the
/// thumb would otherwise come out as a rounded square.
pub fn slider() -> cosmic::theme::style::iced::Slider {
    use cosmic::iced::widget::slider::{Catalog, HandleShape, Status};
    use cosmic::theme::style::iced::Slider;
    use std::rc::Rc;
    fn round(t: &cosmic::Theme, status: Status) -> cosmic::iced::widget::slider::Style {
        let mut s = t.style(&Slider::Standard, status);
        if let HandleShape::Rectangle { width, height, .. } = s.handle.shape {
            s.handle.shape = HandleShape::Rectangle { width, height, border_radius: (f32::from(width.max(height)) / 2.0).into() };
        }
        s
    }
    Slider::Custom {
        active: Rc::new(|t| round(t, Status::Active)),
        hovered: Rc::new(|t| round(t, Status::Hovered)),
        dragging: Rc::new(|t| round(t, Status::Dragged)),
    }
}

/// A page in a sidebar (Task Manager, Settings…): plain text, a soft
/// highlight on hover, and a filled background when it's the open page.
pub fn nav_item(selected: bool) -> Button {
    fn plain(mut s: Style, t: &cosmic::Theme) -> Style {
        let on: cosmic::iced::Color = t.cosmic().on_bg_color().into();
        s.text_color = Some(on);
        s.icon_color = Some(on);
        square(s)
    }
    let kind = move || if selected { Button::Standard } else { Button::Text };
    Button::Custom {
        active: Box::new(move |f, t| plain(t.active(f, false, &kind()), t)),
        disabled: Box::new(move |t| plain(t.disabled(&kind()), t)),
        hovered: Box::new(move |f, t| plain(t.hovered(f, false, &kind()), t)),
        pressed: Box::new(move |f, t| plain(t.pressed(f, false, &kind()), t)),
    }
}

/// Windows 11's title: the app's icon and name at the left of the title
/// bar (the same look the libcosmic patch gives COSMIC's own apps).
pub fn title_bar<'a, M: 'static>(icon_name: &'a str, title: &'a str) -> cosmic::Element<'a, M> {
    cosmic::widget::row::with_capacity(2)
        .spacing(10)
        .padding([0, 0, 0, 6])
        .align_y(cosmic::iced::Alignment::Center)
        .push(cosmic::widget::icon::from_name(icon_name).size(16))
        .push(cosmic::widget::text::body(title))
        .into()
}
