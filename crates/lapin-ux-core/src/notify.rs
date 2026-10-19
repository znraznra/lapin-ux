// SPDX-License-Identifier: GPL-3.0-only
//! Toast queue for the notification daemon.
//!
//! Implements the timing rules of the freedesktop notification spec, styled
//! after the benchmark: toasts stack in the bottom-right corner, at most
//! `max_visible` at once, normal ones disappear after a few seconds,
//! critical ones stay until dismissed, and "Do not disturb" holds back
//! everything except critical notifications.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

impl Urgency {
    pub fn from_hint(byte: u8) -> Self {
        match byte {
            0 => Urgency::Low,
            2 => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<Action>,
    pub urgency: Urgency,
    /// Raw `expire_timeout` from Notify(): -1 = server default, 0 = never.
    pub expire_timeout: i32,
    pub transient: bool,
    pub desktop_entry: Option<String>,
    /// When it arrived (for "5m ago" in the notification centre).
    pub received: SystemTime,
    /// Picture sent with the notification (e.g. a contact photo).
    pub image: Option<Image>,
}

/// RGBA pixels, shared cheaply between the toast and the history.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}

impl Image {
    /// Convert the spec's `image-data` hint (width, height, rowstride,
    /// has_alpha, bits_per_sample, channels, data) to tightly packed RGBA.
    pub fn from_hint(width: i32, height: i32, rowstride: i32, has_alpha: bool, bits: i32, channels: i32, data: &[u8]) -> Option<Image> {
        if width <= 0 || height <= 0 || bits != 8 || !(channels == 3 || channels == 4) {
            return None;
        }
        if has_alpha != (channels == 4) {
            return None;
        }
        let (w, h, stride, ch) = (width as usize, height as usize, rowstride as usize, channels as usize);
        if stride < w * ch || data.len() < stride * (h - 1) + w * ch {
            return None;
        }
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            let row = &data[y * stride..y * stride + w * ch];
            for px in row.chunks_exact(ch) {
                out.extend_from_slice(&[px[0], px[1], px[2], if ch == 4 { px[3] } else { 255 }]);
            }
        }
        Some(Image { width: width as u32, height: height as u32, rgba: Arc::new(out) })
    }
}

impl Notification {
    /// Pair up the flat `[key, label, key, label, …]` array from D-Bus.
    pub fn parse_actions(flat: &[String]) -> Vec<Action> {
        flat.chunks_exact(2)
            .map(|c| Action { key: c[0].clone(), label: c[1].clone() })
            .collect()
    }

    /// How long the toast should stay up. `None` = until dismissed.
    pub fn display_time(&self) -> Option<Duration> {
        if self.urgency == Urgency::Critical {
            return None;
        }
        match self.expire_timeout {
            t if t < 0 => Some(DEFAULT_TIMEOUT),
            0 => None,
            // Clamp absurd values so a buggy app cannot pin a toast forever.
            t => Some(Duration::from_millis((t as u64).clamp(1500, 30_000))),
        }
    }
}

/// Why a notification closed, as numbered by the spec's NotificationClosed signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    CloseCall = 3,
    Undefined = 4,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub notification: Notification,
    pub shown_at: Instant,
    pub deadline: Option<Instant>,
}

#[derive(Debug)]
pub struct Queue {
    next_id: u32,
    pub max_visible: usize,
    pub do_not_disturb: bool,
    /// On screen, oldest first (rendered top-to-bottom, newest nearest the taskbar).
    visible: Vec<Toast>,
    /// Waiting for a free slot.
    pending: VecDeque<Notification>,
    /// Everything not transient, newest first, for the notification center.
    history: VecDeque<Notification>,
    pub history_limit: usize,
    /// History entries the user hasn't seen in the notification centre yet.
    unread: HashSet<u32>,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            next_id: 1,
            max_visible: 3,
            do_not_disturb: false,
            visible: Vec::new(),
            pending: VecDeque::new(),
            history: VecDeque::new(),
            history_limit: 100,
            unread: HashSet::new(),
        }
    }
}

