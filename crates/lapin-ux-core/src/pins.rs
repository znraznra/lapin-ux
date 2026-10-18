// SPDX-License-Identifier: GPL-3.0-only
//! What's pinned to Start: apps, files and folders, websites, and folders
//! of apps (made by dragging one pinned app onto another).
//!
//! Stored as strings so older configs (plain app ids) keep working:
//! `firefox`, `path:/home/me/Report.odt`, `site:Name\turl`,
//! `folder:Name\tapp1\tapp2`.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pin {
    App(String),
    Path(PathBuf),
    Site { name: String, url: String },
    Folder { name: String, apps: Vec<String> },
}

impl Pin {
    pub fn decode(s: &str) -> Pin {
        if let Some(p) = s.strip_prefix("path:") {
            return Pin::Path(PathBuf::from(p));
        }
        if let Some(rest) = s.strip_prefix("site:") {
            let (name, url) = rest.split_once('\t').unwrap_or((rest, rest));
            return Pin::Site { name: name.to_string(), url: url.to_string() };
        }
        if let Some(rest) = s.strip_prefix("folder:") {
            let mut parts = rest.split('\t');
            let name = parts.next().unwrap_or_default().to_string();
            return Pin::Folder { name, apps: parts.filter(|a| !a.is_empty()).map(str::to_string).collect() };
        }
        Pin::App(s.to_string())
    }

    pub fn encode(&self) -> String {
        match self {
            Pin::App(id) => id.clone(),
            Pin::Path(p) => format!("path:{}", p.display()),
            Pin::Site { name, url } => format!("site:{}\t{}", clean(name), url),
            Pin::Folder { name, apps } => {
                let mut s = format!("folder:{}", clean(name));
                for a in apps {
                    s.push('\t');
                    s.push_str(a);
                }
                s
            }
        }
    }

    /// Display name for non-app pins (apps are named by the catalog).
    pub fn label(&self) -> String {
        match self {
            Pin::App(id) => id.clone(),
            Pin::Path(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()),
            Pin::Site { name, .. } | Pin::Folder { name, .. } => name.clone(),
        }
    }
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n'], " ")
}

/// A website pin from a URL: named after its host ("www." dropped).
pub fn site(url: &str) -> Option<Pin> {
    let url = url.trim();
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.strip_prefix("www.").unwrap_or(host);
    (!host.is_empty()).then(|| Pin::Site { name: host.to_string(), url: url.to_string() })
}

/// The pinned list, decoded.
pub fn decode_all(list: &[String]) -> Vec<Pin> {
    list.iter().map(|s| Pin::decode(s)).collect()
}

pub fn encode_all(pins: &[Pin]) -> Vec<String> {
    pins.iter().map(Pin::encode).collect()
}

/// Is this app pinned (on its own or inside a folder)?
pub fn has_app(pins: &[Pin], app: &str) -> bool {
    pins.iter().any(|p| match p {
        Pin::App(a) => a == app,
        Pin::Folder { apps, .. } => apps.iter().any(|a| a == app),
        _ => false,
    })
}

/// Unpin an app wherever it is; a folder left with one app turns back
/// into that app, an empty one disappears.
pub fn remove_app(pins: &mut Vec<Pin>, app: &str) {
    pins.retain(|p| p != &Pin::App(app.to_string()));
    for p in pins.iter_mut() {
        if let Pin::Folder { apps, .. } = p {
            apps.retain(|a| a != app);
        }
    }
    normalize(pins);
}

fn normalize(pins: &mut Vec<Pin>) {
    for p in pins.iter_mut() {
        if let Pin::Folder { apps, .. } = p
            && apps.len() == 1
        {
            *p = Pin::App(apps[0].clone());
        }
    }
    pins.retain(|p| !matches!(p, Pin::Folder { apps, .. } if apps.is_empty()));
}

/// Where a dragged Start tile was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drop {
    /// Onto another tile: make (or add to) a folder.
    Onto(usize),
    /// Between tiles: move so it ends up at this index.
    Before(usize),
}

