// SPDX-License-Identifier: GPL-3.0-only
//! Lapin UX notification daemon: owns `org.freedesktop.Notifications`,
//! shows toasts stacked in the bottom-right corner above the taskbar, and
//! the notification centre (history + calendar) opened from the clock.
//!
//!   lapin-ux-notifyd          run the daemon
//!   lapin-ux-notifyd center   toggle the notification centre (Super+N)

mod center;
mod dbus;
mod quiet;

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use cosmic::app::{Core, Settings, Task};
use cosmic::iced::platform_specific::runtime::wayland::CornerRadius;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{IcedMargin, IcedOutput, SctkLayerSurfaceSettings};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, Layer, destroy_layer_surface, get_layer_surface,
};
use cosmic::iced::event::wayland::{self, LayerEvent};
use cosmic::iced::event::PlatformSpecific;
use cosmic::iced::keyboard::{self, Key, key::Named};
use cosmic::iced::{Alignment, Background, Border, Color, Length, Limits, Shadow, Subscription, Vector, window};
use cosmic::surface::action::{LiveSettings, app_layer_shell};
use cosmic::widget::{self, autosize, button, column, container, icon, mouse_area, row, text};
use cosmic::{Application, Element, theme};
use lapin_ux_core::notify::{CloseReason, Notification, Queue, Toast, Urgency};

const WIDTH: u32 = 364;

static AUTOSIZE: LazyLock<cosmic::widget::Id> = LazyLock::new(|| cosmic::widget::Id::new("lapin-ux-toasts"));

#[derive(Debug, Clone)]
pub enum Message {
    Dbus(dbus::Event),
    /// COSMIC's toolkit settings changed (icon theme).
    Toolkit(Box<cosmic::config::CosmicTk>),
    Tick,
    Hover(bool),
    Dismiss(u32),
    Invoke(u32, String),
    /// Typing in a notification's reply box, and sending it.
    ReplyInput(u32, String),
    ReplySend(u32),
    /// Click on a card in the notification centre.
    CenterOpen(u32),
    /// Remove from the notification centre (×).
    Forget(u32),
    /// Remove every notification of one app.
    ForgetApp(String),
    ClearAll,
    ToggleDnd,
    CloseCenter,
    /// Collapse or expand the calendar under the notifications.
    ToggleCalendar,
    Layer(LayerEvent, window::Id),
    Escape(window::Id),
    CalendarSelect(jiff::civil::Date),
    CalendarPrev,
    CalendarNext,
    Quiet(quiet::QuietMsg),
    /// Schedule or app rules changed in Settings.
    Prefs(Box<quiet::Prefs>),
    Noop,
}

pub struct App {
    core: Core,
    pub queue: Queue,
    conn: Option<zbus::Connection>,
    surface: Option<window::Id>,
    hovered: bool,
    /// The notification centre's layer surface, when open.
    pub center: Option<window::Id>,
    center_closed_at: Instant,
    /// Transparent surface under the centre so clicking elsewhere closes it
    /// (cosmic-comp keeps focus on exclusive-keyboard surfaces).
    catcher: Option<window::Id>,
    pub calendar: cosmic::widget::calendar::CalendarModel,
    pub calendar_collapsed: bool,
    settings: Option<cosmic::cosmic_config::Config>,
    pub quiet: quiet::Quiet,
    /// Ids for notices the daemon itself shows (focus session over…).
    local_id: u32,
    /// Replies being typed, by notification.
    pub replies: std::collections::HashMap<u32, String>,
}

