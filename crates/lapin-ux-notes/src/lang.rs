// SPDX-License-Identifier: GPL-3.0-only
//! Syntax highlighting (syntect) with a per-line state cache, and the
//! Language menu's list and file-extension detection.

use std::sync::OnceLock;

use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter as Hl, Theme, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// Above this many lines highlighting is switched off so big files stay fast.
pub const MAX_HIGHLIGHT_LINES: usize = 60_000;
/// Lines longer than this are drawn plain.
pub const MAX_HIGHLIGHT_LINE: usize = 2_000;

pub const PLAIN: &str = "Plain text";

fn syntaxes() -> &'static SyntaxSet {
    static S: OnceLock<SyntaxSet> = OnceLock::new();
    S.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn themes() -> &'static ThemeSet {
    static T: OnceLock<ThemeSet> = OnceLock::new();
    T.get_or_init(ThemeSet::load_defaults)
}

/// Names for the Language menu, sorted, "Plain text" first.
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = syntaxes().syntaxes().iter().filter(|s| !s.hidden).map(|s| s.name.clone()).collect();
    v.sort_by_key(|s| s.to_lowercase());
    v.dedup();
    v.retain(|s| s != "Plain Text");
    v.insert(0, PLAIN.to_string());
    v
}

fn find(name: &str) -> Option<&'static SyntaxReference> {
    if name == PLAIN {
        return None;
    }
    syntaxes().find_syntax_by_name(name)
}

/// Language for a file name (by extension or well-known name), else Plain text.
pub fn detect(file_name: &str, first_line: &str) -> String {
    let ss = syntaxes();
    let ext = file_name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let by = ss
        .find_syntax_by_extension(ext)
        .or_else(|| ss.find_syntax_by_extension(file_name))
        .or_else(|| ss.find_syntax_by_first_line(first_line));
    match by {
        Some(s) if s.name != "Plain Text" => s.name.clone(),
        _ => match file_name {
            "Makefile" | "makefile" | "GNUmakefile" => "Makefile".into(),
            _ => PLAIN.into(),
        },
    }
}

pub type Rgb = (u8, u8, u8);

/// A coloured piece of one line.
#[derive(Debug, Clone, PartialEq)]
pub struct Seg {
    pub text: String,
    pub color: Rgb,
}

pub struct Highlighter {
    lang: String,
    dark: bool,
    theme: Theme,
    /// State at the start of line i (valid for i < states.len()).
    states: Vec<(ParseState, HighlightState)>,
    pub fg: Rgb,
}

impl Highlighter {
    pub fn new(lang: &str, dark: bool) -> Self {
        let name = if dark { "base16-ocean.dark" } else { "InspiredGitHub" };
        let theme = themes().themes[name].clone();
        let fg = theme.settings.foreground.map(|c| (c.r, c.g, c.b)).unwrap_or(if dark { (220, 220, 220) } else { (30, 30, 30) });
        Self { lang: lang.to_string(), dark, theme, states: Vec::new(), fg }
    }

    pub fn lang(&self) -> &str {
        &self.lang
    }

    pub fn is_dark(&self) -> bool {
        self.dark
    }

    pub fn active(&self) -> bool {
        find(&self.lang).is_some()
    }

    /// Lines from `line` on changed: forget what was computed for them.
    pub fn invalidate(&mut self, line: usize) {
        self.states.truncate(line + 1);
    }

    /// Coloured pieces for `lines[line]`. Plain when the language is plain
    /// or the file is too big.
    pub fn line(&mut self, lines: &[String], line: usize) -> Vec<Seg> {
        let plain = || vec![Seg { text: lines[line].clone(), color: self.fg }];
        let Some(syn) = find(&self.lang) else { return plain() };
        if lines.len() > MAX_HIGHLIGHT_LINES {
            return plain();
        }
        let ss = syntaxes();
        let hl = Hl::new(&self.theme);
        if self.states.is_empty() {
            self.states.push((ParseState::new(syn), HighlightState::new(&hl, ScopeStack::new())));
        }
        // Bring the cache up to the start of `line`.
        while self.states.len() <= line {
            let i = self.states.len() - 1;
            let (mut ps, mut hs) = self.states[i].clone();
            let text = format!("{}\n", lines[i]);
            if lines[i].len() <= MAX_HIGHLIGHT_LINE {
                if let Ok(ops) = ps.parse_line(&text, ss) {
                    for _ in HighlightIterator::new(&mut hs, &ops, &text, &hl) {}
                }
            }
            self.states.push((ps, hs));
        }
        if lines[line].len() > MAX_HIGHLIGHT_LINE {
            return plain();
        }
        let (mut ps, mut hs) = self.states[line].clone();
        let text = format!("{}\n", lines[line]);
        let Ok(ops) = ps.parse_line(&text, ss) else { return plain() };
        HighlightIterator::new(&mut hs, &ops, &text, &hl)
            .map(|(st, s)| Seg { text: s.trim_end_matches('\n').to_string(), color: (st.foreground.r, st.foreground.g, st.foreground.b) })
            .filter(|s| !s.text.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension() {
        assert_eq!(detect("main.rs", ""), "Rust");
        assert_eq!(detect("a.py", ""), "Python");
        assert_eq!(detect("notes.txt", ""), PLAIN);
        assert_eq!(detect("noext", ""), PLAIN);
        assert_eq!(detect("run", "#!/bin/bash"), "Bourne Again Shell (bash)");
    }

    #[test]
    fn menu_list_starts_plain() {
        let n = names();
        assert_eq!(n[0], PLAIN);
        assert!(n.iter().any(|x| x == "Rust"));
    }

    #[test]
    fn highlights_and_keeps_text() {
        let lines: Vec<String> = vec!["fn main() {".into(), "    let s = \"a\"; // c".into(), "}".into()];
        let mut h = Highlighter::new("Rust", false);
        for i in 0..3 {
            let segs = h.line(&lines, i);
            assert_eq!(segs.iter().map(|s| s.text.as_str()).collect::<String>(), lines[i]);
        }
        let colours: std::collections::HashSet<_> = h.line(&lines, 1).iter().map(|s| s.color).collect();
        assert!(colours.len() > 1);
    }

    #[test]
    fn multiline_state_and_invalidate() {
        let mut lines: Vec<String> = vec!["/* a".into(), "b */ x".into()];
        let mut h = Highlighter::new("Rust", true);
        let before = h.line(&lines, 1);
        lines[0] = "// a".into();
        h.invalidate(0);
        let after = h.line(&lines, 1);
        assert_ne!(before, after);
    }

    #[test]
    fn plain_is_one_segment() {
        let lines = vec!["x y".to_string()];
        let mut h = Highlighter::new(PLAIN, false);
        assert!(!h.active());
        assert_eq!(h.line(&lines, 0).len(), 1);
    }
}
