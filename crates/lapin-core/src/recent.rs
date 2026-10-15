// SPDX-License-Identifier: GPL-3.0-only
//! Recently opened files for the Start menu's "Recommended" section.
//!
//! GTK, Qt (via KDE's bridge) and many other apps record opened files in
//! `~/.local/share/recently-used.xbel`. Only the attributes we need are read;
//! a malformed file just yields fewer entries.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentFile {
    pub uri: String,
    pub path: PathBuf,
    pub name: String,
    /// RFC 3339 timestamp as written by the app; sorts correctly as text.
    pub modified: String,
}

pub fn default_path() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("recently-used.xbel"))
}

/// Most recent local files that still exist, newest first.
pub fn load(limit: usize) -> Vec<RecentFile> {
    let Some(path) = default_path() else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    parse(&text, limit, |p| p.exists())
}

pub fn parse(xml: &str, limit: usize, exists: impl Fn(&std::path::Path) -> bool) -> Vec<RecentFile> {
    let mut out: Vec<RecentFile> = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<bookmark ") {
        rest = &rest[start + "<bookmark ".len()..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        let (Some(href), modified) = (attr(tag, "href"), attr(tag, "modified").or_else(|| attr(tag, "added")))
        else {
            continue;
        };
        let Some(path) = file_uri_to_path(&href) else { continue };
        if !exists(&path) {
            continue;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.retain(|r| r.uri != href);
        out.push(RecentFile { uri: href, path, name, modified: modified.unwrap_or_default() });
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out.truncate(limit);
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')? + i;
    Some(unescape_xml(&tag[i..j]))
}

fn unescape_xml(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// `file:///home/me/My%20File.txt` -> `/home/me/My File.txt`. Non-local URIs are skipped.
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Only local files: "file:///path" or "file://localhost/path".
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

/// "Just now", "12m ago", "3h ago", "Yesterday", "4 days ago", or a date.
pub fn relative(modified: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(modified) else { return String::new() };
    let secs = (now - t.with_timezone(&chrono::Utc)).num_seconds();
    match secs {
        s if s < 60 => "Just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s if s < 2 * 86_400 => "Yesterday".into(),
        s if s < 7 * 86_400 => format!("{} days ago", s / 86_400),
        _ => t.format("%b %-d").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_times() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-15T12:00:00Z").unwrap().to_utc();
        assert_eq!(relative("2026-10-15T11:59:30Z", now), "Just now");
        assert_eq!(relative("2026-10-15T11:48:00.000000Z", now), "12m ago");
        assert_eq!(relative("2026-10-15T09:00:00Z", now), "3h ago");
        assert_eq!(relative("2026-10-14T08:00:00Z", now), "Yesterday");
        assert_eq!(relative("2026-10-11T12:00:00Z", now), "4 days ago");
        assert_eq!(relative("2026-09-01T12:00:00Z", now), "Sep 1");
        assert_eq!(relative("garbage", now), "");
    }

    const XBEL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
  <bookmark href="file:///home/me/Documents/Report%20Q3.odt" added="2026-10-01T09:00:00.000000Z" modified="2026-10-14T18:22:10.000000Z" visited="2026-10-14T18:22:10.000000Z">
    <info><metadata owner="http://freedesktop.org"><mime:mime-type type="application/vnd.oasis.opendocument.text"/></metadata></info>
  </bookmark>
  <bookmark href="file:///home/me/Pictures/cat.png" added="2026-10-12T10:00:00Z" modified="2026-10-12T10:00:00Z">
  </bookmark>
  <bookmark href="sftp://server/remote.txt" modified="2026-10-15T00:00:00Z"></bookmark>
  <bookmark href="file:///home/me/gone.txt" modified="2026-10-15T01:00:00Z"></bookmark>
  <bookmark href="file:///home/me/Tom%20%26%20Jerry.txt" modified="2026-10-13T00:00:00Z"></bookmark>
</xbel>"#;

    #[test]
    fn parses_sorts_filters() {
        let r = parse(XBEL, 10, |p| !p.ends_with("gone.txt"));
        let names: Vec<_> = r.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Report Q3.odt", "Tom & Jerry.txt", "cat.png"]);
        assert_eq!(r[0].path, PathBuf::from("/home/me/Documents/Report Q3.odt"));
    }

    #[test]
    fn limit_and_garbage() {
        assert_eq!(parse(XBEL, 1, |_| true).len(), 1);
        assert!(parse("<not xml", 5, |_| true).is_empty());
        assert!(parse("<bookmark href=\"file:///a", 5, |_| true).len() <= 1);
    }

    #[test]
    fn uri_decoding() {
        assert_eq!(file_uri_to_path("file:///a%2Fb/c%20d"), Some(PathBuf::from("/a/b/c d")));
        assert_eq!(file_uri_to_path("file://localhost/x"), Some(PathBuf::from("/x")));
        assert_eq!(file_uri_to_path("file://otherhost/x"), None);
        assert_eq!(file_uri_to_path("https://example.com"), None);
        assert_eq!(file_uri_to_path("file:///trailing%2"), Some(PathBuf::from("/trailing%2")));
    }
}