impl App {
    /// Create or destroy the toast layer surface to match whether toasts exist.
    fn sync_surface(&mut self) -> Task<Message> {
        match (self.queue.visible().is_empty(), self.surface) {
            (false, None) => {
                let id = window::Id::unique();
                self.surface = Some(id);
                get_layer_surface(SctkLayerSurfaceSettings {
                    id,
                    layer: Layer::Overlay,
                    keyboard_interactivity: KeyboardInteractivity::OnDemand,
                    anchor: Anchor::BOTTOM | Anchor::RIGHT,
                    output: IcedOutput::Active,
                    namespace: "lapin-ux-notifications".into(),
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

    /// Tell the taskbar about the unread count and Do Not Disturb.
    fn publish(&self) -> Task<Message> {
        let Some(conn) = self.conn.clone() else { return Task::none() };
        let unread = self.queue.unread_count() as u32;
        let dnd = self.queue.do_not_disturb;
        Task::future(async move {
            dbus::publish_state(conn, unread, dnd).await;
            cosmic::Action::App(Message::Noop)
        })
    }

    /// A notification from the desktop itself.
    pub(crate) fn local_notice(&mut self, summary: &str, body: &str) -> Task<Message> {
        self.local_id += 1;
        let n = Notification {
            id: 0xF000_0000 + self.local_id,
            app_name: "Clock".into(),
            app_icon: "alarm-symbolic".into(),
            summary: summary.into(),
            body: body.into(),
            actions: Vec::new(),
            urgency: Urgency::Normal,
            expire_timeout: -1,
            transient: false,
            desktop_entry: None,
            received: std::time::SystemTime::now(),
            image: None,
            progress: None,
            reply: None,
            sound: Some("alarm-clock-elapsed".into()),
        };
        if let Some(sound) = &n.sound {
            play_sound(sound);
        }
        self.queue.push(n, Instant::now());
        Task::batch([self.sync_surface(), self.publish()])
    }

    fn open_center(&mut self) -> Task<Message> {
        if self.center.is_some() {
            return Task::none();
        }
        // The click that closed it (focus moved to the clock) also toggles.
        if self.center_closed_at.elapsed() < Duration::from_millis(250) {
            return Task::none();
        }
        let id = window::Id::unique();
        self.center = Some(id);
        self.queue.mark_all_read();
        self.calendar = cosmic::widget::calendar::CalendarModel::now();
        self.quiet.settings_open = false;
        let r = 8u32;
        let catcher = window::Id::unique();
        self.catcher = Some(catcher);
        Task::batch([
            cosmic::surface::surface_task(app_layer_shell(
                move |_: &App| LiveSettings {
                    // No compositor blur: the surface runs the whole screen
                    // height, so the blur showed as a frosted strip above and
                    // around the cards. The cards draw their own backgrounds.
                    blur: Some(false),
                    corners: Some(CornerRadius { top_left: r, top_right: r, bottom_left: r, bottom_right: r }),
                    ..Default::default()
                },
                move |_: &mut App| SctkLayerSurfaceSettings {
                    id,
                    layer: Layer::Overlay,
                    keyboard_interactivity: KeyboardInteractivity::Exclusive,
                    anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::RIGHT,
                    output: IcedOutput::Active,
                    namespace: "lapin-ux-notification-centre".into(),
                    margin: IcedMargin { top: 12, right: 12, bottom: 12, left: 0 },
                    size: Some((Some(center::WIDTH), None)),
                    exclusive_zone: 0,
                    size_limits: Limits::NONE.min_width(center::WIDTH as f32).max_width(center::WIDTH as f32),
                    ..Default::default()
                },
                None,
            )),
            get_layer_surface(SctkLayerSurfaceSettings {
                id: catcher,
                layer: Layer::Top,
                keyboard_interactivity: KeyboardInteractivity::None,
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                output: IcedOutput::Active,
                namespace: "lapin-ux-dismiss".into(),
                size: Some((None, None)),
                exclusive_zone: 0,
                ..Default::default()
            }),
            self.publish(),
            quiet::load_events(self.quiet.calendars.clone()),
        ])
    }

    fn close_center(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        if let Some(id) = self.catcher.take() {
            tasks.push(destroy_layer_surface(id));
        }
        if let Some(id) = self.center.take() {
            self.center_closed_at = Instant::now();
            tasks.push(destroy_layer_surface(id));
        }
        Task::batch(tasks)
    }

    fn invoke(&mut self, id: u32, key: String) -> Task<Message> {
        let now = Instant::now();
        self.queue.close(id, now);
        self.queue.forget(id);
        let mut tasks = vec![self.sync_surface(), self.publish()];
        if let Some(conn) = self.conn.clone() {
            tasks.push(Task::future(async move {
                dbus::emit_action(conn.clone(), id, key, None).await;
                // Invoking an action also closes the notification.
                dbus::emit_closed(conn, id, CloseReason::Dismissed).await;
                cosmic::Action::App(Message::Noop)
            }));
        }
        Task::batch(tasks)
    }
}

impl Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = lapin_ux_core::ids::NOTIFYD_APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _: ()) -> (Self, Task<Message>) {
        // The icon theme as configured now (Lapin UX's shell may have just set it).
        if let Ok(cfg) = cosmic::config::CosmicTk::config() {
            use cosmic::cosmic_config::CosmicConfigEntry;
            let tk = cosmic::config::CosmicTk::get_entry(&cfg).unwrap_or_else(|(_, tk)| tk);
            cosmic::icon_theme::set_default(tk.icon_theme);
        }
        let settings = cosmic::cosmic_config::Config::new(lapin_ux_core::ids::NOTIFYD_APP_ID, 1)
            .inspect_err(|e| tracing::warn!("notification settings unavailable: {e}"))
            .ok();
        let mut queue = Queue::default();
        use cosmic::cosmic_config::ConfigGet;
        let mut quiet = quiet::Quiet::load(settings.as_ref());
        let now_min = { use chrono::Timelike; let t = chrono::Local::now(); t.hour() * 60 + t.minute() };
        queue.do_not_disturb = quiet.dnd.update(now_min, std::time::SystemTime::now()).active;
        let sources = quiet.calendars.clone();
        (
            App {
                core,
                queue,
                conn: None,
                surface: None,
                hovered: false,
                center: None,
                center_closed_at: Instant::now() - Duration::from_secs(10),
                catcher: None,
                calendar: cosmic::widget::calendar::CalendarModel::now(),
                calendar_collapsed: settings.as_ref().and_then(|c| c.get::<bool>("calendar_collapsed").ok()).unwrap_or(false),
                settings,
                quiet,
                local_id: 0,
                replies: Default::default(),
            },
            quiet::load_events(sources),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        unreachable!("no main window")
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if self.center == Some(id) {
            return center::view(self);
        }
        if self.catcher == Some(id) {
            return mouse_area(widget::Space::new().width(Length::Fill).height(Length::Fill))
                .on_press(Message::CloseCenter)
                .on_right_press(Message::CloseCenter)
                .into();
        }
        let cards: Vec<Element<'_, Message>> = self.queue.visible().iter().map(|t| toast(self, t)).collect();
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
            // Follow icon theme changes by watching the file directly (works
            // without COSMIC's settings daemon too).
            cosmic::cosmic_config::config_subscription::<_, cosmic::config::CosmicTk>(
                std::any::TypeId::of::<cosmic::config::CosmicTk>(),
                std::borrow::Cow::Borrowed("com.system76.CosmicTk"),
                <cosmic::config::CosmicTk as cosmic::cosmic_config::CosmicConfigEntry>::VERSION,
            )
            .map(|u| Message::Toolkit(Box::new(u.config))),
            cosmic::cosmic_config::config_subscription::<_, quiet::Prefs>(
                std::any::TypeId::of::<quiet::Prefs>(),
                std::borrow::Cow::Borrowed(lapin_ux_core::ids::NOTIFYD_APP_ID),
                1,
            )
            .map(|u| Message::Prefs(Box::new(u.config))),
            cosmic::iced::event::listen_raw(|e, _, window| match e {
                // The pointer can leave the surface without crossing a widget
                // edge (e.g. the toast under it closes); treat that as un-hover.
                cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::CursorLeft) => Some(Message::Hover(false)),
                cosmic::iced::Event::PlatformSpecific(PlatformSpecific::Wayland(wayland::Event::Layer(e, _, id))) => {
                    Some(Message::Layer(e, id))
                }
                cosmic::iced::Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Escape), .. }) => {
                    Some(Message::Escape(window))
                }
                _ => None,
            }),
        ];
        subs.push(cosmic::iced::time::every(Duration::from_secs(30)).map(|_| Message::Quiet(quiet::QuietMsg::Minute)));
        if self.surface.is_some() || self.center.is_some() {
            subs.push(cosmic::iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick));
        }
        Subscription::batch(subs)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        lapin_ux_core::crash::note(&message);
        let now = Instant::now();
        match message {
            Message::Prefs(p) => return self.prefs_changed(*p),
            Message::Toolkit(tk) => {
                if cosmic::icon_theme::default() != tk.icon_theme {
                    cosmic::icon_theme::set_default(tk.icon_theme.clone());
                }
            }
            Message::Dbus(ev) => match ev {
                dbus::Event::Ready(conn) => {
                    self.conn = Some(conn);
                    return self.publish();
                }
                dbus::Event::Failed(e) => {
                    tracing::error!("cannot own {}: {e} (is another notification daemon running?)", dbus::NAME);
                }
                dbus::Event::Notify(n) => {
                    if !n.app_name.is_empty() && !self.quiet.seen_apps.contains(&n.app_name) {
                        self.quiet.seen_apps.push(n.app_name.clone());
                        // Remembered for Settings > Notifications' app list.
                        if let Some(cfg) = &self.settings {
                            use cosmic::cosmic_config::ConfigSet;
                            let _ = cfg.set("seen_apps", &self.quiet.seen_apps);
                        }
                    }
                    match lapin_ux_core::dnd::delivery(&self.quiet.rules, &n.app_name) {
                        lapin_ux_core::dnd::Delivery::Drop => return Task::none(),
                        lapin_ux_core::dnd::Delivery::Silent if n.urgency != Urgency::Critical => {
                            self.queue.push_quiet(*n);
                            return self.publish();
                        }
                        _ => {}
                    }
                    if self.center.is_some() {
                        // Shown directly in the open centre; no toast.
                        self.queue.push_silent(*n);
                        return Task::none();
                    }
                    if let Some(sound) = &n.sound {
                        play_sound(sound);
                    }
                    self.queue.push(*n, now);
                    return Task::batch([self.sync_surface(), self.publish()]);
                }
                dbus::Event::Close(id) => {
                    let existed = self.queue.close(id, now);
                    self.queue.forget(id);
                    let signal = if existed { self.signal_closed([id], CloseReason::CloseCall) } else { Task::none() };
                    return Task::batch([signal, self.sync_surface(), self.publish()]);
                }
                dbus::Event::ToggleCenter => {
                    if self.center.is_some() {
                        return self.close_center();
                    }
                    return self.open_center();
                }
                dbus::Event::CloseCenter => return self.close_center(),
                dbus::Event::SetDoNotDisturb(on) => return self.set_dnd(on),
                dbus::Event::ClearAll => return self.update(Message::ClearAll),
            },
            Message::Tick => {
                if self.hovered || self.replies.values().any(|r| !r.is_empty()) {
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
            Message::Invoke(id, key) => return self.invoke(id, key),
            Message::ReplyInput(id, text) => {
                self.replies.insert(id, text);
            }
            Message::ReplySend(id) => {
                let text = self.replies.remove(&id).unwrap_or_default();
                if text.trim().is_empty() {
                    return Task::none();
                }
                self.queue.close(id, now);
                self.queue.forget(id);
                let mut tasks = vec![self.sync_surface(), self.publish()];
                if let Some(conn) = self.conn.clone() {
                    tasks.push(Task::future(async move {
                        dbus::emit_reply(conn.clone(), id, text).await;
                        dbus::emit_closed(conn, id, CloseReason::Dismissed).await;
                        cosmic::Action::App(Message::Noop)
                    }));
                }
                return Task::batch(tasks);
            }
            Message::CenterOpen(id) => {
                let default = self
                    .queue
                    .history()
                    .find(|n| n.id == id)
                    .and_then(|n| n.actions.iter().find(|a| a.key == "default"))
                    .map(|a| a.key.clone());
                if let Some(key) = default {
                    let close = self.close_center();
                    return Task::batch([close, self.invoke(id, key)]);
                }
            }
            Message::Forget(id) => {
                self.queue.close(id, now);
                self.queue.forget(id);
                let signal = self.signal_closed([id], CloseReason::Dismissed);
                return Task::batch([signal, self.sync_surface(), self.publish()]);
            }
            Message::ForgetApp(app) => {
                let ids: Vec<u32> = self.queue.history().filter(|n| n.app_name == app).map(|n| n.id).collect();
                for id in &ids {
                    self.queue.close(*id, now);
                    self.queue.forget(*id);
                }
                let signal = self.signal_closed(ids, CloseReason::Dismissed);
                return Task::batch([signal, self.sync_surface(), self.publish()]);
            }
            Message::ClearAll => {
                let ids = self.queue.clear_history();
                for id in &ids {
                    self.queue.close(*id, now);
                }
                let signal = self.signal_closed(ids, CloseReason::Dismissed);
                return Task::batch([signal, self.sync_surface(), self.publish()]);
            }
            Message::ToggleDnd => {
                let on = !self.queue.do_not_disturb;
                return self.set_dnd(on);
            }
            Message::CloseCenter => return self.close_center(),
            Message::ToggleCalendar => {
                self.calendar_collapsed = !self.calendar_collapsed;
                if let Some(cfg) = &self.settings
                    && let Err(e) = cosmic::cosmic_config::ConfigSet::set(cfg, "calendar_collapsed", self.calendar_collapsed)
                {
                    tracing::warn!("saving calendar state: {e}");
                }
            }
            Message::Layer(LayerEvent::Unfocused, id) if self.center == Some(id) => return self.close_center(),
            Message::Layer(..) => {}
            Message::Escape(id) if self.center == Some(id) => return self.close_center(),
            Message::Escape(_) => {}
            Message::CalendarSelect(d) => self.calendar.set_selected_visible(d),
            Message::CalendarPrev => self.calendar.show_prev_month(),
            Message::CalendarNext => self.calendar.show_next_month(),
            Message::Quiet(m) => return self.update_quiet(m),
            Message::Noop => {}
        }
        Task::none()
    }
}

