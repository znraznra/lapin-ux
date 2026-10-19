// SPDX-License-Identifier: GPL-3.0-only
//! When notifications stay quiet: Do not disturb turned on by hand, by a
//! daily schedule, or by a focus session; and per-app notification rules.

use std::time::{Duration, SystemTime};

/// Per-app notification settings ("Notifications" / "Show banners").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppRule {
    pub app: String,
    /// Off: the app's notifications are dropped entirely.
    pub enabled: bool,
    /// Off: they go straight to the notification centre without a pop-up.
    pub banners: bool,
}

/// What to do with a new notification from `app`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Drop,
    Silent,
    Show,
}

pub fn delivery(rules: &[AppRule], app: &str) -> Delivery {
    match rules.iter().find(|r| r.app == app) {
        Some(r) if !r.enabled => Delivery::Drop,
        Some(r) if !r.banners => Delivery::Silent,
        _ => Delivery::Show,
    }
}

/// Set one app's rule (rules equal to the default are removed).
pub fn set_rule(rules: &mut Vec<AppRule>, app: &str, enabled: bool, banners: bool) {
    rules.retain(|r| r.app != app);
    if !(enabled && banners) {
        rules.push(AppRule { app: app.to_string(), enabled, banners });
        rules.sort_by(|a, b| a.app.cmp(&b.app));
    }
}

/// "Turn on Do not disturb automatically" between two times of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Schedule {
    pub enabled: bool,
    /// Minutes after midnight.
    pub start: u32,
    pub end: u32,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule { enabled: false, start: 22 * 60, end: 7 * 60 }
    }
}

impl Schedule {
    /// Is `minute` (after midnight) inside the window? Windows may cross midnight.
    pub fn contains(&self, minute: u32) -> bool {
        if !self.enabled || self.start == self.end {
            return false;
        }
        if self.start < self.end {
            (self.start..self.end).contains(&minute)
        } else {
            minute >= self.start || minute < self.end
        }
    }
}

pub fn format_minutes(m: u32) -> String {
    let (h, mm) = (m / 60 % 24, m % 60);
    let (h12, ampm) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    format!("{h12}:{mm:02} {ampm}")
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dnd {
    /// Turned on by hand.
    pub manual: bool,
    pub schedule: Schedule,
    /// A focus session runs until then.
    pub focus_until: Option<SystemTime>,
    /// Turned off by hand during the scheduled window (until it ends).
    suppressed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub active: bool,
    /// A focus session just ran out (say so once).
    pub focus_ended: bool,
}

impl Dnd {
    pub fn update(&mut self, minute: u32, now: SystemTime) -> Status {
        let mut focus_ended = false;
        if self.focus_until.is_some_and(|t| t <= now) {
            self.focus_until = None;
            focus_ended = true;
        }
        let scheduled = self.schedule.contains(minute);
        if !scheduled {
            self.suppressed = false;
        }
        Status { active: self.manual || self.focus_until.is_some() || (scheduled && !self.suppressed), focus_ended }
    }

    /// The Do not disturb switch: off ends everything that turned it on.
    pub fn toggle(&mut self, minute: u32, now: SystemTime) {
        if self.update(minute, now).active {
            self.manual = false;
            self.focus_until = None;
            self.suppressed = self.schedule.contains(minute);
        } else {
            self.manual = true;
        }
    }

    pub fn start_focus(&mut self, length: Duration, now: SystemTime) {
        self.focus_until = Some(now + length);
    }

    pub fn focus_left(&self, now: SystemTime) -> Option<Duration> {
        self.focus_until.and_then(|t| t.duration_since(now).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        let mut r = Vec::new();
        set_rule(&mut r, "Chat", true, false);
        set_rule(&mut r, "Spam", false, true);
        assert_eq!(delivery(&r, "Chat"), Delivery::Silent);
        assert_eq!(delivery(&r, "Spam"), Delivery::Drop);
        assert_eq!(delivery(&r, "Mail"), Delivery::Show);
        set_rule(&mut r, "Chat", true, true);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn schedule_and_focus() {
        let s = Schedule { enabled: true, start: 22 * 60, end: 7 * 60 };
        assert!(s.contains(23 * 60) && s.contains(60) && !s.contains(12 * 60));
        let day = Schedule { enabled: true, start: 9 * 60, end: 17 * 60 };
        assert!(day.contains(9 * 60) && !day.contains(17 * 60));
        assert_eq!(format_minutes(22 * 60 + 30), "10:30 PM");
        assert_eq!(format_minutes(0), "12:00 AM");

        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut d = Dnd { schedule: s, ..Default::default() };
        assert!(d.update(23 * 60, t0).active);
        // Turned off during the window: stays off until the window ends.
        d.toggle(23 * 60, t0);
        assert!(!d.update(23 * 60 + 5, t0).active);
        assert!(!d.update(12 * 60, t0).active);
        assert!(d.update(22 * 60, t0).active);
        // Focus.
        let mut f = Dnd::default();
        f.start_focus(Duration::from_secs(60), t0);
        assert!(f.update(600, t0).active);
        let st = f.update(600, t0 + Duration::from_secs(61));
        assert!(!st.active && st.focus_ended);
    }
}
