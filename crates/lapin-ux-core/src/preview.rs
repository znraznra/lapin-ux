// SPDX-License-Identifier: GPL-3.0-only
//! What Start's search preview pane shows for a file or folder, as in
//! Windows 11: a picture or the first lines of a text file, then the type,
//! size, date modified and location.

use std::io::Read;
use std::path::Path;
use std::time::SystemTime;

/// The big area at the top of the pane.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// Show the picture itself.
    Image,
    /// The first lines of a text file.
    Text(String),
    /// A folder: how many items are directly inside.
    Folder { items: u64 },
    /// Only the file's icon.
    Icon,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilePreview {
    pub body: Body,
    /// "Text Document (.txt)", "File folder"…
    pub kind: String,
    /// Bytes (files only).
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];
const TEXT_EXT: &[&str] = &[
    "txt", "md", "log", "csv", "json", "toml", "yaml", "yml", "ini", "conf", "cfg", "xml", "html", "css", "js", "ts", "rs", "py",
    "sh", "c", "h", "cpp", "java", "go", "rb", "lua", "desktop",
];
/// Pictures larger than this show their icon (decoding them would stall).
const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024;
/// How much of a text file is read.
const TEXT_BYTES: usize = 4096;
pub const TEXT_LINES: usize = 12;
const LINE_CHARS: usize = 80;

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// The first lines of `bytes`, if it reads as text.
pub fn text_excerpt(bytes: &[u8]) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    // A cut multi-byte character at the end is fine; anything else isn't text.
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) if e.valid_up_to() + 4 >= bytes.len() => std::str::from_utf8(&bytes[..e.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    let lines: Vec<String> = text
        .lines()
        .take(TEXT_LINES)
        .map(|l| {
            let l = l.replace('\t', "    ");
            if l.chars().count() > LINE_CHARS { l.chars().take(LINE_CHARS - 1).chain(['…']).collect() } else { l }
        })
        .collect();
    Some(lines.join("\n"))
}

pub fn file_preview(path: &Path, is_dir: bool) -> FilePreview {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let kind = crate::props::kind_name(&name, is_dir);
    let meta = std::fs::metadata(path).ok();
    let modified = meta.as_ref().and_then(|m| m.modified().ok());
    if is_dir {
        let items = std::fs::read_dir(path).map(|d| d.take(100_000).count() as u64).unwrap_or(0);
        return FilePreview { body: Body::Folder { items }, kind, size: None, modified };
    }
    let size = meta.as_ref().map(|m| m.len());
    let e = ext(path);
    let body = if IMAGE_EXT.contains(&e.as_str()) && size.is_some_and(|s| s <= MAX_IMAGE_BYTES) {
        Body::Image
    } else if TEXT_EXT.contains(&e.as_str()) || e.is_empty() {
        let mut buf = vec![0u8; TEXT_BYTES];
        let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)).unwrap_or(0);
        text_excerpt(&buf[..n]).filter(|t| !t.trim().is_empty()).map(Body::Text).unwrap_or(Body::Icon)
    } else {
        Body::Icon
    };
    FilePreview { body, kind, size, modified }
}

/// "10/20/2026 9:41 AM", as Windows writes dates in the pane.
pub fn date_text(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%-m/%-d/%Y %-I:%M %p").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt() {
        assert_eq!(text_excerpt(b"a\nb\tc\n").as_deref(), Some("a\nb    c"));
        assert_eq!(text_excerpt(b"bin\0ary"), None);
        let many: String = (0..30).map(|i| format!("line {i}\n")).collect();
        assert_eq!(text_excerpt(many.as_bytes()).unwrap().lines().count(), TEXT_LINES);
        let long = "x".repeat(200);
        assert_eq!(text_excerpt(long.as_bytes()).unwrap().chars().count(), LINE_CHARS);
        // A multi-byte character cut by the read limit still reads as text.
        let mut cut = "héllo".as_bytes().to_vec();
        cut.push(0xC3);
        assert_eq!(text_excerpt(&cut).as_deref(), Some("héllo"));
    }

    #[test]
    fn kinds_of_preview() {
        let dir = std::env::temp_dir().join(format!("lapin-preview-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("notes.txt"), "Shopping\nmilk").unwrap();
        std::fs::write(dir.join("photo.png"), [0u8; 16]).unwrap();
        std::fs::write(dir.join("archive.zip"), [0u8; 16]).unwrap();
        assert_eq!(file_preview(&dir.join("notes.txt"), false).body, Body::Text("Shopping\nmilk".into()));
        assert_eq!(file_preview(&dir.join("photo.png"), false).body, Body::Image);
        assert_eq!(file_preview(&dir.join("archive.zip"), false).body, Body::Icon);
        let d = file_preview(&dir, true);
        assert_eq!(d.body, Body::Folder { items: 4 });
        assert_eq!(d.size, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
