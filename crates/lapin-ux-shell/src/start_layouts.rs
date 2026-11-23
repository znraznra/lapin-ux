// SPDX-License-Identifier: GPL-3.0-only
//! The other Start layouts (#198): Classic, Classic with two columns and the
//! Programs list. They share the data and the helpers of the Current layout
//! (search, pinned and all-apps, other-desktop filtering, power actions).

use cosmic::iced::{Alignment, Background, Border, Length};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text, text_input};
use cosmic::{theme, Element};

use crate::app::{Message, PowerAction, SEARCH_ID, Shell, StartMenu, StartPage, rect_key};
use crate::config::{Role, StartLayout, find_app};
use crate::panels::PanelMsg;
use crate::start_view::{list_row, search_results, tracked};
use crate::style;

impl StartLayout {
    /// Width of the Start surface for this layout.
    pub fn width(self) -> u32 {
        match self {
            StartLayout::Current => style::START_WIDTH,
            StartLayout::Classic => 340,
            StartLayout::ClassicTwoColumns | StartLayout::ProgramsList => 600,
        }
    }
    /// The page Start opens on.
    pub fn first_page(self) -> StartPage {
        match self {
            StartLayout::ProgramsList => StartPage::AllApps,
            _ => StartPage::Home,
        }
    }
}

/// A flat menu row: icon, label, optional arrow for "opens more".
fn entry<'a>(name: &'static str, label: &'static str, arrow: bool, msg: Message) -> Element<'a, Message> {
    let mut r = row::with_capacity(4)
        .spacing(12)
        .align_y(Alignment::Center)
        .push(icon::from_name(name).size(20))
        .push(text::body(label).width(Length::Fill));
    if arrow {
        r = r.push(icon::from_name("go-next-symbolic").size(12));
    }
    button::custom(r)
        .width(Length::Fill)
        .padding([8, 12])
        .class(style::flat_button(false))
        .name(label)
        .on_press(msg)
        .into()
}

fn open_dir<'a>(name: &'static str, label: &'static str, dir: &str) -> Element<'a, Message> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    entry(name, label, false, Message::OpenFile(home.join(dir)))
}

fn files_app<'a>(shell: &Shell, label: &'static str) -> Element<'a, Message> {
    match find_app(&shell.catalog, Role::Files) {
        Some(id) => entry("computer-symbolic", label, false, Message::Launch(id)),
        None => widget::Space::new().into(),
    }
}

fn sep<'a>() -> Element<'a, Message> {
    container(widget::Space::new().height(1))
        .width(Length::Fill)
        .class(theme::Container::custom(|t| cosmic::iced::widget::container::Style {
            background: Some(Background::Color(t.cosmic().bg_divider().into())),
            ..Default::default()
        }))
        .into()
}

fn search_box<'a>(start: &'a StartMenu) -> Element<'a, Message> {
    container(
        text_input::search_input("Search for apps and documents", &start.query)
            .on_input(Message::StartQuery)
            .on_paste(Message::StartQuery)
            .on_submit(|_| Message::StartSubmit)
            .id(SEARCH_ID.clone())
            .always_active()
            .width(Length::Fill),
    )
    .padding([8, 12])
    .into()
}

fn power_row<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    row::with_children(vec![
        entry("system-shutdown-symbolic", "Shut Down", false, Message::Power(PowerAction::ShutDown)),
        tracked(
            shell,
            rect_key(start.id, "power"),
            button::custom(icon::from_name("go-next-symbolic").size(14))
                .padding(10)
                .class(style::flat_button(false))
                .name("Power options")
                .on_press(Message::PowerMenu),
        ),
    ])
    .align_y(Alignment::Center)
    .into()
}

/// Plain strip down the left edge, in the user's accent colour. No text.
fn strip<'a>() -> Element<'a, Message> {
    container(widget::Space::new())
        .width(14)
        .height(Length::Fill)
        .class(theme::Container::custom(|t| cosmic::iced::widget::container::Style {
            background: Some(Background::Color(t.cosmic().accent_color().into())),
            border: Border { radius: [style::PANEL_RADIUS - 1.0, 0.0, 0.0, style::PANEL_RADIUS - 1.0].into(),
                ..Default::default() },
            ..Default::default()
        }))
        .into()
}

/// Pinned apps, as rows.
fn pinned_rows<'a>(shell: &'a Shell, start: &'a StartMenu) -> Vec<Element<'a, Message>> {
    shell
        .config
        .pinned_start
        .iter()
        .filter(|a| shell.catalog.get(a).is_some())
        .map(|a| list_row(shell, start, a, None, false))
        .collect()
}

/// Recently opened apps, as rows (what is not already pinned).
fn recent_rows<'a>(shell: &'a Shell, start: &'a StartMenu) -> Vec<Element<'a, Message>> {
    shell
        .state
        .recent_apps
        .iter()
        .filter(|a| shell.catalog.get(a).is_some() && !shell.config.pinned_start.contains(a))
        .take(4)
        .map(|a| list_row(shell, start, a, None, false))
        .collect()
}

fn programs_row<'a>() -> Element<'a, Message> {
    entry("view-app-grid-symbolic", "Programs", true, Message::StartPage(StartPage::AllApps))
}

