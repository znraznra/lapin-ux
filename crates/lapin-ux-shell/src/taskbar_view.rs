// SPDX-License-Identifier: GPL-3.0-only
//! Taskbar and its popups (window picker, right-click menu, calendar).

use lapin_ux_core::taskbar::Button;
use cosmic::desktop::IconSourceExt;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, column, container, icon, mouse_area, row, text};
use cosmic::Element;

use crate::app::{Bar, Message, Popup, PopupKind, PowerAction, Shell, rect_key};
use crate::config::TaskbarAlignment;
use crate::style;
use crate::wayland::WindowId as Handle;

pub const START_ICON: &[u8] = include_bytes!("../../../data/icons/lapin-ux-start.svg");

pub(crate) fn track<'a>(shell: &'a Shell, key: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    match &shell.tracker {
        Some(t) => t.container(key, content).into(),
        None => content.into(),
    }
}

pub fn app_icon(shell: &Shell, app: &str) -> icon::Handle {
    match shell.catalog.data_for(app) {
        Some(d) => d.icon.as_cosmic_icon(),
        None => icon::from_name(app.to_string())
            .fallback(Some(icon::IconFallback::Names(vec!["application-x-executable".into()])))
            .handle(),
    }
}

pub fn app_name(shell: &Shell, app: &str) -> String {
    shell.catalog.data_for(app).map(|d| d.name.clone()).unwrap_or_else(|| app.to_string())
}

