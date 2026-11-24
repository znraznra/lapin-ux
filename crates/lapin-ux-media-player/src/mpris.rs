// SPDX-License-Identifier: GPL-3.0-only
//! Media keys and the desktop's "now playing" controls (MPRIS), plus a logind
//! idle/sleep inhibitor while something plays. The shell, lock screen and any
//! `playerctl` talk to the player through the session bus name
//! `org.mpris.MediaPlayer2.dev.lapinux.MediaPlayer`; nothing here writes a
//! shared setting.

use std::collections::HashMap;
use std::sync::OnceLock;

use cosmic::iced::{Subscription, stream};
use tokio::sync::{mpsc, watch};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

/// A request from the desktop.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    PlayPause,
    Play,
    Pause,
    Next,
    Prev,
    Stop,
    /// Relative seek, seconds.
    SeekBy(f64),
    /// Absolute position, seconds.
    SeekTo(f64),
    Raise,
}

/// What the player shows right now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub has_media: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub dur: f64,
    pub pos: f64,
    pub volume: f64,
}

fn state_tx() -> &'static watch::Sender<State> {
    static TX: OnceLock<watch::Sender<State>> = OnceLock::new();
    TX.get_or_init(|| watch::channel(State::default()).0)
}

/// Called by the window whenever something changed.
pub fn publish(s: State) {
    state_tx().send_if_modified(|old| {
        if *old == s {
            false
        } else {
            *old = s;
            true
        }
    });
}

pub fn playback_status(s: &State) -> &'static str {
    if !s.has_media { "Stopped" } else if s.playing { "Playing" } else { "Paused" }
}

fn secs_to_us(s: f64) -> i64 {
    (s.max(0.0) * 1_000_000.0) as i64
}

pub fn metadata(s: &State) -> HashMap<String, OwnedValue> {
    let mut m: HashMap<String, OwnedValue> = HashMap::new();
    let mut put = |k: &str, v: Value<'_>| {
        if let Ok(v) = OwnedValue::try_from(v) {
            m.insert(k.to_string(), v);
        }
    };
    if let Ok(p) = ObjectPath::try_from("/dev/lapinux/MediaPlayer/track/current") {
        put("mpris:trackid", Value::from(p));
    }
    if s.has_media {
        put("xesam:title", Value::from(s.title.clone()));
        if !s.artist.is_empty() {
            put("xesam:artist", Value::from(vec![s.artist.clone()]));
        }
        if !s.album.is_empty() {
            put("xesam:album", Value::from(s.album.clone()));
        }
        if s.dur > 0.0 {
            put("mpris:length", Value::from(secs_to_us(s.dur)));
        }
    }
    m
}

struct Root {
    cmds: mpsc::UnboundedSender<Cmd>,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        let _ = self.cmds.send(Cmd::Raise);
    }
    fn quit(&self) {}
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> String {
        "Lapin Media Player".into()
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "dev.lapinux.MediaPlayer".into()
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["file".into(), "http".into(), "https".into()]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec!["audio/mpeg".into(), "audio/flac".into(), "audio/ogg".into(), "video/mp4".into()]
    }
}

struct Player {
    cmds: mpsc::UnboundedSender<Cmd>,
    state: watch::Receiver<State>,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        let _ = self.cmds.send(Cmd::Next);
    }
    fn previous(&self) {
        let _ = self.cmds.send(Cmd::Prev);
    }
    fn pause(&self) {
        let _ = self.cmds.send(Cmd::Pause);
    }
    fn play_pause(&self) {
        let _ = self.cmds.send(Cmd::PlayPause);
    }
    fn stop(&self) {
        let _ = self.cmds.send(Cmd::Stop);
    }
    fn play(&self) {
        let _ = self.cmds.send(Cmd::Play);
    }
    fn seek(&self, offset_us: i64) {
        let _ = self.cmds.send(Cmd::SeekBy(offset_us as f64 / 1e6));
    }
    fn set_position(&self, _track: ObjectPath<'_>, position_us: i64) {
        let _ = self.cmds.send(Cmd::SeekTo(position_us as f64 / 1e6));
    }
    #[zbus(property)]
    fn playback_status(&self) -> String {
        playback_status(&self.state.borrow()).into()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.state.borrow())
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        secs_to_us(self.state.borrow().pos)
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        (self.state.borrow().volume / 100.0).clamp(0.0, 1.0)
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// Holds the logind "don't idle or sleep" lock; dropping the fd releases it.
async fn inhibit() -> anyhow::Result<zbus::zvariant::OwnedFd> {
    let conn = zbus::Connection::system().await?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "Inhibit",
            &("idle:sleep", "Lapin Media Player", "Playing media", "block"),
        )
        .await?;
    Ok(reply.body().deserialize::<zbus::zvariant::OwnedFd>()?)
}

