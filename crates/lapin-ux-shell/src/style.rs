// SPDX-License-Identifier: GPL-3.0-only
//! Visual language: translucent, blurred surfaces, small 4 px radii on
//! controls, 8 px on panels, a short accent "pill" under the focused app.
//! Colors come from the active theme so light/dark and accent changes apply
//! live.

use cosmic::iced::{Background, Border, Color, Shadow, Vector};
use cosmic::theme;
use cosmic::widget::{button, container};

pub const TASKBAR_HEIGHT: u32 = 48;
pub const TASK_BUTTON: f32 = 40.0;
pub const TASK_ICON: u16 = 24;
pub const START_WIDTH: u32 = 640;
pub const START_HEIGHT: u32 = 720;
pub const PANEL_RADIUS: f32 = 8.0;
pub const CONTROL_RADIUS: f32 = 4.0;

fn with_alpha(c: impl Into<Color>, a: f32) -> Color {
    let mut c: Color = c.into();
    c.a = a;
    c
}

pub fn taskbar_bg() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        container::Style {
            background: Some(Background::Color(with_alpha(c.background(true).base, 0.82))),
            border: Border { color: with_alpha(c.bg_divider(), 0.6), width: 0.0, radius: 0.0.into() },
            text_color: Some(c.on_bg_color().into()),
            icon_color: Some(c.on_bg_color().into()),
            ..Default::default()
        }
    })
}

pub fn panel_bg() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        container::Style {
            background: Some(Background::Color(with_alpha(c.background(true).base, 0.96))),
            border: Border { color: with_alpha(c.bg_divider(), 0.9), width: 1.0, radius: PANEL_RADIUS.into() },
            shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.3), offset: Vector::new(0.0, 8.0), blur_radius: 24.0 },
            text_color: Some(c.on_bg_color().into()),
            icon_color: Some(c.on_bg_color().into()),
            ..Default::default()
        }
    })
}

/// Bottom strip of the Start menu (account + power).
pub fn panel_footer() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        container::Style {
            background: Some(Background::Color(with_alpha(c.primary(true).base, 0.55))),
            border: Border {
                color: with_alpha(c.bg_divider(), 0.9),
                width: 0.0,
                radius: [0.0, 0.0, PANEL_RADIUS, PANEL_RADIUS].into(),
            },
            ..Default::default()
        }
    })
}

/// A taskbar button whose window wants attention: warm highlight.
pub fn urgent_button() -> theme::Button {
    let look = move |alpha: f32| {
        move |t: &cosmic::Theme| {
            let c = t.cosmic();
            let warn: Color = c.warning_color().into();
            button::Style {
                background: Some(Background::Color(Color { a: alpha, ..warn })),
                border_radius: CONTROL_RADIUS.into(),
                text_color: Some(c.on_bg_color().into()),
                icon_color: Some(c.on_bg_color().into()),
                ..Default::default()
            }
        }
    };
    theme::Button::Custom {
        active: Box::new(move |_f, t| look(0.45)(t)),
        disabled: Box::new(move |t| look(0.45)(t)),
        hovered: Box::new(move |_f, t| look(0.55)(t)),
        pressed: Box::new(move |_f, t| look(0.35)(t)),
    }
}

/// Flat button that lights up on hover; `selected` gives it a resting tint
/// (focused app on the taskbar, keyboard selection in Start).
pub fn flat_button(selected: bool) -> theme::Button {
    let resting = move |t: &cosmic::Theme| {
        let c = t.cosmic();
        let bg = selected.then(|| Background::Color(with_alpha(c.on_bg_color(), 0.09)));
        button::Style {
            background: bg,
            border_radius: CONTROL_RADIUS.into(),
            text_color: Some(c.on_bg_color().into()),
            icon_color: Some(c.on_bg_color().into()),
            ..Default::default()
        }
    };
    let tinted = move |alpha: f32| {
        move |_focused: bool, t: &cosmic::Theme| {
            let c = t.cosmic();
            button::Style {
                background: Some(Background::Color(with_alpha(c.on_bg_color(), alpha))),
                border_radius: CONTROL_RADIUS.into(),
                text_color: Some(c.on_bg_color().into()),
                icon_color: Some(c.on_bg_color().into()),
                ..Default::default()
            }
        }
    };
    theme::Button::Custom {
        active: Box::new(move |_f, t| resting(t)),
        disabled: Box::new(move |t| resting(t)),
        hovered: Box::new(tinted(if selected { 0.13 } else { 0.07 })),
        pressed: Box::new(tinted(0.04)),
    }
}