pub fn taskbar<'a>(shell: &'a Shell, bar: &'a Bar) -> Element<'a, Message> {
    if shell.config.auto_hide && !bar.revealed {
        // Hidden: an invisible strip along the screen edge that wakes the bar.
        return widget::Space::new().width(Length::Fill).height(Length::Fill).into();
    }
    let start_open = shell.start.is_some();
    let start_btn = button::custom(
        container(icon(icon::from_svg_bytes(START_ICON)).size(style::TASK_ICON))
            .center(Length::Fill),
    )
    .width(style::TASK_BUTTON)
    .height(style::TASK_BUTTON)
    .padding(0)
    .class(style::flat_button(start_open))
    .on_press(Message::ToggleStart);

    let mut center = row::with_capacity(16)
        .spacing(4)
        .align_y(Alignment::Center)
        .push(track(shell, rect_key(bar.id, "start"), start_btn));

    if shell.config.show_search {
        let pill = container(
            row::with_children(vec![
                icon::from_name("system-search-symbolic").size(16).into(),
                text::body("Search").into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([0, 14])
        .height(32)
        .width(180)
        .align_y(Alignment::Center)
        .class(style::search_pill());
        center = center.push(
            button::custom(pill)
                .padding([0, 4])
                .class(style::flat_button(false))
                .on_press(Message::ToggleStart),
        );
    }

    // Task buttons get their own row so the widget tree keeps a stable
    // shape when the search box is toggled or buttons come and go.
    let tasks: Vec<Element<'_, Message>> = shell.taskbar.buttons().into_iter().map(|b| task_button(shell, bar, b)).collect();
    center = center.push(row::with_children(tasks).spacing(4).align_y(Alignment::Center));

    let clock_style = shell.clock_style();
    let time = column::with_children(vec![
        text::caption(clock_style.time(&shell.now)).into(),
        text::caption(clock_style.date(&shell.now)).into(),
    ])
    .align_x(Alignment::End);
    let mut clock_content = row::with_capacity(2).spacing(8).align_y(Alignment::Center).push(time);
    if shell.notify.available {
        clock_content = clock_content.push(notify_badge(shell.notify));
    }
    let clock = button::custom(clock_content)
    .padding([4, 10])
    .class(style::flat_button(matches!(shell.popup.as_ref().map(|p| &p.kind), Some(PopupKind::Calendar))))
    .on_press(Message::ClockClick(bar.id));

    // Tray: hidden icons behind ^, chosen ones inline.
    let mut tray: Vec<Element<'_, Message>> = Vec::new();
    if shell.tray.iter().any(|t| !shell.tray_inline(t)) {
        let open = matches!(shell.popup.as_ref().map(|p| &p.kind), Some(PopupKind::TrayOverflow));
        let btn = button::custom(container(icon::from_name("pan-up-symbolic").size(16)).center(Length::Fill))
            .width(28)
            .height(style::TASK_BUTTON)
            .padding(0)
            .class(style::flat_button(open))
            .on_press(Message::TrayOverflow(bar.id));
        tray.push(track(shell, rect_key(bar.id, "tray-overflow"), btn));
    }
    tray.extend(shell.tray.iter().filter(|t| shell.tray_inline(t)).map(|t| tray_button(shell, bar, t)));
    let mut right = row::with_capacity(4).spacing(2).align_y(Alignment::Center);
    right = right.push(row::with_children(tray).spacing(0).align_y(Alignment::Center));
    let status = crate::quick::status_icons(shell);
    let mut status_slot: Vec<Element<'_, Message>> = Vec::with_capacity(1);
    if !status.is_empty() {
        let btn = button::custom(container(row::with_children(status).spacing(8).align_y(Alignment::Center)).center_y(Length::Fill))
            .height(style::TASK_BUTTON)
            .padding([0, 10])
            .class(style::flat_button(shell.quick_open()))
            .on_press(Message::Quick(crate::quick::QuickMsg::Toggle));
        // Scrolling over it changes the volume, like the benchmark.
        let area = mouse_area(btn).on_scroll(|d| {
            let dy = match d {
                cosmic::iced::mouse::ScrollDelta::Lines { y, .. } => y,
                cosmic::iced::mouse::ScrollDelta::Pixels { y, .. } => y / 20.0,
            };
            Message::Quick(crate::quick::QuickMsg::Scroll(dy))
        });
        status_slot.push(track(shell, rect_key(bar.id, "quick"), area));
    }
    // Own row so the widget tree keeps its shape when the icons come and go.
    right = right.push(row::with_children(status_slot));
    right = right.push(track(shell, rect_key(bar.id, "clock"), clock));
    if shell.config.show_desktop_corner {
        right = right.push(
            button::custom(widget::Space::new().width(6).height(Length::Fill))
                .padding(0)
                .height(Length::Fill)
                .class(style::flat_button(false))
                .on_press(Message::ShowDesktop),
        );
    }

    let content: Element<'_, Message> = match shell.config.alignment {
        TaskbarAlignment::Center => row::with_children(vec![
            widget::Space::new().width(Length::Fill).into(),
            center.into(),
            container(right).width(Length::Fill).align_x(Alignment::End).into(),
        ])
        .align_y(Alignment::Center)
        .into(),
        TaskbarAlignment::Left => row::with_children(vec![
            container(center).width(Length::Fill).into(),
            right.into(),
        ])
        .align_y(Alignment::Center)
        .into(),
    };

    let bar_body = container(content)
        .padding([4, 4, 4, 12])
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .class(style::taskbar_bg());
    // Hover for auto-hide comes from the surface's own enter/leave events
    // (see app.rs), which are exact; a widget can report a stray exit.
    mouse_area(bar_body).on_right_press(Message::TaskbarContext(bar.id)).into()
}

fn task_button<'a>(shell: &'a Shell, bar: &'a Bar, b: Button<Handle>) -> Element<'a, Message> {
    let active = b.active();
    let indicator: Element<'_, Message> = if b.urgent() {
        container(widget::Space::new().width(16).height(3)).class(style::urgent_indicator()).into()
    } else if b.running() {
        container(widget::Space::new().width(if active { 16 } else { 6 }).height(3))
            .class(style::indicator(active))
            .into()
    } else {
        widget::Space::new().width(6).height(3).into()
    };

    let content = column::with_children(vec![
        widget::Space::new().height(4).into(),
        icon(app_icon(shell, &b.app)).size(style::TASK_ICON).into(),
        widget::Space::new().height(Length::Fill).into(),
        indicator,
        widget::Space::new().height(1).into(),
    ])
    .align_x(Alignment::Center)
    .width(Length::Fill)
    .height(Length::Fill);

    let dragging = shell.drag.as_ref().is_some_and(|d| d.active && d.app == b.app);
    let btn = button::custom(content)
        .width(style::TASK_BUTTON)
        .height(style::TASK_BUTTON)
        .padding(0)
        .class(style::flat_button(active || dragging))
        .on_press(Message::TaskClick(bar.id, b.app.clone()));

    let area = mouse_area(btn)
        .on_right_press(Message::TaskContext(bar.id, b.app.clone()))
        .on_middle_press(Message::Launch(b.app.clone()))
        .on_enter(Message::Preview(crate::previews::PreviewMsg::ButtonHover(bar.id, b.app.clone(), true)))
        .on_exit(Message::Preview(crate::previews::PreviewMsg::ButtonHover(bar.id, b.app.clone(), false)));

    track(shell, rect_key(bar.id, &b.app), area)
}

fn tray_button<'a>(shell: &'a Shell, bar: &'a Bar, t: &'a crate::app::TrayEntry) -> Element<'a, Message> {
    let name = t.item.name().to_string();
    let content: Element<'_, Message> = match &t.icon {
        Some(h) => icon(h.clone()).size(16).into(),
        None => icon::from_name("image-missing-symbolic").size(16).into(),
    };
    let open = matches!(&shell.popup, Some(Popup { kind: PopupKind::TrayMenu(n), .. }) if *n == name);
    let btn = button::custom(container(content).center(Length::Fill))
        .width(32)
        .height(style::TASK_BUTTON)
        .padding(0)
        .class(style::flat_button(open))
        .on_press(Message::TrayClick(bar.id, name.clone()));
    let area = mouse_area(btn).on_right_press(Message::TrayContext(bar.id, name.clone()));
    track(shell, rect_key(bar.id, &format!("tray:{name}")), area)
}