/// All apps as a scrolling list with a Back row at the bottom.
fn programs_list<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    let apps = shell.catalog.apps();
    let names: Vec<&str> = apps.iter().map(|a| a.data.name.as_str()).collect();
    let mut list = column::with_capacity(apps.len() + 30).spacing(2);
    let mut index = 0usize;
    for section in lapin_ux_core::apps::alphabet_sections(&names) {
        list = list.push(container(text::heading(section.header)).padding([8, 12, 2, 12]));
        for i in section.items {
            list = list.push(list_row(shell, start, apps[i].data.id.as_str(), None, start.selected == Some(index)));
            index += 1;
        }
    }
    column::with_children(vec![
        scrollable(list).height(Length::Fill).into(),
        sep(),
        entry("go-previous-symbolic", "Back", false, Message::StartPage(StartPage::Home)),
    ])
    .height(Length::Fill)
    .into()
}

/// The left column of the two-column layouts.
fn left_column<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    if !start.query.trim().is_empty() {
        return search_results(shell, start);
    }
    if start.page == StartPage::AllApps {
        return programs_list(shell, start);
    }
    let mut list = column::with_capacity(12).spacing(2);
    for r in pinned_rows(shell, start).into_iter().chain(recent_rows(shell, start)) {
        list = list.push(r);
    }
    column::with_children(vec![
        scrollable(list).height(Length::Fill).into(),
        sep(),
        programs_row(),
    ])
    .height(Length::Fill)
    .into()
}

fn user_header<'a>(shell: &'a Shell) -> Element<'a, Message> {
    row::with_children(vec![
        crate::account::avatar(shell, 48),
        text::title4(crate::start_view::account_name()).into(),
    ])
    .spacing(12)
    .align_y(Alignment::Center)
    .padding([12, 12])
    .into()
}

fn run_row<'a>() -> Element<'a, Message> {
    entry("system-run-symbolic", "Run...", false, Message::Panel(PanelMsg::OpenRun))
}

fn help_row<'a>() -> Element<'a, Message> {
    entry("help-about-symbolic", "Help and support", false, Message::OpenSettings("about"))
}

fn search_row<'a>() -> Element<'a, Message> {
    entry("system-search-symbolic", "Search", false, Message::OpenStartAt(StartPage::Home))
}

fn two_columns<'a>(shell: &'a Shell, start: &'a StartMenu, programs: bool) -> Element<'a, Message> {
    let mut right = column::with_capacity(16).spacing(2).push(user_header(shell));
    if programs {
        right = right
            .push(open_dir("folder-documents-symbolic", "Documents", "Documents"))
            .push(open_dir("folder-pictures-symbolic", "Pictures", "Pictures"))
            .push(open_dir("folder-music-symbolic", "Music", "Music"))
            .push(open_dir("folder-games-symbolic", "Games", "Games"))
            .push(entry("document-open-recent-symbolic", "Recent items", false, Message::OpenFile(
                std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Documents"))))
            .push(files_app(shell, "Computer"))
            .push(entry("preferences-system-symbolic", "Control Panel", false, Message::OpenSettings("")))
            .push(entry("printer-symbolic", "Devices and printers", false, Message::OpenSettings("printers")))
            .push(help_row())
            .push(run_row());
    } else {
        right = right
            .push(open_dir("folder-documents-symbolic", "Documents", "Documents"))
            .push(open_dir("folder-pictures-symbolic", "Pictures", "Pictures"))
            .push(files_app(shell, "Computer"))
            .push(entry("document-open-recent-symbolic", "Recent items", false, Message::OpenFile(
                std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Documents"))))
            .push(entry("preferences-system-symbolic", "Control Panel", false, Message::OpenSettings("")))
            .push(entry("printer-symbolic", "Printers", false, Message::OpenSettings("printers")))
            .push(search_row())
            .push(help_row())
            .push(run_row());
    }
    let left = column::with_children(vec![
        container(left_column(shell, start)).height(Length::Fill).padding([8, 4]).into(),
        sep(),
        search_box(start),
    ]);
    let right = column::with_children(vec![
        scrollable(right).height(Length::Fill).into(),
        sep(),
        power_row(shell, start),
    ]);
    container(
        row::with_children(vec![
            left.width(Length::FillPortion(1)).into(),
            right.width(Length::FillPortion(1)).into(),
        ]),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .class(style::menu_bg())
    .into()
}

fn classic<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    let body: Element<'_, Message> = if !start.query.trim().is_empty() {
        search_results(shell, start)
    } else if start.page == StartPage::AllApps {
        programs_list(shell, start)
    } else {
        let mut list = column::with_capacity(16).spacing(2);
        for r in pinned_rows(shell, start) {
            list = list.push(r);
        }
        list = list
            .push(sep())
            .push(programs_row())
            .push(entry("folder-documents-symbolic", "Documents", true,
                Message::OpenFile(std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Documents"))))
            .push(entry("preferences-system-symbolic", "Settings", false, Message::OpenSettings("")))
            .push(search_row())
            .push(help_row())
            .push(run_row());
        scrollable(list).height(Length::Fill).into()
    };
    container(row::with_children(vec![
        strip(),
        column::with_children(vec![
            container(body).height(Length::Fill).padding([8, 4]).into(),
            sep(),
            power_row(shell, start),
            search_box(start),
        ])
        .width(Length::Fill)
        .into(),
    ]))
    .width(Length::Fill)
    .height(Length::Fill)
    .class(style::menu_bg())
    .into()
}

/// The Start menu for a layout other than Current.
pub fn menu<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    match shell.config.start_layout {
        StartLayout::Classic => classic(shell, start),
        StartLayout::ClassicTwoColumns => two_columns(shell, start, false),
        StartLayout::ProgramsList => two_columns(shell, start, true),
        StartLayout::Current => unreachable!("Current uses start_view::start_menu"),
    }
}
