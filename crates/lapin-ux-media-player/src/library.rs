// SPDX-License-Identifier: GPL-3.0-only
//! The library: files found in Music and Videos, radio stations, playlists.
//! All plain data and plain files, so it is tested without a window.
//! Everything Lapin Media Player stores lives in `dev.lapinux.MediaPlayer`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const AUDIO: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "wma", "ape", "mka", "aiff"];
const VIDEO: &[&str] = &["mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "flv", "mpg", "mpeg", "ts", "ogv"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Audio,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub kind: Kind,
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if AUDIO.contains(&ext.as_str()) {
        Some(Kind::Audio)
    } else if VIDEO.contains(&ext.as_str()) {
        Some(Kind::Video)
    } else {
        None
    }
}

/// "03 - Song", "03. Song", "03_Song" -> "Song".
fn clean_title(stem: &str) -> String {
    let t = stem.trim_start_matches(|c: char| c.is_ascii_digit());
    let t = t.trim_start_matches(|c: char| matches!(c, ' ' | '-' | '.' | '_'));
    let t = if t.is_empty() { stem } else { t };
    t.replace('_', " ").trim().to_string()
}

/// Artist and album come from the folders (`Artist/Album/Song`), so no tag
/// reader is needed for a first stage. Files straight in the root have neither.
pub fn track_from_path(root: &Path, path: &Path) -> Option<Track> {
    let kind = kind_of(path)?;
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let (artist, album) = match (kind, parts.len()) {
        (Kind::Audio, n) if n >= 3 => (parts[0].clone(), parts[1].clone()),
        (Kind::Audio, 2) => (parts[0].clone(), String::new()),
        _ => (String::new(), String::new()),
    };
    Some(Track { path: path.to_path_buf(), title: clean_title(&stem), artist, album, kind })
}

pub fn scan(root: &Path) -> Vec<Track> {
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<Track>) {
        if depth > 8 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            // symlinks are not followed: no loops, nothing outside the folder
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                if !e.file_name().to_string_lossy().starts_with('.') {
                    walk(root, &p, depth + 1, out);
                }
            } else if ft.is_file() {
                if let Some(t) = track_from_path(root, &p) {
                    out.push(t);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out
}

/// Distinct non-empty values, in first-seen order.
#[allow(dead_code)]
pub fn distinct<'a>(tracks: &'a [Track], f: impl Fn(&'a Track) -> &'a str) -> Vec<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    for t in tracks {
        let v = f(t);
        if !v.is_empty() && !seen.contains(&v) {
            seen.push(v);
        }
    }
    seen
}

pub fn matches(t: &Track, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || [&t.title, &t.artist, &t.album].iter().any(|s| s.to_lowercase().contains(&q))
}

// ---- playlists (.m3u, .m3u8, .pls) ----

pub fn parse_playlist(text: &str, base: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        let item = if let Some(rest) = line.strip_prefix("File").and_then(|r| r.split_once('=')) {
            rest.1.trim() // .pls: File1=...
        } else if line.is_empty() || line.starts_with('#') || line.starts_with('[') || line.contains('=') && !line.contains("://") {
            continue;
        } else {
            line
        };
        if item.is_empty() {
            continue;
        }
        if item.contains("://") || Path::new(item).is_absolute() {
            out.push(item.to_string());
        } else {
            out.push(base.join(item).to_string_lossy().into_owned());
        }
    }
    out
}

pub fn write_m3u(items: &[(String, String)]) -> String {
    let mut s = String::from("#EXTM3U\n");
    for (title, target) in items {
        s.push_str(&format!("#EXTINF:-1,{}\n{}\n", title.replace('\n', " "), target));
    }
    s
}

// ---- radio stations ----

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Station {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub logo: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub favourite: bool,
}

pub fn stations_path() -> PathBuf {
    crate::engine::config_dir().join("stations.json")
}