/// Turn a tray icon update into a drawable icon: a themed name when the app
/// gives one, otherwise the largest ARGB pixmap converted to RGBA.
pub fn tray_icon(update: crate::tray::IconUpdate) -> icon::Handle {
    if let Some(name) = update.name.as_deref() {
        let path = std::path::Path::new(name);
        if path.is_absolute() && path.exists() {
            return icon::from_path(path.to_path_buf());
        }
        let mut b = icon::from_name(name.to_string())
            .prefer_svg(true)
            .fallback(Some(icon::IconFallback::Names(vec!["application-x-executable".into()])));
        if let Some(p) = update.theme_path {
            b = b.with_extra_paths(vec![p]);
        }
        return b.handle();
    }
    let best = update
        .pixmap
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.width > 0 && p.height > 0 && p.bytes.len() == (p.width * p.height * 4) as usize)
        // Prefer 32 px-ish (crisp at 16 px on HiDPI), else the largest.
        .min_by_key(|p| ((p.width - 32).abs(), -(p.width)));
    match best {
        Some(mut p) => {
            for px in p.bytes.chunks_exact_mut(4) {
                px.rotate_left(1); // ARGB (network order) -> RGBA
            }
            icon::from_raster_pixels(p.width as u32, p.height as u32, p.bytes)
        }
        None => icon::from_name("image-missing-symbolic").handle(),
    }
}

