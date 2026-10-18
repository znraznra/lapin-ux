// SPDX-License-Identifier: GPL-3.0-only
//! Taskbar badges and progress: apps announce them with the
//! `com.canonical.Unity.LauncherEntry` `Update` signal (Firefox, Chromium,
//! Thunderbird, Telegram, Discord, Steam and many GTK/Qt/Electron apps do).

/// One property from an `Update` signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Prop {
    Count(i64),
    CountVisible(bool),
    Progress(f64),
    ProgressVisible(bool),
    Urgent(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Badge {
    pub count: i64,
    pub count_visible: bool,
    pub progress: f64,
    pub progress_visible: bool,
    pub urgent: bool,
}

impl Badge {
    pub fn apply(&mut self, p: Prop) {
        match p {
            Prop::Count(c) => self.count = c,
            Prop::CountVisible(v) => self.count_visible = v,
            Prop::Progress(v) => self.progress = v.clamp(0.0, 1.0),
            Prop::ProgressVisible(v) => self.progress_visible = v,
            Prop::Urgent(v) => self.urgent = v,
        }
    }

    /// Text for the badge: "1"…"99", then "99+"; None when hidden.
    pub fn label(&self) -> Option<String> {
        (self.count_visible && self.count > 0).then(|| if self.count > 99 { "99+".into() } else { self.count.to_string() })
    }

    /// Progress 0…1 while shown.
    pub fn progress(&self) -> Option<f32> {
        self.progress_visible.then_some(self.progress as f32)
    }

    pub fn is_empty(&self) -> bool {
        self.label().is_none() && self.progress().is_none() && !self.urgent
    }
}

/// "application://firefox.desktop" → "firefox".
pub fn app_id(uri: &str) -> Option<String> {
    let id = uri.strip_prefix("application://").unwrap_or(uri);
    let id = id.strip_suffix(".desktop").unwrap_or(id);
    (!id.is_empty()).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges() {
        let mut b = Badge::default();
        assert!(b.is_empty());
        b.apply(Prop::Count(3));
        assert_eq!(b.label(), None);
        b.apply(Prop::CountVisible(true));
        assert_eq!(b.label().as_deref(), Some("3"));
        b.apply(Prop::Count(250));
        assert_eq!(b.label().as_deref(), Some("99+"));
        b.apply(Prop::Progress(1.7));
        b.apply(Prop::ProgressVisible(true));
        assert_eq!(b.progress(), Some(1.0));
        assert_eq!(app_id("application://org.telegram.desktop.desktop").as_deref(), Some("org.telegram.desktop"));
        assert_eq!(app_id("application://").as_deref(), None);
    }
}
