// SPDX-License-Identifier: GPL-3.0-only
//! File tags and cover art. Tags are read with `ffprobe` and covers made with
//! `ffmpeg` (both come with mpv's own dependencies), only on this computer and
//! only for files in the library. Results are cached in the player's folder, so
//! a second start reads nothing again unless a file changed.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::library::{Kind, Track};

const PROBE_SECONDS: u64 = 10;
const COVER_SIDE: u32 = 256;
/// Names a folder image usually has, in the order they are tried.
const FOLDER_IMAGES: &[&str] = &["cover", "folder", "front", "album", "albumart"];
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tags {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    /// Modified time (seconds) and size when the tags were read.
    mtime: u64,
    size: u64,
    tags: Tags,
}

fn cache_path() -> PathBuf {
    crate::engine::config_dir().join("tags.json")
}

fn covers_dir() -> PathBuf {
    crate::engine::config_dir().join("covers")
}

fn stamp(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let secs = m.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    Some((secs, m.len()))
}

/// Same input, same name, on every run (the standard hasher is not promised to be).
fn fnv(text: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Tags from `ffprobe -print_format json`. Tag names differ in case between
/// formats (TITLE, Title, title), so they are compared in lower case.
pub fn parse_tags(json: &str) -> Option<Tags> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let map = v.get("format")?.get("tags")?.as_object()?;
    let get = |k: &str| -> String {
        map.iter()
            .find(|(name, _)| name.to_ascii_lowercase() == k)
            .and_then(|(_, val)| val.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let artist = {
        let a = get("artist");
        if a.is_empty() { get("album_artist") } else { a }
    };
    Some(Tags { title: get("title"), artist, album: get("album") })
}

/// Runs a command with a time limit and returns what it printed (at most 1 MB).
/// A broken file must never leave the player waiting.
fn run_limited(mut cmd: Command, seconds: u64) -> Option<(bool, Vec<u8>)> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut out).take(1 << 20).read_to_end(&mut buf);
        // keep draining so the child never blocks on a full pipe
        let _ = std::io::copy(&mut out, &mut std::io::sink());
        buf
    });
    let start = std::time::Instant::now();
    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if start.elapsed() < Duration::from_secs(seconds) => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let buf = reader.join().unwrap_or_default();
    Some((ok, buf))
}

fn probe(path: &Path) -> Option<Tags> {
    let mut cmd = Command::new("ffprobe");
    cmd.args(["-v", "quiet", "-protocol_whitelist", "file", "-print_format", "json", "-show_entries", "format_tags", "-i"]).arg(path);
    let (ok, out) = run_limited(cmd, PROBE_SECONDS)?;
    if !ok {
        return None;
    }
    parse_tags(&String::from_utf8_lossy(&out))
}

fn load_cache() -> HashMap<String, Entry> {
    std::fs::read_to_string(cache_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_cache(map: &HashMap<String, Entry>) {
    let dir = crate::engine::config_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let tmp = dir.join("tags.json.tmp");
    if let Ok(s) = serde_json::to_string(map) {
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, cache_path());
        }
    }
}

/// Fills in title, artist and album from the files' own tags. A tag that is
/// missing keeps what the folder names gave. Only music is read: for videos the
/// file name is already the title.
pub fn apply(mut tracks: Vec<Track>) -> Vec<Track> {
    let mut cache = load_cache();
    let mut todo: Vec<(usize, PathBuf)> = Vec::new();
    for (i, t) in tracks.iter().enumerate() {
        if t.kind != Kind::Audio {
            continue;
        }
        let key = t.path.to_string_lossy().into_owned();
        let fresh = match (cache.get(&key), stamp(&t.path)) {
            (Some(e), Some((m, s))) => e.mtime == m && e.size == s,
            _ => false,
        };
        if !fresh {
            todo.push((i, t.path.clone()));
        }
    }
    if !todo.is_empty() {
        let found = probe_many(&todo);
        for (path, tags) in found {
            if let Some((mtime, size)) = stamp(&path) {
                cache.insert(path.to_string_lossy().into_owned(), Entry { mtime, size, tags });
            }
        }
        // forget files that are gone, so the cache does not grow for ever
        let live: std::collections::HashSet<String> = tracks.iter().map(|t| t.path.to_string_lossy().into_owned()).collect();
        cache.retain(|k, _| live.contains(k));
        save_cache(&cache);
    }
    for t in tracks.iter_mut().filter(|t| t.kind == Kind::Audio) {
        if let Some(e) = cache.get(t.path.to_string_lossy().as_ref()) {
            merge(t, &e.tags);
        }
    }
    tracks
}

pub fn merge(t: &mut Track, tags: &Tags) {
    if !tags.title.is_empty() {
        t.title = tags.title.clone();
    }
    if !tags.artist.is_empty() {
        t.artist = tags.artist.clone();
    }
    if !tags.album.is_empty() {
        t.album = tags.album.clone();
    }
}

/// A few files at a time. A file that cannot be read is cached as empty, so it
/// is not tried again until it changes.
fn probe_many(todo: &[(usize, PathBuf)]) -> Vec<(PathBuf, Tags)> {
    let workers = std::thread::available_parallelism().map(|n| n.get().min(4)).unwrap_or(2);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::with_capacity(todo.len()));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((_, path)) = todo.get(i) else { break };
                let tags = probe(path).unwrap_or_default();
                if let Ok(mut r) = results.lock() {
                    r.push((path.clone(), tags));
                }
            });
        }
    });
    results.into_inner().unwrap_or_default()
}

