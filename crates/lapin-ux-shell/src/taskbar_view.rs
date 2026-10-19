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
        None => crate::icons::full(app),
    }
}

/// Two overlapping window frames (our own drawing).
const TASK_VIEW_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="2.5" y="6.5" width="13" height="11" rx="2" fill="#8f9bb3" stroke="#e8ecf4" stroke-width="1.2"/><rect x="8.5" y="3.5" width="13" height="11" rx="2" fill="#3d8fe0" stroke="#e8ecf4" stroke-width="1.2"/></svg>"##;

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
        container(icon(icon::from_svg_bytes(START_ICON)).size(style::task_icon()))
            .center(Length::Fill),
    )
    .width(style::task_button())
    .height(style::task_button())
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

    // Task view, like the benchmark's button after Search.
    let tv_open = matches!(shell.panel.as_ref().map(|p| &p.kind), Some(crate::panels::PanelKind::TaskView(_)));
    center = center.push(
        button::custom(container(icon(icon::from_svg_bytes(TASK_VIEW_ICON)).size(style::task_icon())).center(Length::Fill))
            .width(style::task_button())
            .height(style::task_button())
            .padding(0)
            .class(style::flat_button(tv_open))
            .on_press(Message::TaskView(crate::taskview::TaskViewMsg::Toggle)),
    );

    // Task buttons get their own row so the widget tree keeps a stable
    // shape when the search box is toggled or buttons come and go.
    let main = shell.bars.first().is_some_and(|b| b.id == bar.id);
    let mut buttons = shell.taskbar.buttons_for(bar.output_name.as_deref(), main, shell.config.multi_display);
    // Only windows on the desktop in view, like the benchmark's default.
    let here = shell
        .desktops
        .list
        .iter()
        .find(|d| d.active && bar.output_name.as_ref().is_none_or(|o| d.outputs.is_empty() || d.outputs.contains(o)))
        .map(|d| d.id);
    if let Some(ws) = here {
        buttons = buttons
            .into_iter()
            .filter_map(|mut b| {
                // Windows asking for attention show wherever they are.
                b.windows.retain(|w| w.urgent || shell.desktops.of_window.get(&w.handle).is_none_or(|l| l.contains(&ws)));
                (b.pinned || !b.windows.is_empty()).then_some(b)
            })
            .collect();
    }
    // "Combine buttons: never" (or when the taskbar has room): a labelled
    // button per window; apps without windows keep their icon-only button.
    let tasks: Vec<Element<'_, Message>> = if shell.fit.labels {
        buttons
            .into_iter()
            .flat_map(|b| {
                if b.windows.is_empty() {
                    return vec![task_button(shell, bar, b, None)];
                }
                let Button { app, pinned, windows } = b;
                let first = windows.first().map(|w| w.handle);
                windows
                    .into_iter()
                    .map(|w| {
                        let label = (w.handle, if w.title.is_empty() { app_name(shell, &app) } else { w.title.clone() }, Some(w.handle) == first);
                        task_button(shell, bar, Button { app: app.clone(), pinned, windows: vec![w] }, Some(label))
                    })
                    .collect()
            })
            .collect()
    } else {
        buttons.into_iter().map(|b| task_button(shell, bar, b, None)).collect()
    };
    center = center.push(row::with_children(tasks).spacing(4).align_y(Alignment::Center));

    let clock_style = shell.clock_style();
    // Smaller buttons: a slightly smaller clock so both lines fit the 40 px bar.
    let clock_size = if style::small_taskbar() { 11.0 } else { 12.0 };
    let time = column::with_children(vec![
        text::caption(clock_style.time(&shell.now)).size(clock_size).into(),
        text::caption(clock_style.date(&shell.now)).size(clock_size).into(),
    ])
    .align_x(Alignment::End);
    let mut clock_content = row::with_capacity(2).spacing(8).align_y(Alignment::Center).push(time);
    if shell.notify.available {
        clock_content = clock_content.push(notify_badge(shell.notify));
    }
    let clock = button::custom(clock_content)
    .padding(if style::small_taskbar() { [0, 10] } else { [4, 10] })
    .class(style::flat_button(matches!(shell.popup.as_ref().map(|p| &p.kind), Some(PopupKind::Calendar))))
    .on_press(Message::ClockClick(bar.id));

    // Tray: hidden icons behind ^, chosen ones inline.
    let mut tray: Vec<Element<'_, Message>> = Vec::new();
    if shell.tray.iter().any(|t| !shell.tray_inline(t)) {
        let open = matches!(shell.popup.as_ref().map(|p| &p.kind), Some(PopupKind::TrayOverflow));
        let btn = button::custom(container(icon::from_name("pan-up-symbolic").size(16)).center(Length::Fill))
            .width(28)
            .height(style::task_button())
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
            .height(style::task_button())
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
    // Input indicator (several layouts or input methods), then status.
    if let Some(ind) = crate::input::indicator(shell, bar.id) {
        status_slot.insert(0, ind);
    }
    // Own row so the widget tree keeps its shape when the icons come and go.
    right = right.push(row::with_children(status_slot));
    right = right.push(track(shell, rect_key(bar.id, "clock"), clock));
    if shell.config.show_desktop_corner {
        // Resting on it peeks at the desktop (see bar_pointer).
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

/// `label`: one window's button with its title (window, title, first of its app).
fn task_button<'a>(shell: &'a Shell, bar: &'a Bar, b: Button<Handle>, label: Option<(Handle, String, bool)>) -> Element<'a, Message> {
    let active = b.active();
    let badge = shell.badge_for(&b.app).copied().unwrap_or_default();
    let indicator: Element<'_, Message> = if let Some(p) = badge.progress() {
        // Progress replaces the running line, like the benchmark.
        let fill = (24.0 * p).round().max(2.0);
        container(container(widget::Space::new().width(fill).height(3)).class(style::progress_fill()))
            .width(24)
            .class(style::progress_track())
            .into()
    } else if b.urgent() || badge.urgent {
        container(widget::Space::new().width(16).height(3)).class(style::urgent_indicator()).into()
    } else if b.running() {
        container(widget::Space::new().width(if active { 16 } else { 6 }).height(3))
            .class(style::indicator(active))
            .into()
    } else {
        widget::Space::new().width(6).height(3).into()
    };

    // The icon sits dead centre (equal space on all four sides, like Start
    // and the other taskbar buttons); the running line is laid over the
    // bottom margin instead of pushing the icon up.
    let centred = |el: Element<'a, Message>| -> Element<'a, Message> { container(el).center(Length::Fill).into() };
    let app_icon_el: Element<'_, Message> = icon(app_icon(shell, &b.app)).size(style::task_icon()).into();
    let main_layer: Element<'_, Message> = match &label {
        Some((_, title, _)) => container(
            row::with_children(vec![
                app_icon_el,
                text::caption(title.clone()).wrapping(cosmic::iced::widget::text::Wrapping::None).width(Length::Fill).into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([0, 8])
        .center_y(Length::Fill)
        .clip(true)
        .into(),
        None => {
            // Lifted while the app starts (padding below a centred box moves it up).
            let lift = shell.launch_lift(&b.app);
            centred(container(app_icon_el).padding(cosmic::iced::Padding { top: 0.0, right: 0.0, bottom: 2.0 * lift, left: 0.0 }).into())
        }
    };
    let mut layers: Vec<Element<'_, Message>> = vec![main_layer];
    if let Some(label) = badge.label() {
        // Count badge at the icon's top-right corner.
        layers.push(
            container(
                container(text::caption(label).size(10))
                    .padding([0, 4])
                    .height(16)
                    .align_y(Alignment::Center)
                    .class(style::badge()),
            )
            .width(Length::Fill)
            .align_x(Alignment::End)
            .padding([2, 2, 0, 0])
            .into(),
        );
    }
    layers.push(
        container(indicator)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::End)
            .padding([0, 0, 1, 0])
            .into(),
    );
    let content = cosmic::iced::widget::Stack::with_children(layers).width(Length::Fill).height(Length::Fill);

    let dragging = shell.drag.as_ref().is_some_and(|d| d.active && d.app == b.app);
    // Asking for attention: the button flashes a few times, then stays lit.
    let flashing = b.urgent() || badge.urgent;
    let class = if flashing {
        let since = b.windows.iter().filter_map(|w| shell.attention.get(&w.handle)).min().map(|t| t.elapsed().as_secs());
        let blink_off = since.is_some_and(|s| s < 7 && s % 2 == 1);
        if blink_off { style::flat_button(active) } else { style::urgent_button() }
    } else {
        style::flat_button(active || dragging)
    };
    let width = match &label {
        Some(_) => lapin_ux_core::taskbar::button_width(true, style::small_taskbar()) - 4.0,
        None => style::task_button(),
    };
    let press = match &label {
        Some((h, _, _)) => Message::TaskWindowClick(*h),
        None => Message::TaskClick(bar.id, b.app.clone()),
    };
    let btn = button::custom(content).width(width).height(style::task_button()).padding(0).class(class).on_press(press);

    let area = mouse_area(btn)
        .on_right_press(Message::TaskContext(bar.id, b.app.clone()))
        .on_middle_press(Message::Launch(b.app.clone()))
        .on_enter(Message::Preview(crate::previews::PreviewMsg::ButtonHover(bar.id, b.app.clone(), true)))
        .on_exit(Message::Preview(crate::previews::PreviewMsg::ButtonHover(bar.id, b.app.clone(), false)));

    // A file dragged onto a running app's button brings that app forward.
    // Every button gets the drop target, so the widget tree keeps the same
    // shape when an app starts or quits (iced can't swap it in place).
    let (enter, leave) = (b.app.clone(), b.app.clone());
    let area: Element<'_, Message> =
        cosmic::widget::dnd_destination::dnd_destination(area, vec!["text/uri-list".into(), "text/plain".into()])
            .on_enter(move |_, _, _| Message::DragHover(enter.clone(), true))
            .on_leave(move || Message::DragHover(leave.clone(), false))
            .into();
    // Previews and menus open over the app's (first) button.
    let key = match &label {
        Some((h, _, false)) => rect_key(bar.id, &format!("{}#{h:?}", b.app)),
        _ => rect_key(bar.id, &b.app),
    };
    track(shell, key, area)
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
        .height(style::task_button())
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
            // App icons in full colour; symbolic status icons stay symbolic.
            .size(if name.ends_with("-symbolic") { 16 } else { 128 })
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
            // No icon: no empty space either.
            (None, None) => widget::Space::new().width(0).height(16).into(),
        };
        let gap = if child.toggled().is_none() && child.icon_name().is_none() { 0 } else { 12 };
        let mut r = row::with_capacity(3).spacing(gap).align_y(Alignment::Center).push(lead);
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

fn taskbar_menu(shell: &Shell, popup: cosmic::iced::window::Id) -> Element<'_, Message> {
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
    // Plain rows close any open submenu when hovered.
    items = items.into_iter().map(crate::submenu::leaf).collect();
    items.push(crate::submenu::leaf(menu_divider()));
    items.push(crate::submenu::parent_entry(
        shell,
        popup,
        "tb-combine",
        crate::submenu::Lead::Blank,
        "Combine buttons and hide labels",
        Message::TaskbarOption(O::CombineMenu),
    ));
    items.push(crate::submenu::parent_entry(
        shell,
        popup,
        "tb-small",
        crate::submenu::Lead::Blank,
        "Show smaller taskbar buttons",
        Message::TaskbarOption(O::SmallMenu),
    ));
    if shell.bars.len() > 1 {
        items.push(crate::submenu::leaf(menu_divider()));
        items.push(crate::submenu::parent_entry(
            shell,
            popup,
            "tb-displays",
            crate::submenu::Lead::Blank,
            "Show taskbar apps on",
            Message::TaskbarOption(O::DisplaysMenu),
        ));
    }
    panel(column::with_children(items).spacing(2), 320.0)
}

/// "Always / When taskbar is full / Never" choices.
fn three_way<'a, T: Copy + PartialEq + 'static>(current: T, opts: [(T, &'static str); 3], msg: fn(T) -> Message) -> Element<'a, Message> {
    use crate::submenu::{Lead, entry};
    let items = opts
        .into_iter()
        .map(|(v, label)| entry(if current == v { Lead::Mark("\u{2022}") } else { Lead::Blank }, label, None, Some(msg(v))))
        .collect::<Vec<_>>();
    panel(column::with_children(items).spacing(2), 0.0)
}

fn taskbar_combine(shell: &Shell) -> Element<'_, Message> {
    use crate::taskbar_extras::TaskbarOption as O;
    use lapin_ux_core::taskbar::Combine as C;
    three_way(
        shell.config.combine_buttons,
        [(C::Always, "Always"), (C::WhenFull, "When taskbar is full"), (C::Never, "Never")],
        |c| Message::TaskbarOption(O::Combine(c)),
    )
}

fn taskbar_small(shell: &Shell) -> Element<'_, Message> {
    use crate::taskbar_extras::TaskbarOption as O;
    use lapin_ux_core::taskbar::SmallButtons as S;
    three_way(
        shell.config.small_buttons,
        [(S::Always, "Always"), (S::WhenFull, "When taskbar is full"), (S::Never, "Never")],
        |s| Message::TaskbarOption(O::Small(s)),
    )
}

fn taskbar_displays(shell: &Shell) -> Element<'_, Message> {
    use crate::taskbar_extras::TaskbarOption as O;
    use lapin_ux_core::taskbar::MultiDisplay as M;
    use crate::submenu::{Lead, entry};
    let items = [
        (M::AllTaskbars, "All taskbars"),
        (M::MainAndWhereOpen, "Main taskbar and taskbar where window is open"),
        (M::WhereOpen, "Taskbar where window is open"),
    ]
    .into_iter()
    .map(|(m, label)| {
        let lead = if shell.config.multi_display == m { Lead::Mark("\u{2022}") } else { Lead::Blank };
        entry(lead, label, None, Some(Message::TaskbarOption(O::MultiDisplay(m))))
    })
    .collect::<Vec<_>>();
    panel(column::with_children(items).spacing(2), 0.0)
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
    menu_item_with(icon::from_name(icon_name.to_string()).handle(), label, msg)
}

/// A menu row with a ready-made icon (e.g. a full-colour file icon).
pub fn menu_item_with<'a>(handle: icon::Handle, label: impl Into<String>, msg: Message) -> Element<'a, Message> {
    button::custom(
        row::with_children(vec![
            icon(handle).size(16).into(),
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
            if !shell.jump.is_empty() {
                items.push(container(text::caption("Recent").class(style::dim_text())).padding([6, 12, 2, 12]).into());
                for f in &shell.jump {
                    items.push(menu_item_with(
                        crate::icons::for_file(&f.name, false),
                        f.name.clone(),
                        Message::OpenWith(app.clone(), f.path.clone()),
                    ));
                }
                items.push(menu_divider());
            }
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
                let exec = shell.catalog.data_for(app).and_then(|d| d.exec.as_deref());
                if lapin_ux_core::procs::app_match(app, exec).is_some() {
                    items.push(menu_item("process-stop-symbolic", "End task", Message::EndTask(app.clone())));
                }
                items.push(menu_item("window-close-symbolic", label, Message::CloseApp(app.clone())));
            }
            panel(column::with_children(items).spacing(2), 280.0)
        }
        PopupKind::StartContext(app) => {
            let mut items: Vec<Element<'_, Message>> = Vec::new();
            if let Some((_, _, apps)) = shell.open_folder()
                && apps.contains(app)
            {
                items.push(menu_item("folder-symbolic", "Move out of folder", Message::Pins(crate::start_pins::PinMsg::TakeOut(app.clone()))));
            }
            if lapin_ux_core::pins::has_app(&lapin_ux_core::pins::decode_all(&shell.config.pinned_start), app) {
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
        PopupKind::TaskbarMenu => taskbar_menu(shell, popup.id),
        PopupKind::Account => crate::account::menu(shell),
        PopupKind::MoveToDesktop(w) => crate::taskview::move_menu(shell, *w),
        PopupKind::DesktopTile(i) => crate::taskview::desktop_menu(shell, *i),
        PopupKind::InputMenu => crate::input::menu(shell),
        PopupKind::DesktopBackground(i) => crate::taskview::background_menu(shell, *i),
        PopupKind::ConfirmEmptyTrash => panel(
            column::with_children(vec![
                text::heading("Delete multiple items").into(),
                text::body("Are you sure you want to permanently delete everything in the Recycle Bin?").into(),
                row::with_children(vec![
                    widget::Space::new().width(Length::Fill).into(),
                    button::standard("No").on_press(Message::ClosePopup).into(),
                    button::destructive("Yes").on_press(Message::Desktop(crate::desktop::DesktopMsg::EmptyTrash)).into(),
                ])
                .spacing(8)
                .into(),
            ])
            .spacing(12)
            .padding(16),
            320.0,
        ),
        PopupKind::PinContext(i) => crate::start_pins::context(shell, *i),
        PopupKind::TaskbarDisplays => taskbar_displays(shell),
        PopupKind::TaskbarCombine => taskbar_combine(shell),
        PopupKind::TaskbarSmall => taskbar_small(shell),
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
