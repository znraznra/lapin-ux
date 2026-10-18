// SPDX-License-Identifier: GPL-3.0-only
//! Start menu: search box, pinned grid, recommended files, All apps list,
//! account and power controls.

use std::sync::LazyLock;

use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, column, container, icon, mouse_area, row, scrollable, text, text_input};
use cosmic::Element;

use crate::start_search::StartItem;
use crate::app::{Message, SEARCH_ID, Shell, StartMenu, StartPage, rect_key};
use crate::style;
use crate::taskbar_view::{app_icon, app_name};

pub const PINNED_COLUMNS: usize = 6;

struct Account {
    name: String,
    avatar: Option<std::path::PathBuf>,
}

static ACCOUNT: LazyLock<Account> = LazyLock::new(|| {
    // Find our passwd line by uid (USER may be unset), then prefer the full
    // name from the GECOS field over the login name.
    let uid = std::fs::read_to_string("/proc/self/status").ok().and_then(|s| {
        s.lines().find_map(|l| l.strip_prefix("Uid:")).and_then(|v| v.split_whitespace().next().map(str::to_string))
    });
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    let entry = passwd.lines().map(|l| l.split(':').collect::<Vec<_>>()).find(|f| f.get(2).map(|u| u.to_string()) == uid);
    let login = entry
        .as_ref()
        .and_then(|f| f.first().map(|s| s.to_string()))
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_default();
    let full = entry
        .as_ref()
        .and_then(|f| f.get(4).map(|g| g.split(',').next().unwrap_or("").trim().to_string()))
        .filter(|g| !g.is_empty());
    let avatar = std::path::PathBuf::from(format!("/var/lib/AccountsService/icons/{login}"));
    Account { name: full.unwrap_or(login), avatar: avatar.exists().then_some(avatar) }
});

pub(crate) fn account_name() -> String {
    ACCOUNT.name.clone()
}

pub(crate) fn account_avatar() -> Option<std::path::PathBuf> {
    ACCOUNT.avatar.clone()
}

fn section_header<'a>(title: &'static str, action: Option<(&'static str, Message)>) -> Element<'a, Message> {
    let mut r = row::with_capacity(3)
        .align_y(Alignment::Center)
        .push(text::heading(title).width(Length::Fill));
    if let Some((label, msg)) = action {
        r = r.push(
            button::custom(
                row::with_children(vec![
                    text::caption(label).into(),
                    icon::from_name("go-next-symbolic").size(12).into(),
                ])
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([4, 10])
            .class(style::flat_button(true))
            .on_press(msg),
        );
    }
    r.into()
}

fn tracked<'a>(shell: &'a Shell, key: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    match &shell.tracker {
        Some(t) => t.container(key, content).into(),
        None => content.into(),
    }
}

/// Compact list row: icon and name, optional secondary line.
fn list_row<'a>(
    shell: &'a Shell,
    start: &'a StartMenu,
    app: &'a str,
    sub: Option<String>,
    selected: bool,
) -> Element<'a, Message> {
    let mut label = column::with_capacity(2).push(text::body(app_name(shell, app)));
    if let Some(sub) = sub {
        label = label.push(text::caption(sub).class(style::dim_text()));
    }
    let b = button::custom(
        row::with_children(vec![icon(app_icon(shell, app)).size(24).into(), label.into()])
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([6, 12])
    .class(style::flat_button(selected))
    .on_press(Message::Launch(app.to_string()));
    tracked(shell, rect_key(start.id, app), mouse_area(b).on_right_press(Message::StartContext(app.to_string())))
}

