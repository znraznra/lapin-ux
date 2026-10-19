// SPDX-License-Identifier: GPL-3.0-only
//! The notification centre: notifications grouped by app above a calendar,
//! along the right edge of the screen, opened from the taskbar clock.

use cosmic::iced::{Alignment, Background, Border, Color, Length, Shadow, Vector};
use cosmic::widget::{self, button, column, container, icon, mouse_area, row, scrollable, text, toggler};
use cosmic::{Element, theme};
use lapin_ux_core::notify::Notification;

use crate::{App, Message, icon_for};

pub const WIDTH: u32 = 380;

fn card<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message, cosmic::Theme> {
    container(content).padding(12).width(Length::Fill).class(theme::Container::custom(|t| {
        let c = t.cosmic();
        let mut bg: Color = c.background(true).base.into();
        bg.a = 0.97;
        container::Style {
            background: Some(Background::Color(bg)),
            border: Border { color: c.bg_divider().into(), width: 1.0, radius: 8.0.into() },
            shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.25), offset: Vector::new(0.0, 4.0), blur_radius: 16.0 },
            text_color: Some(c.on_bg_color().into()),
            icon_color: Some(c.on_bg_color().into()),
            ..Default::default()
        }
    }))
}

fn item_style() -> theme::Container<'static> {
    theme::Container::custom(|t| {
        let c = t.cosmic();
        container::Style {
            background: Some(Background::Color(c.background(true).component.base.into())),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        }
    })
}

fn dim<'a>(s: impl Into<String>) -> Element<'a, Message> {
    text::caption(s.into())
        .class(theme::Text::Custom(|t| {
            let mut c: Color = t.cosmic().on_bg_color().into();
            c.a = 0.7;
            cosmic::iced::widget::text::Style { color: Some(c), ..Default::default() }
        }))
        .into()
}

fn when(n: &Notification) -> String {
    let t: chrono::DateTime<chrono::Utc> = n.received.into();
    lapin_ux_core::recent::relative(&t.to_rfc3339(), chrono::Utc::now())
}

fn entry(n: &Notification) -> Element<'_, Message> {
    let mut texts = column::with_capacity(3).spacing(2).width(Length::Fill);
    if !n.summary.is_empty() {
        texts = texts.push(text::heading(n.summary.clone()));
    }
    if !n.body.is_empty() {
        texts = texts.push(text::body(n.body.clone()));
    }
    texts = texts.push(dim(when(n)));
    let line = row::with_children(vec![
        icon(icon_for(n)).size(32).into(),
        texts.into(),
        button::custom(icon::from_name("window-close-symbolic").size(12))
            .padding(4)
            .class(theme::Button::Icon)
            .on_press(Message::Forget(n.id))
            .into(),
    ])
    .spacing(10)
    .align_y(Alignment::Start);

    let mut body = column::with_capacity(2).spacing(8).push(line);
    let buttons: Vec<Element<'_, Message>> = n
        .actions
        .iter()
        .filter(|a| a.key != "default")
        .take(3)
        .map(|a| button::standard(a.label.clone()).width(Length::Fill).on_press(Message::Invoke(n.id, a.key.clone())).into())
        .collect();
    if !buttons.is_empty() {
        body = body.push(row::with_children(buttons).spacing(8));
    }
    let boxed = container(body).padding(10).width(Length::Fill).class(item_style());
    mouse_area(boxed).on_press(Message::CenterOpen(n.id)).into()
}

fn notifications(app: &App) -> Element<'_, Message> {
    if app.quiet.settings_open {
        return card(crate::quiet::settings(app)).height(Length::Fill).into();
    }
    let dnd = app.queue.do_not_disturb;
    let header = row::with_children(vec![
        text::title4("Notifications").width(Length::Fill).into(),
        button::custom(icon::from_name("emblem-system-symbolic").size(16))
            .padding(6)
            .class(theme::Button::Icon)
            .on_press(Message::Quiet(crate::quiet::QuietMsg::OpenSettings(true)))
            .into(),
        button::text("Clear all").on_press_maybe((app.queue.history().next().is_some()).then_some(Message::ClearAll)).into(),
    ])
    .align_y(Alignment::Center);
    let dnd_row = row::with_children(vec![
        icon::from_name(if dnd { "notification-disabled-symbolic" } else { "notification-symbolic" })
            .size(16)
            .into(),
        text::body("Do not disturb").width(Length::Fill).into(),
        toggler(dnd).on_toggle(|_| Message::ToggleDnd).into(),
    ])
    .spacing(8)
    .align_y(Alignment::Center);

    let groups = app.queue.history_by_app();
    let mut col = column::with_capacity(3).spacing(10).push(header).push(dnd_row);
    if groups.is_empty() {
        col = col.push(
            container(dim("No new notifications"))
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .padding([16, 0]),
        );
        return card(col).into();
    }
    let mut list = column::with_capacity(groups.len()).spacing(12);
    for (name, items) in groups {
        let shown = if name.is_empty() { "Other" } else { name };
        let title = row::with_children(vec![
            icon(icon_for(items[0])).size(16).into(),
            text::heading(shown.to_string()).width(Length::Fill).into(),
            button::custom(icon::from_name("window-close-symbolic").size(12))
                .padding(4)
                .class(theme::Button::Icon)
                .on_press(Message::ForgetApp(name.to_string()))
                .into(),
        ])
        .spacing(8)
        .align_y(Alignment::Center);
        let mut g = column::with_capacity(items.len() + 1).spacing(6).push(title);
        for n in items {
            g = g.push(entry(n));
        }
        list = list.push(g);
    }
    col = col.push(scrollable(list).height(Length::Fill));
    card(col).height(Length::Fill).into()
}

fn calendar(app: &App) -> Element<'_, Message> {
    let now = chrono::Local::now();
    let collapsed = app.calendar_collapsed;
    let header = row::with_children(vec![
        text::heading(now.format("%A, %B %-d").to_string()).width(Length::Fill).into(),
        button::custom(icon::from_name(if collapsed { "go-up-symbolic" } else { "go-down-symbolic" }).size(16))
            .padding(4)
            .class(theme::Button::Icon)
            .on_press(Message::ToggleCalendar)
            .into(),
    ])
    .align_y(Alignment::Center);
    if collapsed {
        return card(column::with_children(vec![header.into(), crate::quiet::focus_row(app)]).spacing(10)).into();
    }
    let cal = widget::calendar(
        &app.calendar,
        Message::CalendarSelect,
        || Message::CalendarPrev,
        || Message::CalendarNext,
        if lapin_ux_core::clock::week_starts_sunday_env() {
            jiff::civil::Weekday::Sunday
        } else {
            jiff::civil::Weekday::Monday
        },
    );
    card(
        column::with_children(vec![
            header.into(),
            container(cal).center_x(Length::Fill).into(),
            crate::quiet::agenda(app),
            widget::divider::horizontal::default().into(),
            crate::quiet::focus_row(app),
        ])
        .spacing(10),
    )
    .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let empty = app.queue.history().next().is_none() && !app.quiet.settings_open;
    let mut col = column::with_capacity(3).spacing(12).width(Length::Fill).height(Length::Fill);
    if empty {
        // Transparent space above the cards; clicking it closes the centre.
        col = col.push(mouse_area(widget::Space::new().width(Length::Fill).height(Length::Fill)).on_press(Message::CloseCenter));
    }
    col = col.push(notifications(app));
    // Settings get the whole height.
    if !app.quiet.settings_open {
        col = col.push(calendar(app));
    }
    col.into()
}
