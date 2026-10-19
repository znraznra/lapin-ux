// SPDX-License-Identifier: GPL-3.0-only
//! A file's Properties window (General tab): name, type, default app,
//! location, size, contents, dates and the Read-only / Hidden attributes.

use std::path::PathBuf;
use std::time::SystemTime;

use cosmic::app::Task;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, checkbox, column, container, icon, row, text, text_input};
use cosmic::{Application, Element};
use lapin_ux_core::props::{self, Props};

use crate::app::{Message, Shell};
use crate::panels::{Panel, PanelKind};
use crate::style;

pub const WIDTH: u32 = 420;

#[derive(Debug, Clone, PartialEq)]
pub struct PropsPanel {
    pub props: Props,
    pub name: String,
    pub read_only: bool,
    pub hidden: bool,
    /// Folder totals are still being counted.
    pub counting: bool,
    /// Default app: (desktop id, name).
    pub opens_with: Option<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum PropsMsg {
    Open(PathBuf),
    Totals(u64, u64, u64, u64),
    OpensWith(Option<String>),
    Name(String),
    ReadOnly(bool),
    Hidden(bool),
    Apply,
    Ok,
}

fn date(t: Option<SystemTime>) -> String {
    t.map(|t| {
        let dt: chrono::DateTime<chrono::Local> = t.into();
        dt.format("%A, %B %-d, %Y, %-I:%M:%S %p").to_string()
    })
    .unwrap_or_else(|| "Unknown".into())
}

async fn default_app(path: PathBuf) -> Option<String> {
    let out = tokio::process::Command::new("xdg-mime").arg("query").arg("filetype").arg(&path).output().await.ok()?;
    let mime = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if mime.is_empty() {
        return None;
    }
    let out = tokio::process::Command::new("xdg-mime").args(["query", "default", &mime]).output().await.ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().trim_end_matches(".desktop").to_string();
    (!id.is_empty()).then_some(id)
}

impl Shell {
    fn props_panel(&mut self) -> Option<&mut PropsPanel> {
        match &mut self.panel {
            Some(Panel { kind: PanelKind::Properties(p), .. }) => Some(p),
            _ => None,
        }
    }

    pub(crate) fn update_props(&mut self, msg: PropsMsg) -> Task<Message> {
        match msg {
            PropsMsg::Open(path) => {
                let Some(p) = props::read(&path) else { return Task::none() };
                let panel = PropsPanel {
                    name: p.name.clone(),
                    read_only: p.read_only,
                    hidden: p.hidden,
                    counting: p.is_dir,
                    opens_with: None,
                    props: p.clone(),
                };
                let open = self.open_panel(PanelKind::Properties(Box::new(panel)));
                let totals = if p.is_dir {
                    let dir = path.clone();
                    Task::future(async move {
                        let (a, b, c, d) = tokio::task::spawn_blocking(move || props::folder_totals(&dir)).await.unwrap_or_default();
                        cosmic::Action::App(Message::Props(PropsMsg::Totals(a, b, c, d)))
                    })
                } else {
                    Task::none()
                };
                let app = if p.is_dir {
                    Task::none()
                } else {
                    Task::future(async move { cosmic::Action::App(Message::Props(PropsMsg::OpensWith(default_app(path).await))) })
                };
                return Task::batch([open, totals, app]);
            }
            PropsMsg::Totals(size, disk, files, folders) => {
                if let Some(p) = self.props_panel() {
                    p.props.size = size;
                    p.props.size_on_disk = disk;
                    p.props.files = files;
                    p.props.folders = folders;
                    p.counting = false;
                }
            }
            PropsMsg::OpensWith(id) => {
                let named = id.map(|id| {
                    let name = crate::taskbar_view::app_name(self, &id);
                    (id, name)
                });
                if let Some(p) = self.props_panel() {
                    p.opens_with = named;
                }
            }
            PropsMsg::Name(n) => {
                if let Some(p) = self.props_panel() {
                    p.name = n;
                }
            }
            PropsMsg::ReadOnly(on) => {
                if let Some(p) = self.props_panel() {
                    p.read_only = on;
                }
            }
            PropsMsg::Hidden(on) => {
                if let Some(p) = self.props_panel() {
                    p.hidden = on;
                }
            }
            PropsMsg::Apply | PropsMsg::Ok => {
                let ok = matches!(msg, PropsMsg::Ok);
                let Some(p) = self.props_panel().cloned() else { return Task::none() };
                let mut path = p.props.path.clone();
                let mut problem = None;
                // Read-only: drop or restore the write bits.
                if p.read_only != p.props.read_only
                    && let Ok(meta) = std::fs::metadata(&path)
                {
                    let mut perm = meta.permissions();
                    use std::os::unix::fs::PermissionsExt;
                    let mode = perm.mode();
                    perm.set_mode(if p.read_only { mode & !0o222 } else { mode | 0o200 });
                    if let Err(e) = std::fs::set_permissions(&path, perm) {
                        problem = Some(e.to_string());
                    }
                }
                // Name, then Hidden (a leading dot on Linux).
                let mut name = p.name.trim().to_string();
                if p.hidden && !name.starts_with('.') {
                    name = format!(".{name}");
                } else if !p.hidden && name.starts_with('.') && p.props.hidden {
                    name = name.trim_start_matches('.').to_string();
                }
                if !name.is_empty() && name != p.props.name {
                    let dir = path.parent().map(PathBuf::from).unwrap_or_default();
                    match lapin_ux_core::desktop::rename_problem(&dir, &p.props.name, &name) {
                        Some(why) => problem = Some(why.to_string()),
                        None => {
                            let to = dir.join(&name);
                            match std::fs::rename(&path, &to) {
                                Ok(()) => path = to,
                                Err(e) => problem = Some(e.to_string()),
                            }
                        }
                    }
                }
                self.reload_desktop();
                if let Some(e) = problem {
                    return Task::future(async move {
                        crate::start_search::notify("Couldn't change the properties", &e).await;
                        cosmic::Action::App(Message::Noop)
                    });
                }
                if ok {
                    return self.close_panel();
                }
                return self.update(Message::Props(PropsMsg::Open(path)));
            }
        }
        Task::none()
    }
}

fn line<'a>(label: &'static str, value: impl Into<String>) -> Element<'a, Message> {
    row::with_children(vec![
        text::body(label).width(120).into(),
        text::body(value.into()).width(Length::Fill).into(),
    ])
    .spacing(8)
    .into()
}