pub fn icon_for(n: &Notification) -> icon::Handle {
    if let Some(img) = &n.image {
        return icon::from_raster_pixels(img.width, img.height, img.rgba.as_ref().clone());
    }
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

/// Progress bar and reply box, shared by toasts and the notification centre.
pub fn extras<'a>(app: &'a App, n: &'a Notification) -> Vec<Element<'a, Message>> {
    let mut out = Vec::new();
    if let Some(p) = n.progress {
        out.push(
            row::with_children(vec![
                cosmic::widget::progress_bar::determinate_linear(p as f32 / 100.0).girth(4).width(Length::Fill).into(),
                text::caption(format!("{p}%")).into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        );
    }
    if let Some(r) = &n.reply {
        let draft = app.replies.get(&n.id).map(String::as_str).unwrap_or("");
        let id = n.id;
        out.push(
            row::with_children(vec![
                cosmic::widget::text_input::text_input(r.placeholder.clone(), draft)
                    .on_input(move |s| Message::ReplyInput(id, s))
                    .on_submit(move |_| Message::ReplySend(id))
                    .width(Length::Fill)
                    .into(),
                lapin_ux_ui::button::suggested(r.submit.clone()).on_press_maybe((!draft.trim().is_empty()).then_some(Message::ReplySend(id))).into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        );
    }
    out
}

fn toast<'a>(app: &'a App, t: &'a Toast) -> Element<'a, Message> {
    let n = &t.notification;
    let header = row::with_children(vec![
        text::caption(if n.app_name.is_empty() { "Notification".to_string() } else { n.app_name.clone() })
            .width(Length::Fill)
            .into(),
        button::custom(icon::from_name("window-close-symbolic").size(12))
            .padding(4)
            .class(lapin_ux_ui::icon())
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

    let mut body = column::with_capacity(6).spacing(8).push(header).push(main);
    for e in extras(app, n) {
        body = body.push(e);
    }
    let buttons: Vec<Element<'_, Message>> = n
        .actions
        .iter()
        .filter(|a| a.key != "default" && a.key != lapin_ux_core::notify::INLINE_REPLY)
        .take(3)
        .map(|a| {
            lapin_ux_ui::button::standard(a.label.clone())
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

/// `lapin-ux-notifyd center`: ask the running daemon to toggle the centre.
fn toggle_center_and_exit() -> ! {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
    let res: zbus::Result<()> = rt.block_on(async {
        let conn = zbus::Connection::session().await?;
        conn.call_method(Some(dbus::CONTROL_NAME), dbus::CONTROL_PATH, Some(dbus::CONTROL_NAME), "ToggleCenter", &())
            .await?;
        Ok(())
    });
    if let Err(e) = res {
        eprintln!("lapin-ux-notifyd: could not reach the running notification daemon: {e}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

fn main() -> cosmic::iced::Result {
    lapin_ux_core::crash::install("lapin-ux-notifyd");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,lapin_ux_notifyd=info".into()),
        )
        .init();
    match std::env::args().nth(1).as_deref() {
        Some("center" | "centre") => toggle_center_and_exit(),
        Some("-h" | "--help") => {
            println!("usage: lapin-ux-notifyd [center]");
            return Ok(());
        }
        _ => {}
    }
    cosmic::app::run::<App>(
        Settings::default().antialiasing(true).client_decorations(true).no_main_window(true).exit_on_close(false),
        (),
    )
}

/// A theme sound name, or a file an app named with `sound-file`.
fn play_sound(sound: &str) {
    if sound.starts_with('/') {
        let file = sound.to_string();
        std::thread::spawn(move || {
            let _ = std::process::Command::new("pw-play").arg(file).status();
        });
    } else {
        lapin_ux_core::sound::play(sound);
    }
}
