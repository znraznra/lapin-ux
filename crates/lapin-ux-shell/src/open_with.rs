// SPDX-License-Identifier: GPL-3.0-only
//! "How do you want to open this file?": the app picker behind Open with ›
//! Choose another app. Suggested apps (the ones that say they handle the
//! file type) come first, then every other app; "Always use this app"
//! makes the choice the default (xdg-mime).

use std::path::PathBuf;

use cosmic::app::Task;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, checkbox, column, container, icon, row, scrollable, text};
use cosmic::{Application, Element};

use crate::app::{Message, Shell};
use crate::panels::{Panel, PanelKind};
use crate::style;

pub const WIDTH: u32 = 400;

#[derive(Debug, Clone, PartialEq)]
pub struct OpenWithPanel {
    pub path: PathBuf,
    pub mime: String,
    pub suggested: Vec<String>,
    pub others: Vec<String>,
    pub selected: Option<String>,
    pub always: bool,
    pub more: bool,
}

#[derive(Debug, Clone)]
pub enum OpenWithMsg {
    Open(PathBuf),
    Select(String),
    Always(bool),
    More,
    Ok,
}

fn mime_of(path: &std::path::Path) -> String {
    if path.is_dir() {
        return "inode/directory".into();
    }
    mime_guess::from_path(path).first_raw().unwrap_or("application/octet-stream").to_string()
}

impl Shell {
    fn open_with_panel(&mut self) -> Option<&mut OpenWithPanel> {
        match &mut self.panel {
            Some(Panel { kind: PanelKind::OpenWith(p), .. }) => Some(p),
            _ => None,
        }
    }

    pub(crate) fn update_open_with(&mut self, msg: OpenWithMsg) -> Task<Message> {
        match msg {
            OpenWithMsg::Open(path) => {
                let suggested = crate::desktop::open_with_apps(self, &path);
                let mut others: Vec<String> = self
                    .catalog
                    .apps()
                    .iter()
                    .map(|a| a.data.id.clone())
                    .filter(|id| !suggested.contains(id))
                    .collect();
                others.sort_by_key(|id| crate::taskbar_view::app_name(self, id).to_lowercase());
                others.dedup();
                let selected = suggested.first().cloned();
                let mime = mime_of(&path);
                let close = self.close_popup();
                let panel = OpenWithPanel { path, mime, suggested, others, selected, always: false, more: false };
                return Task::batch([close, self.open_panel(PanelKind::OpenWith(Box::new(panel)))]);
            }
            OpenWithMsg::Select(id) => {
                if let Some(p) = self.open_with_panel() {
                    p.selected = Some(id);
                }
            }
            OpenWithMsg::Always(on) => {
                if let Some(p) = self.open_with_panel() {
                    p.always = on;
                }
            }
            OpenWithMsg::More => {
                if let Some(p) = self.open_with_panel() {
                    p.more = true;
                }
            }
            OpenWithMsg::Ok => {
                let Some(p) = self.open_with_panel().cloned() else { return Task::none() };
                let Some(app) = p.selected else { return Task::none() };
                if p.always {
                    let (desktop, mime) = (format!("{app}.desktop"), p.mime.clone());
                    std::thread::spawn(move || {
                        let _ = std::process::Command::new("xdg-mime").args(["default", &desktop, &mime]).status();
                    });
                }
                let close = self.close_panel();
                return Task::batch([close, self.update(Message::OpenWith(app, p.path))]);
            }
        }
        Task::none()
    }
}

fn app_row<'a>(shell: &'a Shell, id: &'a str, selected: bool) -> Element<'a, Message> {
    button::custom(
        row::with_children(vec![
            icon(crate::taskbar_view::app_icon(shell, id)).size(32).into(),
            text::body(crate::taskbar_view::app_name(shell, id)).into(),
        ])
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([6, 10])
    .class(style::flat_button(selected))
    .on_press(Message::OpenWithPick(OpenWithMsg::Select(id.to_string())))
    .into()
}

pub fn view<'a>(shell: &'a Shell, p: &'a OpenWithPanel) -> Element<'a, Message> {
    let name = p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = name.rsplit_once('.').map(|(_, e)| format!(".{e}")).filter(|_| !p.path.is_dir());
    let mut list = column::with_capacity(p.suggested.len() + p.others.len() + 3).spacing(2);
    if !p.suggested.is_empty() {
        list = list.push(container(text::caption("Suggested apps").class(style::dim_text())).padding([4, 10]));
        for id in &p.suggested {
            list = list.push(app_row(shell, id, p.selected.as_deref() == Some(id.as_str())));
        }
    }
    if p.more || p.suggested.is_empty() {
        list = list.push(container(text::caption("Other options").class(style::dim_text())).padding([8, 10, 4, 10]));
        for id in &p.others {
            list = list.push(app_row(shell, id, p.selected.as_deref() == Some(id.as_str())));
        }
    } else {
        list = list.push(
            lapin_ux_ui::button::text("More apps ↓").on_press(Message::OpenWithPick(OpenWithMsg::More)),
        );
    }
    let always = match &ext {
        Some(e) => format!("Always use this app to open {e} files"),
        None => "Always use this app".to_string(),
    };
    column::with_children(vec![
        container(text::title4("How do you want to open this file?")).padding([20, 20, 4, 20]).into(),
        container(text::caption(name).class(style::dim_text())).padding([0, 20, 8, 20]).into(),
        container(scrollable(list).height(Length::Fixed(320.0))).padding([0, 10]).into(),
        container(checkbox(p.always).label(always).on_toggle(|b| Message::OpenWithPick(OpenWithMsg::Always(b)))).padding([12, 20]).into(),
        container(
            row::with_children(vec![
                widget::Space::new().width(Length::Fill).into(),
                lapin_ux_ui::button::suggested("OK").on_press_maybe(p.selected.is_some().then_some(Message::OpenWithPick(OpenWithMsg::Ok))).into(),
            ]),
        )
        .padding([12, 20, 16, 20])
        .class(style::panel_footer())
        .into(),
    ])
    .into()
}