pub fn load_stations() -> Vec<Station> {
    std::fs::read_to_string(stations_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_stations(list: &[Station]) -> std::io::Result<()> {
    let path = stations_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(list).unwrap_or_else(|_| "[]".into()))?;
    std::fs::rename(tmp, path)
}

/// A stream URL needs a scheme mpv can open.
pub fn valid_stream_url(url: &str) -> bool {
    let u = url.trim();
    ["http://", "https://", "mms://", "rtmp://", "rtsp://"].iter().any(|p| u.len() > p.len() && u.starts_with(p))
}

/// Stations from an `.m3u` (the `#EXTINF` name) or a `.json` export.
pub fn import_stations(text: &str) -> Vec<Station> {
    if let Ok(list) = serde_json::from_str::<Vec<Station>>(text) {
        return list.into_iter().filter(|s| valid_stream_url(&s.url)).collect();
    }
    let mut out = Vec::new();
    let mut name = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            name = info.split_once(',').map(|x| x.1.trim().to_string()).unwrap_or_default();
        } else if valid_stream_url(line) {
            let n = if name.is_empty() { line.to_string() } else { std::mem::take(&mut name) };
            out.push(Station { name: n, url: line.to_string(), ..Default::default() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_lose_track_numbers() {
        assert_eq!(clean_title("03 - Blue Monday"), "Blue Monday");
        assert_eq!(clean_title("07_my_song"), "my song");
        assert_eq!(clean_title("1999"), "1999"); // all digits: keep
    }

    #[test]
    fn artist_and_album_from_folders() {
        let root = Path::new("/m");
        let t = track_from_path(root, Path::new("/m/Band/Record/01 - Intro.flac")).unwrap();
        assert_eq!((t.artist.as_str(), t.album.as_str(), t.title.as_str()), ("Band", "Record", "Intro"));
        let t = track_from_path(root, Path::new("/m/Loose.mp3")).unwrap();
        assert!(t.artist.is_empty() && t.album.is_empty());
        let t = track_from_path(root, Path::new("/m/Films/Holiday/clip.mkv")).unwrap();
        assert_eq!(t.kind, Kind::Video);
        assert!(t.artist.is_empty());
        assert!(track_from_path(root, Path::new("/m/notes.txt")).is_none());
    }

    #[test]
    fn search_is_case_blind() {
        let t = track_from_path(Path::new("/m"), Path::new("/m/Band/Record/01 Intro.mp3")).unwrap();
        assert!(matches(&t, "band") && matches(&t, " INTRO ") && matches(&t, ""));
        assert!(!matches(&t, "zzz"));
    }

    #[test]
    fn playlists_round_trip() {
        let text = write_m3u(&[("A".into(), "/m/a.mp3".into()), ("B".into(), "http://x/y".into())]);
        let items = parse_playlist(&text, Path::new("/base"));
        assert_eq!(items, vec!["/m/a.mp3", "http://x/y"]);
        let rel = parse_playlist("#EXTM3U\nsong.mp3\n", Path::new("/base"));
        assert_eq!(rel, vec!["/base/song.mp3"]);
        let pls = parse_playlist("[playlist]\nFile1=/m/a.mp3\nTitle1=A\nNumberOfEntries=1\n", Path::new("/b"));
        assert_eq!(pls, vec!["/m/a.mp3"]);
    }

    #[test]
    fn stations_import_and_validate() {
        let m3u = "#EXTM3U\n#EXTINF:-1,Jazz FM\nhttp://a/stream\nnot a url\nhttps://b/s\n";
        let list = import_stations(m3u);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Jazz FM");
        assert_eq!(list[1].name, "https://b/s");
        let json = serde_json::to_string(&list).unwrap();
        assert_eq!(import_stations(&json), list);
        assert!(!valid_stream_url("http://") && !valid_stream_url("ftp://x") && valid_stream_url(" https://x "));
    }

    #[test]
    fn scan_finds_files_and_skips_hidden() {
        let dir = std::env::temp_dir().join(format!("lapin-mp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("A/B")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join("A/B/01 x.mp3"), b"").unwrap();
        std::fs::write(dir.join(".hidden/y.mp3"), b"").unwrap();
        std::fs::write(dir.join("readme.txt"), b"").unwrap();
        let found = scan(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].artist, "A");
    }
}
