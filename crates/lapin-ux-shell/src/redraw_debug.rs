// SPDX-License-Identifier: GPL-3.0-only
//! Opt-in diagnostics for taskbar flicker and dead zones (#32, #34), which
//! only show on some hardware. `touch ~/.local/state/lapin-ux/debug-redraw`
//! (or `LAPIN_UX_DEBUG_REDRAW=1`) before signing in logs (to session.log) how many messages the shell handles
//! every 5 s, every tracked position change, and every pop-up and surface
//! the shell opens or closes, with where it was anchored. A steady stream of
//! messages or positions while nothing happens is a redraw loop; a surface
//! that opens near the dead zone is what's catching the clicks.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

pub static ON: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("LAPIN_UX_DEBUG_REDRAW").is_ok_and(|v| v == "1")
        || std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".local/state/lapin-ux/debug-redraw"))
            .is_some_and(|p| p.exists())
});

static MESSAGES: AtomicU64 = AtomicU64::new(0);
static RECTS: AtomicU64 = AtomicU64::new(0);
static SINCE: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// Count a message; log the totals every 5 s.
pub fn message() {
    MESSAGES.fetch_add(1, Relaxed);
    let Ok(mut since) = SINCE.lock() else { return };
    if since.is_none_or(|t| t.elapsed() >= Duration::from_secs(5)) {
        tracing::info!(
            messages = MESSAGES.swap(0, Relaxed),
            positions = RECTS.swap(0, Relaxed),
            "redraw debug: last 5 s"
        );
        *since = Some(Instant::now());
    }
}

/// A tracked element's position changed.
pub fn rect(key: &str, r: &cosmic::iced::Rectangle) {
    RECTS.fetch_add(1, Relaxed);
    tracing::info!(key, x = r.x, y = r.y, w = r.width, h = r.height, "redraw debug: position");
}

/// Something about the shell's surfaces happened.
pub fn surface(what: &str, detail: impl std::fmt::Debug) {
    tracing::info!("redraw debug: {what} {detail:?}");
}