fn rule<'a>() -> Element<'a, Message> {
    widget::divider::horizontal::light().into()
}

pub fn view<'a>(shell: &'a Shell, p: &'a PropsPanel) -> Element<'a, Message> {
    let pp = &p.props;
    let title = container(
        row::with_children(vec![
            text::body(format!("{} Properties", pp.name)).width(Length::Fill).into(),
            button::custom(icon::from_name("window-close-symbolic").size(16))
                .padding(6)
                .class(style::flat_button(false))
                .on_press(Message::Panel(crate::panels::PanelMsg::Close))
                .into(),
        ])
        .align_y(Alignment::Center),
    )
    .padding([4, 4, 4, 14])
    .class(style::panel_footer());

    let tab = container(text::body("General")).padding([4, 10]).class(style::card());
    let mut body = column::with_capacity(16).spacing(10).push(tab).push(
        row::with_children(vec![
            widget::icon(crate::icons::for_file(&pp.name, pp.is_dir)).size(32).into(),
            text_input::text_input("", &p.name).on_input(|s| Message::Props(PropsMsg::Name(s))).width(Length::Fill).into(),
        ])
        .spacing(14)
        .align_y(Alignment::Center),
    );
    body = body.push(rule());
    body = body.push(line(if pp.is_dir { "Type:" } else { "Type of file:" }, pp.kind.clone()));
    if !pp.is_dir {
        let app: Element<'a, Message> = match &p.opens_with {
            Some((id, name)) => row::with_children(vec![
                widget::icon(crate::taskbar_view::app_icon(shell, id)).size(16).into(),
                text::body(name.clone()).into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            None => text::body("Pick an app").class(style::dim_text()).into(),
        };
        body = body.push(row::with_children(vec![text::body("Opens with:").width(120).into(), app]).spacing(8));
    }
    body = body.push(rule());
    body = body.push(line("Location:", pp.location.clone()));
    let counting = |s: String| if p.counting { "Counting…".to_string() } else { s };
    body = body.push(line("Size:", counting(props::size_text(pp.size))));
    body = body.push(line("Size on disk:", counting(props::size_text(pp.size_on_disk))));
    if pp.is_dir {
        body = body.push(line("Contains:", counting(format!("{} Files, {} Folders", pp.files, pp.folders))));
    }
    body = body.push(rule());
    body = body.push(line("Created:", date(pp.created)));
    body = body.push(line("Modified:", date(pp.modified)));
    body = body.push(line("Accessed:", date(pp.accessed)));
    body = body.push(rule());
    body = body.push(
        row::with_children(vec![
            text::body("Attributes:").width(120).into(),
            checkbox(p.read_only).label("Read-only").on_toggle(|b| Message::Props(PropsMsg::ReadOnly(b))).into(),
            checkbox(p.hidden).label("Hidden").on_toggle(|b| Message::Props(PropsMsg::Hidden(b))).into(),
        ])
        .spacing(16)
        .align_y(Alignment::Center),
    );
    let changed = p.name != pp.name || p.read_only != pp.read_only || p.hidden != pp.hidden;
    body = body.push(
        row::with_children(vec![
            widget::Space::new().width(Length::Fill).into(),
            lapin_ux_ui::button::suggested("OK").on_press(Message::Props(PropsMsg::Ok)).into(),
            lapin_ux_ui::button::standard("Cancel").on_press(Message::Panel(crate::panels::PanelMsg::Close)).into(),
            lapin_ux_ui::button::standard("Apply").on_press_maybe(changed.then_some(Message::Props(PropsMsg::Apply))).into(),
        ])
        .spacing(8),
    );
    column::with_children(vec![title.into(), container(body).padding(16).into()]).into()
}
