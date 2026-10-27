// SPDX-License-Identifier: GPL-3.0-only
//! Crash reports: when a Lapin UX program panics, write what happened, a
//! backtrace and the last things it was doing to its log (stderr, which the
//! session keeps in `~/.local/state/lapin-ux/session.log`) and to
//! `~/.local/state/lapin-ux/crash-<program>.txt`, so a bug report needs no
//! terminal session or debugger (issue #29).

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Mutex;

/// How many recent events a report lists.
const KEEP: usize = 40;
/// Longest event text kept (longer ones are cut, cheaply).
const EVENT_MAX: usize = 200;

static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// A `fmt::Write` that stops (with an error) once it holds `max` bytes, so
/// describing a message that carries a big buffer costs next to nothing.
struct Capped {
    s: String,
    max: usize,
}

impl std::fmt::Write for Capped {
    fn write_str(&mut self, t: &str) -> std::fmt::Result {
        let room = self.max.saturating_sub(self.s.len());
        if t.len() <= room {
            self.s.push_str(t);
            Ok(())
        } else {
            let mut cut = room;
            while cut > 0 && !t.is_char_boundary(cut) {
                cut -= 1;
            }
            self.s.push_str(&t[..cut]);
            self.s.push('…');
            Err(std::fmt::Error)
        }
    }
}

/// Remember an event (e.g. the message a program is handling) for the next
/// crash report. Cheap: the text is cut at a couple of hundred bytes while
/// it's being formatted.
pub fn note(event: &dyn std::fmt::Debug) {
    let mut w = Capped { s: String::new(), max: EVENT_MAX };
    let _ = write!(w, "{event:?}");
    if let Ok(mut r) = RECENT.lock() {
        if r.len() == KEEP {
            r.pop_front();
        }
        r.push_back(w.s);
    }
}

fn state_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
        .map(|p| p.join("lapin-ux"))
}

/// The report text (also used by the tests).
fn report(program: &str, info: &str, backtrace: &str, recent: &[String]) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "=== {program} crashed (Lapin UX {}) ===", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(s, "{info}");
    if !recent.is_empty() {
        let _ = writeln!(s, "--- last {} events, oldest first ---", recent.len());
        for e in recent {
            let _ = writeln!(s, "  {e}");
        }
    }
    let _ = writeln!(s, "--- backtrace ---\n{backtrace}");
    s
}

/// Set once the first panic of this process has been reported, so a later
/// one (e.g. while unwinding) can't overwrite the report that matters.
static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Install the crash reporter for `program` (call first thing in `main`).
/// The previous hook still runs afterwards, so the usual panic line stays.
///
/// A panic on the main thread ends the program here, with exit code 101 like
/// a normal Rust panic, instead of unwinding: tearing the program down
/// mid-update (D-Bus proxies, Wayland objects) can panic again and abort,
/// and the session restarts it either way. Panics on other threads (e.g. a
/// background task) keep their usual behaviour.
pub fn install(program: &'static str) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let first = !REPORTED.swap(true, std::sync::atomic::Ordering::SeqCst);
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let recent: Vec<String> = RECENT.lock().map(|r| r.iter().cloned().collect()).unwrap_or_default();
        let text = report(program, &info.to_string(), &bt, &recent);
        eprintln!("{text}");
        if first
            && let Some(dir) = state_dir()
            && std::fs::create_dir_all(&dir).is_ok()
        {
            let _ = std::fs::write(dir.join(format!("crash-{program}.txt")), &text);
        }
        previous(info);
        if std::thread::current().name() == Some("main") {
            std::process::exit(101);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_capped_and_rolling() {
        let big = vec![0u8; 100_000];
        note(&big);
        let r = RECENT.lock().unwrap();
        let last = r.back().unwrap();
        assert!(last.len() <= EVENT_MAX + '…'.len_utf8());
        drop(r);
        for i in 0..(KEEP + 5) {
            note(&i);
        }
        let r = RECENT.lock().unwrap();
        assert_eq!(r.len(), KEEP);
        assert_eq!(r.back().unwrap(), &(KEEP + 4).to_string());
    }

    #[test]
    fn report_has_everything() {
        let t = report("lapin-ux-shell", "panicked at tree.rs:491:38:\nDowncast widget state", "0: main", &["Tick".into(), "Noop".into()]);
        assert!(t.contains("lapin-ux-shell crashed"));
        assert!(t.contains("Downcast widget state"));
        assert!(t.contains("  Tick\n  Noop"));
        assert!(t.contains("--- backtrace ---\n0: main"));
    }
}
