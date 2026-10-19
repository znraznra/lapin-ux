// SPDX-License-Identifier: GPL-3.0-only
//! Features that lean on Lapin UX's compositor additions (lapin_shell v2)
//! or on system services: desktop peek, night light and its schedule,
//! touchpad swipes, and screen auto-rotation with rotation lock.

use std::time::Duration;

use chrono::Timelike;
use cosmic::Application;
use cosmic::app::Task;
use cosmic::iced::{Subscription, stream};
use futures::{SinkExt, StreamExt};
use lapin_ux_core::{display, nightlight};

use crate::app::{Message, Shell};
use crate::wayland::WaylandRequest;

/// How long the pointer rests on the taskbar corner before peeking.
const PEEK_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
pub struct Extras {
    /// lapin_shell version the compositor offers (0: not Lapin UX's).
    pub version: u32,
    /// Night light value last sent (kelvin, 0 off).
    night_sent: Option<u32>,
    /// Minute of the day at the last schedule check.
    last_minute: Option<u16>,
    /// Pointer on the show-desktop corner (a counter, so stale timers lose).
    peek_hover: u64,
    pub peeking: bool,
    /// The pointer is on the taskbar's far corner.
    pub in_corner: bool,
    /// The machine has an accelerometer (iio-sensor-proxy).
    pub can_rotate: bool,
    /// Last orientation reported, and the one applied to the screen.
    orientation: Option<String>,
    applied: Option<String>,
    /// Background update check (`lapin-ux-update --check`) and when next.
    update_check: Option<std::process::Child>,
    next_check: Option<std::time::Instant>,
}

#[derive(Debug, Clone)]
pub enum ExtrasMsg {
    /// Pointer entered / left the show-desktop corner.
    CornerHover(bool),
    PeekNow(u64),
    /// Touchpad swipe: fingers, direction (0 up, 1 down, 2 left, 3 right).
    Gesture(u32, u32),
    NightLight(bool),
    RotationLock(bool),
    Sensor(Sensor),
}

#[derive(Debug, Clone)]
pub enum Sensor {
    Available(bool),
    Orientation(String),
}

impl Shell {
    pub(crate) fn update_extras(&mut self, msg: ExtrasMsg) -> Task<Message> {
        tracing::debug!(?msg, version = self.extras.version, "extras");
        match msg {
            ExtrasMsg::CornerHover(on) => {
                self.extras.peek_hover += 1;
                if !on {
                    return self.set_peek(false);
                }
                if self.config.no_desktop_peek || self.extras.version < 2 {
                    return Task::none();
                }
                let n = self.extras.peek_hover;
                return Task::future(async move {
                    tokio::time::sleep(PEEK_DELAY).await;
                    cosmic::Action::App(Message::Extras(ExtrasMsg::PeekNow(n)))
                });
            }
            ExtrasMsg::PeekNow(n) => {
                if n == self.extras.peek_hover {
                    return self.set_peek(true);
                }
            }
            ExtrasMsg::Gesture(fingers, dir) => return self.gesture(fingers, dir),
            ExtrasMsg::NightLight(on) => {
                self.config.night_light = on;
                self.persist_config();
                self.sync_night_light();
            }
            ExtrasMsg::RotationLock(on) => {
                self.config.rotation_lock = on;
                self.persist_config();
                if !on {
                    return self.apply_rotation();
                }
            }
            ExtrasMsg::Sensor(Sensor::Available(on)) => self.extras.can_rotate = on,
            ExtrasMsg::Sensor(Sensor::Orientation(o)) => {
                self.extras.orientation = Some(o);
                if !self.config.rotation_lock {
                    return self.apply_rotation();
                }
            }
        }
        Task::none()
    }

    pub(crate) fn set_peek(&mut self, on: bool) -> Task<Message> {
        if self.extras.peeking != on {
            self.extras.peeking = on;
            self.request(WaylandRequest::Peek(on));
        }
        Task::none()
    }

    /// The compositor connected (or reconnected): resend what it should know.
    pub(crate) fn extras_connected(&mut self, version: u32) {
        self.extras.version = version;
        self.extras.night_sent = None;
        self.extras.peeking = false;
        self.sync_night_light();
    }

    /// Send the night light state if it changed.
    pub(crate) fn sync_night_light(&mut self) {
        if self.extras.version < 2 {
            return;
        }
        let want = if self.config.night_light { nightlight::kelvin(self.config.night_light_kelvin) } else { 0 };
        if self.extras.night_sent != Some(want) {
            self.extras.night_sent = Some(want);
            self.request(WaylandRequest::NightLight(want));
        }
    }

