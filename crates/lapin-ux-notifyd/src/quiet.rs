// SPDX-License-Identifier: GPL-3.0-only
//! Do not disturb (by hand, on a schedule, or during a focus session),
//! per-app notification settings, and the calendar agenda — the state,
//! its saving, and the notification centre's settings page.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use chrono::Timelike;
use cosmic::app::Task;
use cosmic::cosmic_config::{ConfigGet, ConfigSet};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text, text_input, toggler};
use cosmic::{Element, theme};
use lapin_ux_core::dnd::{self, AppRule, Dnd, Schedule};
use lapin_ux_core::ics::{self, Event};

use crate::{App, Message};

#[derive(Debug, Clone)]
pub enum QuietMsg {
    /// Every half minute: schedule and focus timers.
    Minute,
    OpenSettings(bool),
    Schedule(bool),
    ScheduleStart(i32),
    ScheduleEnd(i32),
    FocusLength(i32),
    StartFocus,
    StopFocus,
    AppRule(String, bool, bool),
    CalendarInput(String),
    AddCalendar,
    RemoveCalendar(String),
    EventsLoaded(Vec<Event>),
}

#[derive(Default)]
pub struct Quiet {
    pub dnd: Dnd,
    pub rules: Vec<AppRule>,
    pub calendars: Vec<String>,
    pub events: Vec<Event>,
    pub settings_open: bool,
    /// Focus session length, minutes.
    pub focus_minutes: u32,
    pub calendar_input: String,
    /// Apps seen this session (for the settings list).
    pub seen_apps: Vec<String>,
}

fn minute_now() -> u32 {
    let t = chrono::Local::now();
    t.hour() * 60 + t.minute()
}

impl Quiet {
    pub fn load(cfg: Option<&cosmic::cosmic_config::Config>) -> Quiet {
        let mut q = Quiet { focus_minutes: 30, ..Default::default() };
        if let Some(c) = cfg {
            q.dnd.manual = c.get::<bool>("do_not_disturb").unwrap_or(false);
            q.dnd.schedule = c.get::<Schedule>("dnd_schedule").unwrap_or_default();
            q.rules = c.get::<Vec<AppRule>>("app_rules").unwrap_or_default();
            q.calendars = c.get::<Vec<String>>("calendar_sources").unwrap_or_default();
            q.focus_minutes = c.get::<u32>("focus_minutes").unwrap_or(30).clamp(5, 240);
        }
        q
    }
}

fn save<T: serde::Serialize>(cfg: Option<&cosmic::cosmic_config::Config>, key: &str, v: &T) {
    if let Some(c) = cfg
        && let Err(e) = c.set(key, v)
    {
        tracing::warn!("saving {key}: {e}");
    }
}

/// Calendars on this computer: Evolution's local calendars plus the user's list.
fn calendar_files(sources: &[String]) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(data.join("evolution/calendar"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("calendar.ics"))
        .filter(|p| p.exists())
        .collect();
    for s in sources {
        if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("webcal://") {
            files.push(cache_path(s));
        } else {
            let p = s.strip_prefix("file://").unwrap_or(s);
            files.push(PathBuf::from(p.replacen('~', &home.to_string_lossy(), 1)));
        }
    }
    files
}

fn cache_path(url: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".cache"));
    let name: String = url.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(120).collect();
    cache.join("lapin-ux/calendars").join(format!("{name}.ics"))
}

/// Download subscribed calendars that are older than half an hour, then
/// read every calendar.
pub fn load_events(sources: Vec<String>) -> Task<Message> {
    Task::future(async move {
        for url in sources.iter().filter(|s| s.contains("://") && !s.starts_with("file://")) {
            let path = cache_path(url);
            let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|e| e < Duration::from_secs(1800)));
            if fresh {
                continue;
            }
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
            let tmp = path.with_extension("part");
            let url = url.replacen("webcal://", "https://", 1);
            let ok = tokio::process::Command::new("curl")
                .args(["-sfL", "--max-time", "20", "-o"])
                .arg(&tmp)
                .arg(&url)
                .status()
                .await
                .is_ok_and(|s| s.success());
            if ok {
                let _ = std::fs::rename(&tmp, &path);
            } else {
                tracing::warn!("couldn't download calendar {url}");
            }
        }
        let events = tokio::task::spawn_blocking(move || {
            calendar_files(&sources).iter().filter_map(|p| std::fs::read_to_string(p).ok()).flat_map(|t| ics::parse(&t)).collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        cosmic::Action::App(Message::Quiet(QuietMsg::EventsLoaded(events)))
    })
}

