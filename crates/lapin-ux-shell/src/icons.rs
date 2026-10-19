// SPDX-License-Identifier: GPL-3.0-only
//! Which icon set draws what.
//!
//! Files, folders, apps, shortcuts and desktop items use the full-colour app
//! and file icons (the Win11 icon theme by default). Everything else (menus,
//! status, buttons) uses `-symbolic` names, which the Lapin UX theme fills
//! with Fluent UI System Icons.

use cosmic::widget::icon;

/// A full-colour file, folder or app icon. Looked up as a large scalable
/// image, so icon themes' small monochrome variants are never picked, then
/// drawn at whatever size the caller sets.
pub fn full(name: &str) -> icon::Handle {
    icon::from_name(name.to_string())
        .prefer_svg(true)
        .size(128)
        .fallback(Some(icon::IconFallback::Names(vec!["text-x-generic".into(), "application-x-executable".into()])))
        .handle()
}

/// Icon for a file or folder from its name.
pub fn for_file(name: &str, is_dir: bool) -> icon::Handle {
    if is_dir { full("folder") } else { full(crate::start_view::mime_icon(name)) }
}
