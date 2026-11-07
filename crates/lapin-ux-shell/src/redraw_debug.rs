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

static VIEWS: std::sync::Mutex<Vec<(&'static str, u32, Duration)>> = std::sync::Mutex::new(Vec::new());

/// Time spent building one surface's view (#51).
pub fn view(kind: &'static str, took: Duration) {
    if let Ok(mut v) = VIEWS.lock() {
        match v.iter_mut().find(|(k, ..)| *k == kind) {
            Some((_, n, t)) => (*n += 1, *t += took).1,
            None => v.push((kind, 1, took)),
        }
    }
}

static REDRAWN: AtomicU64 = AtomicU64::new(0);
static SKIPPED: AtomicU64 = AtomicU64::new(0);

/// Surfaces redrawn and left alone after a batch of messages (#51).
pub fn redraws(done: usize, skipped: usize) {
    REDRAWN.fetch_add(done as u64, Relaxed);
    SKIPPED.fetch_add(skipped as u64, Relaxed);
}

static KINDS: std::sync::Mutex<Vec<(String, u64)>> = std::sync::Mutex::new(Vec::new());

/// Count a message (by kind: its name up to the first bracket); log the
/// totals and the busiest kinds every 5 s (#51).
pub fn message(kind: &dyn std::fmt::Debug) {
    MESSAGES.fetch_add(1, Relaxed);
    let name = format!("{kind:?}");
    let name: String = name.split(['(', '{', ' ']).take(2).collect::<Vec<_>>().join(":").chars().take(48).collect();
    if let Ok(mut kinds) = KINDS.lock() {
        match kinds.iter_mut().find(|(k, _)| *k == name) {
            Some((_, n)) => *n += 1,
            None => kinds.push((name, 1)),
        }
    }
    let Ok(mut since) = SINCE.lock() else { return };
    if since.is_none_or(|t| t.elapsed() >= Duration::from_secs(5)) {
        let busiest = KINDS.lock().map(|mut k| {
            k.sort_by(|a, b| b.1.cmp(&a.1));
            let top: Vec<String> = k.iter().take(6).map(|(n, c)| format!("{n}={c}")).collect();
            k.clear();
            top.join(" ")
        });
        tracing::info!(
            messages = MESSAGES.swap(0, Relaxed),
            positions = RECTS.swap(0, Relaxed),
            redrawn = REDRAWN.swap(0, Relaxed),
            skipped = SKIPPED.swap(0, Relaxed),
            busiest = busiest.unwrap_or_default(),
            views = VIEWS.lock().map(|mut v| {
                let s: Vec<String> = v.iter().map(|(k, n, t)| format!("{k}={n}x{:.2}ms", t.as_secs_f64() * 1000.0 / *n as f64)).collect();
                v.clear();
                s.join(" ")
            }).unwrap_or_default(),
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
