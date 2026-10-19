// SPDX-License-Identifier: GPL-3.0-only
//! Task view (Super+Tab, or the taskbar button next to Search): the open
//! windows of a desktop as large thumbnails above a row of desktops.
//! Click a window to switch to it; hover a desktop to see its windows and
//! click to switch; "New desktop" adds one; right-click a window to move it.

use std::collections::HashMap;

use cosmic::app::Task;
use cosmic::iced::{Alignment, Background, Border, Color, Length};
use cosmic::widget::{self, button, column, container, icon, mouse_area, row, text};
use cosmic::{Element, theme};

use crate::app::{Message, PopupKind, Shell, rect_key};
use crate::panels::PanelKind;
use crate::style;
use crate::taskbar_view::{app_icon, app_name};
use crate::wayland::{WaylandRequest, WindowId, WorkspaceId, WorkspaceInfo};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TaskView {
    /// Desktop under the pointer in the bottom row (its windows are shown).
    pub hover: Option<WorkspaceId>,
}

#[derive(Debug, Clone)]
pub enum TaskViewMsg {
    Toggle,
    Hover(Option<WorkspaceId>),
    Window(WindowId),
    Desktop(WorkspaceId),
    NewDesktop,
    RemoveDesktop(WorkspaceId),
    /// Right-click on a window: "Move to".
    Context(WindowId),
    MoveTo(WindowId, WorkspaceId),
    Close,
}

#[derive(Default)]
pub struct Desktops {
    pub list: Vec<WorkspaceInfo>,
    pub can_create: bool,
    /// Which desktops each window is on.
    pub of_window: HashMap<WindowId, Vec<WorkspaceId>>,
}

impl Shell {
    fn task_view(&mut self) -> Option<&mut TaskView> {
        match &mut self.panel {
            Some(crate::panels::Panel { kind: PanelKind::TaskView(t), .. }) => Some(t),
            _ => None,
        }
    }

    /// Desktops of the monitor the task view is on (the first one's if unsure).
    fn desktops(&self) -> Vec<&WorkspaceInfo> {
        let out = self.bars.first().and_then(|b| b.output_name.clone());
        let mine: Vec<&WorkspaceInfo> =
            self.desktops.list.iter().filter(|w| out.as_ref().is_none_or(|o| w.outputs.is_empty() || w.outputs.contains(o))).collect();
        if mine.is_empty() { self.desktops.list.iter().collect() } else { mine }
    }

    fn windows_on(&self, ws: Option<WorkspaceId>) -> Vec<WindowId> {
        self.taskbar
            .windows()
            .iter()
            .filter(|w| match (ws, self.desktops.of_window.get(&w.handle)) {
                (Some(ws), Some(list)) => list.contains(&ws),
                // No desktop information (other compositors): every window.
                _ => true,
            })
            .map(|w| w.handle)
            .collect()
    }

    pub(crate) fn update_taskview(&mut self, msg: TaskViewMsg) -> Task<Message> {
        match msg {
            TaskViewMsg::Toggle => {
                if self.task_view().is_some() {
                    return self.close_panel();
                }
                for id in self.taskbar.windows().iter().map(|w| w.handle).collect::<Vec<_>>() {
                    self.request(WaylandRequest::Capture(id));
                }
                return self.open_panel(PanelKind::TaskView(TaskView::default()));
            }
            TaskViewMsg::Hover(ws) => {
                // Capture the hovered desktop's windows too.
                if let Some(ws) = ws {
                    for id in self.windows_on(Some(ws)) {
                        if !self.thumbs.contains_key(&id) {
                            self.request(WaylandRequest::Capture(id));
                        }
                    }
                }
                if let Some(t) = self.task_view() {
                    t.hover = ws;
                }
            }
            TaskViewMsg::Window(id) => {
                let close = self.close_panel();
                return Task::batch([
                    close,
                    Task::future(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        cosmic::Action::App(Message::ActivateWindow(id))
                    }),
                ]);
            }
            TaskViewMsg::Desktop(ws) => {
                self.request(WaylandRequest::ActivateWorkspace(ws));
                return self.close_panel();
            }
            TaskViewMsg::NewDesktop => self.request(WaylandRequest::NewWorkspace),
            TaskViewMsg::RemoveDesktop(ws) => self.request(WaylandRequest::RemoveWorkspace(ws)),
            TaskViewMsg::Context(win) => {
                let Some(p) = &self.panel else { return Task::none() };
                let (parent, key) = (p.id, rect_key(p.id, &format!("tv:{win:?}")));
                return self.open_popup(parent, &key, PopupKind::MoveToDesktop(win));
            }
            TaskViewMsg::MoveTo(win, ws) => {
                self.request(WaylandRequest::MoveToWorkspace(win, ws));
                return self.close_popup();
            }
            TaskViewMsg::Close => return self.close_panel(),
        }
        Task::none()
    }
}

fn card_style(selected: bool) -> theme::Container<'static> {
    theme::Container::custom(move |t| {
        let c = t.cosmic();
        let accent: Color = c.accent_color().into();
        container::Style {
            background: Some(Background::Color(Color { a: 0.92, ..c.background(true).base.into() })),
            border: Border { color: if selected { accent } else { Color::TRANSPARENT }, width: 2.0, radius: 8.0.into() },
            text_color: Some(c.on_bg_color().into()),
            icon_color: Some(c.on_bg_color().into()),
            ..Default::default()
        }
    })
}

fn backdrop() -> theme::Container<'static> {
    theme::Container::custom(|_| container::Style { background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45))), ..Default::default() })
}

