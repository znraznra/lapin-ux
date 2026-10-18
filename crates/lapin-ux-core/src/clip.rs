// SPDX-License-Identifier: GPL-3.0-only
//! Clipboard history (Super+V) and the emoji / symbols panel (Super+.):
//! the history model and the keymap used to type characters into the
//! focused app.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipData {
    Text(String),
    /// PNG bytes plus size (for the thumbnail).
    Image { png: Arc<Vec<u8>>, width: u32, height: u32 },
}

impl ClipData {
    fn fingerprint(&self) -> u64 {
        let mut h = DefaultHasher::new();
        match self {
            ClipData::Text(t) => t.hash(&mut h),
            ClipData::Image { png, .. } => png.hash(&mut h),
        }
        h.finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipItem {
    pub id: u64,
    pub data: ClipData,
    pub pinned: bool,
    pub copied: SystemTime,
}

#[derive(Debug, Clone)]
pub struct History {
    items: Vec<ClipItem>,
    next_id: u64,
    /// Unpinned items kept (like the benchmark's 25).
    pub limit: usize,
}

impl Default for History {
    fn default() -> Self {
        History { items: Vec::new(), next_id: 1, limit: 25 }
    }
}

/// Text longer than this isn't kept (it would make the panel useless and
/// is usually a file's contents, not something to re-paste).
pub const MAX_TEXT: usize = 256 * 1024;
pub const MAX_IMAGE: usize = 8 << 20;

impl History {
    pub fn items(&self) -> &[ClipItem] {
        &self.items
    }

    /// Newest first. Copying something already in the list moves it to the top.
    /// Returns false when the item isn't kept (empty, whitespace-only or too big).
    pub fn push(&mut self, data: ClipData, now: SystemTime) -> bool {
        match &data {
            ClipData::Text(t) if t.trim().is_empty() || t.len() > MAX_TEXT => return false,
            ClipData::Image { png, .. } if png.is_empty() || png.len() > MAX_IMAGE => return false,
            _ => {}
        }
        let fp = data.fingerprint();
        let pinned = match self.items.iter().position(|i| i.data.fingerprint() == fp && i.data == data) {
            Some(pos) => self.items.remove(pos).pinned,
            None => false,
        };
        let id = self.next_id;
        self.next_id += 1;
        self.items.insert(0, ClipItem { id, data, pinned, copied: now });
        let mut unpinned = 0;
        self.items.retain(|i| {
            if i.pinned {
                return true;
            }
            unpinned += 1;
            unpinned <= self.limit
        });
        true
    }

    pub fn get(&self, id: u64) -> Option<&ClipItem> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn toggle_pin(&mut self, id: u64) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.pinned = !i.pinned;
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|i| i.id != id);
    }

    /// "Clear all" keeps pinned items, like the benchmark.
    pub fn clear(&mut self) {
        self.items.retain(|i| i.pinned);
    }

    /// Restore pinned texts saved from an earlier session (oldest last).
    pub fn restore_pinned(&mut self, texts: &[String]) {
        for t in texts.iter().rev() {
            if self.push(ClipData::Text(t.clone()), SystemTime::UNIX_EPOCH)
                && let Some(first) = self.items.first_mut()
            {
                first.pinned = true;
            }
        }
    }

    pub fn pinned_texts(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| i.pinned)
            .filter_map(|i| match &i.data {
                ClipData::Text(t) => Some(t.clone()),
                ClipData::Image { .. } => None,
            })
            .collect()
    }
}

/// Short single-paragraph preview of copied text.
pub fn preview(text: &str, max_chars: usize) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).take(4).collect();
    let joined = lines.join("\n");
    if joined.chars().count() > max_chars {
        format!("{}\u{2026}", joined.chars().take(max_chars).collect::<String>())
    } else {
        joined
    }
}

