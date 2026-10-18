// SPDX-License-Identifier: GPL-3.0-only
//! Desktop icons: what's in ~/Desktop, where each icon sits on the grid,
//! and the file operations the desktop offers (new, rename, move to trash).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopItem {
    pub path: PathBuf,
    /// File name on disk.
    pub file_name: String,
    pub is_dir: bool,
    /// A `.desktop` launcher (shown with the app's name and icon).
    pub launcher: bool,
    pub modified: Option<SystemTime>,
    /// Bytes (0 for folders).
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SortBy {
    #[default]
    Name,
    Modified,
    Type,
    /// Largest last, folders first (like the benchmark).
    Size,
}

/// The desktop folder: XDG_DESKTOP_DIR from user-dirs.dirs, else ~/Desktop.
pub fn desktop_dir(home: &Path, config_home: &Path) -> PathBuf {
    crate::files::user_dirs(home, config_home).into_iter().next().unwrap_or_else(|| home.join("Desktop"))
}

/// Visible entries of `dir` (dotfiles hidden), sorted.
pub fn list(dir: &Path, sort: SortBy) -> Vec<DesktopItem> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut items: Vec<DesktopItem> = rd
        .flatten()
        .filter_map(|e| {
            let file_name = e.file_name().to_string_lossy().into_owned();
            if file_name.starts_with('.') {
                return None;
            }
            let meta = std::fs::metadata(e.path()).ok();
            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
            Some(DesktopItem {
                size: if is_dir { 0 } else { meta.as_ref().map_or(0, |m| m.len()) },
                launcher: !is_dir && file_name.ends_with(".desktop"),
                modified: meta.and_then(|m| m.modified().ok()),
                path: e.path(),
                file_name,
                is_dir,
            })
        })
        .collect();
    sort_items(&mut items, sort);
    items
}

pub fn sort_items(items: &mut [DesktopItem], sort: SortBy) {
    let key = |i: &DesktopItem| i.file_name.to_lowercase();
    match sort {
        // Folders first, like a file manager.
        SortBy::Name => items.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(key(a).cmp(&key(b)))),
        SortBy::Size => items.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.size.cmp(&b.size)).then(key(a).cmp(&key(b)))),
        SortBy::Modified => items.sort_by(|a, b| b.modified.cmp(&a.modified).then(key(a).cmp(&key(b)))),
        SortBy::Type => items.sort_by(|a, b| {
            let ext = |i: &DesktopItem| if i.is_dir { String::new() } else { i.file_name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default() };
            b.is_dir.cmp(&a.is_dir).then(ext(a).cmp(&ext(b))).then(key(a).cmp(&key(b)))
        }),
    }
}

/// Grid cell (column, row); icons fill top-to-bottom, then left-to-right.
pub type Cell = (u32, u32);

/// Place icons: remembered cells first (if still on screen and free), then
/// everything else into the free cells in column order.
pub fn arrange(items: &[DesktopItem], saved: &HashMap<String, Cell>, cols: u32, rows: u32) -> Vec<(usize, Cell)> {
    let (cols, rows) = (cols.max(1), rows.max(1));
    let mut taken = std::collections::HashSet::new();
    let mut placed = Vec::with_capacity(items.len());
    let mut rest = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match saved.get(&item.file_name) {
            Some(&(c, r)) if c < cols && r < rows && taken.insert((c, r)) => placed.push((i, (c, r))),
            _ => rest.push(i),
        }
    }
    let mut free = (0..cols).flat_map(|c| (0..rows).map(move |r| (c, r))).filter(|cell| !taken.contains(cell));
    for i in rest {
        // More icons than cells: stack the overflow in the last cell.
        let cell = free.next().unwrap_or((cols - 1, rows - 1));
        placed.push((i, cell));
    }
    placed
}

/// "New folder", then "New folder (2)", "New folder (3)"…
pub fn unique_name(dir: &Path, base: &str, ext: &str) -> String {
    let make = |n: u32| {
        let stem = if n == 1 { base.to_string() } else { format!("{base} ({n})") };
        if ext.is_empty() { stem } else { format!("{stem}.{ext}") }
    };
    (1..).map(make).find(|name| !dir.join(name).exists()).unwrap_or_else(|| make(1))
}

/// Why a new name can't be used, or None if it's fine.
pub fn rename_problem(dir: &Path, old: &str, new: &str) -> Option<&'static str> {
    let new = new.trim();
    if new.is_empty() {
        Some("A file name can't be empty.")
    } else if new.contains('/') || new == "." || new == ".." {
        Some("A file name can't contain /")
    } else if new != old && dir.join(new).exists() {
        Some("There's already a file with that name.")
    } else {
        None
    }
}