impl App {
    /// Recompute Do not disturb; tell the taskbar if it changed.
    pub(crate) fn refresh_dnd(&mut self) -> Task<Message> {
        let st = self.quiet.dnd.update(minute_now(), SystemTime::now());
        let mut tasks = Vec::new();
        if st.focus_ended {
            tasks.push(self.local_notice("Focus session complete", "Nice work! Notifications are back on."));
        }
        if st.active != self.queue.do_not_disturb {
            self.queue.do_not_disturb = st.active;
            tasks.push(self.publish());
        }
        Task::batch(tasks)
    }

    /// The Do not disturb switch (here or in quick settings).
    pub(crate) fn set_dnd(&mut self, on: bool) -> Task<Message> {
        let minute = minute_now();
        if self.quiet.dnd.update(minute, SystemTime::now()).active != on {
            self.quiet.dnd.toggle(minute, SystemTime::now());
        }
        save(self.settings.as_ref(), "do_not_disturb", &self.quiet.dnd.manual);
        let t = self.refresh_dnd();
        Task::batch([t, self.publish()])
    }

    pub(crate) fn update_quiet(&mut self, msg: QuietMsg) -> Task<Message> {
        let cfg = self.settings.clone();
        match msg {
            QuietMsg::Minute => return self.refresh_dnd(),
            QuietMsg::OpenSettings(on) => self.quiet.settings_open = on,
            QuietMsg::Schedule(on) => {
                self.quiet.dnd.schedule.enabled = on;
                save(cfg.as_ref(), "dnd_schedule", &self.quiet.dnd.schedule);
                return self.refresh_dnd();
            }
            QuietMsg::ScheduleStart(d) | QuietMsg::ScheduleEnd(d) => {
                let s = &mut self.quiet.dnd.schedule;
                let field = if matches!(msg, QuietMsg::ScheduleStart(_)) { &mut s.start } else { &mut s.end };
                *field = ((*field as i32 + d).rem_euclid(24 * 60)) as u32;
                save(cfg.as_ref(), "dnd_schedule", &self.quiet.dnd.schedule);
                return self.refresh_dnd();
            }
            QuietMsg::FocusLength(d) => {
                self.quiet.focus_minutes = (self.quiet.focus_minutes as i32 + d).clamp(5, 240) as u32;
                save(cfg.as_ref(), "focus_minutes", &self.quiet.focus_minutes);
            }
            QuietMsg::StartFocus => {
                self.quiet.dnd.start_focus(Duration::from_secs(self.quiet.focus_minutes as u64 * 60), SystemTime::now());
                return self.refresh_dnd();
            }
            QuietMsg::StopFocus => {
                self.quiet.dnd.focus_until = None;
                return self.refresh_dnd();
            }
            QuietMsg::AppRule(app, enabled, banners) => {
                dnd::set_rule(&mut self.quiet.rules, &app, enabled, banners);
                save(cfg.as_ref(), "app_rules", &self.quiet.rules);
            }
            QuietMsg::CalendarInput(s) => self.quiet.calendar_input = s,
            QuietMsg::AddCalendar => {
                let s = self.quiet.calendar_input.trim().to_string();
                if !s.is_empty() && !self.quiet.calendars.contains(&s) {
                    self.quiet.calendars.push(s);
                    save(cfg.as_ref(), "calendar_sources", &self.quiet.calendars);
                }
                self.quiet.calendar_input.clear();
                return load_events(self.quiet.calendars.clone());
            }
            QuietMsg::RemoveCalendar(s) => {
                self.quiet.calendars.retain(|c| c != &s);
                save(cfg.as_ref(), "calendar_sources", &self.quiet.calendars);
                return load_events(self.quiet.calendars.clone());
            }
            QuietMsg::EventsLoaded(ev) => self.quiet.events = ev,
        }
        Task::none()
    }
}

