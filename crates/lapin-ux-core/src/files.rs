// SPDX-License-Identifier: GPL-3.0-only
//! File search for the Start menu: a small in-memory index of the user's
//! folders (Desktop, Documents, Downloads, Music, Pictures, Videos and the
//! top level of home), rebuilt in the background.
//!
//! Like the benchmark's default "Classic" indexing, it covers the user's
//! libraries rather than the whole disk, so it stays fast and private.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::search::normalize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub modified: Option<SystemTime>,
    /// Normalized name, for matching.
    key: String,
}

#[derive(Debug, Clone, Default)]
pub struct FileIndex {
    pub entries: Vec<FileEntry>,
    pub built: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_depth: usize,
    pub max_entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_depth: 8, max_entries: 60_000 }
    }
}

/// Folders that are never worth descending into.
const SKIP_DIRS: &[&str] = &["node_modules", "target", "__pycache__", "venv", ".venv", "site-packages", "build", "dist"];

/// The user's library folders (XDG user dirs), plus home itself (top level only).
pub fn default_roots(home: &Path, user_dirs: &[PathBuf]) -> Vec<(PathBuf, usize)> {
    let mut roots: Vec<(PathBuf, usize)> = user_dirs
        .iter()
        .filter(|d| d.as_path() != home && d.starts_with(home))
        .map(|d| (d.clone(), usize::MAX))
        .collect();
    roots.push((home.to_path_buf(), 1));
    roots
}

/// XDG user dirs from `~/.config/user-dirs.dirs`, falling back to the
/// English defaults.
pub fn user_dirs(home: &Path, config_home: &Path) -> Vec<PathBuf> {
    let keys = ["DESKTOP", "DOCUMENTS", "DOWNLOAD", "MUSIC", "PICTURES", "VIDEOS"];
    let defaults = ["Desktop", "Documents", "Downloads", "Music", "Pictures", "Videos"];
    let text = std::fs::read_to_string(config_home.join("user-dirs.dirs")).unwrap_or_default();
    keys.iter()
        .zip(defaults)
        .map(|(k, d)| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("XDG_{k}_DIR=")))
                .map(|v| v.trim().trim_matches('"').replace("$HOME", &home.to_string_lossy()))
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(d))
        })
        .collect()
}

impl FileIndex {
    /// Walk `roots` (path, max depth). Hidden files and folders are skipped.
    pub fn build(roots: &[(PathBuf, usize)], limits: Limits) -> FileIndex {
        let mut entries = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (root, depth) in roots {
            let depth = (*depth).min(limits.max_depth);
            walk(root, depth, &mut entries, &mut seen, limits.max_entries);
        }
        FileIndex { entries, built: Some(SystemTime::now()) }
    }

    pub fn age(&self) -> Option<std::time::Duration> {
        self.built.and_then(|b| b.elapsed().ok())
    }

    /// Best matches first. Name starts > a word in the name starts > contains;
    /// then recently modified first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&FileEntry> {
        let q = normalize(query);
        let q = q.trim();
        if q.chars().count() < 2 {
            return Vec::new();
        }
        let mut hits: Vec<(u32, Option<SystemTime>, &FileEntry)> = self
            .entries
            .iter()
            .filter_map(|e| score(q, &e.key).map(|s| (s, e.modified, e)))
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.name.cmp(&b.2.name)));
        hits.into_iter().take(limit).map(|h| h.2).collect()
    }
}

fn score(q: &str, key: &str) -> Option<u32> {
    let stem = key.rsplit_once('.').map_or(key, |(s, _)| s);
    if stem == q || key == q {
        Some(100)
    } else if key.starts_with(q) {
        Some(80)
    } else if key.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(q)) {
        Some(60)
    } else if q.len() >= 3 && key.contains(q) {
        Some(40)
    } else {
        None
    }
}

fn walk(
    dir: &Path,
    depth: usize,
    out: &mut Vec<FileEntry>,
    seen: &mut std::collections::HashSet<PathBuf>,
    max: usize,
) {
    if depth == 0 || out.len() >= max {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut subdirs = Vec::new();
    for e in rd.flatten() {
        if out.len() >= max {
            return;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let path = e.path();
        if !seen.insert(path.clone()) {
            continue;
        }
        let modified = e.metadata().ok().and_then(|m| m.modified().ok());
        let is_dir = ft.is_dir();
        out.push(FileEntry { key: normalize(&name), name: name.clone(), path: path.clone(), is_dir, modified });
        if is_dir && !SKIP_DIRS.contains(&name.as_str()) {
            subdirs.push(path);
        }
    }
    for d in subdirs {
        walk(&d, depth - 1, out, seen, max);
    }
}

/// Fill a web-search URL template (`{}` = the query), percent-encoding the query.
pub fn web_search_url(template: &str, query: &str) -> String {
    let mut enc = String::new();
    for b in query.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => enc.push(b as char),
            b' ' => enc.push('+'),
            _ => enc.push_str(&format!("%{b:02X}")),
        }
    }
    if template.contains("{}") { template.replace("{}", &enc) } else { format!("{template}{enc}") }
}

