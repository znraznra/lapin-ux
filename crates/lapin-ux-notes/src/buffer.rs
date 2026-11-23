// SPDX-License-Identifier: GPL-3.0-only
//! The text of one tab: lines, a caret and a selection, and undo.
//!
//! Positions are `(line, column)` with the column counted in characters, so
//! the code here never splits a character. Lines never contain `\n`; the
//! file's line ending is kept separately and put back when saving.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    /// Where the change starts.
    at: Pos,
    removed: String,
    inserted: String,
    caret_before: Pos,
    caret_after: Pos,
}

#[derive(Debug, Clone)]
pub struct Buffer {
    lines: Vec<String>,
    /// The caret. `anchor` is the other end of the selection.
    pub caret: Pos,
    pub anchor: Pos,
    undo: Vec<Vec<Edit>>,
    redo: Vec<Vec<Edit>>,
    /// Edits of the current typing run are merged into one undo step.
    open_group: bool,
    /// Bumped on every change, so caches know when to rebuild.
    pub revision: u64,
    saved_revision: u64,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new("")
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn byte_at(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(i, _)| i).unwrap_or(s.len())
}

impl Buffer {
    /// `text` may use any line ending; they are all treated as breaks.
    pub fn new(text: &str) -> Self {
        let mut b = Self {
            lines: split_lines(text),
            caret: Pos::default(),
            anchor: Pos::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            open_group: false,
            revision: 0,
            saved_revision: 0,
        };
        if b.lines.is_empty() {
            b.lines.push(String::new());
        }
        b
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
    pub fn line(&self, i: usize) -> &str {
        self.lines.get(i).map(String::as_str).unwrap_or("")
    }
    pub fn line_len(&self, i: usize) -> usize {
        char_len(self.line(i))
    }
    /// Characters in the whole text (a line break counts as one).
    pub fn len_chars(&self) -> usize {
        self.lines.iter().map(|l| char_len(l)).sum::<usize>() + self.lines.len().saturating_sub(1)
    }
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
    pub fn is_modified(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        Pos { line, col: p.col.min(self.line_len(line)) }
    }

    // --- selection ---

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }
    pub fn selection(&self) -> (Pos, Pos) {
        if self.caret <= self.anchor { (self.caret, self.anchor) } else { (self.anchor, self.caret) }
    }
    pub fn selected_text(&self) -> String {
        let (a, b) = self.selection();
        self.slice(a, b)
    }
    pub fn selected_chars(&self) -> usize {
        let (a, b) = self.selection();
        self.slice(a, b).chars().count()
    }
    pub fn select_all(&mut self) {
        self.anchor = Pos::new(0, 0);
        let last = self.lines.len() - 1;
        self.caret = Pos::new(last, self.line_len(last));
    }
    pub fn set_caret(&mut self, p: Pos, extend: bool) {
        self.caret = self.clamp(p);
        if !extend {
            self.anchor = self.caret;
        }
        self.open_group = false;
    }
    pub fn select_range(&mut self, a: Pos, b: Pos) {
        self.anchor = self.clamp(a);
        self.caret = self.clamp(b);
        self.open_group = false;
    }

    /// Text between two positions (`a <= b`), lines joined with `\n`.
    pub fn slice(&self, a: Pos, b: Pos) -> String {
        let a = self.clamp(a);
        let b = self.clamp(b);
        if a.line == b.line {
            let l = self.line(a.line);
            return l[byte_at(l, a.col)..byte_at(l, b.col)].to_string();
        }
        let mut out = String::new();
        let first = self.line(a.line);
        out.push_str(&first[byte_at(first, a.col)..]);
        for i in a.line + 1..b.line {
            out.push('\n');
            out.push_str(&self.lines[i]);
        }
        out.push('\n');
        let last = self.line(b.line);
        out.push_str(&last[..byte_at(last, b.col)]);
        out
    }

    // --- movement (callers decide `extend` from Shift) ---