/// The folder image next to a song, when there is one.
pub fn folder_image(dir: &Path) -> Option<PathBuf> {
    let names: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| (e.file_name().to_string_lossy().to_lowercase(), e.path()))
        .collect();
    for base in FOLDER_IMAGES {
        for ext in IMAGE_EXTS {
            let want = format!("{base}.{ext}");
            if let Some((_, p)) = names.iter().find(|(n, _)| *n == want) {
                return Some(p.clone());
            }
        }
    }
    None
}

fn scale(src: &Path, out: &Path) -> bool {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-nostdin", "-v", "error", "-y", "-protocol_whitelist", "file", "-i"])
        .arg(src)
        .args(["-map", "0:v:0", "-an", "-frames:v", "1", "-vf"])
        .arg(format!("scale={COVER_SIDE}:{COVER_SIDE}:force_original_aspect_ratio=decrease"))
        .arg(out);
    matches!(run_limited(cmd, PROBE_SECONDS), Some((true, _))) && out.is_file()
}

/// A small picture for one folder: the folder image, or the picture embedded in
/// its first song. Made once and kept; a folder without any is remembered too.
pub fn cover_for(dir: &Path, first_song: &Path) -> Option<PathBuf> {
    let source = folder_image(dir).unwrap_or_else(|| first_song.to_path_buf());
    let (mtime, size) = stamp(&source)?;
    let name = format!("{:016x}", fnv(&format!("{}|{mtime}|{size}", source.to_string_lossy())));
    let png = covers_dir().join(format!("{name}.png"));
    let none = covers_dir().join(format!("{name}.none"));
    if png.is_file() {
        return Some(png);
    }
    if none.is_file() {
        return None;
    }
    std::fs::create_dir_all(covers_dir()).ok()?;
    if scale(&source, &png) {
        Some(png)
    } else {
        let _ = std::fs::remove_file(&png);
        let _ = std::fs::write(&none, b"");
        None
    }
}

/// One cover per folder that holds music, looked up in a few at a time.
pub fn covers(tracks: &[Track]) -> Vec<(PathBuf, PathBuf)> {
    let mut dirs: Vec<(PathBuf, PathBuf)> = Vec::new();
    for t in tracks.iter().filter(|t| t.kind == Kind::Audio) {
        if let Some(d) = t.path.parent() {
            if !dirs.iter().any(|(x, _)| x == d) {
                dirs.push((d.to_path_buf(), t.path.clone()));
            }
        }
    }
    let workers = std::thread::available_parallelism().map(|n| n.get().min(4)).unwrap_or(2);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((dir, song)) = dirs.get(i) else { break };
                if let Some(png) = cover_for(dir, song) {
                    if let Ok(mut r) = results.lock() {
                        r.push((dir.clone(), png));
                    }
                }
            });
        }
    });
    results.into_inner().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_read_whatever_their_case() {
        let t = parse_tags(r#"{"format":{"tags":{"TITLE":" Song ","Artist":"Band","ALBUM":"Record"}}}"#).unwrap();
        assert_eq!(t, Tags { title: "Song".into(), artist: "Band".into(), album: "Record".into() });
    }

    #[test]
    fn album_artist_is_used_when_there_is_no_artist() {
        let t = parse_tags(r#"{"format":{"tags":{"album_artist":"Group"}}}"#).unwrap();
        assert_eq!(t.artist, "Group");
    }

    #[test]
    fn no_tags_or_bad_text_gives_nothing() {
        assert!(parse_tags(r#"{"format":{}}"#).is_none());
        assert!(parse_tags("not json").is_none());
    }

    #[test]
    fn a_missing_tag_keeps_the_folder_name() {
        let mut t = Track { path: "/m/A/B/x.mp3".into(), title: "x".into(), artist: "A".into(), album: "B".into(), kind: Kind::Audio };
        merge(&mut t, &Tags { title: "Real".into(), ..Default::default() });
        assert_eq!((t.title.as_str(), t.artist.as_str(), t.album.as_str()), ("Real", "A", "B"));
    }

    #[test]
    fn the_cache_name_is_stable() {
        assert_eq!(fnv("a"), fnv("a"));
        assert_ne!(fnv("a"), fnv("b"));
        assert_eq!(fnv(""), 0xcbf29ce484222325);
    }

    #[test]
    fn folder_image_prefers_cover_and_ignores_case() {
        let d = std::env::temp_dir().join(format!("lapin-mp-cover-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Folder.JPG"), b"x").unwrap();
        std::fs::write(d.join("Cover.png"), b"x").unwrap();
        assert_eq!(folder_image(&d).unwrap().file_name().unwrap(), "Cover.png");
        std::fs::remove_file(d.join("Cover.png")).unwrap();
        assert_eq!(folder_image(&d).unwrap().file_name().unwrap(), "Folder.JPG");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_command_that_hangs_is_stopped() {
        let mut c = Command::new("sleep");
        c.arg("30");
        let start = std::time::Instant::now();
        let (ok, _) = run_limited(c, 1).unwrap();
        assert!(!ok);
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
