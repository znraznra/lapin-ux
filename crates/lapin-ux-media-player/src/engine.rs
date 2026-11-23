// SPDX-License-Identifier: GPL-3.0-only
//! The mpv engine. Lapin Media Player starts its own `mpv` (never touches the
//! system's settings: it reads `~/.config/dev.lapinux.MediaPlayer/mpv/` only)
//! and talks to it over mpv's JSON IPC socket. mpv stays a separate program,
//! so its licence never mixes with ours and a crash in a codec cannot take the
//! window down.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cosmic::iced::{Subscription, stream};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

/// What a track of the playing file carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Subtitle,
    Video,
}

/// One audio, subtitle or video track of the file that is playing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub title: String,
    pub lang: String,
    pub selected: bool,
}

/// What the player tells the window.
#[derive(Debug, Clone)]
pub enum Event {
    /// mpv is running; commands can be sent through the handle.
    Ready(Engine),
    Pause(bool),
    Time(f64),
    Duration(f64),
    Volume(f64),
    Mute(bool),
    Speed(f64),
    /// The chapters of the file: title and start time in seconds (video only).
    Chapters(Vec<(String, f64)>),
    /// The chapter that is playing, counted from 0; -1 before the first one (video only).
    Chapter(i64),
    /// One end of the A-B loop was set or cleared (`false` is A, `true` is B; video only).
    LoopPoint(bool, bool),
    /// `media-title`, or the stream's own title when it sends one.
    Title(String),
    Artist(String),
    Album(String),
    /// The station's current song (ICY title).
    StreamTitle(String),
    /// A picture or video track is playing (mpv opens its own video window).
    HasVideo(bool),
    /// The audio and subtitle tracks of the file (video engine only).
    Tracks(Vec<Track>),
    /// The sound outputs mpv can use: (name, description).
    Devices(Vec<(String, String)>),
    /// The output in use (mpv's `audio-device` name).
    Device(String),
    /// mpv began opening a file. A file-end that arrives before this belongs to
    /// the file that was playing before.
    Started,
    Loaded,
    /// mpv refused a command that carried an id (see `Engine::seek_to`).
    Rejected { id: u64, why: String },
    /// The file ended by itself (`error` is mpv's reason when it failed).
    Ended { error: Option<String> },
    Failed(String),
}

/// Sends commands to mpv. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Engine {
    tx: mpsc::UnboundedSender<(Vec<Value>, Option<u64>)>,
}

/// `request_id` of seeks, so a refused seek (a stream that cannot seek) is told apart.
pub const SEEK_ID: u64 = 9001;

impl Engine {
    /// A handle plus the receiving end, for an engine that is not the mpv process (the embedded video one).
    pub fn channel() -> (Engine, mpsc::UnboundedReceiver<(Vec<Value>, Option<u64>)>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Engine { tx }, rx)
    }
    pub fn command(&self, args: Vec<Value>) {
        let _ = self.tx.send((args, None));
    }
    fn command_with_id(&self, args: Vec<Value>, id: u64) {
        let _ = self.tx.send((args, Some(id)));
    }
    pub fn load(&self, target: &str) {
        self.command(vec![json!("loadfile"), json!(target), json!("replace")]);
    }
    pub fn stop(&self) {
        self.command(vec![json!("stop")]);
    }
    pub fn set(&self, name: &str, value: Value) {
        self.command(vec![json!("set_property"), json!(name), value]);
    }
    pub fn seek_to(&self, seconds: f64) {
        self.command_with_id(vec![json!("seek"), json!(seconds), json!("absolute")], SEEK_ID);
    }
    pub fn seek_by(&self, seconds: f64) {
        self.command_with_id(vec![json!("seek"), json!(seconds), json!("relative")], SEEK_ID);
    }
    pub fn toggle(&self, name: &str) {
        self.command(vec![json!("cycle"), json!(name)]);
    }
    pub fn frame_step(&self) {
        self.command(vec![json!("frame-step")]);
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("dev.lapinux.MediaPlayer")
}

/// The IPC socket. mpv does not check who connects, so the folder is the only
/// protection: `XDG_RUNTIME_DIR` is private to the user; without it, a folder
/// made just for this process with mode 0700 (never the shared temp folder itself).
fn socket_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()) {
        return dir.join(format!("lapin-ux-media-player-{}.sock", std::process::id()));
    }
    use std::os::unix::fs::DirBuilderExt;
    let dir = std::env::temp_dir().join(format!("lapin-ux-media-player-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::DirBuilder::new().mode(0o700).create(&dir);
    dir.join("mpv.sock")
}

/// Properties the window follows: (observe id, name).
const OBSERVED: &[&str] = &[
    "pause", "time-pos", "duration", "volume", "mute", "speed", "media-title", "metadata", "vid",
    "audio-device-list", "audio-device",
];

/// One line from mpv's socket, as events. Pure, so it is tested without mpv.
pub fn parse_line(line: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return Vec::new() };
    // A reply to a command: only refusals of commands that carried an id matter.
    if v.get("event").is_none() {
        let err = v.get("error").and_then(Value::as_str).unwrap_or("success");
        return match v.get("request_id").and_then(Value::as_u64) {
            Some(id) if err != "success" => vec![Event::Rejected { id, why: err.to_string() }],
            _ => Vec::new(),
        };
    }
    match v.get("event").and_then(Value::as_str) {
        Some("start-file") => vec![Event::Started],
        Some("property-change") => {
            let name = v.get("name").and_then(Value::as_str).unwrap_or("");
            let data = v.get("data").cloned().unwrap_or(Value::Null);
            property(name, &data)
        }
        Some("file-loaded") => vec![Event::Loaded],
        Some("end-file") => match v.get("reason").and_then(Value::as_str) {
            Some("eof") => vec![Event::Ended { error: None }],
            Some("error") => {
                let why = v.get("file_error").and_then(Value::as_str).unwrap_or("it could not be played");
                vec![Event::Ended { error: Some(why.to_string()) }]
            }
            _ => Vec::new(), // stop, quit, redirect: we asked for it
        },
        _ => Vec::new(),
    }
}

