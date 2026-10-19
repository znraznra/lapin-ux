// SPDX-License-Identifier: GPL-3.0-only
//! `org.freedesktop.Notifications` server.
//!
//! Spec: https://specifications.freedesktop.org/notification-spec/latest/
//! The D-Bus side only parses and forwards; all timing and display
//! decisions happen in the UI thread with `lapin_ux_core::notify::Queue`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use lapin_ux_core::notify::{CloseReason, Image, Notification, Urgency, strip_markup};
use cosmic::iced::{Subscription, stream};
use futures::SinkExt;
use tokio::sync::mpsc;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

pub const PATH: &str = "/org/freedesktop/Notifications";
pub const NAME: &str = "org.freedesktop.Notifications";

#[derive(Debug, Clone)]
pub enum Event {
    /// The server is up; keep the connection to emit signals.
    Ready(zbus::Connection),
    /// Could not own the bus name (another notification daemon is running).
    Failed(String),
    Notify(Box<Notification>),
    Close(u32),
    /// From the desktop (clock click, Super+N).
    ToggleCenter,
    CloseCenter,
    SetDoNotDisturb(bool),
    ClearAll,
}

struct Server {
    tx: mpsc::UnboundedSender<Event>,
    next_id: AtomicU32,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["actions", "body", "icon-static", "persistence", "inline-reply", "x-kde-display-appname"]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            // 0 is reserved for "no id".
            let mut id = self.next_id.fetch_add(1, Ordering::Relaxed);
            if id == 0 {
                id = self.next_id.fetch_add(1, Ordering::Relaxed);
            }
            id
        };
        let urgency = hints.get("urgency").and_then(|v| u8::try_from(v).ok()).map(Urgency::from_hint).unwrap_or_default();
        let transient = hints.get("transient").and_then(|v| bool::try_from(v).ok()).unwrap_or(false);
        let desktop_entry = hints.get("desktop-entry").and_then(|v| String::try_from(v.clone()).ok());
        // Prefer an explicit image path over the app icon.
        let image = ["image-path", "image_path"]
            .iter()
            .find_map(|k| hints.get(*k).and_then(|v| String::try_from(v.clone()).ok()))
            .filter(|s| !s.is_empty());
        let picture = ["image-data", "image_data", "icon_data"].iter().find_map(|k| hints.get(*k).and_then(parse_image));
        // Progress: the common `value` hint (0–100).
        let progress = hints
            .get("value")
            .and_then(|v| i32::try_from(v).ok().or_else(|| u32::try_from(v).ok().map(|u| u as i32)))
            .map(|v| v.clamp(0, 100) as u8);
        let text_hint = |k: &str| hints.get(k).and_then(|v| String::try_from(v.clone()).ok()).filter(|s| !s.is_empty());
        let actions = Notification::parse_actions(&actions);
        // Sound: none if asked; else the app's own, else the theme's.
        let suppress = hints.get("suppress-sound").and_then(|v| bool::try_from(v).ok()).unwrap_or(false);
        let sound = (!suppress).then(|| {
            text_hint("sound-file").or_else(|| text_hint("sound-name")).unwrap_or_else(|| {
                if urgency == Urgency::Critical { "dialog-warning".into() } else { "message-new-instant".into() }
            })
        });
        let reply = actions.iter().find(|a| a.key == lapin_ux_core::notify::INLINE_REPLY).map(|a| lapin_ux_core::notify::Reply {
            placeholder: text_hint("x-kde-reply-placeholder-text").unwrap_or_else(|| "Type a reply".into()),
            submit: text_hint("x-kde-reply-submit-button-text").unwrap_or_else(|| if a.label.is_empty() { "Send".into() } else { a.label.clone() }),
        });
        let n = Notification {
            id,
            app_name,
            app_icon: image.unwrap_or(app_icon),
            summary: strip_markup(&summary),
            body: strip_markup(&body),
            actions,
            urgency,
            expire_timeout,
            transient,
            desktop_entry,
            received: std::time::SystemTime::now(),
            image: picture,
            progress,
            reply,
            sound,
        };
        let _ = self.tx.send(Event::Notify(Box::new(n)));
        id
    }

    fn close_notification(&self, id: u32) {
        let _ = self.tx.send(Event::Close(id));
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("Lapin UX Notifications", "Lapin UX", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[zbus(signal)]
    pub async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;

    /// KDE's inline-reply extension.
    #[zbus(signal)]
    pub async fn notification_replied(emitter: &SignalEmitter<'_>, id: u32, text: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn activation_token(emitter: &SignalEmitter<'_>, id: u32, activation_token: &str) -> zbus::Result<()>;
}

/// `image-data` is a (iiibiiay) structure.
fn parse_image(v: &OwnedValue) -> Option<Image> {
    let (w, h, stride, alpha, bits, ch, data): (i32, i32, i32, bool, i32, i32, Vec<u8>) = v.try_clone().ok()?.try_into().ok()?;
    Image::from_hint(w, h, stride, alpha, bits, ch, &data)
}

/// Desktop-facing control interface: the taskbar reads the unread count
/// and Do Not Disturb state, and asks for the notification centre.
pub const CONTROL_NAME: &str = "dev.lapinux.Notifications";
pub const CONTROL_PATH: &str = "/dev/lapinux/Notifications";

pub struct Control {
    tx: mpsc::UnboundedSender<Event>,
    pub unread: u32,
    pub dnd: bool,
}

#[zbus::interface(name = "dev.lapinux.Notifications")]
impl Control {
    fn close_center(&self) {
        let _ = self.tx.send(Event::CloseCenter);
    }

    fn toggle_center(&self) {
        let _ = self.tx.send(Event::ToggleCenter);
    }

    fn clear_all(&self) {
        let _ = self.tx.send(Event::ClearAll);
    }

    #[zbus(property)]
    fn unread(&self) -> u32 {
        self.unread
    }

    #[zbus(property)]
    fn do_not_disturb(&self) -> bool {
        self.dnd
    }

    #[zbus(property)]
    fn set_do_not_disturb(&mut self, on: bool) {
        let _ = self.tx.send(Event::SetDoNotDisturb(on));
    }
}

/// Push new state to D-Bus listeners (emits PropertiesChanged).
pub async fn publish_state(conn: zbus::Connection, unread: u32, dnd: bool) {
    let Ok(iface) = conn.object_server().interface::<_, Control>(CONTROL_PATH).await else { return };
    let mut c = iface.get_mut().await;
    let emitter = iface.signal_emitter();
    if c.unread != unread {
        c.unread = unread;
        let _ = c.unread_changed(emitter).await;
    }
    if c.dnd != dnd {
        c.dnd = dnd;
        let _ = c.do_not_disturb_changed(emitter).await;
    }
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run_with("lapin-ux-notifications-dbus", |_| {
        stream::channel(64, |mut output: futures::channel::mpsc::Sender<Event>| async move {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let server = Server { tx: tx.clone(), next_id: AtomicU32::new(1) };
            let control = Control { tx, unread: 0, dnd: false };
            let conn = async {
                zbus::connection::Builder::session()?
                    .serve_at(PATH, server)?
                    .serve_at(CONTROL_PATH, control)?
                    .name(NAME)?
                    .name(CONTROL_NAME)?
                    .build()
                    .await
            }
            .await;
            match conn {
                Ok(conn) => {
                    tracing::info!("serving {NAME}");
                    let _ = output.send(Event::Ready(conn)).await;
                }
                Err(e) => {
                    let _ = output.send(Event::Failed(e.to_string())).await;
                    futures::future::pending::<()>().await;
                }
            }
            while let Some(ev) = rx.recv().await {
                let _ = output.send(ev).await;
            }
            futures::future::pending::<()>().await;
        })
    })
}

async fn emitter(conn: &zbus::Connection) -> zbus::Result<SignalEmitter<'static>> {
    SignalEmitter::new(conn, PATH).map(|e| e.into_owned())
}

pub async fn emit_closed(conn: zbus::Connection, id: u32, reason: CloseReason) {
    if let Ok(e) = emitter(&conn).await
        && let Err(err) = Server::notification_closed(&e, id, reason as u32).await
    {
        tracing::warn!("NotificationClosed: {err}");
    }
}

pub async fn emit_reply(conn: zbus::Connection, id: u32, text: String) {
    let Ok(e) = emitter(&conn).await else { return };
    if let Err(err) = Server::notification_replied(&e, id, &text).await {
        tracing::warn!("NotificationReplied: {err}");
    }
}

pub async fn emit_action(conn: zbus::Connection, id: u32, key: String, token: Option<String>) {
    let Ok(e) = emitter(&conn).await else { return };
    if let Some(t) = token {
        let _ = Server::activation_token(&e, id, &t).await;
    }
    if let Err(err) = Server::action_invoked(&e, id, &key).await {
        tracing::warn!("ActionInvoked: {err}");
    }
}