impl Queue {
    /// Allocate an id for Notify(); honours `replaces_id` when it is known.
    pub fn assign_id(&mut self, replaces_id: u32) -> u32 {
        if replaces_id != 0 && self.contains(replaces_id) {
            return replaces_id;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    pub fn contains(&self, id: u32) -> bool {
        self.visible.iter().any(|t| t.notification.id == id)
            || self.pending.iter().any(|n| n.id == id)
            || self.history.iter().any(|n| n.id == id)
    }

    pub fn visible(&self) -> &[Toast] {
        &self.visible
    }

    pub fn history(&self) -> impl Iterator<Item = &Notification> {
        self.history.iter()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    fn remember(&mut self, n: &Notification) {
        if n.transient {
            return;
        }
        self.history.retain(|h| h.id != n.id);
        self.history.push_front(n.clone());
        self.unread.insert(n.id);
        while self.history.len() > self.history_limit {
            if let Some(old) = self.history.pop_back() {
                self.unread.remove(&old.id);
            }
        }
    }

    /// Record a notification without showing a toast (used while the
    /// notification centre is open, where it appears directly).
    /// Into the notification centre without a pop-up, still unread.
    pub fn push_quiet(&mut self, n: Notification) {
        self.remember(&n);
    }

    pub fn push_silent(&mut self, n: Notification) {
        self.remember(&n);
        self.unread.remove(&n.id);
    }

    pub fn unread_count(&self) -> usize {
        self.unread.len()
    }

    pub fn mark_all_read(&mut self) {
        self.unread.clear();
    }

    /// Clear the notification centre. Returns the ids removed.
    pub fn clear_history(&mut self) -> Vec<u32> {
        self.unread.clear();
        self.history.drain(..).map(|n| n.id).collect()
    }

    /// History grouped by app, newest group first, newest entry first.
    pub fn history_by_app(&self) -> Vec<(&str, Vec<&Notification>)> {
        let mut groups: Vec<(&str, Vec<&Notification>)> = Vec::new();
        for n in &self.history {
            match groups.iter_mut().find(|(app, _)| *app == n.app_name) {
                Some((_, v)) => v.push(n),
                None => groups.push((n.app_name.as_str(), vec![n])),
            }
        }
        groups
    }

    /// Add or replace a notification.
    pub fn push(&mut self, n: Notification, now: Instant) {
        self.remember(&n);
        // Replacement updates in place without resetting the slot order.
        if let Some(t) = self.visible.iter_mut().find(|t| t.notification.id == n.id) {
            t.deadline = n.display_time().map(|d| now + d);
            t.notification = n;
            return;
        }
        if let Some(p) = self.pending.iter_mut().find(|p| p.id == n.id) {
            *p = n;
            return;
        }
        if self.do_not_disturb && n.urgency != Urgency::Critical {
            return; // still in history, never shown as a toast
        }
        self.pending.push_back(n);
        self.promote(now);
    }

    /// Remove a notification wherever it is. Returns true if it existed.
    pub fn close(&mut self, id: u32, now: Instant) -> bool {
        let before = self.visible.len() + self.pending.len();
        self.visible.retain(|t| t.notification.id != id);
        self.pending.retain(|n| n.id != id);
        let removed = before != self.visible.len() + self.pending.len();
        self.promote(now);
        removed
    }

    /// Dismiss from the notification center too.
    pub fn forget(&mut self, id: u32) {
        self.history.retain(|n| n.id != id);
        self.unread.remove(&id);
    }

    /// Expire toasts whose time is up. Returns the ids that expired.
    pub fn tick(&mut self, now: Instant) -> Vec<u32> {
        let expired: Vec<u32> = self
            .visible
            .iter()
            .filter(|t| t.deadline.is_some_and(|d| d <= now))
            .map(|t| t.notification.id)
            .collect();
        if !expired.is_empty() {
            self.visible.retain(|t| !expired.contains(&t.notification.id));
            self.promote(now);
        }
        expired
    }

    /// Pause the countdown while the pointer is over the toasts.
    pub fn hold(&mut self, now: Instant) {
        for t in &mut self.visible {
            if t.deadline.is_some() {
                t.deadline = t.notification.display_time().map(|d| now + d);
            }
        }
    }

    /// Next instant at which `tick` would do something.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.visible.iter().filter_map(|t| t.deadline).min()
    }

    fn promote(&mut self, now: Instant) {
        // Critical notifications jump the line.
        while self.visible.len() < self.max_visible {
            let idx = self
                .pending
                .iter()
                .position(|n| n.urgency == Urgency::Critical)
                .unwrap_or(0);
            let Some(n) = self.pending.remove(idx) else { break };
            let deadline = n.display_time().map(|d| now + d);
            self.visible.push(Toast { notification: n, shown_at: now, deadline });
        }
    }
}

/// Notification bodies may carry a small HTML subset (`<b>`, `<i>`, `<a>`,
/// `<br>`…). We render plain text: tags are dropped, `<br>` becomes a line
/// break and entities are decoded.
pub fn strip_markup(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        match rest[lt..].find('>') {
            Some(gt) => {
                let tag = rest[lt + 1..lt + gt].trim().to_ascii_lowercase();
                if tag.starts_with("br") || tag == "p" || tag == "/p" {
                    out.push('\n');
                }
                rest = &rest[lt + gt + 1..];
            }
            None => {
                // A lone '<' is just text.
                out.push('<');
                rest = &rest[lt + 1..];
            }
        }
    }
    out.push_str(rest);
    let decoded = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    decoded.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_is_stripped() {
        assert_eq!(strip_markup("<b>Bold</b> and <i>it</i>"), "Bold and it");
        assert_eq!(strip_markup("line1<br/>line2"), "line1\nline2");
        assert_eq!(strip_markup("a &lt; b &amp;&amp; c &gt; d"), "a < b && c > d");
        assert_eq!(strip_markup("<a href=\"https://x\">link</a>"), "link");
        assert_eq!(strip_markup("3 < 4 is true"), "3 < 4 is true");
        assert_eq!(strip_markup("  plain  "), "plain");
    }

    fn n(id: u32, urgency: Urgency, timeout: i32) -> Notification {
        Notification {
            id,
            app_name: "test".into(),
            app_icon: String::new(),
            summary: format!("s{id}"),
            body: String::new(),
            actions: vec![],
            urgency,
            expire_timeout: timeout,
            transient: false,
            desktop_entry: None,
            received: SystemTime::UNIX_EPOCH,
            image: None,
        }
    }

    #[test]
    fn unread_and_grouping() {
        let t0 = Instant::now();
        let mut q = Queue::default();
        let mut a = n(1, Urgency::Normal, -1);
        a.app_name = "Mail".into();
        let mut b = n(2, Urgency::Normal, -1);
        b.app_name = "Chat".into();
        let mut c = n(3, Urgency::Normal, -1);
        c.app_name = "Mail".into();
        q.push(a, t0);
        q.push(b, t0);
        q.push(c, t0);
        assert_eq!(q.unread_count(), 3);
        let g = q.history_by_app();
        assert_eq!(g.iter().map(|(a, v)| (*a, v.len())).collect::<Vec<_>>(), vec![("Mail", 2), ("Chat", 1)]);
        assert_eq!(g[0].1[0].id, 3);
        q.forget(2);
        assert_eq!(q.unread_count(), 2);
        q.mark_all_read();
        assert_eq!(q.unread_count(), 0);
        q.push_silent(n(9, Urgency::Normal, -1));
        assert_eq!(q.unread_count(), 0);
        assert_eq!(q.history().count(), 3);
        let mut cleared = q.clear_history();
        cleared.sort();
        assert_eq!(cleared, vec![1, 3, 9]);
        assert_eq!(q.history().count(), 0);
    }

    #[test]
    fn image_hint_conversion() {
        // 2x1 RGB with padded rows (rowstride 8).
        let data = [255, 0, 0, 0, 255, 0, 9, 9];
        let img = Image::from_hint(2, 1, 8, false, 8, 3, &data).unwrap();
        assert_eq!(*img.rgba, vec![255, 0, 0, 255, 0, 255, 0, 255]);
        // RGBA passes through.
        let img = Image::from_hint(1, 1, 4, true, 8, 4, &[1, 2, 3, 4]).unwrap();
        assert_eq!(*img.rgba, vec![1, 2, 3, 4]);
        // Bad input is rejected rather than read out of bounds.
        assert!(Image::from_hint(4, 4, 16, true, 8, 4, &[0; 10]).is_none());
        assert!(Image::from_hint(1, 1, 3, true, 8, 3, &[0; 3]).is_none());
        assert!(Image::from_hint(1, 1, 4, true, 16, 4, &[0; 8]).is_none());
    }

    fn ids(q: &Queue) -> Vec<u32> {
        q.visible().iter().map(|t| t.notification.id).collect()
    }

    #[test]
    fn timeouts() {
        assert_eq!(n(1, Urgency::Normal, -1).display_time(), Some(DEFAULT_TIMEOUT));
        assert_eq!(n(1, Urgency::Normal, 0).display_time(), None);
        assert_eq!(n(1, Urgency::Normal, 10).display_time(), Some(Duration::from_millis(1500)));
        assert_eq!(n(1, Urgency::Normal, 999_999).display_time(), Some(Duration::from_secs(30)));
        assert_eq!(n(1, Urgency::Critical, 3000).display_time(), None);
    }

    #[test]
    fn overflow_waits_then_promotes() {
        let t0 = Instant::now();
        let mut q = Queue::default();
        for i in 1..=5 {
            q.push(n(i, Urgency::Normal, -1), t0);
        }
        assert_eq!(ids(&q), [1, 2, 3]);
        assert_eq!(q.pending_len(), 2);
        let expired = q.tick(t0 + DEFAULT_TIMEOUT);
        assert_eq!(expired, [1, 2, 3]);
        assert_eq!(ids(&q), [4, 5]);
    }

    #[test]
    fn critical_jumps_the_line_and_stays() {
        let t0 = Instant::now();
        let mut q = Queue { max_visible: 1, ..Default::default() };
        q.push(n(1, Urgency::Normal, -1), t0);
        q.push(n(2, Urgency::Normal, -1), t0);
        q.push(n(3, Urgency::Critical, -1), t0);
        q.tick(t0 + DEFAULT_TIMEOUT);
        assert_eq!(ids(&q), [3]);
        q.tick(t0 + Duration::from_secs(3600));
        assert_eq!(ids(&q), [3]);
        assert!(q.close(3, t0));
        assert_eq!(ids(&q), [2]);
    }

    #[test]
    fn replace_in_place() {
        let t0 = Instant::now();
        let mut q = Queue::default();
        let id = q.assign_id(0);
        q.push(n(id, Urgency::Normal, -1), t0);
        let second = q.assign_id(0);
        q.push(n(second, Urgency::Normal, -1), t0);
        assert_eq!(q.assign_id(id), id);
        let mut updated = n(id, Urgency::Normal, -1);
        updated.summary = "updated".into();
        q.push(updated, t0 + Duration::from_secs(4));
        assert_eq!(ids(&q), [1, 2]);
        assert_eq!(q.visible()[0].notification.summary, "updated");
        // Replacement restarted its timer.
        assert_eq!(q.tick(t0 + DEFAULT_TIMEOUT), vec![2]);
        assert_eq!(q.history().count(), 2);
    }

    #[test]
    fn unknown_replaces_id_gets_fresh_id() {
        let mut q = Queue::default();
        assert_eq!(q.assign_id(42), 1);
        assert_eq!(q.assign_id(0), 2);
    }

    #[test]
    fn do_not_disturb() {
        let t0 = Instant::now();
        let mut q = Queue { do_not_disturb: true, ..Default::default() };
        q.push(n(1, Urgency::Normal, -1), t0);
        q.push(n(2, Urgency::Critical, -1), t0);
        assert_eq!(ids(&q), [2]);
        assert_eq!(q.history().count(), 2);
    }

    #[test]
    fn transient_not_in_history() {
        let mut q = Queue::default();
        let mut t = n(1, Urgency::Low, -1);
        t.transient = true;
        q.push(t, Instant::now());
        assert_eq!(q.history().count(), 0);
        assert_eq!(q.visible().len(), 1);
    }

    #[test]
    fn hover_hold_extends() {
        let t0 = Instant::now();
        let mut q = Queue::default();
        q.push(n(1, Urgency::Normal, -1), t0);
        q.hold(t0 + Duration::from_secs(4));
        assert!(q.tick(t0 + DEFAULT_TIMEOUT).is_empty());
        assert_eq!(q.next_deadline(), Some(t0 + Duration::from_secs(4) + DEFAULT_TIMEOUT));
    }

    #[test]
    fn actions_pairs() {
        let a = Notification::parse_actions(&["default".into(), "Open".into(), "dangling".into()]);
        assert_eq!(a, vec![Action { key: "default".into(), label: "Open".into() }]);
    }
}