fn tray_menu<'a>(shell: &'a Shell, name: &'a str) -> Element<'a, Message> {
    let Some(entry) = shell.tray.iter().find(|t| t.item.name() == name) else {
        return panel(text::body("Not available"), 200.0);
    };
    let Some(root) = entry.menu.as_ref() else {
        return panel(container(text::body("Loading…").class(style::dim_text())).padding(8), 200.0);
    };
    let Some(node) = root.find(&shell.tray_menu_path) else {
        return panel(text::body("Menu changed"), 200.0);
    };
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    if !shell.tray_menu_path.is_empty() {
        items.push(menu_item("go-previous-symbolic", node.label(), Message::TrayMenuBack));
        items.push(menu_divider());
    }
    for child in node.children().iter().filter(|c| c.visible()) {
        if child.is_separator() {
            items.push(menu_divider());
            continue;
        }
        let lead: Element<'_, Message> = match (child.toggled(), child.icon_name()) {
            (Some(true), _) => icon::from_name("object-select-symbolic").size(16).into(),
            (Some(false), _) => widget::Space::new().width(16).height(16).into(),
            (None, Some(n)) => icon::from_name(n.to_string()).size(16).into(),
            (None, None) => widget::Space::new().width(16).height(16).into(),
        };
        let mut r = row::with_capacity(3).spacing(12).align_y(Alignment::Center).push(lead);
        let label = text::body(child.label()).width(Length::Fill);
        r = r.push(if child.enabled() { label } else { label.class(style::dim_text()) });
        if child.is_submenu() {
            r = r.push(icon::from_name("go-next-symbolic").size(12));
        }
        let mut b = button::custom(r).width(Length::Fill).padding([7, 12]).class(style::flat_button(false));
        if child.enabled() {
            b = b.on_press(Message::TrayMenuItem(name.to_string(), child.id(), child.is_submenu()));
        }
        items.push(b.into());
    }
    if items.is_empty() {
        items.push(container(text::body("No actions").class(style::dim_text())).padding(8).into());
    }
    if shell.tray_menu_path.is_empty()
        && let Some(id) = entry.id.clone()
    {
        items.push(menu_divider());
        if shell.tray_inline(entry) {
            items.push(menu_item("go-down-symbolic", "Hide in overflow menu", Message::TrayPin(id, false)));
        } else {
            items.push(menu_item("view-pin-symbolic", "Show on taskbar", Message::TrayPin(id, true)));
        }
    }
    panel(column::with_children(items).spacing(2), 260.0)
}

/// A menu row with a check mark when `on`.
pub fn menu_check<'a>(label: &'a str, on: bool, msg: Message) -> Element<'a, Message> {
    let mark: Element<'_, Message> = if on {
        icon::from_name("object-select-symbolic").size(16).into()
    } else {
        widget::Space::new().width(16).height(16).into()
    };
    button::custom(row::with_children(vec![mark, text::body(label).into()]).spacing(12).align_y(Alignment::Center))
        .width(Length::Fill)
        .padding([7, 12])
        .class(style::flat_button(false))
        .on_press(msg)
        .into()
}

fn taskbar_menu(shell: &Shell) -> Element<'_, Message> {
    use crate::config::{Role, find_app};
    use crate::taskbar_extras::TaskbarOption as O;
    let mut items: Vec<Element<'_, Message>> = Vec::new();
    if let Some(id) = find_app(&shell.catalog, Role::TaskManager) {
        items.push(menu_item("utilities-system-monitor-symbolic", "Task Manager", Message::Launch(id)));
        items.push(menu_divider());
    }
    let center = shell.config.alignment == TaskbarAlignment::Center;
    items.push(menu_check("Center taskbar icons", center, Message::TaskbarOption(O::Align(TaskbarAlignment::Center))));
    items.push(menu_check("Left-align taskbar icons", !center, Message::TaskbarOption(O::Align(TaskbarAlignment::Left))));
    items.push(menu_divider());
    items.push(menu_check("Automatically hide the taskbar", shell.config.auto_hide, Message::TaskbarOption(O::ToggleAutoHide)));
    items.push(menu_check("Show Search box", shell.config.show_search, Message::TaskbarOption(O::ToggleSearch)));
    items.push(menu_check(
        "Show desktop button",
        shell.config.show_desktop_corner,
        Message::TaskbarOption(O::ToggleDesktopButton),
    ));
    panel(column::with_children(items).spacing(2), 260.0)
}

