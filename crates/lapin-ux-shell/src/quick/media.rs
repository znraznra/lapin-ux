// SPDX-License-Identifier: GPL-3.0-only
//! Media controls in quick settings: what's playing (MPRIS players on the
//! session bus) with previous / play-pause / next.

use std::collections::HashMap;
use std::time::Duration;

use cosmic::iced::{Subscription, stream};
use futures::{FutureExt, SinkExt, StreamExt};
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Player {
    pub bus: String,
    /// The app's name ("Firefox") and desktop id, for its icon.
    pub identity: String,
    pub desktop_entry: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub can_prev: bool,
    pub can_next: bool,
    pub can_toggle: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Previous,
    PlayPause,
    Next,
}

async fn props(conn: &Connection, bus: &str, iface: &str) -> zbus::Result<HashMap<String, OwnedValue>> {
    let p = zbus::fdo::PropertiesProxy::builder(conn).destination(bus.to_string())?.path(PATH)?.build().await?;
    let iface = zbus::names::InterfaceName::try_from(iface.to_string()).map_err(zbus::Error::from)?;
    Ok(p.get_all(iface).await?)
}

fn get<T: TryFrom<OwnedValue>>(m: &HashMap<String, OwnedValue>, k: &str) -> Option<T> {
    m.get(k).and_then(|v| v.try_clone().ok()).and_then(|v| T::try_from(v).ok())
}

async fn player(conn: &Connection, bus: &str) -> Option<Player> {
    let root = props(conn, bus, "org.mpris.MediaPlayer2").await.unwrap_or_default();
    let p = props(conn, bus, "org.mpris.MediaPlayer2.Player").await.ok()?;
    let meta: HashMap<String, OwnedValue> = get(&p, "Metadata").unwrap_or_default();
    let artist = get::<Vec<String>>(&meta, "xesam:artist").map(|a| a.join(", ")).or_else(|| get::<String>(&meta, "xesam:artist"));
    let title: String = get(&meta, "xesam:title").unwrap_or_default();
    Some(Player {
        bus: bus.to_string(),
        identity: get(&root, "Identity").unwrap_or_else(|| bus.trim_start_matches(PREFIX).to_string()),
        desktop_entry: get(&root, "DesktopEntry").unwrap_or_default(),
        title,
        artist: artist.unwrap_or_default(),
        playing: get::<String>(&p, "PlaybackStatus").as_deref() == Some("Playing"),
        can_prev: get(&p, "CanGoPrevious").unwrap_or(false),
        can_next: get(&p, "CanGoNext").unwrap_or(false),
        can_toggle: get::<bool>(&p, "CanPlay").unwrap_or(false) || get::<bool>(&p, "CanPause").unwrap_or(false),
    })
}

/// Players with something loaded; the playing one first.
async fn snapshot(conn: &Connection) -> Vec<Player> {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else { return Vec::new() };
    let names = dbus.list_names().await.unwrap_or_default();
    let mut out = Vec::new();
    for n in names.iter().map(|n| n.as_str()).filter(|n| n.starts_with(PREFIX)) {
        if let Some(p) = player(conn, n).await
            && !p.title.is_empty()
        {
            out.push(p);
        }
    }
    out.sort_by_key(|p| !p.playing);
    out
}

pub fn subscription() -> Subscription<Vec<Player>> {
    Subscription::run_with("lapin-ux-media", |_| {
        stream::channel(4, |mut out: futures::channel::mpsc::Sender<Vec<Player>>| async move {
            let Ok(conn) = Connection::session().await else { return futures::future::pending().await };
            let rules = [
                MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .interface("org.freedesktop.DBus.Properties")
                    .and_then(|b| b.path(PATH))
                    .map(|b| b.build()),
                MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .sender("org.freedesktop.DBus")
                    .and_then(|b| b.member("NameOwnerChanged"))
                    .and_then(|b| b.arg0ns("org.mpris.MediaPlayer2"))
                    .map(|b| b.build()),
            ];
            let mut streams = Vec::new();
            for r in rules.into_iter().flatten() {
                if let Ok(s) = MessageStream::for_match_rule(r, &conn, Some(32)).await {
                    streams.push(s);
                }
            }
            let mut signals = futures::stream::select_all(streams);
            loop {
                let _ = out.send(snapshot(&conn).await).await;
                if signals.next().await.is_none() {
                    return futures::future::pending().await;
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
                while let Some(Some(_)) = signals.next().now_or_never() {}
            }
        })
    })
}

pub async fn command(bus: String, c: Command) {
    let Ok(conn) = Connection::session().await else { return };
    let method = match c {
        Command::Previous => "Previous",
        Command::PlayPause => "PlayPause",
        Command::Next => "Next",
    };
    if let Err(e) = conn.call_method(Some(bus.as_str()), PATH, Some("org.mpris.MediaPlayer2.Player"), method, &()).await {
        tracing::warn!("media {method}: {e}");
    }
}