/// Accent pill under a taskbar button. Wide when the app is focused,
/// a short neutral dash when it only runs in the background.
pub fn indicator(active: bool) -> theme::Container<'static> {
    theme::Container::custom(move |t| {
        let c = t.cosmic();
        let color: Color = if active { c.accent_color().into() } else { with_alpha(c.on_bg_color(), 0.55) };
        container::Style {
            background: Some(Background::Color(color)),
            border: Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        }
    })
}

/// A folder of apps in Start's pinned grid.
pub fn folder_tile() -> theme::Container<'static> {
    theme::Container::custom(|t| container::Style {
        background: Some(Background::Color(with_alpha(t.cosmic().on_bg_color(), 0.08))),
        border: Border { radius: 6.0.into(), ..Default::default() },
        ..Default::default()
    })
}

/// An open folder in Start.
pub fn folder_card() -> theme::Container<'static> {
    theme::Container::custom(|t| container::Style {
        background: Some(Background::Color(with_alpha(t.cosmic().on_bg_color(), 0.05))),
        border: Border { radius: 8.0.into(), width: 1.0, color: with_alpha(t.cosmic().on_bg_color(), 0.1) },
        ..Default::default()
    })
}

/// The small count bubble on a taskbar button.
pub fn badge() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        container::Style {
            background: Some(Background::Color(c.accent_color().into())),
            text_color: Some(c.on_accent_color().into()),
            border: Border { radius: 8.0.into(), width: 1.5, color: c.bg_color().into() },
            ..Default::default()
        }
    })
}

/// Progress on a taskbar button: track and fill.
pub fn progress_track() -> theme::Container<'static> {
    theme::Container::custom(|t| container::Style {
        background: Some(Background::Color(with_alpha(t.cosmic().on_bg_color(), 0.2))),
        border: Border { radius: 2.0.into(), ..Default::default() },
        ..Default::default()
    })
}

pub fn progress_fill() -> theme::Container<'static> {
    theme::Container::custom(|t| container::Style {
        background: Some(Background::Color(t.cosmic().success_color().into())),
        border: Border { radius: 2.0.into(), ..Default::default() },
        ..Default::default()
    })
}

pub fn urgent_indicator() -> theme::Container<'static> {
    theme::Container::custom(|t| container::Style {
        background: Some(Background::Color(t.cosmic().warning_color().into())),
        border: Border { radius: 2.0.into(), ..Default::default() },
        ..Default::default()
    })
}

/// Rounded "pill" search box look used on the taskbar and in Start.
pub fn search_pill() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        container::Style {
            background: Some(Background::Color(with_alpha(c.on_bg_color(), 0.06))),
            border: Border { color: with_alpha(c.on_bg_color(), 0.12), width: 1.0, radius: 20.0.into() },
            text_color: Some(with_alpha(c.on_bg_color(), 0.75)),
            icon_color: Some(with_alpha(c.on_bg_color(), 0.75)),
            ..Default::default()
        }
    })
}

/// Raised card inside a panel (search detail pane).
pub fn card() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        container::Style {
            background: Some(Background::Color(with_alpha(c.on_bg_color(), 0.05))),
            border: Border { color: with_alpha(c.on_bg_color(), 0.10), width: 1.0, radius: PANEL_RADIUS.into() },
            text_color: Some(c.on_bg_color().into()),
            icon_color: Some(c.on_bg_color().into()),
            ..Default::default()
        }
    })
}

pub fn dim_text() -> theme::Text {
    theme::Text::Custom(|t| cosmic::iced::widget::text::Style {
        color: Some(with_alpha(t.cosmic().on_bg_color(), 0.7)),
        ..Default::default()
    })
}

/// Transparent window background so only our containers draw.
pub fn transparent_window() -> cosmic::iced::theme::Style {
    let t = cosmic::theme::active();
    cosmic::iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: t.cosmic().on_bg_color().into(),
        icon_color: t.cosmic().on_bg_color().into(),
    }
}