fn tray_overflow(shell: &Shell, bar: cosmic::iced::window::Id) -> Element<'_, Message> {
    let hidden: Vec<&crate::app::TrayEntry> = shell.tray.iter().filter(|t| !shell.tray_inline(t)).collect();
    let mut grid = column::with_capacity(4).spacing(2);
    for chunk in hidden.chunks(4) {
        let mut r = row::with_capacity(4).spacing(2);
        for t in chunk {
            let name = t.item.name().to_string();
            let content: Element<'_, Message> = match &t.icon {
                Some(h) => icon(h.clone()).size(16).into(),
                None => icon::from_name("image-missing-symbolic").size(16).into(),
            };
            let btn = button::custom(container(content).center(Length::Fill))
                .width(40)
                .height(40)
                .padding(0)
                .class(style::flat_button(false))
                .on_press(Message::TrayClick(bar, name.clone()));
            r = r.push(mouse_area(btn).on_right_press(Message::TrayContext(bar, name.clone())));
        }
        grid = grid.push(r);
    }
    panel(grid, 0.0)
}

/// One row in a popup menu.
pub fn menu_item<'a>(icon_name: &'a str, label: impl Into<String>, msg: Message) -> Element<'a, Message> {
    button::custom(
        row::with_children(vec![
            icon::from_name(icon_name.to_string()).size(16).into(),
            text::body(label.into()).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([7, 12])
    .class(style::flat_button(false))
    .on_press(msg)
    .into()
}

pub(crate) fn menu_divider<'a>() -> Element<'a, Message> {
    container(widget::divider::horizontal::light()).padding([4, 8]).into()
}

/// `width` 0 means "fit the content".
pub(crate) fn panel<'a>(content: impl Into<Element<'a, Message>>, width: f32) -> Element<'a, Message> {
    let c = container(content).padding(4).class(style::panel_bg());
    if width > 0.0 { c.width(width).into() } else { c.into() }
}

