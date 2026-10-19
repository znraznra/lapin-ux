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

/// Files on the clipboard: cut (move) or copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileClip {
    pub cut: bool,
    pub paths: Vec<PathBuf>,
}

/// `x-special/gnome-copied-files`: "copy" or "cut", then one URI per line.
pub fn gnome_copied_files(clip: &FileClip) -> String {
    let mut s = String::from(if clip.cut { "cut" } else { "copy" });
    for p in &clip.paths {
        s.push('\n');
        s.push_str(&file_uri(p));
    }
    s
}

/// `text/uri-list`, CRLF-separated as the spec says.
pub fn uri_list(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| file_uri(p) + "\r\n").collect()
}

pub fn file_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for b in p.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => s.push(b as char),
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    s
}

/// Read a clipboard offer of files (gnome-copied-files or a uri-list of
/// local files). None if it isn't files.
pub fn parse_file_clip(mime: &str, text: &str) -> Option<FileClip> {
    let mut lines = text.lines().map(|l| l.trim_matches(['\r', '\0', ' '])).filter(|l| !l.is_empty() && !l.starts_with('#'));
    let cut = if mime == "x-special/gnome-copied-files" {
        match lines.next()? {
            "cut" => true,
            "copy" => false,
            _ => return None,
        }
    } else {
        false
    };
    let paths: Option<Vec<PathBuf>> = lines.map(crate::recent::file_uri_to_path).collect();
    let paths = paths?;
    (!paths.is_empty()).then_some(FileClip { cut, paths })
}

/// Where a pasted `name` goes in `dir`: the same name if free, else
/// "name - Copy.ext", "name - Copy (2).ext"…
pub fn paste_name(dir: &Path, name: &str, is_dir: bool) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !is_dir && !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (1..)
        .map(|n| if n == 1 { format!("{stem} - Copy{ext}") } else { format!("{stem} - Copy ({n}){ext}") })
        .find(|c| !dir.join(c).exists())
        .unwrap_or_else(|| name.to_string())
}

/// Copy a file or folder (recursively, symlinks as links).
pub fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(from)?;
        return std::os::unix::fs::symlink(target, to);
    }
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_tree(&e.path(), &to.join(e.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(from, to).map(|_| ())
}

/// Put `src` into `dir` (copy, or move when `cut`). Moving within the same
/// folder does nothing; copying there makes "… - Copy". Returns the new path.
pub fn paste_one(src: &Path, dir: &Path, cut: bool) -> std::io::Result<PathBuf> {
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    if cut && src.parent() == Some(dir) {
        return Ok(src.to_path_buf());
    }
    // Never paste a folder into itself.
    if src.is_dir() && dir.starts_with(src) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "can't put a folder inside itself"));
    }
    let target = dir.join(paste_name(dir, &name, src.is_dir()));
    if cut {
        match std::fs::rename(src, &target) {
            Ok(()) => return Ok(target),
            // Another filesystem: copy, then remove the original.
            Err(e) if e.raw_os_error() == Some(18) => {
                copy_tree(src, &target)?;
                if src.is_dir() { std::fs::remove_dir_all(src)? } else { std::fs::remove_file(src)? }
                return Ok(target);
            }
            Err(e) => return Err(e),
        }
    }
    copy_tree(src, &target)?;
    Ok(target)
}

#[cfg(test)]
mod clip_tests {
    use super::*;

    #[test]
    fn file_clipboard() {
        let c = FileClip { cut: true, paths: vec!["/home/u/My File.txt".into(), "/home/u/b".into()] };
        let s = gnome_copied_files(&c);
        assert_eq!(s, "cut\nfile:///home/u/My%20File.txt\nfile:///home/u/b");
        assert_eq!(parse_file_clip("x-special/gnome-copied-files", &s), Some(c.clone()));
        assert_eq!(parse_file_clip("text/uri-list", &uri_list(&c.paths)), Some(FileClip { cut: false, paths: c.paths.clone() }));
        assert_eq!(parse_file_clip("text/uri-list", "https://example.org/\r\n"), None);
    }

    #[test]
    fn pasting() {
        let d = std::env::temp_dir().join(format!("lapin-paste-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::create_dir_all(a.join("sub")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("notes.txt"), b"hi").unwrap();
        std::fs::write(a.join("sub/x"), b"x").unwrap();
        // Copy into the same folder → "- Copy".
        assert_eq!(paste_one(&a.join("notes.txt"), &a, false).unwrap(), a.join("notes - Copy.txt"));
        assert_eq!(paste_one(&a.join("notes.txt"), &a, false).unwrap(), a.join("notes - Copy (2).txt"));
        // Folder copy is recursive; folder into itself is refused.
        assert!(paste_one(&a.join("sub"), &b, false).unwrap().join("x").exists());
        assert!(paste_one(&a, &a.join("sub"), false).is_err());
        // Cut moves.
        let moved = paste_one(&a.join("notes.txt"), &b, true).unwrap();
        assert!(moved.exists() && !a.join("notes.txt").exists());
        std::fs::remove_dir_all(d).unwrap();
    }
}
