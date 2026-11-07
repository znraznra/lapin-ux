// SPDX-License-Identifier: GPL-3.0-only
//! Pointer motion without a message per motion event (#51, #32).
//!
//! Every message makes iced rebuild every shell surface (taskbars, the
//! desktop, Start, pop-ups), and a gaming mouse reports motion hundreds of
//! times a second, so turning each motion into a message kept the shell
//! busy whenever the pointer moved over it. Motion only needs handling
//! straight away while a button is held (dragging pins and taskbar buttons,
//! box selection on the desktop) and at a taskbar's "Show desktop" corner.
//! Otherwise the position is only remembered here, and the shell catches up
//! on it (`take_pending`) before it handles its next message, so a click or
//! a menu still sees where the pointer is.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use cosmic::iced::{Point, window};

static BUTTON_HELD: AtomicBool = AtomicBool::new(false);
static IN_CORNER: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<(window::Id, Point)>> = Mutex::new(None);
/// Taskbar surfaces with their output width, when the corner is on.
static CORNERS: Mutex<Vec<(window::Id, f32)>> = Mutex::new(Vec::new());

/// The width of the "Show desktop" corner at a taskbar's far end.
pub const CORNER: f32 = 8.0;

pub fn button(held: bool) {
    BUTTON_HELD.store(held, Relaxed);
}

/// The pointer moved over a shell surface: whether it needs a message now.
/// When it doesn't, the position waits for the next message.
pub fn moved(id: window::Id, p: Point) -> bool {
    let corner = IN_CORNER.load(Relaxed)
        || CORNERS.lock().is_ok_and(|bars| bars.iter().any(|(bar, w)| *bar == id && p.x >= w - CORNER));
    let now = BUTTON_HELD.load(Relaxed) || corner;
    if let Ok(mut pending) = PENDING.lock() {
        *pending = if now { None } else { Some((id, p)) };
    }
    now
}

/// The last position not yet handled, if any.
pub fn take_pending() -> Option<(window::Id, Point)> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

/// Keep the shell's view of the taskbars and the corner state here.
pub fn sync(bars: impl Iterator<Item = (window::Id, Option<u32>)>, corner_on: bool, in_corner: bool) {
    IN_CORNER.store(in_corner, Relaxed);
    let want: Vec<(window::Id, f32)> =
        if corner_on { bars.filter_map(|(id, w)| w.map(|w| (id, w as f32))).collect() } else { Vec::new() };
    if let Ok(mut have) = CORNERS.lock()
        && *have != want
    {
        *have = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_waits_unless_needed() {
        let bar = window::Id::unique();
        let other = window::Id::unique();
        sync([(bar, Some(1000))].into_iter(), true, false);
        button(false);
        // Over a pop-up, or the middle of the taskbar: no message, remembered.
        assert!(!moved(other, Point::new(500.0, 10.0)));
        assert!(!moved(bar, Point::new(500.0, 10.0)));
        assert_eq!(take_pending(), Some((bar, Point::new(500.0, 10.0))));
        assert_eq!(take_pending(), None);
        // The corner, and anything while a button is held: straight away.
        assert!(moved(bar, Point::new(995.0, 10.0)));
        assert_eq!(take_pending(), None);
        button(true);
        assert!(moved(other, Point::new(1.0, 1.0)));
        button(false);
        // Leaving the corner is reported too.
        sync([(bar, Some(1000))].into_iter(), true, true);
        assert!(moved(bar, Point::new(10.0, 10.0)));
    }
}