/// Apply a drag of pin `from`. Returns false if nothing changed.
pub fn drop_pin(pins: &mut Vec<Pin>, from: usize, to: Drop) -> bool {
    if from >= pins.len() {
        return false;
    }
    match to {
        Drop::Before(i) => {
            let i = i.min(pins.len());
            if i == from || i == from + 1 {
                return false;
            }
            let p = pins.remove(from);
            pins.insert(if i > from { i - 1 } else { i }, p);
            true
        }
        Drop::Onto(i) => {
            if i == from || i >= pins.len() {
                return false;
            }
            let Pin::App(dragged) = pins[from].clone() else { return false };
            match &mut pins[i] {
                Pin::App(target) => {
                    let target = target.clone();
                    pins[i] = Pin::Folder { name: "Folder".into(), apps: vec![target, dragged] };
                }
                Pin::Folder { apps, .. } => apps.push(dragged),
                _ => return false,
            }
            pins.remove(from);
            true
        }
    }
}

/// Take app `app` out of folder `folder`, placing it right after the folder.
pub fn take_out(pins: &mut Vec<Pin>, folder: usize, app: &str) {
    let Some(Pin::Folder { apps, .. }) = pins.get_mut(folder) else { return };
    let before = apps.len();
    apps.retain(|a| a != app);
    if apps.len() == before {
        return;
    }
    pins.insert(folder + 1, Pin::App(app.to_string()));
    normalize(pins);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let pins = vec![
            Pin::App("firefox".into()),
            Pin::Path("/home/me/My Report.odt".into()),
            Pin::Site { name: "example.org".into(), url: "https://example.org/a?b".into() },
            Pin::Folder { name: "Work\tstuff".into(), apps: vec!["a".into(), "b".into()] },
        ];
        let back = decode_all(&encode_all(&pins));
        assert_eq!(back[..3], pins[..3]);
        assert_eq!(back[3], Pin::Folder { name: "Work stuff".into(), apps: vec!["a".into(), "b".into()] });
        assert_eq!(site("https://www.example.com/x"), Some(Pin::Site { name: "example.com".into(), url: "https://www.example.com/x".into() }));
        assert_eq!(site("ftp://x"), None);
    }

    #[test]
    fn folders() {
        let app = |s: &str| Pin::App(s.into());
        let mut pins = vec![app("a"), app("b"), app("c"), Pin::Path("/x".into())];
        // c onto a: folder at a's place.
        assert!(drop_pin(&mut pins, 2, Drop::Onto(0)));
        assert_eq!(pins[0], Pin::Folder { name: "Folder".into(), apps: vec!["a".into(), "c".into()] });
        assert_eq!(pins.len(), 3);
        // b into the folder.
        assert!(drop_pin(&mut pins, 1, Drop::Onto(0)));
        assert!(has_app(&pins, "b"));
        // Files don't go into folders.
        assert!(!drop_pin(&mut pins, 1, Drop::Onto(0)));
        take_out(&mut pins, 0, "a");
        assert_eq!(pins[1], app("a"));
        remove_app(&mut pins, "c");
        assert_eq!(pins[0], app("b"));
        // Reorder.
        let mut pins = vec![app("a"), app("b"), app("c")];
        assert!(drop_pin(&mut pins, 0, Drop::Before(3)));
        assert_eq!(pins, vec![app("b"), app("c"), app("a")]);
        assert!(!drop_pin(&mut pins, 0, Drop::Before(1)));
    }
}

/// Lines of a `text/uri-list` (comments dropped); plain text works too:
/// absolute paths become file URIs.
pub fn parse_uri_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().trim_end_matches('\0'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            if l.starts_with('/') {
                Some(format!("file://{l}"))
            } else if l.contains("://") {
                Some(l.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// What dropping a URI on Start pins: a file/folder or a website.
pub fn pin_for_uri(uri: &str) -> Option<Pin> {
    if uri.starts_with("file://") {
        return crate::recent::file_uri_to_path(uri).map(Pin::Path);
    }
    site(uri)
}

#[cfg(test)]
mod uri_tests {
    use super::*;
    #[test]
    fn uri_lists() {
        assert_eq!(parse_uri_list("# c\r\nfile:///a%20b\r\nhttps://x.org/\n\nhello\n/tmp/z"), vec!["file:///a%20b", "https://x.org/", "file:///tmp/z"]);
        assert_eq!(pin_for_uri("file:///a%20b"), Some(Pin::Path("/a b".into())));
        assert_eq!(pin_for_uri("https://x.org/"), Some(Pin::Site { name: "x.org".into(), url: "https://x.org/".into() }));
    }
}