/// An XKB keymap where key `n` (evdev code `n + 1`) types `text`'s n-th
/// distinct character, plus Control_L — enough for a virtual keyboard to
/// type any Unicode text or press Ctrl+V. Returns the keymap and, for each
/// char of `text`, its evdev key code.
pub fn typing_keymap(text: &str) -> (String, Vec<u32>) {
    let mut distinct: Vec<char> = Vec::new();
    let codes: Vec<u32> = text
        .chars()
        .map(|c| {
            let i = distinct.iter().position(|d| *d == c).unwrap_or_else(|| {
                distinct.push(c);
                distinct.len() - 1
            });
            i as u32 + 1
        })
        .collect();
    let ctrl = distinct.len() as u32 + 1;
    let mut keycodes = String::new();
    let mut symbols = String::new();
    for (i, c) in distinct.iter().enumerate() {
        let n = i + 1;
        keycodes.push_str(&format!("    <K{n}> = {};\n", n + 8));
        symbols.push_str(&format!("    key <K{n}> {{ [ {} ] }};\n", keysym_name(*c)));
    }
    keycodes.push_str(&format!("    <K{ctrl}> = {};\n", ctrl + 8));
    symbols.push_str(&format!("    key <K{ctrl}> {{ [ Control_L ] }};\n    modifier_map Control {{ <K{ctrl}> }};\n"));
    let keymap = format!(
        "xkb_keymap {{\nxkb_keycodes \"lapin\" {{\n    minimum = 8;\n    maximum = {};\n{keycodes}}};\nxkb_types \"lapin\" {{ include \"complete\" }};\nxkb_compatibility \"lapin\" {{ include \"complete\" }};\nxkb_symbols \"lapin\" {{\n{symbols}}};\n}};\n",
        ctrl + 9
    );
    (keymap, codes)
}

/// Evdev code of Control_L in the keymap built for `text`.
pub fn control_code(text: &str) -> u32 {
    let distinct: std::collections::HashSet<char> = text.chars().collect();
    distinct.len() as u32 + 1
}

fn keysym_name(c: char) -> String {
    match c {
        '\n' => "Return".into(),
        '\t' => "Tab".into(),
        // Letters and digits by name so Ctrl+<letter> shortcuts work.
        c if c.is_ascii_alphanumeric() => c.to_string(),
        c => format!("U{:04X}", c as u32),
    }
}

pub const KAOMOJI: &[&str] = &[
    "(\u{30C4})", "\u{00AF}\\_(\u{30C4})_/\u{00AF}", "(\u{256F}\u{00B0}\u{25A1}\u{00B0})\u{256F}\u{FE35} \u{253B}\u{2501}\u{253B}",
    "\u{252C}\u{2500}\u{252C}\u{30CE}( \u{00BA} _ \u{00BA}\u{30CE})", "( \u{0361}\u{00B0} \u{035C}\u{0296} \u{0361}\u{00B0})",
    "(\u{2565}\u{FE4F}\u{2565})", "(\u{FF61}\u{25D5}\u{203F}\u{25D5}\u{FF61})", "\u{0295}\u{2022}\u{1D25}\u{2022}\u{0294}",
    "(\u{2022}_\u{2022})", "( \u{2022}_\u{2022})>\u{2310}\u{25A0}-\u{25A0}", "(\u{2310}\u{25A0}_\u{25A0})", "\u{0CA0}_\u{0CA0}",
    "(\u{3065}\u{FF61}\u{25D5}\u{203F}\u{203F}\u{25D5}\u{FF61})\u{3065}", "(\u{FF89}\u{25D5}\u{30EE}\u{25D5})\u{FF89}*:\u{FF65}\u{FF9F}\u{2727}",
    "(\u{2267}\u{25E1}\u{2266})", "(*^\u{25BD}^*)", "(\u{00B4}\u{30FB}\u{03C9}\u{30FB}`)", "(\u{FF1B}\u{4E00}_\u{4E00})",
    "(\u{0E07} \u{2022}\u{0300}_\u{2022}\u{0301})\u{0E07}", "\u{30FD}(\u{3007}\u{0414}\u{3007})\u{FF89}", "(\u{FF0F}\u{FF9F}\u{0414}\u{FF9F})\u{FF0F}",
    "(\u{1D54}\u{1D25}\u{1D54})", "(\u{3063}\u{02D8}\u{06A1}\u{02D8}\u{03C2})", "\u{2570}(*\u{00B4}\u{fe36}`*)\u{256F}\u{2661}",
];

