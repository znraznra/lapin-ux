// SPDX-License-Identifier: GPL-3.0-only
//! Taskbar button model.
//!
//! Pinned apps keep their slots whether or not they are running. Running apps
//! that are not pinned appear after the pinned ones, in the order they were
//! first opened. All windows of one app share a button (the default
//! "always combine" behaviour of the benchmark).
//!
//! `W` is the window handle type (a Wayland toplevel handle in the shell,
//! a plain integer in tests).

/// One window known to the taskbar.
/// "Combine taskbar buttons and hide labels".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Combine {
    #[default]
    Always,
    WhenFull,
    Never,
}

/// "Show smaller taskbar buttons".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SmallButtons {
    Always,
    #[default]
    WhenFull,
    Never,
}

/// How the taskbar's buttons are drawn right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fit {
    /// One button per window, with its title.
    pub labels: bool,
    pub small: bool,
}

/// Width one button takes (button + gap), in logical pixels.
pub fn button_width(labels: bool, small: bool) -> f32 {
    match (labels, small) {
        (true, false) => 168.0,
        (true, true) => 144.0,
        (false, false) => 44.0,
        (false, true) => 36.0,
    }
}

/// Decide labels and size for `space` pixels of room, `combined` buttons
/// when grouped by app and `split` when every window gets its own.
pub fn fit(space: f32, combined: usize, split: usize, combine: Combine, small: SmallButtons) -> Fit {
    let fits = |labels: bool, small: bool| {
        let n = if labels { split } else { combined };
        n as f32 * button_width(labels, small) <= space
    };
    let labels = match combine {
        Combine::Always => false,
        Combine::Never => true,
        // Labels while they fit (at either size the user allows).
        Combine::WhenFull => fits(true, false) || (small != SmallButtons::Never && fits(true, true)),
    };
    let small = match small {
        SmallButtons::Always => true,
        SmallButtons::Never => false,
        SmallButtons::WhenFull => !fits(labels, false),
    };
    Fit { labels, small }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Window<W> {
    pub handle: W,
    /// The desktop-entry id the window was matched to (or its raw app_id).
    pub app: String,
    pub title: String,
    pub active: bool,
    pub minimized: bool,
    pub urgent: bool,
    /// Names of the monitors it's on (e.g. "eDP-1"); empty when unknown.
    pub outputs: Vec<String>,
}

/// "Show my taskbar apps on" (with a taskbar on every monitor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum MultiDisplay {
    #[default]
    AllTaskbars,
    /// Pinned apps on the main taskbar; each taskbar shows its monitor's windows.
    WhereOpen,
    /// The main taskbar shows everything; the others only their monitor's windows.
    MainAndWhereOpen,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Button<W> {
    pub app: String,
    pub pinned: bool,
    pub windows: Vec<Window<W>>,
}

impl<W> Button<W> {
    pub fn running(&self) -> bool {
        !self.windows.is_empty()
    }
    pub fn active(&self) -> bool {
        self.windows.iter().any(|w| w.active)
    }
    pub fn urgent(&self) -> bool {
        self.windows.iter().any(|w| w.urgent)
    }
}

/// What a left click on a button should do.
#[derive(Debug, Clone, PartialEq)]
pub enum ClickAction<W> {
    Launch(String),
    Activate(W),
    Minimize(W),
    /// Several windows: show the window picker above the button.
    ShowPicker,
}

#[derive(Debug, Clone)]
pub struct Taskbar<W> {
    pinned: Vec<String>,
    /// Running windows in the order they appeared.
    windows: Vec<Window<W>>,
}

impl<W> Default for Taskbar<W> {
    fn default() -> Self {
        Self { pinned: Vec::new(), windows: Vec::new() }
    }
}

impl<W: Clone + PartialEq> Taskbar<W> {
    pub fn new(pinned: Vec<String>) -> Self {
        Self { pinned, windows: Vec::new() }
    }

    pub fn pinned(&self) -> &[String] {
        &self.pinned
    }

    pub fn set_pinned(&mut self, pinned: Vec<String>) {
        self.pinned = pinned;
    }

    pub fn is_pinned(&self, app: &str) -> bool {
        self.pinned.iter().any(|p| p == app)
    }

    /// Pin at the end (like "Pin to taskbar"). Returns true if it changed.
    pub fn pin(&mut self, app: &str) -> bool {
        if self.is_pinned(app) {
            return false;
        }
        self.pinned.push(app.to_string());
        true
    }

    pub fn unpin(&mut self, app: &str) -> bool {
        let before = self.pinned.len();
        self.pinned.retain(|p| p != app);
        before != self.pinned.len()
    }

    /// Move a pinned app to `to` (drag-reorder). Out-of-range is clamped.
    pub fn move_pinned(&mut self, app: &str, to: usize) {
        if let Some(from) = self.pinned.iter().position(|p| p == app) {
            let item = self.pinned.remove(from);
            let to = to.min(self.pinned.len());
            self.pinned.insert(to, item);
        }
    }

    /// Insert or update a window. Activating one window deactivates the rest.
    pub fn upsert(&mut self, win: Window<W>) {
        if win.active {
            for w in &mut self.windows {
                w.active = false;
            }
        }
        match self.windows.iter_mut().find(|w| w.handle == win.handle) {
            Some(existing) => *existing = win,
            None => self.windows.push(win),
        }
    }

    pub fn remove(&mut self, handle: &W) {
        self.windows.retain(|w| &w.handle != handle);
    }

    pub fn windows(&self) -> &[Window<W>] {
        &self.windows
    }

    pub fn buttons(&self) -> Vec<Button<W>> {
        let mut out: Vec<Button<W>> = self
            .pinned
            .iter()
            .map(|app| Button { app: app.clone(), pinned: true, windows: Vec::new() })
            .collect();
        for w in &self.windows {
            match out.iter_mut().find(|b| b.app == w.app) {
                Some(b) => b.windows.push(w.clone()),
                None => out.push(Button { app: w.app.clone(), pinned: false, windows: vec![w.clone()] }),
            }
        }
        out
    }

    /// Buttons for the taskbar on monitor `output`. `main` is the taskbar on
    /// the main monitor; windows with no known monitor count as on it.
    pub fn buttons_for(&self, output: Option<&str>, main: bool, mode: MultiDisplay) -> Vec<Button<W>> {
        let all = self.buttons();
        if mode == MultiDisplay::AllTaskbars || (main && mode == MultiDisplay::MainAndWhereOpen) {
            return all;
        }
        let here = |w: &Window<W>| match output {
            Some(o) if !w.outputs.is_empty() => w.outputs.iter().any(|x| x == o),
            _ => main,
        };
        all.into_iter()
            .filter_map(|mut b| {
                b.windows.retain(|w| here(w));
                let keep = !b.windows.is_empty() || (main && b.pinned);
                keep.then_some(b)
            })
            .collect()
    }

    /// Left-click behaviour for the button of `app`.
    pub fn click(&self, app: &str) -> ClickAction<W> {
        let wins: Vec<&Window<W>> = self.windows.iter().filter(|w| w.app == app).collect();
        match wins.as_slice() {
            [] => ClickAction::Launch(app.to_string()),
            [only] if only.active && !only.minimized => ClickAction::Minimize(only.handle.clone()),
            [only] => ClickAction::Activate(only.handle.clone()),
            _ => ClickAction::ShowPicker,
        }
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    #[test]
    fn when_full_drops_labels_then_shrinks() {
        // Room for 3 labelled buttons (504 px).
        let f = fit(510.0, 3, 3, Combine::WhenFull, SmallButtons::Never);
        assert_eq!(f, Fit { labels: true, small: false });
        let f = fit(510.0, 4, 6, Combine::WhenFull, SmallButtons::Never);
        assert_eq!(f, Fit { labels: false, small: false });
        // 12 icons need 528 px normal, 432 px small.
        let f = fit(500.0, 12, 12, Combine::Always, SmallButtons::WhenFull);
        assert_eq!(f, Fit { labels: false, small: true });
        let f = fit(500.0, 12, 12, Combine::Always, SmallButtons::Never);
        assert!(!f.small);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(handle: u32, app: &str, active: bool) -> Window<u32> {
        Window { handle, app: app.into(), title: format!("{app} {handle}"), active, minimized: false, urgent: false, outputs: vec![] }
    }

    #[test]
    fn pinned_first_then_running_in_open_order() {
        let mut t = Taskbar::new(vec!["files".into(), "browser".into()]);
        t.upsert(w(1, "editor", false));
        t.upsert(w(2, "browser", false));
        t.upsert(w(3, "music", false));
        t.upsert(w(4, "editor", true));
        let b = t.buttons();
        let apps: Vec<_> = b.iter().map(|b| b.app.as_str()).collect();
        assert_eq!(apps, ["files", "browser", "editor", "music"]);
        assert!(!b[0].running());
        assert!(b[1].running() && b[1].pinned);
        assert_eq!(b[2].windows.len(), 2);
        assert!(b[2].active());
    }

    #[test]
    fn single_active_window() {
        let mut t = Taskbar::new(vec![]);
        t.upsert(w(1, "a", true));
        t.upsert(w(2, "b", false));
        assert!(!t.windows()[0].active || !t.windows()[1].active);
        assert_eq!(t.click("a"), ClickAction::Minimize(1));
        assert_eq!(t.click("b"), ClickAction::Activate(2));
        t.upsert(w(2, "b", true));
        assert!(!t.windows()[0].active);
        assert_eq!(t.click("a"), ClickAction::Activate(1));
    }

    #[test]
    fn click_rules() {
        let mut t = Taskbar::new(vec!["a".into()]);
        assert_eq!(t.click("a"), ClickAction::Launch("a".into()));
        t.upsert(w(1, "a", false));
        t.upsert(w(2, "a", false));
        assert_eq!(t.click("a"), ClickAction::ShowPicker);
        let mut min = w(3, "c", true);
        min.minimized = true;
        t.upsert(min);
        assert_eq!(t.click("c"), ClickAction::Activate(3));
    }

    #[test]
    fn close_window_keeps_pinned_slot() {
        let mut t = Taskbar::new(vec!["a".into()]);
        t.upsert(w(1, "a", true));
        t.upsert(w(2, "b", false));
        t.remove(&1);
        t.remove(&2);
        let b = t.buttons();
        assert_eq!(b.len(), 1);
        assert!(!b[0].running());
    }

    #[test]
    fn pin_unpin_move() {
        let mut t: Taskbar<u32> = Taskbar::new(vec!["a".into(), "b".into(), "c".into()]);
        assert!(!t.pin("a"));
        assert!(t.pin("d"));
        t.move_pinned("d", 0);
        assert_eq!(t.pinned(), ["d", "a", "b", "c"]);
        t.move_pinned("d", 99);
        assert_eq!(t.pinned(), ["a", "b", "c", "d"]);
        assert!(t.unpin("b"));
        assert!(!t.unpin("zzz"));
        assert_eq!(t.pinned(), ["a", "c", "d"]);
    }

    #[test]
    fn unpinning_running_app_moves_it_after_pinned() {
        let mut t = Taskbar::new(vec!["a".into(), "b".into()]);
        t.upsert(w(1, "a", false));
        t.unpin("a");
        let apps: Vec<_> = t.buttons().into_iter().map(|b| b.app).collect();
        assert_eq!(apps, ["b", "a"]);
    }

    #[test]
    fn per_monitor_buttons() {
        let mut t = Taskbar::new(vec!["files".into(), "browser".into()]);
        let mut on = |h: u32, app: &str, out: &str| {
            let mut x = w(h, app, false);
            x.outputs = vec![out.into()];
            t.upsert(x);
        };
        on(1, "browser", "DP-1");
        on(2, "editor", "HDMI-1");
        on(3, "browser", "HDMI-1");
        let names = |v: Vec<Button<u32>>| v.into_iter().map(|b| (b.app, b.windows.len())).collect::<Vec<_>>();
        use MultiDisplay::*;
        assert_eq!(names(t.buttons_for(Some("HDMI-1"), false, AllTaskbars)).len(), 3);
        assert_eq!(names(t.buttons_for(Some("DP-1"), true, WhereOpen)), vec![("files".into(), 0), ("browser".into(), 1)]);
        assert_eq!(names(t.buttons_for(Some("HDMI-1"), false, WhereOpen)), vec![("browser".into(), 1), ("editor".into(), 1)]);
        assert_eq!(names(t.buttons_for(Some("DP-1"), true, MainAndWhereOpen)).len(), 3);
        assert_eq!(names(t.buttons_for(Some("HDMI-1"), false, MainAndWhereOpen)), vec![("browser".into(), 1), ("editor".into(), 1)]);
    }
}