fn dim<'a>(s: impl Into<String>) -> Element<'a, Message> {
    text::caption(s.into())
        .class(theme::Text::Custom(|t| {
            let mut c: cosmic::iced::Color = t.cosmic().on_bg_color().into();
            c.a = 0.7;
            cosmic::iced::widget::text::Style { color: Some(c), ..Default::default() }
        }))
        .into()
}

fn stepper<'a>(value: String, minus: Message, plus: Message) -> Element<'a, Message> {
    row::with_children(vec![
        button::icon(icon::from_name("list-remove-symbolic").size(14)).on_press(minus).into(),
        container(text::body(value)).width(80).align_x(Alignment::Center).into(),
        button::icon(icon::from_name("list-add-symbolic").size(14)).on_press(plus).into(),
    ])
    .align_y(Alignment::Center)
    .into()
}

/// The agenda under the calendar: events on the selected day.
pub fn agenda(app: &App) -> Element<'_, Message> {
    let d = app.calendar.selected;
    let Some(day) = chrono::NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32) else {
        return widget::Space::new().into();
    };
    let today = chrono::Local::now().date_naive();
    let title = if day == today { "Today".to_string() } else { day.format("%A, %B %-d").to_string() };
    let items = ics::agenda(&app.quiet.events, day);
    let n = items.len();
    let mut list = column::with_capacity(n).spacing(6).width(Length::Fill);
    if items.is_empty() {
        list = list.push(dim("No events"));
    }
    for o in items {
        let time = match (o.start, o.end) {
            (Some(s), Some(e)) => format!("{} – {}", s.format("%-I:%M %p"), e.format("%-I:%M %p")),
            (Some(s), None) => s.format("%-I:%M %p").to_string(),
            _ => "All day".into(),
        };
        let mut info = column::with_capacity(3).push(text::body(o.summary)).push(dim(time));
        if !o.location.is_empty() {
            info = info.push(dim(o.location));
        }
        list = list.push(
            row::with_children(vec![
                container(widget::Space::new().width(3).height(32)).class(theme::Container::custom(|t| cosmic::widget::container::Style {
                    background: Some(cosmic::iced::Background::Color(t.cosmic().accent_color().into())),
                    border: cosmic::iced::Border { radius: 2.0.into(), ..Default::default() },
                    ..Default::default()
                }))
                .into(),
                info.into(),
            ])
            .spacing(10),
        );
    }
    // Two events fit; more scroll, so the notifications keep their room.
    let list: Element<'_, Message> = if n > 2 { scrollable(list).width(Length::Fill).height(Length::Fixed(96.0)).into() } else { list.into() };
    column::with_children(vec![text::heading(title).into(), list]).spacing(6).into()
}