    pub fn move_left(&mut self, extend: bool) {
        if !extend && self.has_selection() {
            let (a, _) = self.selection();
            return self.set_caret(a, false);
        }
        let c = self.caret;
        let p = if c.col > 0 {
            Pos::new(c.line, c.col - 1)
        } else if c.line > 0 {
            Pos::new(c.line - 1, self.line_len(c.line - 1))
        } else {
            c
        };
        self.set_caret(p, extend);
    }
    pub fn move_right(&mut self, extend: bool) {
        if !extend && self.has_selection() {
            let (_, b) = self.selection();
            return self.set_caret(b, false);
        }
        let c = self.caret;
        let p = if c.col < self.line_len(c.line) {
            Pos::new(c.line, c.col + 1)
        } else if c.line + 1 < self.lines.len() {
            Pos::new(c.line + 1, 0)
        } else {
            c
        };
        self.set_caret(p, extend);
    }
    pub fn move_vertical(&mut self, delta: isize, want_col: usize, extend: bool) {
        let line = (self.caret.line as isize + delta).clamp(0, self.lines.len() as isize - 1) as usize;
        let col = want_col.min(self.line_len(line));
        // Moving past the first or last line goes to its start or end.
        let p = if delta < 0 && self.caret.line == 0 {
            Pos::new(0, 0)
        } else if delta > 0 && self.caret.line + 1 >= self.lines.len() {
            Pos::new(line, self.line_len(line))
        } else {
            Pos::new(line, col)
        };
        self.set_caret(p, extend);
    }
    /// Home: first non-blank, then the line start.
    pub fn move_home(&mut self, extend: bool) {
        let l = self.line(self.caret.line);
        let first = l.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let col = if self.caret.col == first || first == l.chars().count() { 0 } else { first };
        self.set_caret(Pos::new(self.caret.line, col), extend);
    }
    pub fn move_end(&mut self, extend: bool) {
        self.set_caret(Pos::new(self.caret.line, self.line_len(self.caret.line)), extend);
    }
    pub fn move_doc_start(&mut self, extend: bool) {
        self.set_caret(Pos::new(0, 0), extend);
    }
    pub fn move_doc_end(&mut self, extend: bool) {
        let l = self.lines.len() - 1;
        self.set_caret(Pos::new(l, self.line_len(l)), extend);
    }
    pub fn move_word(&mut self, dir: i32, extend: bool) {
        let c = self.caret;
        let chars: Vec<char> = self.line(c.line).chars().collect();
        let word = |ch: char| ch.is_alphanumeric() || ch == '_';
        let mut col = c.col;
        if dir < 0 {
            if col == 0 {
                return self.move_left(extend);
            }
            while col > 0 && !word(chars[col - 1]) {
                col -= 1;
            }
            while col > 0 && word(chars[col - 1]) {
                col -= 1;
            }
        } else {
            if col >= chars.len() {
                return self.move_right(extend);
            }
            while col < chars.len() && word(chars[col]) {
                col += 1;
            }
            while col < chars.len() && !word(chars[col]) {
                col += 1;
            }
        }
        self.set_caret(Pos::new(c.line, col), extend);
    }
    pub fn select_word_at(&mut self, p: Pos) {
        let p = self.clamp(p);
        let chars: Vec<char> = self.line(p.line).chars().collect();
        let word = |ch: char| ch.is_alphanumeric() || ch == '_';
        let (mut a, mut b) = (p.col, p.col);
        if a < chars.len() && word(chars[a]) || (a > 0 && word(chars[a - 1])) {
            while a > 0 && word(chars[a - 1]) {
                a -= 1;
            }
            while b < chars.len() && word(chars[b]) {
                b += 1;
            }
        } else if a < chars.len() {
            b = a + 1;
        }
        self.select_range(Pos::new(p.line, a), Pos::new(p.line, b));
    }
    pub fn select_line_at(&mut self, line: usize) {
        let line = line.min(self.lines.len() - 1);
        let (a, b) = if line + 1 < self.lines.len() {
            (Pos::new(line, 0), Pos::new(line + 1, 0))
        } else {
            (Pos::new(line, 0), Pos::new(line, self.line_len(line)))
        };
        self.select_range(a, b);
    }

    // --- editing ---