pub fn popup<'a>(shell: &'a Shell, popup: &'a Popup) -> Element<'a, Message> {
    match &popup.kind {
        // Clicking an app with several windows shows the same thumbnails.
        PopupKind::Windows(app) | PopupKind::Preview(app) => crate::previews::preview(shell, app),
        PopupKind::TaskContext(app) => {
            let mut items: Vec<Element<'_, Message>> = Vec::new();
            if let Some(data) = shell.catalog.data_for(app) {
                for action in &data.desktop_actions {
                    items.push(menu_item(
                        "go-next-symbolic",
                        action.name.clone(),
                        Message::LaunchAction(app.clone(), action.exec.clone()),
                    ));
                }
                if !data.desktop_actions.is_empty() {
                    items.push(menu_divider());
                }
            }
            items.push(
                button::custom(
                    row::with_children(vec![
                        icon(app_icon(shell, app)).size(16).into(),
                        text::body(app_name(shell, app)).into(),
                    ])
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([7, 12])
                .class(style::flat_button(false))
                .on_press(Message::Launch(app.clone()))
                .into(),
            );
            if shell.taskbar.is_pinned(app) {
                items.push(menu_item("view-pin-symbolic", "Unpin from taskbar", Message::Unpin(app.clone())));
            } else if shell.catalog.get(app).is_some() {
                items.push(menu_item("view-pin-symbolic", "Pin to taskbar", Message::Pin(app.clone())));
            }
            let n = shell.taskbar.windows().iter().filter(|w| &w.app == app).count();
            if n > 0 {
                let label = if n == 1 { "Close window" } else { "Close all windows" };
                items.push(menu_item("window-close-symbolic", label, Message::CloseApp(app.clone())));
            }
            panel(column::with_children(items).spacing(2), 280.0)
        }
        PopupKind::StartContext(app) => {
            let mut items: Vec<Element<'_, Message>> = Vec::new();
            if shell.config.pinned_start.contains(app) {
                items.push(menu_item("view-pin-symbolic", "Unpin from Start", Message::UnpinStart(app.clone())));
            } else {
                items.push(menu_item("view-pin-symbolic", "Pin to Start", Message::PinStart(app.clone())));
            }
            if shell.taskbar.is_pinned(app) {
                items.push(menu_item("view-pin-symbolic", "Unpin from taskbar", Message::Unpin(app.clone())));
            } else {
                items.push(menu_item("view-pin-symbolic", "Pin to taskbar", Message::Pin(app.clone())));
            }
            if crate::start_search::can_uninstall(shell, app) {
                items.push(menu_divider());
                items.push(menu_item("user-trash-symbolic", "Uninstall", Message::UninstallAsk(app.clone())));
            }
            panel(column::with_children(items).spacing(2), 240.0)
        }
        PopupKind::ConfirmUninstall(app) => {
            let name = shell.catalog.get(app).map_or(app.clone(), |a| a.data.name.clone());
            panel(
                column::with_children(vec![
                    text::body(format!("{name} and its related info will be uninstalled.")).into(),
                    row::with_children(vec![
                        widget::Space::new().width(Length::Fill).into(),
                        button::destructive("Uninstall").on_press(Message::Uninstall(app.clone())).into(),
                    ])
                    .into(),
                ])
                .spacing(16)
                .padding(16),
                300.0,
            )
        }
        PopupKind::TrayMenu(name) => tray_menu(shell, name),
        PopupKind::TaskbarMenu => taskbar_menu(shell),
        PopupKind::DesktopMenu(on, page) => crate::desktop::menu(shell, on, page),
        PopupKind::TrayOverflow => tray_overflow(shell, popup.parent),
        PopupKind::Power => panel(
            column::with_children(vec![
                menu_item("system-lock-screen-symbolic", "Lock", Message::Power(PowerAction::Lock)),
                menu_item("system-log-out-symbolic", "Sign out", Message::Power(PowerAction::SignOut)),
                menu_divider(),
                menu_item("weather-clear-night-symbolic", "Sleep", Message::Power(PowerAction::Sleep)),
                menu_item("system-reboot-symbolic", "Restart", Message::Power(PowerAction::Restart)),
                menu_item("system-shutdown-symbolic", "Shut down", Message::Power(PowerAction::ShutDown)),
            ])
            .spacing(2),
            200.0,
        ),
        PopupKind::Calendar => {
            let clock = shell.clock_style();
            let mut long = clock;
            long.seconds = true;
            let header = column::with_children(vec![
                text::title3(long.time(&shell.now)).into(),
                text::body(clock.long_date(&shell.now)).class(style::dim_text()).into(),
            ])
            .spacing(2)
            .padding([8, 12]);
            let cal = widget::calendar(
                &shell.calendar,
                Message::CalendarSelect,
                || Message::CalendarPrev,
                || Message::CalendarNext,
                if lapin_ux_core::clock::week_starts_sunday_env() {
                    jiff::civil::Weekday::Sunday
                } else {
                    jiff::civil::Weekday::Monday
                },
            );
            panel(
                column::with_children(vec![header.into(), menu_divider(), container(cal).padding(8).into()]),
                400.0,
            )
        }
    }
}

/// Bell next to the clock: a count bubble for unread notifications, or a
/// crossed-out bell while Do Not Disturb is on.
fn notify_badge(state: crate::notify_client::State) -> Element<'static, Message> {
    if state.dnd {
        return icon::from_name("notification-disabled-symbolic").size(16).into();
    }
    if state.unread == 0 {
        return icon::from_name("notification-symbolic").size(16).into();
    }
    let label = if state.unread > 99 { "99+".to_string() } else { state.unread.to_string() };
    container(text::caption(label))
        .padding([0, 6])
        .height(18)
        .center_y(18)
        .class(cosmic::theme::Container::custom(|t| {
            let c = t.cosmic();
            container::Style {
                background: Some(cosmic::iced::Background::Color(c.accent_color().into())),
                text_color: Some(c.on_accent_color().into()),
                border: cosmic::iced::Border { radius: 9.0.into(), ..Default::default() },
                ..Default::default()
            }
        }))
        .into()
}