/// Move `path` to the user's trash (freedesktop.org Trash spec), so the
/// file manager's Trash can restore it. Fails across filesystems (the
/// caller falls back to `gio trash`).
pub fn trash(path: &Path, data_home: &Path) -> std::io::Result<PathBuf> {
    let trash = data_home.join("Trash");
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), e.to_string()),
        _ => (name.clone(), String::new()),
    };
    let mut target = name.clone();
    let mut n = 2;
    while files.join(&target).exists() || info.join(format!("{target}.trashinfo")).exists() {
        target = if ext.is_empty() { format!("{stem}.{n}") } else { format!("{stem}.{n}.{ext}") };
        n += 1;
    }
    let abs = std::path::absolute(path)?;
    let info_text = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encode_path(&abs.to_string_lossy()),
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S")
    );
    let info_path = info.join(format!("{target}.trashinfo"));
    std::fs::write(&info_path, info_text)?;
    if let Err(e) = std::fs::rename(path, files.join(&target)) {
        let _ = std::fs::remove_file(&info_path);
        return Err(e);
    }
    Ok(files.join(target))
}

fn encode_path(p: &str) -> String {
    let mut out = String::new();
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lapin-desk-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn listing_and_sorting() {
        let d = tmp("list");
        for f in ["b.txt", "A.pdf", ".hidden", "firefox.desktop"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        std::fs::create_dir(d.join("zeta")).unwrap();
        let names: Vec<_> = list(&d, SortBy::Name).into_iter().map(|i| i.file_name).collect();
        assert_eq!(names, vec!["zeta", "A.pdf", "b.txt", "firefox.desktop"]);
        let items = list(&d, SortBy::Type);
        assert_eq!(items.iter().map(|i| i.file_name.as_str()).collect::<Vec<_>>(), vec!["zeta", "firefox.desktop", "A.pdf", "b.txt"]);
        assert!(items[1].launcher);
        std::fs::remove_dir_all(d).unwrap();
    }

    fn item(name: &str) -> DesktopItem {
        DesktopItem { path: name.into(), file_name: name.into(), is_dir: false, launcher: false, modified: None, size: 0 }
    }

    #[test]
    fn arranging() {
        let items = vec![item("a"), item("b"), item("c"), item("d")];
        let saved = HashMap::from([("c".to_string(), (0, 0)), ("d".to_string(), (9, 9))]);
        let mut cells = arrange(&items, &saved, 2, 2);
        cells.sort();
        // c keeps (0,0); d's saved cell is off-grid so it's re-placed; a,b,d fill column-first.
        assert_eq!(cells, vec![(0, (0, 1)), (1, (1, 0)), (2, (0, 0)), (3, (1, 1))]);
    }

    #[test]
    fn names() {
        let d = tmp("names");
        assert_eq!(unique_name(&d, "New folder", ""), "New folder");
        std::fs::create_dir(d.join("New folder")).unwrap();
        assert_eq!(unique_name(&d, "New folder", ""), "New folder (2)");
        assert_eq!(unique_name(&d, "New Text Document", "txt"), "New Text Document.txt");
        assert_eq!(rename_problem(&d, "x", "New folder"), Some("There's already a file with that name."));
        assert_eq!(rename_problem(&d, "x", "a/b"), Some("A file name can't contain /"));
        assert_eq!(rename_problem(&d, "x", "  "), Some("A file name can't be empty."));
        assert_eq!(rename_problem(&d, "New folder", "New folder"), None);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn trashing() {
        let d = tmp("trash");
        let data = d.join("data");
        std::fs::write(d.join("my file.txt"), b"1").unwrap();
        let t = trash(&d.join("my file.txt"), &data).unwrap();
        assert!(!d.join("my file.txt").exists());
        assert_eq!(t, data.join("Trash/files/my file.txt"));
        let info = std::fs::read_to_string(data.join("Trash/info/my file.txt.trashinfo")).unwrap();
        assert!(info.contains("my%20file.txt"));
        std::fs::write(d.join("my file.txt"), b"2").unwrap();
        let t2 = trash(&d.join("my file.txt"), &data).unwrap();
        assert_eq!(t2, data.join("Trash/files/my file.2.txt"));
        std::fs::remove_dir_all(d).unwrap();
    }
}