pub(crate) fn property(name: &str, data: &Value) -> Vec<Event> {
    let num = data.as_f64();
    match name {
        "pause" => data.as_bool().map(Event::Pause).into_iter().collect(),
        "mute" => data.as_bool().map(Event::Mute).into_iter().collect(),
        "time-pos" => vec![Event::Time(num.unwrap_or(0.0))],
        "duration" => vec![Event::Duration(num.unwrap_or(0.0))],
        "volume" => num.map(Event::Volume).into_iter().collect(),
        "speed" => num.map(Event::Speed).into_iter().collect(),
        "media-title" => vec![Event::Title(data.as_str().unwrap_or("").to_string())],
        "audio-device" => vec![Event::Device(data.as_str().unwrap_or("auto").to_string())],
        "audio-device-list" => {
            let list = data
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|d| {
                            let name = d.get("name")?.as_str()?.to_string();
                            let desc = d.get("description").and_then(Value::as_str).unwrap_or(&name).to_string();
                            Some((name, desc))
                        })
                        .collect()
                })
                .unwrap_or_default();
            vec![Event::Devices(list)]
        }
        // `vid` is a track number while a video or cover picture shows, false otherwise.
        "vid" => vec![Event::HasVideo(data.as_i64().is_some())],
        "metadata" => {
            let mut out = Vec::new();
            if let Some(map) = data.as_object() {
                // mpv spells tag names however the file did.
                let get = |key: &str| {
                    map.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).and_then(|(_, v)| v.as_str()).unwrap_or("").to_string()
                };
                out.push(Event::Artist(get("artist")));
                out.push(Event::Album(get("album")));
                out.push(Event::StreamTitle(get("icy-title")));
            } else {
                out.extend([Event::Artist(String::new()), Event::Album(String::new()), Event::StreamTitle(String::new())]);
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The command line mpv is started with.
pub fn args(socket: &Path) -> Vec<String> {
    let conf = config_dir().join("mpv");
    vec![
        "--idle=yes".into(),
        "--no-terminal".into(),
        // Our own folder, so the user's ~/.config/mpv is neither read nor changed.
        format!("--config-dir={}", conf.display()),
        format!("--input-ipc-server={}", socket.display()),
        "--force-window=no".into(),
        "--audio-display=no".into(),
        "--keep-open=no".into(),
        "--title=Lapin Media Player".into(),
        "--wayland-app-id=dev.lapinux.MediaPlayer.Video".into(),
    ]
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run_with("lapin-ux-media-player-mpv", |_| {
        stream::channel(64, |mut out: futures::channel::mpsc::Sender<Event>| async move {
            use futures::SinkExt;
            let socket = socket_path();
            let _ = std::fs::remove_file(&socket);
            let _ = std::fs::create_dir_all(config_dir().join("mpv"));
            let child = tokio::process::Command::new("mpv").args(args(&socket)).kill_on_drop(true).spawn();
            let mut child = match child {
                Ok(c) => c,
                Err(e) => {
                    let why = if e.kind() == std::io::ErrorKind::NotFound {
                        "The player engine (mpv) is not installed.".to_string()
                    } else {
                        format!("The player engine could not start: {e}")
                    };
                    let _ = out.send(Event::Failed(why)).await;
                    std::future::pending::<()>().await;
                    return;
                }
            };
            let mut stream = None;
            for _ in 0..60 {
                if let Ok(s) = UnixStream::connect(&socket).await {
                    stream = Some(s);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let Some(stream) = stream else {
                // mpv started but never opened its socket: stop it now rather than at exit.
                let _ = child.kill().await;
                let _ = std::fs::remove_file(&socket);
                let _ = out.send(Event::Failed("The player engine did not answer.".into())).await;
                std::future::pending::<()>().await;
                return;
            };
            let (read, mut write) = stream.into_split();
            let (tx, mut rx) = mpsc::unbounded_channel::<(Vec<Value>, Option<u64>)>();
            for (i, name) in OBSERVED.iter().enumerate() {
                let _ = tx.send((vec![json!("observe_property"), json!(i + 1), json!(name)], None));
            }
            let _ = out.send(Event::Ready(Engine { tx })).await;
            let mut lines = BufReader::new(read).lines();
            loop {
                tokio::select! {
                    cmd = rx.recv() => {
                        let Some(cmd) = cmd else { break };
                        let mut body = json!({ "command": cmd.0 });
                        if let Some(id) = cmd.1 { body["request_id"] = json!(id); }
                        let mut text = body.to_string();
                        text.push('\n');
                        if write.write_all(text.as_bytes()).await.is_err() { break; }
                    }
                    line = lines.next_line() => match line {
                        Ok(Some(line)) => for e in parse_line(&line) { let _ = out.send(e).await; },
                        _ => break,
                    },
                }
            }
            let _ = out.send(Event::Failed("The player engine stopped.".into())).await;
            let _ = child.kill().await;
            let _ = std::fs::remove_file(&socket);
            if socket.file_name().is_some_and(|n| n == "mpv.sock") {
                let _ = std::fs::remove_dir(socket.parent().unwrap_or(Path::new("/nonexistent")));
            }
            std::future::pending::<()>().await;
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_seek_is_reported() {
        let e = parse_line(r#"{"request_id":9001,"error":"property unavailable"}"#);
        assert!(matches!(e.as_slice(), [Event::Rejected { id: 9001, .. }]));
        assert!(parse_line(r#"{"request_id":9001,"error":"success","data":null}"#).is_empty());
        assert!(parse_line(r#"{"error":"success"}"#).is_empty());
        let e = parse_line(r#"{"event":"start-file","playlist_entry_id":2}"#);
        assert!(matches!(e.as_slice(), [Event::Started]));
    }

    #[test]
    fn reads_properties() {
        let e = parse_line(r#"{"event":"property-change","id":1,"name":"pause","data":true}"#);
        assert!(matches!(e.as_slice(), [Event::Pause(true)]));
        let e = parse_line(r#"{"event":"property-change","id":2,"name":"time-pos","data":12.5}"#);
        assert!(matches!(e.as_slice(), [Event::Time(t)] if (*t - 12.5).abs() < 1e-9));
        // No data: the file closed, the position is back at the start.
        let e = parse_line(r#"{"event":"property-change","id":2,"name":"time-pos"}"#);
        assert!(matches!(e.as_slice(), [Event::Time(t)] if *t == 0.0));
    }

    #[test]
    fn tags_in_any_case_and_station_title() {
        let e = parse_line(r#"{"event":"property-change","name":"metadata","data":{"ARTIST":"A","album":"B","icy-title":"A - Song"}}"#);
        assert!(matches!(&e[0], Event::Artist(a) if a == "A"));
        assert!(matches!(&e[1], Event::Album(a) if a == "B"));
        assert!(matches!(&e[2], Event::StreamTitle(a) if a == "A - Song"));
        let e = parse_line(r#"{"event":"property-change","name":"metadata","data":null}"#);
        assert!(matches!(&e[2], Event::StreamTitle(a) if a.is_empty()));
    }

    #[test]
    fn end_of_file() {
        let e = parse_line(r#"{"event":"end-file","reason":"eof"}"#);
        assert!(matches!(e.as_slice(), [Event::Ended { error: None }]));
        let e = parse_line(r#"{"event":"end-file","reason":"error","file_error":"loading failed"}"#);
        assert!(matches!(e.as_slice(), [Event::Ended { error: Some(m) }] if m == "loading failed"));
        // A stop we asked for must not move on to the next track.
        assert!(parse_line(r#"{"event":"end-file","reason":"stop"}"#).is_empty());
    }

    #[test]
    fn video_track_and_junk() {
        let e = parse_line(r#"{"event":"property-change","name":"vid","data":1}"#);
        assert!(matches!(e.as_slice(), [Event::HasVideo(true)]));
        let e = parse_line(r#"{"event":"property-change","name":"vid","data":false}"#);
        assert!(matches!(e.as_slice(), [Event::HasVideo(false)]));
        assert!(parse_line("not json").is_empty());
        assert!(parse_line(r#"{"error":"success"}"#).is_empty());
    }

    #[test]
    fn never_uses_the_users_mpv_folder() {
        let a = args(Path::new("/run/x.sock"));
        assert!(a.iter().any(|s| s.starts_with("--config-dir=") && s.contains("dev.lapinux.MediaPlayer")));
    }

    #[test]
    fn output_devices() {
        let line = r#"{"event":"property-change","id":9,"name":"audio-device-list","data":[{"name":"auto","description":"Autoselect device"},{"name":"pipewire/alsa_output.x","description":"Speakers"}]}"#;
        match parse_line(line).as_slice() {
            [Event::Devices(d)] => {
                assert_eq!(d.len(), 2);
                assert_eq!(d[1], ("pipewire/alsa_output.x".to_string(), "Speakers".to_string()));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(parse_line(r#"{"event":"property-change","name":"audio-device","data":"auto"}"#).as_slice(), [Event::Device(d)] if d == "auto"));
    }
}