/// Symbol groups shown in the Symbols tab.
pub const SYMBOLS: &[(&str, &str)] = &[
    ("Punctuation", "\u{2013}\u{2014}\u{2026}\u{00AB}\u{00BB}\u{2039}\u{203A}\u{201C}\u{201D}\u{2018}\u{2019}\u{201E}\u{00A1}\u{00BF}\u{00B7}\u{2022}\u{00A7}\u{00B6}\u{2020}\u{2021}\u{2030}"),
    ("Currency", "\u{20AC}\u{00A3}\u{00A5}\u{00A2}\u{20B9}\u{20A9}\u{20BD}\u{20BA}\u{20B1}\u{20AB}\u{20BF}\u{20AA}\u{0E3F}\u{20B4}"),
    ("Math", "\u{00B1}\u{00D7}\u{00F7}\u{2260}\u{2248}\u{2264}\u{2265}\u{221E}\u{221A}\u{2211}\u{220F}\u{222B}\u{2202}\u{2206}\u{03C0}\u{00B0}\u{00B5}\u{2030}\u{00BD}\u{00BC}\u{00BE}\u{00B2}\u{00B3}\u{2208}\u{2229}\u{222A}\u{2200}\u{2203}"),
    ("Arrows", "\u{2190}\u{2191}\u{2192}\u{2193}\u{2194}\u{2195}\u{21D0}\u{21D2}\u{21D4}\u{21BA}\u{21BB}\u{2934}\u{2935}\u{27A4}"),
    ("Latin", "\u{00E1}\u{00E0}\u{00E2}\u{00E4}\u{00E3}\u{00E5}\u{00E6}\u{00E7}\u{00E9}\u{00E8}\u{00EA}\u{00EB}\u{00ED}\u{00EE}\u{00EF}\u{00F1}\u{00F3}\u{00F4}\u{00F6}\u{00F5}\u{00F8}\u{0153}\u{00DF}\u{00FA}\u{00F9}\u{00FB}\u{00FC}\u{00FF}"),
    ("Greek", "\u{03B1}\u{03B2}\u{03B3}\u{03B4}\u{03B5}\u{03B6}\u{03B7}\u{03B8}\u{03BB}\u{03BC}\u{03C0}\u{03C1}\u{03C3}\u{03C4}\u{03C6}\u{03C8}\u{03C9}\u{03A9}\u{03A3}\u{0394}"),
    ("Other", "\u{00A9}\u{00AE}\u{2122}\u{2713}\u{2717}\u{2605}\u{2606}\u{2665}\u{2660}\u{2663}\u{2666}\u{266A}\u{266B}\u{2600}\u{2601}\u{2602}\u{2603}\u{2318}\u{2325}\u{21E7}"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> ClipData {
        ClipData::Text(s.into())
    }

    #[test]
    fn history_dedupes_pins_and_limits() {
        let mut h = History { limit: 3, ..Default::default() };
        let now = SystemTime::UNIX_EPOCH;
        for s in ["a", "b", "c"] {
            assert!(h.push(t(s), now));
        }
        assert!(!h.push(t("   "), now));
        h.push(t("a"), now); // moves to top
        let texts: Vec<_> = h.items().iter().map(|i| i.data.clone()).collect();
        assert_eq!(texts, vec![t("a"), t("c"), t("b")]);
        let b = h.items()[2].id;
        h.toggle_pin(b);
        for s in ["d", "e", "f"] {
            h.push(t(s), now);
        }
        // b is pinned so it survives; only 3 unpinned are kept.
        let texts: Vec<_> = h.items().iter().map(|i| i.data.clone()).collect();
        assert_eq!(texts, vec![t("f"), t("e"), t("d"), t("b")]);
        h.clear();
        assert_eq!(h.items().len(), 1);
        assert_eq!(h.pinned_texts(), vec!["b".to_string()]);
        let mut h2 = History::default();
        h2.restore_pinned(&["x".into(), "y".into()]);
        assert_eq!(h2.pinned_texts(), vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn previews() {
        assert_eq!(preview("  \nhello\n\n world \n", 50), "hello\n world");
        assert_eq!(preview("abcdef", 3), "abc\u{2026}");
    }

    #[test]
    fn keymap() {
        let (km, codes) = typing_keymap("aa😀\n");
        assert_eq!(codes, vec![1, 1, 2, 3]);
        assert!(km.contains("key <K1> { [ a ] };"));
        assert!(km.contains("key <K2> { [ U1F600 ] };"));
        assert!(km.contains("key <K3> { [ Return ] };"));
        assert!(km.contains("key <K4> { [ Control_L ] };"));
        assert!(km.contains("maximum = 13;"));
        assert_eq!(control_code("aa😀\n"), 4);
    }
}