pub const DEFAULT_WEB_SEARCH: &str = "https://duckduckgo.com/?q={}";

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (PathBuf, FileIndex) {
        let base = std::env::temp_dir().join(format!("lapin-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["Documents/Taxes 2025", "Documents/.secret", "Pictures/Holiday", "Documents/proj/node_modules/pkg", "Music"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        for f in [
            "Documents/Taxes 2025/return.pdf",
            "Documents/budget.ods",
            "Documents/.secret/budget-old.ods",
            "Documents/proj/node_modules/pkg/budget.js",
            "Pictures/Holiday/beach.png",
            "Pictures/my-budget-chart.png",
            "notes.txt",
        ] {
            std::fs::write(base.join(f), b"x").unwrap();
        }
        let dirs = user_dirs(&base, &base.join("no-config"));
        let idx = FileIndex::build(&default_roots(&base, &dirs), Limits::default());
        (base, idx)
    }

    #[test]
    fn indexes_and_ranks() {
        let (base, idx) = tree();
        let names: Vec<&str> = idx.search("budget", 10).iter().map(|e| e.name.as_str()).collect();
        // Hidden folders and node_modules are skipped; exact stem beats word match.
        assert_eq!(names, vec!["budget.ods", "my-budget-chart.png"]);
        assert_eq!(idx.search("tax", 5)[0].name, "Taxes 2025");
        assert!(idx.search("tax", 5)[0].is_dir);
        assert_eq!(idx.search("notes", 5)[0].path, base.join("notes.txt"));
        assert!(idx.search("b", 5).is_empty(), "single letters don't search files");
        assert!(idx.search("each", 5).iter().any(|e| e.name == "beach.png"));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn user_dirs_file() {
        let base = std::env::temp_dir().join(format!("lapin-ud-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("user-dirs.dirs"), "XDG_DOCUMENTS_DIR=\"$HOME/Dokumente\"\n").unwrap();
        let d = user_dirs(Path::new("/home/ana"), &base);
        assert_eq!(d[1], PathBuf::from("/home/ana/Dokumente"));
        assert_eq!(d[2], PathBuf::from("/home/ana/Downloads"));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn web_url() {
        assert_eq!(web_search_url(DEFAULT_WEB_SEARCH, "rust & iced"), "https://duckduckgo.com/?q=rust+%26+iced");
        assert_eq!(web_search_url("https://example.org/search?q=", "é"), "https://example.org/search?q=%C3%A9");
    }
}

/// Commands that search file contents with LocalSearch (formerly Tracker),
/// newest name first; whichever is installed is used.
pub fn content_search_commands(query: &str, limit: usize) -> Vec<Vec<String>> {
    let args = |bin: &str| {
        let mut v: Vec<String> =
            [bin, "search", "--disable-color", "--disable-snippets", "-l", &limit.to_string()].iter().map(|s| s.to_string()).collect();
        v.extend(query.split_whitespace().map(str::to_string));
        v
    };
    vec![args("localsearch"), args("tracker3")]
}

/// Local files in a content search's output (any `file://` URI on a line).
pub fn content_hits(output: &str) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for word in output.split_whitespace() {
        // Strip any leftover colour codes around the URI.
        let Some(start) = word.find("file://") else { continue };
        let uri: String = word[start..].chars().take_while(|c| !c.is_control()).collect();
        if let Some(p) = crate::recent::file_uri_to_path(&uri)
            && !out.contains(&p)
        {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod content_tests {
    #[test]
    fn hits() {
        let out = "Files:\n  \u{1b}[32mfile:///home/u/a%20b.txt\u{1b}[0m\n  file:///home/u/c.odt\n  file:///home/u/c.odt\n";
        assert_eq!(super::content_hits(out), vec![std::path::PathBuf::from("/home/u/a b.txt"), "/home/u/c.odt".into()]);
        assert_eq!(super::content_search_commands("tax 2025", 5)[0].last().unwrap(), "2025");
    }
}
