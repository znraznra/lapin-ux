// SPDX-License-Identifier: GPL-3.0-only
//! Talks to `lapin-ux-notifyd`: the unread count and Do Not Disturb state
//! for the clock's badge, and opening the notification centre.

use cosmic::iced::{self, Subscription};
use futures::{SinkExt, StreamExt};
use std::time::Duration;

#[zbus::proxy(
    interface = "dev.lapinux.Notifications",
    default_service = "dev.lapinux.Notifications",
    default_path = "/dev/lapinux/Notifications"
)]
trait Control {
    fn toggle_center(&self) -> zbus::Result<()>;
    #[zbus(property)]
    fn unread(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn do_not_disturb(&self) -> zbus::Result<bool>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    /// The daemon is running and answering.
    pub available: bool,
    pub unread: u32,
    pub dnd: bool,
}

pub fn subscription() -> Subscription<State> {
    Subscription::run_with("lapin-ux-notify-client", |_| {
        iced::stream::channel(8, |mut tx: futures::channel::mpsc::Sender<State>| async move {
            let Ok(conn) = zbus::Connection::session().await else { return futures::future::pending().await };
            loop {
                // The daemon may start after us or restart; keep retrying.
                if let Ok(proxy) = ControlProxy::new(&conn).await
                    && let (Ok(unread), Ok(dnd)) = (proxy.unread().await, proxy.do_not_disturb().await)
                {
                    let mut state = State { available: true, unread, dnd };
                    let _ = tx.send(state).await;
                    let mut u = proxy.receive_unread_changed().await;
                    let mut d = proxy.receive_do_not_disturb_changed().await;
                    let mut owner = proxy.inner().receive_owner_changed().await.ok();
                    loop {
                        tokio::select! {
                            Some(c) = u.next() => if let Ok(v) = c.get().await { state.unread = v },
                            Some(c) = d.next() => if let Ok(v) = c.get().await { state.dnd = v },
                            Some(o) = async { match owner.as_mut() { Some(s) => s.next().await, None => futures::future::pending().await } } => {
                                if o.is_none() { break }
                            }
                        }
                        let _ = tx.send(state).await;
                    }
                    let _ = tx.send(State::default()).await;
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        })
    })
}

/// Returns false if the daemon isn't reachable.
pub async fn toggle_center() -> bool {
    let Ok(conn) = zbus::Connection::session().await else { return false };
    let Ok(proxy) = ControlProxy::new(&conn).await else { return false };
    proxy.toggle_center().await.is_ok()
}