    /// Replaces `a..b` with `text`; the one place text changes. `group` lets
    /// consecutive typing share an undo step.
    fn apply(&mut self, a: Pos, b: Pos, text: &str, caret_before: Pos, group: bool) {
        let a = self.clamp(a);
        let b = self.clamp(b);
        let removed = self.slice(a, b);
        if removed.is_empty() && text.is_empty() {
            return;
        }
        let head = {
            let l = &self.lines[a.line];
            l[..byte_at(l, a.col)].to_string()
        };
        let tail = {
            let l = &self.lines[b.line];
            l[byte_at(l, b.col)..].to_string()
        };
        let joined = format!("{head}{text}{tail}");
        let new_lines: Vec<String> = joined.split('\n').map(str::to_string).collect();
        let caret_after = {
            let parts: Vec<&str> = text.split('\n').collect();
            if parts.len() == 1 {
                Pos::new(a.line, a.col + char_len(text))
            } else {
                Pos::new(a.line + parts.len() - 1, char_len(parts[parts.len() - 1]))
            }
        };
        self.lines.splice(a.line..=b.line, new_lines);
        self.caret = caret_after;
        self.anchor = caret_after;
        self.revision += 1;
        self.redo.clear();
        let edit = Edit { at: a, removed, inserted: text.to_string(), caret_before, caret_after };
        if group && self.open_group {
            if let Some(last) = self.undo.last_mut() {
                last.push(edit);
                return;
            }
        }
        self.undo.push(vec![edit]);
        self.open_group = group;
    }

    /// Ends the current undo group (a click, an arrow key, a pause).
    pub fn break_undo(&mut self) {
        self.open_group = false;
    }

    /// Types `text` over the selection.
    pub fn insert(&mut self, text: &str) {
        let (a, b) = self.selection();
        let before = self.caret;
        // Only plain typing merges: a word at a time.
        let merge = !self.has_selection() && !text.contains('\n') && text.chars().all(|c| !c.is_whitespace());
        self.apply(a, b, text, before, merge);
    }
    /// A paste, replace or command: always its own undo step.
    pub fn replace_selection(&mut self, text: &str) {
        let (a, b) = self.selection();
        let before = self.caret;
        self.apply(a, b, text, before, false);
    }
    pub fn replace_range(&mut self, a: Pos, b: Pos, text: &str) {
        let before = self.caret;
        self.apply(a, b, text, before, false);
    }
    pub fn backspace(&mut self) {
        if self.has_selection() {
            return self.replace_selection("");
        }
        let c = self.caret;
        let a = if c.col > 0 {
            Pos::new(c.line, c.col - 1)
        } else if c.line > 0 {
            Pos::new(c.line - 1, self.line_len(c.line - 1))
        } else {
            return;
        };
        self.apply(a, c, "", c, false);
    }
    pub fn delete(&mut self) {
        if self.has_selection() {
            return self.replace_selection("");
        }
        let c = self.caret;
        let b = if c.col < self.line_len(c.line) {
            Pos::new(c.line, c.col + 1)
        } else if c.line + 1 < self.lines.len() {
            Pos::new(c.line + 1, 0)
        } else {
            return;
        };
        self.apply(c, b, "", c, false);
    }
    pub fn delete_word(&mut self, dir: i32) {
        if self.has_selection() {
            return self.replace_selection("");
        }
        let start = self.caret;
        self.move_word(dir, true);
        if self.caret != start {
            self.replace_selection("");
        }
    }
    /// Replaces the whole text as one undo step (convert, sort, trim…).
    pub fn replace_all(&mut self, text: &str) {
        let last = self.lines.len() - 1;
        let end = Pos::new(last, self.line_len(last));
        let caret = self.caret;
        self.apply(Pos::new(0, 0), end, text, caret, false);
        self.set_caret(self.clamp(caret), false);
    }