fn window_card<'a>(shell: &'a Shell, id: WindowId, panel: cosmic::iced::window::Id) -> Option<Element<'a, Message>> {
    let w = shell.taskbar.windows().iter().find(|w| w.handle == id)?;
    let title = if w.title.is_empty() { app_name(shell, &w.app) } else { w.title.clone() };
    let head = row::with_children(vec![
        icon(app_icon(shell, &w.app)).size(16).into(),
        text::body(title).width(Length::Fill).height(20).into(),
        button::custom(icon::from_name("window-close-symbolic").size(12))
            .padding(4)
            .class(style::flat_button(false))
            .on_press(Message::CloseWindow(id))
            .into(),
    ])
    .spacing(8)
    .align_y(Alignment::Center);
    let pic = crate::previews::picture_pub(shell, id, &w.app, 300.0, 190.0);
    let card = container(column::with_children(vec![head.into(), pic]).spacing(8).width(300))
        .padding(10)
        .class(card_style(w.active));
    let area = mouse_area(button::custom(card).padding(0).class(style::flat_button(false)).on_press(Message::TaskView(TaskViewMsg::Window(id))))
        .on_right_press(Message::TaskView(TaskViewMsg::Context(id)));
    Some(crate::taskbar_view::track(shell, rect_key(panel, &format!("tv:{id:?}")), area))
}

pub fn view<'a>(shell: &'a Shell, tv: &'a TaskView, panel: cosmic::iced::window::Id) -> Element<'a, Message> {
    let desktops = shell.desktops();
    let active = desktops.iter().find(|d| d.active).map(|d| d.id);
    let shown = tv.hover.or(active);
    let wins = shell.windows_on(shown);

    // Windows, four to a row.
    let cards: Vec<Element<'a, Message>> = wins.iter().filter_map(|id| window_card(shell, *id, panel)).collect();
    let grid: Element<'a, Message> = if cards.is_empty() {
        text::title4("No windows on this desktop").into()
    } else {
        let mut col = column::with_capacity(cards.len() / 4 + 1).spacing(20).align_x(Alignment::Center);
        let mut it = cards.into_iter().peekable();
        while it.peek().is_some() {
            col = col.push(row::with_children(it.by_ref().take(4).collect::<Vec<_>>()).spacing(20));
        }
        col.into()
    };

    // Desktops row.
    let mut strip = row::with_capacity(desktops.len() + 1).spacing(14).align_y(Alignment::Center);
    let many = desktops.len() > 1;
    for (i, d) in desktops.iter().enumerate() {
        let name = if d.name.is_empty() || d.name.chars().all(|c| c.is_ascii_digit()) { format!("Desktop {}", i + 1) } else { d.name.clone() };
        let mut head = row::with_capacity(2).align_y(Alignment::Center).push(text::body(name).width(Length::Fill));
        if many && d.can_remove {
            head = head.push(
                button::custom(icon::from_name("window-close-symbolic").size(10))
                    .padding(3)
                    .class(style::flat_button(false))
                    .on_press(Message::TaskView(TaskViewMsg::RemoveDesktop(d.id))),
            );
        }
        let count = shell.windows_on(Some(d.id)).len();
        let body = container(text::caption(if count == 1 { "1 window".to_string() } else { format!("{count} windows") }))
            .width(150)
            .height(56)
            .center(Length::Fill);
        let tile = container(column::with_children(vec![head.into(), body.into()]).spacing(4)).padding(8).width(170).class(card_style(d.active));
        strip = strip.push(
            mouse_area(button::custom(tile).padding(0).class(style::flat_button(false)).on_press(Message::TaskView(TaskViewMsg::Desktop(d.id))))
                .on_enter(Message::TaskView(TaskViewMsg::Hover(Some(d.id))))
                .on_exit(Message::TaskView(TaskViewMsg::Hover(None))),
        );
    }
    strip = strip.push(
        button::custom(
            container(
                column::with_children(vec![icon::from_name("list-add-symbolic").size(20).into(), text::body("New desktop").into()])
                    .spacing(6)
                    .align_x(Alignment::Center),
            )
            .width(150)
            .height(86)
            .center(Length::Fill),
        )
        .padding(0)
        .class(style::flat_button(false))
        .on_press(Message::TaskView(TaskViewMsg::NewDesktop)),
    );

    let content = column::with_children(vec![
        container(grid).width(Length::Fill).height(Length::Fill).center(Length::Fill).into(),
        container(container(strip).padding(12).class(card_style(false))).width(Length::Fill).center_x(Length::Fill).padding([0, 0, 24, 0]).into(),
    ]);
    // Clicking empty space closes it.
    mouse_area(container(content).width(Length::Fill).height(Length::Fill).class(backdrop()))
        .on_press(Message::TaskView(TaskViewMsg::Close))
        .into()
}

/// Right-click on a window: move it to another desktop.
pub fn move_menu(shell: &Shell, win: WindowId) -> Element<'_, Message> {
    use crate::taskbar_view::{menu_item, panel};
    let current = shell.desktops.of_window.get(&win).cloned().unwrap_or_default();
    let items: Vec<Element<'_, Message>> = shell
        .desktops()
        .iter()
        .enumerate()
        .filter(|(_, d)| !current.contains(&d.id))
        .map(|(i, d)| menu_item("go-next-symbolic", format!("Move to Desktop {}", i + 1), Message::TaskView(TaskViewMsg::MoveTo(win, d.id))))
        .collect();
    let items = if items.is_empty() { vec![widget::text::caption("No other desktops").into()] } else { items };
    panel(column::with_children(items).spacing(2), 220.0)
}