async fn serve(mut out: futures::channel::mpsc::Sender<Cmd>) {
    use futures::SinkExt;
    let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
    let state = state_tx().subscribe();
    let path = "/org/mpris/MediaPlayer2";
    let built = async {
        zbus::connection::Builder::session()?
            .name("org.mpris.MediaPlayer2.dev.lapinux.MediaPlayer")?
            .serve_at(path, Root { cmds: tx.clone() })?
            .serve_at(path, Player { cmds: tx, state: state.clone() })?
            .build()
            .await
    }
    .await;
    // No session bus (or the name is taken): the player still works, just
    // without media keys.
    let Ok(conn) = built else {
        tracing::warn!("media keys are unavailable: {:?}", built.err());
        std::future::pending::<()>().await;
        return;
    };
    let mut watch_rx = state;
    let mut last = State::default();
    let mut lock: Option<zbus::zvariant::OwnedFd> = None;
    let mut inhibit_failed = false;
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break };
                if out.send(cmd).await.is_err() { break; }
            }
            changed = watch_rx.changed() => {
                if changed.is_err() { break; }
                let now = watch_rx.borrow_and_update().clone();
                if let Ok(iface) = conn.object_server().interface::<_, Player>(path).await {
                    let ctx = iface.signal_emitter();
                    let g = iface.get().await;
                    if playback_status(&now) != playback_status(&last) {
                        let _ = g.playback_status_changed(ctx).await;
                    }
                    if now.title != last.title || now.artist != last.artist || now.album != last.album
                        || now.has_media != last.has_media || (now.dur - last.dur).abs() > 0.5 {
                        let _ = g.metadata_changed(ctx).await;
                    }
                    if (now.volume - last.volume).abs() > 0.5 {
                        let _ = g.volume_changed(ctx).await;
                    }
                }
                if now.playing && lock.is_none() && !inhibit_failed {
                    match inhibit().await {
                        Ok(fd) => lock = Some(fd),
                        Err(e) => {
                            // Tried once per play; not again on every state change.
                            inhibit_failed = true;
                            tracing::warn!("could not keep the screen awake while playing: {e:#}");
                        }
                    }
                } else if !now.playing {
                    lock = None;
                    inhibit_failed = false;
                }
                last = now;
            }
        }
    }
}

pub fn subscription() -> Subscription<Cmd> {
    Subscription::run_with("lapin-ux-media-player-mpris", |_| {
        stream::channel(16, |out: futures::channel::mpsc::Sender<Cmd>| async move {
            serve(out).await;
            std::future::pending::<()>().await;
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_follows_state() {
        let mut s = State::default();
        assert_eq!(playback_status(&s), "Stopped");
        s.has_media = true;
        assert_eq!(playback_status(&s), "Paused");
        s.playing = true;
        assert_eq!(playback_status(&s), "Playing");
    }

    #[test]
    fn metadata_has_title_and_length() {
        let s = State { has_media: true, title: "Song".into(), artist: "Band".into(), dur: 90.0, ..Default::default() };
        let m = metadata(&s);
        assert!(m.contains_key("xesam:title") && m.contains_key("xesam:artist") && m.contains_key("mpris:length"));
        assert!(metadata(&State::default()).len() == 1);
    }
}