/// Focus session controls at the bottom of the calendar card.
pub fn focus_row(app: &App) -> Element<'_, Message> {
    match app.quiet.dnd.focus_left(SystemTime::now()) {
        Some(left) => {
            let s = left.as_secs();
            row::with_children(vec![
                icon::from_name("alarm-symbolic").size(16).into(),
                text::body(format!("Focus · {}:{:02} left", s / 60, s % 60)).width(Length::Fill).into(),
                button::standard("End session").on_press(Message::Quiet(QuietMsg::StopFocus)).into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
        None => row::with_children(vec![
            stepper(
                format!("{} mins", app.quiet.focus_minutes),
                Message::Quiet(QuietMsg::FocusLength(-5)),
                Message::Quiet(QuietMsg::FocusLength(5)),
            ),
            widget::Space::new().width(Length::Fill).into(),
            button::standard("Focus").leading_icon(icon::from_name("alarm-symbolic").size(16)).on_press(Message::Quiet(QuietMsg::StartFocus)).into(),
        ])
        .align_y(Alignment::Center)
        .into(),
    }
}

/// Notification settings, shown in place of the notification list.
pub fn settings(app: &App) -> Element<'_, Message> {
    let q = &app.quiet;
    let s = q.dnd.schedule;
    let header = row::with_children(vec![
        button::icon(icon::from_name("go-previous-symbolic").size(16)).on_press(Message::Quiet(QuietMsg::OpenSettings(false))).into(),
        text::title4("Notification settings").into(),
    ])
    .spacing(8)
    .align_y(Alignment::Center);

    let mut sched = column::with_capacity(3).spacing(8).push(
        row::with_children(vec![
            text::body("Turn on Do not disturb automatically").width(Length::Fill).into(),
            toggler(s.enabled).on_toggle(|on| Message::Quiet(QuietMsg::Schedule(on))).into(),
        ])
        .align_y(Alignment::Center),
    );
    if s.enabled {
        for (label, v, f) in [("From", s.start, QuietMsg::ScheduleStart as fn(i32) -> QuietMsg), ("To", s.end, QuietMsg::ScheduleEnd)] {
            sched = sched.push(
                row::with_children(vec![
                    text::body(label).width(Length::Fill).into(),
                    stepper(dnd::format_minutes(v), Message::Quiet(f(-30)), Message::Quiet(f(30))),
                ])
                .align_y(Alignment::Center),
            );
        }
    }

    // Apps: ones with rules plus ones seen this session.
    let mut apps: Vec<String> = q.rules.iter().map(|r| r.app.clone()).collect();
    for a in q.seen_apps.iter().chain(app.queue.history().map(|n| &n.app_name)) {
        if !a.is_empty() && !apps.contains(a) {
            apps.push(a.clone());
        }
    }
    apps.sort_by_key(|a| a.to_lowercase());
    let mut list = column::with_capacity(apps.len() + 1).spacing(10).push(text::heading("Notifications from apps"));
    if apps.is_empty() {
        list = list.push(dim("Apps show up here after they send a notification."));
    }
    for a in apps {
        let rule = q.rules.iter().find(|r| r.app == a);
        let (enabled, banners) = rule.map_or((true, true), |r| (r.enabled, r.banners));
        let (a1, a2) = (a.clone(), a.clone());
        list = list.push(
            column::with_children(vec![
                row::with_children(vec![
                    text::body(a.clone()).width(Length::Fill).into(),
                    toggler(enabled).on_toggle(move |on| Message::Quiet(QuietMsg::AppRule(a1.clone(), on, banners))).into(),
                ])
                .align_y(Alignment::Center)
                .into(),
                row::with_children(vec![
                    dim("Show notification banners"),
                    widget::Space::new().width(Length::Fill).into(),
                    toggler(banners && enabled)
                        .on_toggle_maybe(enabled.then(|| move |on| Message::Quiet(QuietMsg::AppRule(a2.clone(), true, on))))
                        .into(),
                ])
                .align_y(Alignment::Center)
                .into(),
            ])
            .spacing(4),
        );
    }

    let mut cals = column::with_capacity(q.calendars.len() + 3).spacing(8).push(text::heading("Calendars"));
    cals = cals.push(dim("Local Evolution calendars are added automatically. Add an .ics file or a calendar link:"));
    for c in &q.calendars {
        cals = cals.push(
            row::with_children(vec![
                text::caption(c.clone()).width(Length::Fill).into(),
                button::icon(icon::from_name("user-trash-symbolic").size(14)).on_press(Message::Quiet(QuietMsg::RemoveCalendar(c.clone()))).into(),
            ])
            .align_y(Alignment::Center),
        );
    }
    cals = cals.push(
        row::with_children(vec![
            text_input::text_input("https://…/calendar.ics", &q.calendar_input)
                .on_input(|s| Message::Quiet(QuietMsg::CalendarInput(s)))
                .on_submit(|_| Message::Quiet(QuietMsg::AddCalendar))
                .width(Length::Fill)
                .into(),
            button::standard("Add").on_press(Message::Quiet(QuietMsg::AddCalendar)).into(),
        ])
        .spacing(8)
        .align_y(Alignment::Center),
    );

    column::with_children(vec![
        header.into(),
        scrollable(column::with_children(vec![sched.into(), list.into(), cals.into()]).spacing(20).padding([0, 8, 0, 0])).height(Length::Fill).into(),
    ])
    .spacing(12)
    .into()
}
