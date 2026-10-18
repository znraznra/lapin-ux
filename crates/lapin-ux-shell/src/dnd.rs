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
