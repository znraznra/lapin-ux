// SPDX-License-Identifier: GPL-3.0-only
//! Atrium notification daemon: owns `org.freedesktop.Notifications` and
//! shows toasts stacked in the bottom-right corner above the taskbar.

mod dbus;

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use atrium_core::notify::{CloseReason, Queue, Toast, Urgency};
use cosmic::app::{Core, Settings, Task};
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{IcedMargin, IcedOutput, SctkLayerSurfaceSettings};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer, destroy_layer_surface, get_layer_surface,
};
use cosmic::iced::{Alignment, Background, Border, Color, Length, Limits, Shadow, Subscription, Vector, window};
use cosmic::widget::{self, autosize, button, column, container, icon, mouse_area, row, text};
use cosmic::{Application, Element, theme};

const WIDTH: u32 = 364;

static AUTOSIZE: LazyLock<cosmic::widget::Id> = LazyLock::new(|| cosmic::widget::Id::new("atrium-toasts"));

#[derive(Debug, Clone)]
enum Message {
    Dbus(dbus::Event),
    Tick,
    Hover(bool),
    Dismiss(u32),
    Invoke(u32, String),
    Noop,
}

struct App {
    core: Core,
    queue: Queue,
    conn: Option<zbus::Connection>,
    surface: Option<window::Id>,
    hovered: bool,
}

impl App {
    /// Create or destroy the layer surface to match whether toasts exist.
    fn sync_surface(&mut self) -> Task<Message> {
        match (self.queue.visible().is_empty(), self.surface) {
            (false, None) => {
                let id = window::Id::unique();
                self.surface = Some(id);
                get_layer_surface(SctkLayerSurfaceSettings {
                    id,
                    layer: Layer::Overlay,
                    keyboard_interactivity: KeyboardInteractivity::None,
                    anchor: Anchor::BOTTOM | Anchor::RIGHT,
                    output: IcedOutput::Active,
                    namespace: "atrium-notifications".into(),
                    margin: IcedMargin { top: 0, right: 12, bottom: 12, left: 0 },
                    size: Some((Some(WIDTH), Some(1))),
                    exclusive_zone: 0,
                    size_limits: Limits::NONE.min_width(WIDTH as f32).max_width(WIDTH as f32).min_height(1.0).max_height(1600.0),
                    ..Default::default()
                })
            }
            (true, Some(id)) => {
                self.surface = None;
                self.hovered = false;
                destroy_layer_surface(id)
            }
            _ => Task::none(),
        }
    }

    fn signal_closed(&self, ids: impl IntoIterator<Item = u32>, reason: CloseReason) -> Task<Message> {
        let Some(conn) = self.conn.clone() else { return Task::none() };
        let ids: Vec<u32> = ids.into_iter().collect();
        if ids.is_empty() {
            return Task::none();
        }
        Task::future(async move {
            for id in ids {
                dbus::emit_closed(conn.clone(), id, reason).await;
            }
            cosmic::Action::App(Message::Noop)
        })
    }
}

impl Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = atrium_core::ids::NOTIFYD_APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _: ()) -> (Self, Task<Message>) {
        (App { core, queue: Queue::default(), conn: None, surface: None, hovered: false }, Task::none())
    }

    fn view(&self) -> Element<'_, Message> {
        unreachable!("no main window")
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Message> {
        let cards: Vec<Element<'_, Message>> = self.queue.visible().iter().map(toast).collect();
        let list = column::with_children(cards).spacing(8).width(Length::Fixed(WIDTH as f32));
        let area = mouse_area(list).on_enter(Message::Hover(true)).on_exit(Message::Hover(false));
        autosize::autosize(area, AUTOSIZE.clone())
            .min_width(WIDTH as f32)
            .max_width(WIDTH as f32)
            .min_height(1.0)
            .max_height(1600.0)
            .into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        let t = cosmic::theme::active();
        Some(cosmic::iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: t.cosmic().on_bg_color().into(),
            icon_color: t.cosmic().on_bg_color().into(),
        })
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            dbus::subscription().map(Message::Dbus),
            // The pointer can leave the surface without crossing a widget
            // edge (e.g. the toast under it closes); treat that as un-hover.
            cosmic::iced::event::listen_raw(|e, _, _| match e {
                cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::CursorLeft) => Some(Message::Hover(false)),
                _ => None,
            }),
        ];
        if self.surface.is_some() {
            subs.push(cosmic::iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick));
        }
        Subscription::batch(subs)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let now = Instant::now();
        match message {
            Message::Dbus(ev) => match ev {
                dbus::Event::Ready(conn) => self.conn = Some(conn),
                dbus::Event::Failed(e) => {
                    tracing::error!("cannot own {}: {e} (is another notification daemon running?)", dbus::NAME);
                }
                dbus::Event::Notify(n) => {
                    self.queue.push(*n, now);
                    return self.sync_surface();
                }
                dbus::Event::Close(id) => {
                    let existed = self.queue.close(id, now);
                    self.queue.forget(id);
                    let signal = if existed { self.signal_closed([id], CloseReason::CloseCall) } else { Task::none() };
                    return Task::batch([signal, self.sync_surface()]);
                }
            },
            Message::Tick => {
                if self.hovered {
                    self.queue.hold(now);
                    return Task::none();
                }
                let expired = self.queue.tick(now);
                if !expired.is_empty() {
                    let signal = self.signal_closed(expired, CloseReason::Expired);
                    return Task::batch([signal, self.sync_surface()]);
                }
            }
            Message::Hover(h) => {
                self.hovered = h;
                if h {
                    self.queue.hold(now);
                }
            }
            Message::Dismiss(id) => {
                self.queue.close(id, now);
                let signal = self.signal_closed([id], CloseReason::Dismissed);
                return Task::batch([signal, self.sync_surface()]);
            }
            Message::Invoke(id, key) => {
                self.queue.close(id, now);
                let mut tasks = vec![self.sync_surface()];
                if let Some(conn) = self.conn.clone() {
                    tasks.push(Task::future(async move {
                        dbus::emit_action(conn.clone(), id, key, None).await;
                        // Invoking an action also closes the notification.
                        dbus::emit_closed(conn, id, CloseReason::Dismissed).await;
                        cosmic::Action::App(Message::Noop)
                    }));
                }
                return Task::batch(tasks);
            }
            Message::Noop => {}
        }
        Task::none()
    }
}

