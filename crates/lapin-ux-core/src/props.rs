// SPDX-License-Identifier: GPL-3.0-only
//! What a file's Properties window shows: type, location, size (and size on
//! disk), what a folder contains, dates and attributes.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Props {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// "Text Document (.txt)", "File folder"…
    pub kind: String,
    pub location: String,
    /// Bytes (a folder: everything inside).
    pub size: u64,
    pub size_on_disk: u64,
    /// For folders: files and folders inside (recursively).
    pub files: u64,
    pub folders: u64,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub read_only: bool,
    pub hidden: bool,
}

/// A friendly type name from the extension.
pub fn kind_name(name: &str, is_dir: bool) -> String {
    if is_dir {
        return "File folder".into();
    }
    let Some((_, ext)) = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()) else { return "File".into() };
    let e = ext.to_ascii_lowercase();
    let label = match e.as_str() {
        "txt" | "md" | "log" => "Text Document",
        "pdf" => "PDF Document",
        "odt" | "doc" | "docx" | "rtf" => "Document",
        "ods" | "xls" | "xlsx" | "csv" => "Spreadsheet",
        "odp" | "ppt" | "pptx" => "Presentation",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "heic" => "Image",
        "mp4" | "mkv" | "webm" | "mov" | "avi" => "Video",
        "mp3" | "flac" | "ogg" | "wav" | "m4a" | "opus" => "Audio",
        "zip" | "tar" | "gz" | "xz" | "7z" | "rar" | "zst" => "Compressed Archive",
        "desktop" => "Shortcut",
        "sh" => "Shell Script",
        "html" | "htm" => "Web Page",
        _ => return format!("{} File", e.to_ascii_uppercase()),
    };
    format!("{label} (.{e})")
}

/// Sizes the way Windows shows them: "1.24 MB (1,301,234 bytes)".
pub fn size_text(bytes: u64) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let short = if u == 0 { format!("{bytes} bytes") } else { format!("{:.2} {}", v, units[u]) };
    if u == 0 { short } else { format!("{short} ({} bytes)", group(bytes)) }
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Read everything but the folder totals (fast; see [`folder_totals`]).
pub fn read(path: &Path) -> Option<Props> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    let is_dir = meta.is_dir();
    Some(Props {
        path: path.to_path_buf(),
        kind: kind_name(&name, is_dir),
        location: path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
        is_dir,
        size: if is_dir { 0 } else { meta.len() },
        size_on_disk: if is_dir { 0 } else { meta.blocks() * 512 },
        files: 0,
        folders: 0,
        created: meta.created().ok(),
        modified: meta.modified().ok(),
        accessed: meta.accessed().ok(),
        read_only: meta.permissions().readonly(),
        hidden: name.starts_with('.'),
        name,
    })
}

/// A folder's size, size on disk and file/folder counts (walks it).
pub fn folder_totals(dir: &Path) -> (u64, u64, u64, u64) {
    let (mut size, mut disk, mut files, mut folders) = (0, 0, 0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                folders += 1;
                stack.push(e.path());
            } else {
                files += 1;
                size += m.len();
                disk += m.blocks() * 512;
            }
        }
    }
    (size, disk, files, folders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_sizes() {
        assert_eq!(kind_name("notes.txt", false), "Text Document (.txt)");
        assert_eq!(kind_name("Projects", true), "File folder");
        assert_eq!(kind_name("data.xyz", false), "XYZ File");
        assert_eq!(kind_name(".bashrc", false), "File");
        assert_eq!(size_text(512), "512 bytes");
        assert_eq!(size_text(1_301_234), "1.24 MB (1,301,234 bytes)");
    }

    #[test]
    fn reads_a_folder() {
        let d = std::env::temp_dir().join(format!("lapin-props-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.txt"), b"hello").unwrap();
        std::fs::write(d.join("sub/b.txt"), b"world!").unwrap();
        let p = read(&d).unwrap();
        assert!(p.is_dir);
        let (size, _, files, folders) = folder_totals(&d);
        assert_eq!((size, files, folders), (11, 2, 1));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
