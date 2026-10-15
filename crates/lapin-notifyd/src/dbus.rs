// SPDX-License-Identifier: GPL-3.0-only
//! `org.freedesktop.Notifications` server.
//!
//! Spec: https://specifications.freedesktop.org/notification-spec/latest/
//! The D-Bus side only parses and forwards; all timing and display
//! decisions happen in the UI thread with `lapin_core::notify::Queue`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use lapin_core::notify::{CloseReason, Notification, Urgency, strip_markup};
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
}

struct Server {
    tx: mpsc::UnboundedSender<Event>,
    next_id: AtomicU32,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["actions", "body", "icon-static", "persistence"]
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
        let n = Notification {
            id,
            app_name,
            app_icon: image.unwrap_or(app_icon),
            summary: strip_markup(&summary),
            body: strip_markup(&body),
            actions: Notification::parse_actions(&actions),
            urgency,
            expire_timeout,
            transient,
            desktop_entry,
        };
        let _ = self.tx.send(Event::Notify(Box::new(n)));
        id
    }

    fn close_notification(&self, id: u32) {
        let _ = self.tx.send(Event::Close(id));
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("Lapin Notifications", "Lapin", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[zbus(signal)]
    pub async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn activation_token(emitter: &SignalEmitter<'_>, id: u32, activation_token: &str) -> zbus::Result<()>;
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run_with("lapin-notifications-dbus", |_| {
        stream::channel(64, |mut output: futures::channel::mpsc::Sender<Event>| async move {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let server = Server { tx, next_id: AtomicU32::new(1) };
            let conn = async {
                zbus::connection::Builder::session()?
                    .serve_at(PATH, server)?
                    .name(NAME)?
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

pub async fn emit_action(conn: zbus::Connection, id: u32, key: String, token: Option<String>) {
    let Ok(e) = emitter(&conn).await else { return };
    if let Some(t) = token {
        let _ = Server::activation_token(&e, id, &t).await;
    }
    if let Err(err) = Server::action_invoked(&e, id, &key).await {
        tracing::warn!("ActionInvoked: {err}");
    }
}
