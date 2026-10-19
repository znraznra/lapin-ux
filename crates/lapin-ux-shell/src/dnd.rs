// SPDX-License-Identifier: GPL-3.0-only
//! Data dropped onto the shell: a list of URIs (files or web links).

use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uris(pub Vec<String>);

const MIMES: &[&str] = &["text/uri-list", "text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

impl cosmic::iced::clipboard::mime::AllowedMimeTypes for Uris {
    fn allowed() -> Cow<'static, [String]> {
        Cow::Owned(MIMES.iter().map(|s| s.to_string()).collect())
    }
}

impl TryFrom<(Vec<u8>, String)> for Uris {
    type Error = ();
    fn try_from((data, _mime): (Vec<u8>, String)) -> Result<Self, ()> {
        let text = String::from_utf8(data).map_err(|_| ())?;
        let list = lapin_ux_core::pins::parse_uri_list(&text);
        if list.is_empty() { Err(()) } else { Ok(Uris(list)) }
    }
}

/// Desktop files being dragged out to other apps (or onto the desktop).
#[derive(Debug, Clone)]
pub struct FileDrag(pub Vec<std::path::PathBuf>);

impl cosmic::iced::clipboard::mime::AsMimeTypes for FileDrag {
    fn available(&self) -> Cow<'static, [String]> {
        Cow::Owned(
            ["text/uri-list", "x-special/gnome-copied-files", "text/plain;charset=utf-8", "text/plain"].iter().map(|s| s.to_string()).collect(),
        )
    }

    fn as_bytes(&self, mime: &str) -> Option<Cow<'static, [u8]>> {
        use lapin_ux_core::desktop::{FileClip, gnome_copied_files, uri_list};
        let bytes = match mime {
            "text/uri-list" => uri_list(&self.0).into_bytes(),
            "x-special/gnome-copied-files" => gnome_copied_files(&FileClip { cut: false, paths: self.0.clone() }).into_bytes(),
            _ => self.0.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n").into_bytes(),
        };
        Some(Cow::Owned(bytes))
    }
}