fn icon_for(n: &atrium_core::notify::Notification) -> icon::Handle {
    let name = if !n.app_icon.is_empty() {
        n.app_icon.as_str()
    } else {
        n.desktop_entry.as_deref().unwrap_or("dialog-information-symbolic")
    };
    if let Some(path) = name.strip_prefix("file://").or_else(|| name.starts_with('/').then_some(name)) {
        return icon::from_path(path.into());
    }
    icon::from_name(name.to_string())
        .fallback(Some(icon::IconFallback::Names(vec!["dialog-information-symbolic".into()])))
        .handle()
}

fn card_style(critical: bool) -> theme::Container<'static> {
    theme::Container::custom(move |t| {
        let c = t.cosmic();
        let mut bg: Color = c.background(true).base.into();
        bg.a = 0.96;
        let border: Color = if critical { c.destructive_color().into() } else { c.bg_divider().into() };
        container::Style {
            background: Some(Background::Color(bg)),
            border: Border { color: border, width: 1.0, radius: 8.0.into() },
            shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.25), offset: Vector::new(0.0, 4.0), blur_radius: 16.0 },
            text_color: Some(c.on_bg_color().into()),
            icon_color: Some(c.on_bg_color().into()),
            ..Default::default()
        }
    })
}

fn toast(t: &Toast) -> Element<'_, Message> {
    let n = &t.notification;
    let header = row::with_children(vec![
        text::caption(if n.app_name.is_empty() { "Notification".to_string() } else { n.app_name.clone() })
            .width(Length::Fill)
            .into(),
        button::custom(icon::from_name("window-close-symbolic").size(12))
            .padding(4)
            .class(theme::Button::Icon)
            .on_press(Message::Dismiss(n.id))
            .into(),
    ])
    .align_y(Alignment::Center);

    let mut texts = column::with_capacity(2).spacing(2).width(Length::Fill);
    if !n.summary.is_empty() {
        texts = texts.push(text::heading(n.summary.clone()));
    }
    if !n.body.is_empty() {
        texts = texts.push(text::body(n.body.clone()));
    }
    let main = row::with_children(vec![icon(icon_for(n)).size(40).into(), texts.into()])
        .spacing(12)
        .align_y(Alignment::Start);

    let default_action = n.actions.iter().find(|a| a.key == "default").map(|a| a.key.clone());
    let main: Element<'_, Message> = match default_action {
        Some(key) => mouse_area(main).on_press(Message::Invoke(n.id, key)).into(),
        None => main.into(),
    };

    let mut body = column::with_capacity(4).spacing(8).push(header).push(main);
    let buttons: Vec<Element<'_, Message>> = n
        .actions
        .iter()
        .filter(|a| a.key != "default")
        .take(3)
        .map(|a| {
            button::standard(a.label.clone())
                .width(Length::Fill)
                .on_press(Message::Invoke(n.id, a.key.clone()))
                .into()
        })
        .collect();
    if !buttons.is_empty() {
        body = body.push(row::with_children(buttons).spacing(8));
    }
    body = body.push(widget::Space::new().height(2));

    container(body).padding([10, 14]).width(Length::Fill).class(card_style(n.urgency == Urgency::Critical)).into()
}

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,atrium_notifyd=info".into()),
        )
        .init();
    cosmic::app::run::<App>(
        Settings::default().antialiasing(true).client_decorations(true).no_main_window(true).exit_on_close(false),
        (),
    )
}