    // --- undo ---

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self) {
        self.open_group = false;
        let Some(group) = self.undo.pop() else { return };
        let caret = group.first().map(|e| e.caret_before).unwrap_or_default();
        for e in group.iter().rev() {
            let end = end_of(e.at, &e.inserted);
            self.splice(e.at, end, &e.removed);
        }
        self.redo.push(group);
        self.revision += 1;
        self.set_caret(caret, false);
    }
    pub fn redo(&mut self) {
        self.open_group = false;
        let Some(group) = self.redo.pop() else { return };
        let caret = group.last().map(|e| e.caret_after).unwrap_or_default();
        for e in group.iter() {
            let end = end_of(e.at, &e.removed);
            self.splice(e.at, end, &e.inserted);
        }
        self.undo.push(group);
        self.revision += 1;
        self.set_caret(caret, false);
    }
    fn splice(&mut self, a: Pos, b: Pos, text: &str) {
        let head = {
            let l = &self.lines[a.line];
            l[..byte_at(l, a.col)].to_string()
        };
        let tail = {
            let l = &self.lines[b.line];
            l[byte_at(l, b.col)..].to_string()
        };
        let joined = format!("{head}{text}{tail}");
        self.lines.splice(a.line..=b.line, joined.split('\n').map(str::to_string));
    }
}

fn end_of(at: Pos, text: &str) -> Pos {
    let parts: Vec<&str> = text.split('\n').collect();
    if parts.len() == 1 {
        Pos::new(at.line, at.col + char_len(text))
    } else {
        Pos::new(at.line + parts.len() - 1, char_len(parts[parts.len() - 1]))
    }
}

/// Splits on `\r\n`, `\n` and a lone `\r`.
pub fn split_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
                out.push(std::mem::take(&mut cur));
            }
            '\n' => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_undo_groups_a_word() {
        let mut b = Buffer::new("");
        for c in "hello".chars() {
            b.insert(&c.to_string());
        }
        b.insert(" ");
        b.insert("x");
        assert_eq!(b.text(), "hello x");
        b.undo();
        b.undo();
        b.undo();
        assert_eq!(b.text(), "");
        b.redo();
        assert_eq!(b.text(), "hello");
    }

    #[test]
    fn newline_splits_and_backspace_joins() {
        let mut b = Buffer::new("ab");
        b.set_caret(Pos::new(0, 1), false);
        b.insert("\n");
        assert_eq!(b.lines(), ["a", "b"]);
        assert_eq!(b.caret, Pos::new(1, 0));
        b.backspace();
        assert_eq!(b.text(), "ab");
        assert_eq!(b.caret, Pos::new(0, 1));
    }

    #[test]
    fn selection_replace_and_undo() {
        let mut b = Buffer::new("one\ntwo\nthree");
        b.select_range(Pos::new(0, 1), Pos::new(2, 2));
        assert_eq!(b.selected_text(), "ne\ntwo\nth");
        b.replace_selection("X");
        assert_eq!(b.text(), "oXree");
        b.undo();
        assert_eq!(b.text(), "one\ntwo\nthree");
        b.redo();
        assert_eq!(b.text(), "oXree");
    }

    #[test]
    fn multibyte_columns() {
        let mut b = Buffer::new("héllo wörld");
        b.set_caret(Pos::new(0, 2), false);
        b.delete();
        assert_eq!(b.text(), "hélo wörld");
        b.move_doc_end(false);
        b.backspace();
        assert_eq!(b.text(), "hélo wörl");
    }

    #[test]
    fn modified_flag_follows_save_and_undo() {
        let mut b = Buffer::new("a");
        assert!(!b.is_modified());
        b.insert("b");
        assert!(b.is_modified());
        b.mark_saved();
        assert!(!b.is_modified());
    }

    #[test]
    fn mixed_line_endings_split() {
        assert_eq!(split_lines("a\r\nb\rc\nd"), ["a", "b", "c", "d"]);
        assert_eq!(split_lines(""), [""]);
        assert_eq!(split_lines("a\n"), ["a", ""]);
    }

    #[test]
    fn home_toggles_between_indent_and_start() {
        let mut b = Buffer::new("    code");
        b.set_caret(Pos::new(0, 6), false);
        b.move_home(false);
        assert_eq!(b.caret.col, 4);
        b.move_home(false);
        assert_eq!(b.caret.col, 0);
    }

    #[test]
    fn word_motion_and_delete() {
        let mut b = Buffer::new("foo bar baz");
        b.move_word(1, false);
        assert_eq!(b.caret.col, 4);
        b.delete_word(1);
        assert_eq!(b.text(), "foo baz");
    }
}