pub fn start_menu<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    let search = container(
        text_input::search_input("Search for apps and documents", &start.query)
            .on_input(Message::StartQuery)
            .on_paste(Message::StartQuery)
            .on_submit(|_| Message::StartSubmit)
            .id(SEARCH_ID.clone())
            .always_active()
            .width(Length::Fill),
    )
    .padding([24, 32, 16, 32]);

    let body: Element<'_, Message> = if !start.query.trim().is_empty() {
        search_results(shell, start)
    } else {
        match start.page {
            StartPage::Home => home(shell, start),
            StartPage::AllApps => all_apps(shell, start),
        }
    };

    let footer = container(
        row::with_children(vec![
            tracked(
                shell,
                rect_key(start.id, "account"),
                button::custom(
                    row::with_children(vec![crate::account::avatar(shell, 28), text::body(ACCOUNT.name.clone()).into()])
                        .spacing(10)
                        .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .class(style::flat_button(false))
                .on_press(Message::Account(crate::account::AccountMsg::Menu)),
            ),
            widget::Space::new().width(Length::Fill).into(),
            tracked(
                shell,
                rect_key(start.id, "power"),
                button::custom(icon::from_name("system-shutdown-symbolic").size(16))
                    .padding(10)
                    .class(style::flat_button(false))
                    .on_press(Message::PowerMenu),
            ),
        ])
        .align_y(Alignment::Center),
    )
    .padding([8, 40])
    .width(Length::Fill)
    .class(style::panel_footer());

    container(
        column::with_children(vec![
            search.into(),
            container(body).padding([0, 32]).height(Length::Fill).into(),
            footer.into(),
        ]),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .class(style::panel_bg())
    .into()
}

fn home<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    let pinned_block = crate::start_pins::pinned_block(shell, start);

    // Recommended: recent documents, or recently opened apps if there are none.
    let now = chrono::Utc::now();
    let mut rec: Vec<Element<'_, Message>> = start
        .recent_files
        .iter()
        .map(|f| {
            let icon_name = mime_icon(&f.name);
            button::custom(
                row::with_children(vec![
                    icon::from_name(icon_name).size(28).into(),
                    column::with_children(vec![
                        text::body(f.name.clone()).into(),
                        text::caption(lapin_ux_core::recent::relative(&f.modified, now)).class(style::dim_text()).into(),
                    ])
                    .into(),
                ])
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([8, 12])
            .class(style::flat_button(false))
            .on_press(Message::OpenFile(f.path.clone()))
            .into()
        })
        .collect();
    if rec.is_empty() {
        rec = shell
            .state
            .recent_apps
            .iter()
            .filter(|a| shell.catalog.get(a).is_some())
            .map(|a| list_row(shell, start, a, Some("Recently opened".into()), false))
            .collect();
    }
    let recommended: Element<'_, Message> = if rec.is_empty() {
        text::body("Files and apps you open will show up here.").class(style::dim_text()).into()
    } else {
        // Two columns, like the benchmark.
        let mut col_a = column::with_capacity(3).spacing(4);
        let mut col_b = column::with_capacity(3).spacing(4);
        for (i, e) in rec.into_iter().enumerate() {
            if i % 2 == 0 {
                col_a = col_a.push(e);
            } else {
                col_b = col_b.push(e);
            }
        }
        row::with_children(vec![col_a.width(Length::Fill).into(), col_b.width(Length::Fill).into()])
            .spacing(8)
            .into()
    };

    column::with_children(vec![
        section_header("Pinned", Some(("All", Message::StartPage(StartPage::AllApps)))),
        widget::Space::new().height(12).into(),
        pinned_block,
        widget::Space::new().height(Length::Fill).into(),
        section_header("Recommended", None),
        widget::Space::new().height(8).into(),
        recommended,
        widget::Space::new().height(16).into(),
    ])
    .height(Length::Fill)
    .into()
}

