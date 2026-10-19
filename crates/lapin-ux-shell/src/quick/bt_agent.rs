// SPDX-License-Identifier: GPL-3.0-only
//! The Bluetooth pairing agent: BlueZ asks it to confirm a passkey, show a
//! PIN to type on the device, or ask the user for one. Requests go to the
//! quick settings panel; the answer comes back through [`answer`].

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use cosmic::iced::{Subscription, stream};
use futures::SinkExt;
use futures::channel::oneshot;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, interface};

const PATH: &str = "/dev/lapinux/BluetoothAgent";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// "Does this PIN match the one on the device?"
    Confirm(u32),
    /// Type this code on the device.
    Display(String),
    /// The device needs a PIN typed here.
    EnterPin,
    /// The device wants to connect (no code).
    Authorize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub id: u64,
    pub device: String,
    pub ask: Ask,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Prompt(Prompt),
    /// BlueZ cancelled the request (timeout, device went away).
    Cancelled,
}

type Reply = oneshot::Sender<Option<String>>;
static PENDING: LazyLock<Mutex<HashMap<u64, Reply>>> = LazyLock::new(Default::default);
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The user's answer: `Some(pin)` (empty for yes) or `None` to refuse.
pub fn answer(id: u64, reply: Option<String>) {
    if let Some(tx) = PENDING.lock().ok().and_then(|mut p| p.remove(&id)) {
        let _ = tx.send(reply);
    }
}

struct Agent {
    events: futures::channel::mpsc::Sender<AgentEvent>,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum BtError {
    Rejected(String),
    Canceled(String),
}

impl Agent {
    async fn device_name(device: &OwnedObjectPath) -> String {
        let Ok(conn) = Connection::system().await else { return String::new() };
        let p = zbus::proxy::Builder::<zbus::Proxy>::new(&conn)
            .destination("org.bluez")
            .and_then(|b| b.path(device.as_str()))
            .and_then(|b| b.interface("org.bluez.Device1"));
        let Ok(b) = p else { return String::new() };
        let Ok(p) = b.cache_properties(zbus::proxy::CacheProperties::No).build().await else { return String::new() };
        p.get_property::<String>("Alias").await.unwrap_or_default()
    }

    /// Ask the user and wait for the answer.
    async fn ask(&mut self, device: &OwnedObjectPath, ask: Ask) -> Result<String, BtError> {
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        if let Ok(mut p) = PENDING.lock() {
            p.insert(id, tx);
        }
        let device = Self::device_name(device).await;
        let _ = self.events.send(AgentEvent::Prompt(Prompt { id, device, ask })).await;
        match rx.await {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(BtError::Rejected("Declined".into())),
            Err(_) => Err(BtError::Canceled("Cancelled".into())),
        }
    }
}

#[interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    async fn request_pin_code(&mut self, device: OwnedObjectPath) -> Result<String, BtError> {
        self.ask(&device, Ask::EnterPin).await
    }

    async fn display_pin_code(&mut self, device: OwnedObjectPath, pincode: String) {
        // Shown until pairing finishes; nothing to answer.
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let device = Self::device_name(&device).await;
        let _ = self.events.send(AgentEvent::Prompt(Prompt { id, device, ask: Ask::Display(pincode) })).await;
    }

    async fn request_passkey(&mut self, device: OwnedObjectPath) -> Result<u32, BtError> {
        let pin = self.ask(&device, Ask::EnterPin).await?;
        pin.trim().parse().map_err(|_| BtError::Rejected("Not a number".into()))
    }

    async fn display_passkey(&mut self, device: OwnedObjectPath, passkey: u32, _entered: u16) {
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let device = Self::device_name(&device).await;
        let _ = self.events.send(AgentEvent::Prompt(Prompt { id, device, ask: Ask::Display(format!("{passkey:06}")) })).await;
    }

    async fn request_confirmation(&mut self, device: OwnedObjectPath, passkey: u32) -> Result<(), BtError> {
        self.ask(&device, Ask::Confirm(passkey)).await.map(|_| ())
    }

    async fn request_authorization(&mut self, device: OwnedObjectPath) -> Result<(), BtError> {
        self.ask(&device, Ask::Authorize).await.map(|_| ())
    }

    /// Services of already-paired devices are fine.
    fn authorize_service(&self, _device: OwnedObjectPath, _uuid: String) {}

    async fn cancel(&mut self) {
        if let Ok(mut p) = PENDING.lock() {
            p.clear();
        }
        let _ = self.events.send(AgentEvent::Cancelled).await;
    }
}

/// Serve the agent and register it with BlueZ (again whenever BlueZ restarts).
pub fn subscription() -> Subscription<AgentEvent> {
    Subscription::run_with("lapin-ux-bt-agent", |_| {
        stream::channel(8, |events: futures::channel::mpsc::Sender<AgentEvent>| async move {
            let conn = match Connection::system().await {
                Ok(c) => c,
                Err(_) => return futures::future::pending().await,
            };
            if conn.object_server().at(PATH, Agent { events }).await.is_err() {
                return futures::future::pending().await;
            }
            let mut registered = false;
            loop {
                let path = OwnedObjectPath::try_from(PATH).expect("agent path");
                let has_bluez = zbus::fdo::DBusProxy::new(&conn)
                    .await
                    .ok()
                    .map(|d| async move { d.name_has_owner(zbus::names::BusName::try_from("org.bluez").expect("name")).await.unwrap_or(false) });
                let up = match has_bluez {
                    Some(f) => f.await,
                    None => false,
                };
                if up && !registered {
                    let r = conn
                        .call_method(Some("org.bluez"), "/org/bluez", Some("org.bluez.AgentManager1"), "RegisterAgent", &(&path, "KeyboardDisplay"))
                        .await;
                    if r.is_ok() {
                        let _ = conn
                            .call_method(Some("org.bluez"), "/org/bluez", Some("org.bluez.AgentManager1"), "RequestDefaultAgent", &(&path,))
                            .await;
                        tracing::info!("bluetooth agent registered");
                        registered = true;
                    }
                } else if !up {
                    registered = false;
                }
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        })
    })
}
