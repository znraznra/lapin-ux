// SPDX-License-Identifier: GPL-3.0-only
//! The library: files found in Music and Videos, radio stations, playlists.
//! All plain data and plain files, so it is tested without a window.
//! Everything Lapin Media Player stores lives in `dev.lapinux.MediaPlayer`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const AUDIO: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "wma", "ape", "mka", "aiff"];
pub const VIDEO: &[&str] = &["mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "flv", "mpg", "mpeg", "ts", "ogv"];

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

/// Files found, and how many folders or entries could not be read.
pub fn scan_report(root: &Path) -> (Vec<Track>, usize) {
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<Track>, unreadable: &mut usize) {
        if depth > 8 {
            return;
        }
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => {
                *unreadable += 1;
                return;
            }
        };
        let mut entries = Vec::new();
        for e in rd {
            match e {
                Ok(e) => entries.push(e),
                Err(_) => *unreadable += 1,
            }
        }
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            // symlinks are not followed: no loops, nothing outside the folder
            let Ok(ft) = e.file_type() else {
                *unreadable += 1;
                continue;
            };
            let p = e.path();
            if ft.is_dir() {
                if !e.file_name().to_string_lossy().starts_with('.') {
                    walk(root, &p, depth + 1, out, unreadable);
                }
            } else if ft.is_file() {
                // mpv is given the name as text; one that is not valid text would
                // be changed on the way, so it is left out and counted instead.
                if p.to_str().is_none() {
                    if kind_of(&p).is_some() {
                        *unreadable += 1;
                    }
                    continue;
                }
                if let Some(t) = track_from_path(root, &p) {
                    out.push(t);
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut unreadable = 0;
    walk(root, root, 0, &mut out, &mut unreadable);
    (out, unreadable)
}

pub fn scan(root: &Path) -> Vec<Track> {
    scan_report(root).0
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

/// Targets of a playlist. Local paths are made absolute against `base`; an
/// address is kept only when it is one of the stream kinds `valid_stream_url`
/// accepts, because mpv trusts a direct load of any protocol (a playlist must
/// not be able to open `avdevice://` or similar).
pub fn parse_playlist(text: &str, base: &Path) -> Vec<String> {
    // `.pls` files say so; only then do `File1=` and `Title1=` lines mean anything.
    let pls = text.lines().any(|l| l.trim().trim_start_matches('\u{feff}').eq_ignore_ascii_case("[playlist]"));
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let item = if pls {
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim();
            let is_file = key.get(..4).is_some_and(|k| k.eq_ignore_ascii_case("file")) && key.len() > 4 && key[4..].chars().all(|c| c.is_ascii_digit());
            if !is_file {
                continue;
            }
            value.trim()
        } else {
            line
        };
        if item.is_empty() {
            continue;
        }
        if item.contains("://") {
            if valid_stream_url(item) {
                out.push(item.to_string());
            }
        } else if Path::new(item).is_absolute() {
            out.push(item.to_string());
        } else {
            out.push(base.join(item).to_string_lossy().into_owned());
        }
    }
    out
}

/// An `.m3u` for the given (title, target) pairs. A line-based format cannot
/// hold a line break, so such a name is refused instead of written wrongly.
pub fn write_m3u(items: &[(String, String)]) -> Result<String, String> {
    let mut s = String::from("#EXTM3U\n");
    for (title, target) in items {
        if target.contains(['\n', '\r']) {
            return Err(format!("{title} has a line break in its name, which a playlist file cannot hold."));
        }
        s.push_str(&format!("#EXTINF:-1,{}\n{}\n", title.replace(['\n', '\r'], " "), target));
    }
    Ok(s)
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

/// The saved stations. A missing file is a first launch (empty list); a file
/// that cannot be read or understood is an error, so it is never overwritten
/// by an empty list.
pub fn load_stations() -> Result<Vec<Station>, String> {
    match std::fs::read_to_string(stations_path()) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("stations.json could not be read ({e})")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("stations.json could not be opened ({e})")),
    }
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
    fn playlists_keep_paths_and_refuse_odd_targets() {
        // `=` inside an M3U path is part of the name, not a PLS key.
        let items = parse_playlist("#EXTM3U\n/Music/A=B.mp3\n", Path::new("/b"));
        assert_eq!(items, vec!["/Music/A=B.mp3".to_string()]);
        // Only stream addresses come through; other protocols are dropped.
        let items = parse_playlist("avdevice://x11grab\nytdl://a\nhttps://r.example/live\n", Path::new("/b"));
        assert_eq!(items, vec!["https://r.example/live".to_string()]);
        // A name with a line break cannot be written to a line-based file.
        assert!(write_m3u(&[("A".into(), "/m/a\nb.mp3".into())]).is_err());
    }

    #[test]
    fn missing_stations_file_is_empty_but_broken_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("lapin-stations-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: tests in this module do not read the environment concurrently.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        assert_eq!(load_stations().unwrap(), Vec::new());
        std::fs::create_dir_all(crate::engine::config_dir()).unwrap();
        std::fs::write(stations_path(), "{ not json").unwrap();
        assert!(load_stations().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pls_keys_with_unicode_do_not_panic() {
        let t = "[playlist]\nFile1=/a.mp3\nファイル1=/b.mp3\nfé=/c.mp3\n";
        assert_eq!(parse_playlist(t, Path::new("/m")), vec!["/a.mp3".to_string()]);
    }

    #[test]
    fn playlists_round_trip() {
        let text = write_m3u(&[("A".into(), "/m/a.mp3".into()), ("B".into(), "http://x/y".into())]).unwrap();
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