fn all_apps<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    let apps = shell.catalog.apps();
    let names: Vec<&str> = apps.iter().map(|a| a.data.name.as_str()).collect();
    let mut list = column::with_capacity(apps.len() + 30).spacing(2);
    let mut index = 0usize;
    for section in lapin_ux_core::apps::alphabet_sections(&names) {
        list = list.push(container(text::heading(section.header)).padding([10, 12, 4, 12]));
        for i in section.items {
            let id = apps[i].data.id.as_str();
            list = list.push(list_row(shell, start, id, None, start.selected == Some(index)));
            index += 1;
        }
    }

    let back = button::custom(
        row::with_children(vec![
            icon::from_name("go-previous-symbolic").size(12).into(),
            text::caption("Back").into(),
        ])
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([4, 10])
    .class(style::flat_button(true))
    .on_press(Message::StartPage(StartPage::Home));

    column::with_children(vec![
        row::with_children(vec![text::heading("All apps").width(Length::Fill).into(), back.into()])
            .align_y(Alignment::Center)
            .into(),
        widget::Space::new().height(8).into(),
        scrollable(list).height(Length::Fill).into(),
    ])
    .height(Length::Fill)
    .into()
}

fn result_row<'a>(icon_handle: icon::Handle, title: String, sub: String, selected: bool, item: StartItem) -> Element<'a, Message> {
    button::custom(
        row::with_children(vec![
            icon(icon_handle).size(24).into(),
            column::with_children(vec![text::body(title).into(), text::caption(sub).class(style::dim_text()).into()]).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([6, 12])
    .class(style::flat_button(selected))
    .on_press(Message::StartActivate(item))
    .into()
}

fn named(name: &str) -> icon::Handle {
    icon::from_name(name.to_string())
        .fallback(Some(icon::IconFallback::Names(vec!["text-x-generic".into(), "application-x-executable".into()])))
        .handle()
}

fn file_icon(path: &std::path::Path, is_dir: bool) -> icon::Handle {
    if is_dir {
        return named("folder");
    }
    named(mime_icon(&file_name(path)))
}

/// Folder shown under a file name, with home abbreviated to ~.
fn location(path: &std::path::Path) -> String {
    let parent = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    match std::env::var("HOME") {
        Ok(home) if parent.starts_with(&home) => format!("~{}", &parent[home.len()..]),
        _ => parent,
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn item_row<'a>(shell: &'a Shell, start: &'a StartMenu, item: &StartItem, selected: bool) -> Element<'a, Message> {
    match item {
        StartItem::App(id) => match shell.catalog.get(id) {
            // Borrow the id from the catalog so the row can outlive `item`.
            Some(app) => list_row(shell, start, &app.data.id, app.generic_name.clone().or_else(|| Some("App".into())), selected),
            None => widget::Space::new().into(),
        },
        StartItem::Setting(p) => result_row(named(p.icon), p.title.to_string(), "System settings".into(), selected, item.clone()),
        StartItem::File { path, is_dir } => result_row(file_icon(path, *is_dir), file_name(path), location(path), selected, item.clone()),
        StartItem::Pin(_) => widget::Space::new().into(),
        StartItem::Web(q) => result_row(
            named("system-search-symbolic"),
            format!("{q} \u{2014} See web results"),
            "Search the web".into(),
            selected,
            item.clone(),
        ),
    }
}

fn detail_card<'a>(shell: &'a Shell, item: &StartItem) -> Element<'a, Message> {
    let head = |handle: icon::Handle, title: String, sub: String| {
        // Full width so everything centres in the card, whatever its contents.
        column::with_capacity(8)
            .width(Length::Fill)
            .spacing(8)
            .align_x(Alignment::Center)
            .push(widget::Space::new().height(16))
            .push(icon(handle).size(64))
            .push(text::title4(title).align_x(Alignment::Center))
            .push(text::caption(sub).class(style::dim_text()).align_x(Alignment::Center))
            .push(widget::Space::new().height(8))
    };
    let action = |label: String, msg: Message| {
        button::custom(text::body(label).width(Length::Fill))
            .width(Length::Fill)
            .padding([6, 12])
            .class(style::flat_button(false))
            .on_press(msg)
    };
    let card = match item {
        StartItem::App(id) => {
            let Some(app) = shell.catalog.get(id) else { return widget::Space::new().into() };
            let mut card = head(app_icon(shell, id), app.data.name.clone(), app.generic_name.clone().unwrap_or_else(|| "App".into()))
                .push(button::suggested("Open").on_press(Message::Launch(id.clone())));
            for a in app.data.desktop_actions.iter().take(5) {
                card = card.push(action(a.name.clone(), Message::LaunchAction(id.clone(), a.exec.clone())));
            }
            if crate::start_search::can_uninstall(shell, id) {
                card = card.push(action("Uninstall".into(), Message::UninstallAsk(id.clone())));
            }
            card
        }
        StartItem::Setting(p) => head(named(p.icon), p.title.to_string(), "System settings".into())
            .push(button::suggested("Open").on_press(Message::StartActivate(item.clone()))),
        StartItem::File { path, is_dir } => {
            let kind = if *is_dir { "Folder" } else { "File" };
            head(file_icon(path, *is_dir), file_name(path), format!("{kind} \u{00B7} {}", location(path)))
                .push(button::suggested("Open").on_press(Message::StartActivate(item.clone())))
                .push(action("Open file location".into(), Message::ShowInFolder(path.clone())))
                .push(action("Pin to Start".into(), Message::Pins(crate::start_pins::PinMsg::PinPath(path.clone()))))
        }
        StartItem::Pin(_) => return widget::Space::new().into(),
        StartItem::Web(q) => head(named("system-search-symbolic"), q.clone(), "Search the web".into())
            .push(button::suggested("Open results in browser").on_press(Message::StartActivate(item.clone()))),
    };
    card.into()
}

fn search_results<'a>(shell: &'a Shell, start: &'a StartMenu) -> Element<'a, Message> {
    let results = shell.search(&start.query);
    let flat = results.flat();
    if flat.is_empty() {
        return container(text::body(format!("No results for \u{201C}{}\u{201D}", start.query.trim())).class(style::dim_text()))
            .padding([24, 0])
            .into();
    }
    let selected = start.selected.unwrap_or(0).min(flat.len() - 1);

    let sections: [(&str, Vec<StartItem>); 6] = [
        ("Best match", results.best.clone().into_iter().collect()),
        ("Apps", results.apps.clone()),
        ("Settings", results.settings.clone()),
        ("Folders", results.folders.clone()),
        ("Documents", results.documents.clone()),
        ("Search the web", results.web.clone().into_iter().collect()),
    ];
    let mut list = column::with_capacity(flat.len() + 6).spacing(2);
    let mut i = 0usize;
    for (title, items) in sections {
        if items.is_empty() {
            continue;
        }
        list = list.push(container(text::heading(title)).padding([8, 0, 4, 0]));
        for it in &items {
            list = list.push(item_row(shell, start, it, i == selected));
            i += 1;
        }
    }

    row::with_children(vec![
        scrollable(list).height(Length::Fill).width(Length::FillPortion(3)).into(),
        container(detail_card(shell, &flat[selected])).padding(12).width(Length::FillPortion(2)).class(style::card()).into(),
    ])
    .spacing(16)
    .height(Length::Fill)
    .into()
}

/// Rough file-type icon from the extension (no MIME database lookup yet).
pub(crate) fn mime_icon(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif" | "heic" => "image-x-generic",
        "mp4" | "mkv" | "webm" | "mov" | "avi" => "video-x-generic",
        "mp3" | "flac" | "ogg" | "wav" | "m4a" | "opus" => "audio-x-generic",
        "pdf" => "application-pdf",
        "odt" | "doc" | "docx" | "rtf" => "x-office-document",
        "ods" | "xls" | "xlsx" | "csv" => "x-office-spreadsheet",
        "odp" | "ppt" | "pptx" => "x-office-presentation",
        "zip" | "tar" | "gz" | "xz" | "7z" | "rar" | "zst" => "package-x-generic",
        "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "sh" | "json" | "toml" | "html" | "css" => "text-x-script",
        _ => "text-x-generic",
    }
}