    /// Look for updates 10 minutes after sign-in, then every 6 hours.
    fn update_tick(&mut self) {
        let now = std::time::Instant::now();
        let next = *self.extras.next_check.get_or_insert(now + Duration::from_secs(600));
        if now < next {
            return;
        }
        self.extras.next_check = Some(now + Duration::from_secs(6 * 3600));
        if let Some(c) = &mut self.extras.update_check
            && matches!(c.try_wait(), Ok(None))
        {
            return; // the last one is still waiting on its notification
        }
        self.extras.update_check = std::process::Command::new("lapin-ux-update").arg("--check").spawn().ok();
    }

    /// Once a second: follow the night light schedule.
    pub(crate) fn extras_tick(&mut self) {
        self.update_tick();
        let now = chrono::Local::now();
        let minute = (now.hour() * 60 + now.minute()) as u16;
        let prev = self.extras.last_minute.replace(minute);
        let Some((from, to)) = self.config.night_light_schedule else { return };
        let change = match prev {
            // First check after start: take the scheduled state.
            None => Some(nightlight::scheduled_on(minute, from, to)),
            Some(p) if p != minute => nightlight::crossed(p, minute, from, to),
            Some(_) => None,
        };
        if let Some(on) = change
            && on != self.config.night_light
        {
            self.config.night_light = on;
            self.persist_config();
        }
        self.sync_night_light();
    }

    fn gesture(&mut self, fingers: u32, dir: u32) -> Task<Message> {
        use crate::previews::PreviewMsg;
        use crate::taskview::TaskViewMsg;
        let taskview_open = matches!(self.panel.as_ref().map(|p| &p.kind), Some(crate::panels::PanelKind::TaskView(_)));
        match (fingers, dir) {
            // Up: Task view. Down: close it, or show the desktop.
            (3 | 4, 0) if !taskview_open => self.update(Message::TaskView(TaskViewMsg::Toggle)),
            (3 | 4, 1) if taskview_open => self.update(Message::TaskView(TaskViewMsg::Toggle)),
            (3 | 4, 1) => self.update(Message::ShowDesktop),
            // Left / right: switch to the last app (and on, while the
            // switcher shows).
            (3, 2 | 3) => {
                let open = self.update(Message::Preview(PreviewMsg::AltTab(dir == 2)));
                Task::batch([open, self.alt_tab_focused()])
            }
            _ => Task::none(),
        }
    }

    /// Turn the built-in screen to match the accelerometer.
    fn apply_rotation(&mut self) -> Task<Message> {
        let Some(o) = self.extras.orientation.clone() else { return Task::none() };
        if self.extras.applied.as_deref() == Some(o.as_str()) {
            return Task::none();
        }
        let Some(transform) = display::transform_for_orientation(&o) else { return Task::none() };
        self.extras.applied = Some(o);
        Task::future(async move {
            let out = tokio::process::Command::new("cosmic-randr").args(["list", "--kdl"]).output().await;
            if let Ok(out) = out {
                let heads = display::parse_kdl(&String::from_utf8_lossy(&out.stdout));
                if let Some(head) = heads.iter().find(|h| h.internal() && h.enabled) {
                    let args = display::rotate_command(head, transform);
                    if let Err(e) = tokio::process::Command::new("cosmic-randr").args(&args).status().await {
                        tracing::warn!("rotate: {e}");
                    }
                }
            }
            cosmic::Action::App(Message::Noop)
        })
    }
}

/// Accelerometer orientation from iio-sensor-proxy (system bus), while the
/// shell runs. Sends `Available(false)` and stops when there's no sensor.
pub fn sensor_subscription() -> Subscription<Sensor> {
    Subscription::run_with("lapin-ux-sensor", |_| {
        stream::channel(4, |mut out: futures::channel::mpsc::Sender<Sensor>| async move {
            let found = async {
                let conn = zbus::Connection::system().await.ok()?;
                let proxy = zbus::proxy::Builder::<zbus::Proxy>::new(&conn)
                    .destination("net.hadess.SensorProxy")
                    .ok()?
                    .path("/net/hadess/SensorProxy")
                    .ok()?
                    .interface("net.hadess.SensorProxy")
                    .ok()?
                    .build()
                    .await
                    .ok()?;
                let has: bool = proxy.get_property("HasAccelerometer").await.ok()?;
                has.then_some((conn, proxy))
            }
            .await;
            let Some((_conn, proxy)) = found else {
                let _ = out.send(Sensor::Available(false)).await;
                return futures::future::pending().await;
            };
            let _ = out.send(Sensor::Available(true)).await;
            if let Err(e) = proxy.call_method("ClaimAccelerometer", &()).await {
                tracing::warn!("accelerometer: {e}");
            }
            let mut changes = proxy.receive_property_changed::<String>("AccelerometerOrientation").await;
            if let Ok(o) = proxy.get_property::<String>("AccelerometerOrientation").await {
                let _ = out.send(Sensor::Orientation(o)).await;
            }
            while let Some(c) = changes.next().await {
                if let Ok(o) = c.get().await {
                    let _ = out.send(Sensor::Orientation(o)).await;
                }
            }
            futures::future::pending().await
        })
    })
}
