// SPDX-License-Identifier: GPL-3.0-only
//! What the editor shows: document lines turned into screen rows (folded
//! lines left out, long lines wrapped) and the cell maths for tabs and
//! wide characters.

use std::collections::BTreeMap;

use unicode_width::UnicodeWidthChar;

use crate::buffer::{Buffer, Pos};

/// Width of a character in cells when it starts at cell `x`.
pub fn char_cells(c: char, x: usize, tab: usize) -> usize {
    if c == '\t' {
        tab - x % tab
    } else {
        c.width().unwrap_or(0).max(if c.is_control() { 0 } else { 0 })
    }
}

/// Cell column where character index `col` of `line` starts.
pub fn cell_of(line: &str, col: usize, tab: usize) -> usize {
    let mut x = 0;
    for c in line.chars().take(col) {
        x += char_cells(c, x, tab);
    }
    x
}

/// Character index whose cell range holds cell `x` (past the end gives the
/// line length).
pub fn col_at_cell(line: &str, x: usize, tab: usize) -> usize {
    let mut cx = 0;
    for (i, c) in line.chars().enumerate() {
        let w = char_cells(c, cx, tab);
        if x < cx + w.max(1) && x >= cx && w > 0 {
            // clicking the right half of a character puts the caret after it
            return if x * 2 >= cx * 2 + w { i + 1 } else { i };
        }
        cx += w;
        if x < cx {
            return i + 1;
        }
    }
    line.chars().count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub line: usize,
    pub from: usize,
    pub to: usize,
    /// First row of its line (the one that carries the line number).
    pub first: bool,
}

/// Folded ranges: start line (stays visible) -> last hidden line.
pub type Folds = BTreeMap<usize, usize>;

pub struct Rows {
    pub rows: Vec<Row>,
    row_of_line: Vec<usize>, // first row of each line, usize::MAX when hidden
}

impl Rows {
    pub fn build(b: &Buffer, folds: &Folds, wrap_cells: Option<usize>, tab: usize) -> Rows {
        let n = b.line_count();
        let mut rows = Vec::with_capacity(n);
        let mut row_of_line = vec![usize::MAX; n];
        let mut i = 0;
        while i < n {
            row_of_line[i] = rows.len();
            let line = b.line(i);
            match wrap_cells {
                None => rows.push(Row { line: i, from: 0, to: line.chars().count(), first: true }),
                Some(w) => wrap_line(i, line, w.max(2), tab, &mut rows),
            }
            i = match folds.get(&i) {
                Some(&last) if last > i => last + 1,
                _ => i + 1,
            };
        }
        Rows { rows, row_of_line }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Row holding position `p`; a hidden line maps to its fold's row.
    pub fn row_of(&self, p: Pos, b: &Buffer) -> usize {
        let mut l = p.line.min(self.row_of_line.len().saturating_sub(1));
        while l > 0 && self.row_of_line[l] == usize::MAX {
            l -= 1;
        }
        let first = self.row_of_line[l];
        let mut r = first;
        while r + 1 < self.rows.len() && self.rows[r + 1].line == l && p.line == l && p.col >= self.rows[r + 1].from {
            r += 1;
        }
        let _ = b;
        r
    }

    /// Position for a click at `row`, cell column `x` (relative to the row's
    /// start).
    pub fn pos_at(&self, b: &Buffer, row: usize, x: usize, tab: usize) -> Pos {
        let row = row.min(self.rows.len().saturating_sub(1));
        let r = self.rows[row];
        let line = b.line(r.line);
        let base = cell_of(line, r.from, tab);
        let col = col_at_cell(line, base + x, tab).clamp(r.from, r.to);
        // At the end of a wrapped row the caret belongs to the next row's start.
        Pos::new(r.line, col)
    }

    /// Row `delta` rows from the one holding `p`, as a position at cell `x`.
    pub fn step(&self, b: &Buffer, p: Pos, delta: isize, x: usize, tab: usize) -> Option<Pos> {
        let r = self.row_of(p, b) as isize + delta;
        if r < 0 || r as usize >= self.rows.len() {
            return None;
        }
        Some(self.pos_at(b, r as usize, x, tab))
    }
}

fn wrap_line(i: usize, line: &str, w: usize, tab: usize, out: &mut Vec<Row>) {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        out.push(Row { line: i, from: 0, to: 0, first: true });
        return;
    }
    let mut from = 0;
    let mut first = true;
    while from < chars.len() {
        let mut x = 0;
        let mut j = from;
        let mut last_break = None;
        while j < chars.len() {
            let cw = char_cells(chars[j], x, tab);
            if x + cw > w && j > from {
                break;
            }
            x += cw;
            if chars[j] == ' ' || chars[j] == '\t' {
                last_break = Some(j + 1);
            }
            j += 1;
        }
        let to = if j >= chars.len() { j } else { last_break.filter(|b| *b > from).unwrap_or(j) };
        out.push(Row { line: i, from, to, first });
        first = false;
        from = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_handle_tabs_and_wide_chars() {
        assert_eq!(cell_of("a\tb", 2, 4), 4);
        assert_eq!(cell_of("日本x", 2, 4), 4);
        assert_eq!(col_at_cell("a\tb", 4, 4), 2);
        assert_eq!(col_at_cell("abc", 99, 4), 3);
        assert_eq!(col_at_cell("abc", 0, 4), 0);
    }

    #[test]
    fn folds_hide_lines() {
        let b = Buffer::new("a\n b\n c\nd");
        let mut f = Folds::new();
        f.insert(0, 2);
        let r = Rows::build(&b, &f, None, 4);
        assert_eq!(r.len(), 2);
        assert_eq!(r.rows[1].line, 3);
        assert_eq!(r.row_of(Pos::new(1, 0), &b), 0);
    }

    #[test]
    fn wraps_at_word_breaks() {
        let b = Buffer::new("aaa bbb ccc");
        let r = Rows::build(&b, &Folds::new(), Some(8), 4);
        let s: Vec<(usize, usize)> = r.rows.iter().map(|x| (x.from, x.to)).collect();
        assert_eq!(s, vec![(0, 8), (8, 11)]);
        assert!(r.rows[0].first && !r.rows[1].first);
        assert_eq!(r.row_of(Pos::new(0, 9), &b), 1);
        assert_eq!(r.pos_at(&b, 1, 1, 4), Pos::new(0, 9));
    }

    #[test]
    fn hard_wraps_long_words() {
        let b = Buffer::new("abcdefghij");
        let r = Rows::build(&b, &Folds::new(), Some(4), 4);
        assert_eq!(r.len(), 3);
    }
}
