// SPDX-License-Identifier: GPL-3.0-only
// Watcher adapted from cosmic-applets/cosmic-applet-status-area (Copyright 2023 System76, GPL-3.0-only).
//! System tray (notification area).
//!
//! As the desktop shell we host `org.kde.StatusNotifierWatcher` ourselves:
//! apps register their tray icons with it, and the taskbar shows them next
//! to the clock. Left click activates the app, right click opens its menu.

pub mod item;

use futures::{Stream, StreamExt};
use std::pin::Pin;
use zbus::fdo::{DBusProxy, RequestNameFlags};
use zbus::message::Header;
use zbus::names::{BusName, UniqueName, WellKnownName};
use zbus::object_server::SignalEmitter;

use cosmic::iced::{self, Subscription};
pub use item::{IconUpdate, Layout, TrayItem};

const WATCHER_NAME: WellKnownName = WellKnownName::from_static_str_unchecked("org.kde.StatusNotifierWatcher");
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

#[derive(Clone, Debug)]
pub enum Event {
    Connected,
    Registered(TrayItem),
    Unregistered(String),
    Error(String),
}

// ---- watcher service ---------------------------------------------------------

#[derive(Default)]
struct Watcher {
    items: Vec<(UniqueName<'static>, String)>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &mut self,
        service: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] ctxt: SignalEmitter<'_>,
    ) {
        let Some(sender) = hdr.sender() else { return };
        // Items may register by object path only (libappindicator does).
        let service = if service.starts_with('/') { format!("{sender}{service}") } else { service.to_string() };
        if !self.items.iter().any(|(a, b)| a == sender && b == &service) {
            let _ = Self::status_notifier_item_registered(&ctxt, &service).await;
            self.items.push((sender.to_owned(), service));
        }
    }

    fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.iter().map(|(_, s)| s.clone()).collect()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(ctxt: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(ctxt: &SignalEmitter<'_>, service: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(ctxt: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Serve the watcher on `conn` and drop items whose owner leaves the bus.
async fn serve_watcher(conn: &zbus::Connection) -> zbus::Result<()> {
    conn.object_server().at(WATCHER_PATH, Watcher::default()).await?;
    let iface = conn.object_server().interface::<_, Watcher>(WATCHER_PATH).await?;
    let dbus = DBusProxy::new(conn).await?;
    let mut owners = dbus.receive_name_owner_changed().await?;
    // Replaceable, so a user-run tray (e.g. another bar) can take over.
    let flags = RequestNameFlags::AllowReplacement | RequestNameFlags::ReplaceExisting;
    if let Err(e) = dbus.request_name(WATCHER_NAME, flags).await {
        tracing::warn!("could not own {WATCHER_NAME}: {e}");
    }
    let conn = conn.clone();
    tokio::spawn(async move {
        while let Some(ev) = owners.next().await {
            let Ok(args) = ev.args() else { continue };
            let BusName::Unique(name) = &args.name else { continue };
            if args.new_owner.is_some() {
                continue;
            }
            let mut w = iface.get_mut().await;
            let gone: Vec<String> = w.items.iter().filter(|(u, _)| u == name).map(|(_, s)| s.clone()).collect();
            w.items.retain(|(u, _)| u != name);
            drop(w);
            if let Ok(ctxt) = SignalEmitter::new(&conn, WATCHER_PATH) {
                for s in gone {
                    let _ = Watcher::status_notifier_item_unregistered(&ctxt, &s).await;
                }
            }
        }
    });
    Ok(())
}

// ---- host (client of the watcher) -------------------------------------------

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait WatcherClient {
    fn register_status_notifier_host(&self, name: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;
    #[zbus(signal)]
    fn status_notifier_item_registered(&self, name: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn status_notifier_item_unregistered(&self, name: &str) -> zbus::Result<()>;
}

type EventStream = Pin<Box<dyn Stream<Item = Event> + Send>>;

async fn load(conn: zbus::Connection, name: String) -> Event {
    match TrayItem::new(&conn, name).await {
        Ok(i) => Event::Registered(i),
        Err(e) => Event::Error(e.to_string()),
    }
}

async fn connect() -> zbus::Result<EventStream> {
    let conn = zbus::Connection::session().await?;
    serve_watcher(&conn).await?;
    let watcher = WatcherClientProxy::new(&conn).await?;
    if let Some(n) = conn.unique_name() {
        let _ = watcher.register_status_notifier_host(n.as_str()).await;
    }
    let c1 = conn.clone();
    let registered = watcher.receive_status_notifier_item_registered().await?.then(move |ev| {
        let c = c1.clone();
        async move {
            match ev.args() {
                Ok(a) => load(c, a.name.to_string()).await,
                Err(e) => Event::Error(e.to_string()),
            }
        }
    });
    let unregistered = watcher.receive_status_notifier_item_unregistered().await?.map(|ev| match ev.args() {
        Ok(a) => Event::Unregistered(a.name.to_string()),
        Err(e) => Event::Error(e.to_string()),
    });
    let existing = watcher.registered_status_notifier_items().await.unwrap_or_default();
    let c2 = conn.clone();
    let initial = futures::stream::iter(existing).then(move |n| load(c2.clone(), n));
    // Keep the connection (and with it the watcher service) alive for as
    // long as the stream is polled.
    let keep = conn.clone();
    Ok(Box::pin(
        initial
            .chain(futures::stream::select(registered.boxed(), unregistered.boxed()))
            .inspect(move |_| {
                let _ = &keep;
            }),
    ))
}

pub fn subscription() -> Subscription<Event> {
    enum State {
        Start,
        Running(EventStream),
        Done,
    }
    Subscription::run_with("atrium-tray", |_| {
        futures::stream::unfold(State::Start, |state| async move {
            match state {
                State::Start => match connect().await {
                    Ok(stream) => Some((Event::Connected, State::Running(stream))),
                    Err(e) => Some((Event::Error(e.to_string()), State::Done)),
                },
                State::Running(mut s) => s.next().await.map(|e| (e, State::Running(s))),
                State::Done => {
                    iced::futures::future::pending::<()>().await;
                    None
                }
            }
        })
    })
}
