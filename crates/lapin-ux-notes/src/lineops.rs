// SPDX-License-Identifier: GPL-3.0-only
//! Line operations, comments, brackets, indentation and folding.
//! Each edit is one undo step.

use crate::buffer::{Buffer, Pos};

/// First and last line touched by the selection (a selection that ends at
/// column 0 of a line does not touch that line).
pub fn selected_lines(b: &Buffer) -> (usize, usize) {
    let (a, z) = b.selection();
    let end = if z.col == 0 && z.line > a.line { z.line - 1 } else { z.line };
    (a.line, end)
}

fn rewrite(b: &mut Buffer, first: usize, last: usize, new: Vec<String>, caret: Pos) {
    let end = Pos::new(last, b.line_len(last));
    b.replace_range(Pos::new(first, 0), end, &new.join("\n"));
    b.set_caret(caret, false);
}

pub fn duplicate(b: &mut Buffer) {
    let (f, l) = selected_lines(b);
    let mut v: Vec<String> = b.lines()[f..=l].to_vec();
    let again = v.clone();
    v.extend(again);
    let n = l - f + 1;
    let c = b.caret;
    rewrite(b, f, l, v, Pos::new(c.line + n, c.col));
}

pub fn delete_lines(b: &mut Buffer) {
    let (f, l) = selected_lines(b);
    if b.line_count() == 1 || (f == 0 && l + 1 == b.line_count()) {
        b.replace_range(Pos::new(0, 0), Pos::new(b.line_count() - 1, b.line_len(b.line_count() - 1)), "");
        return;
    }
    if l + 1 < b.line_count() {
        b.replace_range(Pos::new(f, 0), Pos::new(l + 1, 0), "");
        b.set_caret(Pos::new(f, 0), false);
    } else {
        b.replace_range(Pos::new(f - 1, b.line_len(f - 1)), Pos::new(l, b.line_len(l)), "");
        b.set_caret(Pos::new(f - 1, 0), false);
    }
}

pub fn delete_line_at(b: &mut Buffer, line: usize) {
    b.set_caret(Pos::new(line, 0), false);
    delete_lines(b);
}

/// `dir` is -1 (up) or 1 (down). Returns false when it cannot move.
pub fn move_lines(b: &mut Buffer, dir: i32) -> bool {
    let (f, l) = selected_lines(b);
    let n = b.line_count();
    if (dir < 0 && f == 0) || (dir > 0 && l + 1 >= n) {
        return false;
    }
    let (from, to) = if dir < 0 { (f - 1, l) } else { (f, l + 1) };
    let mut v: Vec<String> = b.lines()[from..=to].to_vec();
    if dir < 0 {
        v.rotate_left(1);
    } else {
        v.rotate_right(1);
    }
    let c = b.caret;
    let nc = Pos::new((c.line as i32 + dir) as usize, c.col);
    rewrite(b, from, to, v, nc);
    true
}

/// Joins the selected lines (or the caret line and the next) with one space.
pub fn join(b: &mut Buffer) {
    let (f, mut l) = selected_lines(b);
    if f == l {
        l = (f + 1).min(b.line_count() - 1);
    }
    if f == l {
        return;
    }
    let mut s = b.line(f).trim_end().to_string();
    for i in f + 1..=l {
        let t = b.line(i).trim();
        if !t.is_empty() {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(t);
        }
    }
    let col = b.line_len(f);
    rewrite(b, f, l, vec![s], Pos::new(f, col));
}

/// Splits the selected lines (or the caret line) at word boundaries so none
/// is longer than `width` characters.
pub fn split_long(b: &mut Buffer, width: usize) {
    let (f, l) = selected_lines(b);
    let mut out = Vec::new();
    for i in f..=l {
        let mut cur = String::new();
        for w in b.line(i).split(' ') {
            if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > width {
                out.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(w);
        }
        out.push(cur);
    }
    rewrite(b, f, l, out, Pos::new(f, 0));
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortKind {
    Ascending,
    Descending,
    IgnoreCase,
    Numeric,
    ByLength,
    Reverse,
}

/// Sorts the selected lines, or all of them when nothing is selected.
pub fn sort(b: &mut Buffer, kind: SortKind) {
    let (f, l) = if b.has_selection() { selected_lines(b) } else { (0, b.line_count() - 1) };
    let mut v: Vec<String> = b.lines()[f..=l].to_vec();
    match kind {
        SortKind::Ascending => v.sort(),
        SortKind::Descending => v.sort_by(|a, c| c.cmp(a)),
        SortKind::IgnoreCase => v.sort_by_key(|s| s.to_lowercase()),
        SortKind::ByLength => v.sort_by_key(|s| s.chars().count()),
        SortKind::Reverse => v.reverse(),
        SortKind::Numeric => v.sort_by(|a, c| {
            let n = |s: &str| {
                let t: String = s.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
                t.parse::<f64>().ok()
            };
            match (n(a), n(c)) {
                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                _ => a.cmp(c),
            }
        }),
    }
    rewrite(b, f, l, v, Pos::new(f, 0));
}

/// Removes repeated lines, keeping the first of each. Returns how many went.
pub fn remove_duplicates(b: &mut Buffer) -> usize {
    let (f, l) = if b.has_selection() { selected_lines(b) } else { (0, b.line_count() - 1) };
    let mut seen = std::collections::HashSet::new();
    let v: Vec<String> = b.lines()[f..=l].iter().filter(|s| seen.insert((*s).clone())).cloned().collect();
    let removed = l - f + 1 - v.len();
    if removed > 0 {
        rewrite(b, f, l, v, Pos::new(f, 0));
    }
    removed
}

pub fn remove_empty_lines(b: &mut Buffer) -> usize {
    let n = b.line_count();
    let v: Vec<String> = b.lines().iter().filter(|s| !s.trim().is_empty()).cloned().collect();
    let removed = n - v.len();
    if removed > 0 {
        rewrite(b, 0, n - 1, v, Pos::new(0, 0));
    }
    removed
}

pub fn trim_trailing(b: &mut Buffer) -> usize {
    let n = b.line_count();
    let mut changed = 0;
    let v: Vec<String> = b
        .lines()
        .iter()
        .map(|s| {
            let t = s.trim_end_matches([' ', '\t']);
            if t.len() != s.len() {
                changed += 1;
            }
            t.to_string()
        })
        .collect();
    if changed > 0 {
        let c = b.caret;
        rewrite(b, 0, n - 1, v, c);
    }
    changed
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    Title,
}

pub fn change_case(b: &mut Buffer, case: Case) {
    let text = b.selected_text();
    if text.is_empty() {
        return;
    }
    let out = match case {
        Case::Upper => text.to_uppercase(),
        Case::Lower => text.to_lowercase(),
        Case::Title => {
            let mut s = String::new();
            let mut start = true;
            for c in text.chars() {
                if c.is_alphanumeric() {
                    s.extend(if start { c.to_uppercase().collect::<String>() } else { c.to_lowercase().collect::<String>() }.chars());
                    start = false;
                } else {
                    s.push(c);
                    start = true;
                }
            }
            s
        }
    };
    let (a, z) = b.selection();
    b.replace_selection(&out);
    b.select_range(a, Pos::new(a.line, a.col)); // placeholder, fixed below
    let end = {
        let parts: Vec<&str> = out.split('\n').collect();
        if parts.len() == 1 { Pos::new(a.line, a.col + out.chars().count()) } else { Pos::new(a.line + parts.len() - 1, parts[parts.len() - 1].chars().count()) }
    };
    let _ = z;
    b.select_range(a, end);
}

// --- indentation ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    pub use_spaces: bool,
    pub width: usize,
}

impl Indent {
    pub fn unit(self) -> String {
        if self.use_spaces { " ".repeat(self.width) } else { "\t".into() }
    }
}

/// Guesses tabs or spaces from the first indented lines.
pub fn detect_indent(b: &Buffer, fallback: Indent) -> Indent {
    let (mut tabs, mut spaces) = (0, 0);
    let mut widths = std::collections::HashMap::new();
    let mut prev = 0usize;
    for l in b.lines().iter().take(500) {
        if l.starts_with('\t') {
            tabs += 1;
        } else if l.starts_with(' ') {
            let n = l.len() - l.trim_start_matches(' ').len();
            if !l.trim().is_empty() {
                spaces += 1;
                if n > prev && n - prev <= 8 {
                    *widths.entry(n - prev).or_insert(0) += 1;
                }
                prev = n;
            }
        } else if !l.trim().is_empty() {
            prev = 0;
        }
    }
    if tabs > spaces {
        Indent { use_spaces: false, width: fallback.width }
    } else if spaces > 0 {
        let w = widths.into_iter().max_by_key(|(w, c)| (*c, usize::MAX - *w)).map(|(w, _)| w).unwrap_or(fallback.width);
        Indent { use_spaces: true, width: w }
    } else {
        fallback
    }
}

fn leading(s: &str) -> &str {
    &s[..s.len() - s.trim_start_matches([' ', '\t']).len()]
}

/// Text for Enter: the break, the current line's indent, one level deeper
/// after an opening bracket or colon, and the closing bracket on its own
/// line when Enter is pressed between `{` and `}`.
pub fn newline_text(b: &Buffer, ind: Indent) -> (String, usize) {
    let c = b.caret;
    let line = b.line(c.line);
    let before: String = line.chars().take(c.col).collect();
    let after: String = line.chars().skip(c.col).collect();
    let base = leading(&before).to_string();
    let t = before.trim_end();
    let opens = t.ends_with(['{', '(', '[']) || t.ends_with(':');
    let next = after.trim_start().chars().next();
    let closes = matches!(next, Some('}' | ')' | ']'));
    if opens && closes && !t.ends_with(':') {
        let inner = format!("{base}{}", ind.unit());
        let n = 1 + inner.chars().count();
        (format!("\n{inner}\n{base}"), n)
    } else if opens {
        let s = format!("\n{base}{}", ind.unit());
        let n = s.chars().count();
        (s, n)
    } else {
        let s = format!("\n{base}");
        let n = s.chars().count();
        (s, n)
    }
}

/// Tab / Shift+Tab on a multi-line selection or a caret.
pub fn indent_lines(b: &mut Buffer, ind: Indent, out: bool) {
    let (f, l) = selected_lines(b);
    let unit = ind.unit();
    let v: Vec<String> = (f..=l)
        .map(|i| {
            let s = b.line(i);
            if !out {
                if s.is_empty() { String::new() } else { format!("{unit}{s}") }
            } else if let Some(r) = s.strip_prefix('\t') {
                r.to_string()
            } else {
                let n = s.chars().take(ind.width).take_while(|c| *c == ' ').count();
                s[n..].to_string()
            }
        })
        .collect();
    let (a, z) = b.selection();
    let collapsed = a == z;
    let before = b.line(f).chars().count();
    rewrite(b, f, l, v, a);
    if collapsed {
        // No selection: keep a caret at the matching column.
        let after = b.line_len(f);
        let col = if after >= before { a.col + (after - before) } else { a.col.saturating_sub(before - after) };
        b.select_range(Pos::new(f, col.min(after)), Pos::new(f, col.min(after)));
    } else {
        b.select_range(Pos::new(f, 0), Pos::new(l, b.line_len(l)));
    }
}

// --- comments ---

/// (line comment, block comment open/close) for a language name.
pub fn comment_syntax(lang: &str) -> (Option<&'static str>, Option<(&'static str, &'static str)>) {
    match lang {
        "Rust" | "C" | "C++" | "C#" | "Java" | "JavaScript" | "TypeScript" | "Go" | "Swift" | "Kotlin" | "PHP" | "JSON" => (Some("//"), Some(("/*", "*/"))),
        "Python" | "Shell" | "Ruby" | "Perl" | "TOML" | "YAML" | "Makefile" | "Dockerfile" | "R" => (Some("#"), None),
        "SQL" | "Lua" | "Haskell" => (Some("--"), None),
        "HTML" | "XML" | "Markdown" => (None, Some(("<!--", "-->"))),
        "CSS" => (None, Some(("/*", "*/"))),
        "INI" => (Some(";"), None),
        "LaTeX" => (Some("%"), None),
        _ => (None, None),
    }
}

/// Comments or uncomments the selected lines. Returns false when the
/// language has no comment syntax.
pub fn toggle_comment(b: &mut Buffer, lang: &str) -> bool {
    let (line_c, block_c) = comment_syntax(lang);
    let (f, l) = selected_lines(b);
    let Some(tok) = line_c else {
        let Some((o, c)) = block_c else { return false };
        let (open, close) = (format!("{o} "), format!(" {c}"));
        let lines: Vec<String> = (f..=l).map(|i| b.line(i).to_string()).collect();
        let all = lines.iter().all(|s| s.trim().is_empty() || (s.trim_start().starts_with(o) && s.trim_end().ends_with(c)));
        let v = lines
            .iter()
            .map(|s| {
                if s.trim().is_empty() {
                    s.clone()
                } else if all {
                    let ind = leading(s).to_string();
                    let t = s.trim();
                    // Strip sequentially so `/*/` (open and close overlap)
                    // cannot panic; text that is too short stays as it is.
                    let inner = t.strip_prefix(o).and_then(|r| r.strip_suffix(c)).unwrap_or(t);
                    let inner = inner.strip_prefix(' ').unwrap_or(inner);
                    let inner = inner.strip_suffix(' ').unwrap_or(inner);
                    format!("{ind}{inner}")
                } else {
                    format!("{}{open}{}{close}", leading(s), s.trim_start())
                }
            })
            .collect();
        let c0 = b.caret;
        rewrite(b, f, l, v, c0);
        return true;
    };
    let lines: Vec<String> = (f..=l).map(|i| b.line(i).to_string()).collect();
    let all = lines.iter().filter(|s| !s.trim().is_empty()).all(|s| s[leading(s).len()..].starts_with(tok));
    let min_ind = lines.iter().filter(|s| !s.trim().is_empty()).map(|s| leading(s).len()).min().unwrap_or(0);
    let v: Vec<String> = lines
        .iter()
        .map(|s| {
            if s.trim().is_empty() {
                s.clone()
            } else if all {
                let ind = leading(s);
                let rest = s[ind.len()..].strip_prefix(tok).unwrap_or(&s[ind.len()..]);
                format!("{ind}{}", rest.strip_prefix(' ').unwrap_or(rest))
            } else {
                // `min_ind` is a byte count of the shortest indent; cut at a
                // char boundary so mixed whitespace (U+2003) cannot panic.
                let cut = (0..=min_ind.min(s.len())).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
                format!("{}{tok} {}", &s[..cut], &s[cut..])
            }
        })
        .collect();
    let c0 = b.caret;
    rewrite(b, f, l, v, c0);
    true
}

// --- brackets ---

const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

/// The bracket matching the one at or just before the caret: (this, other).
/// Looks at most `limit` lines each way. `<` `>` are not matched.
pub fn matching_bracket(b: &Buffer, limit: usize) -> Option<(Pos, Pos)> {
    let c = b.caret;
    let chars: Vec<char> = b.line(c.line).chars().collect();
    let cand = [c.col, c.col.wrapping_sub(1)];
    for col in cand {
        let Some(&ch) = chars.get(col) else { continue };
        for &(o, cl) in &PAIRS[..3] {
            if ch == o {
                return scan(b, Pos::new(c.line, col), o, cl, 1, limit).map(|p| (Pos::new(c.line, col), p));
            }
            if ch == cl {
                return scan(b, Pos::new(c.line, col), cl, o, -1, limit).map(|p| (Pos::new(c.line, col), p));
            }
        }
    }
    None
}

fn scan(b: &Buffer, from: Pos, same: char, other: char, dir: i32, limit: usize) -> Option<Pos> {
    let mut depth = 0i32;
    let mut line = from.line as i64;
    let mut first = true;
    while line >= 0 && (line as usize) < b.line_count() && (line - from.line as i64).unsigned_abs() as usize <= limit {
        let chars: Vec<char> = b.line(line as usize).chars().collect();
        let idx: Vec<usize> = if dir > 0 { (0..chars.len()).collect() } else { (0..chars.len()).rev().collect() };
        for i in idx {
            if first {
                if (dir > 0 && i < from.col) || (dir < 0 && i > from.col) {
                    continue;
                }
            }
            let ch = chars[i];
            if ch == same {
                depth += 1;
            } else if ch == other {
                depth -= 1;
                if depth == 0 {
                    return Some(Pos::new(line as usize, i));
                }
            }
        }
        first = false;
        line += dir as i64;
    }
    None
}

/// What to type for an opening character: the pair to insert and the caret
/// offset, or None when it should be typed plainly.
pub fn auto_close(b: &Buffer, typed: char) -> Option<String> {
    let c = b.caret;
    let next = b.line(c.line).chars().nth(c.col);
    let before = c.col.checked_sub(1).and_then(|i| b.line(c.line).chars().nth(i));
    let word = |ch: Option<char>| ch.map(|x| x.is_alphanumeric()).unwrap_or(false);
    match typed {
        '(' => Some("()".into()),
        '[' => Some("[]".into()),
        '{' => Some("{}".into()),
        '"' | '\'' | '`' if !word(next) && !word(before) && next != Some(typed) => Some(format!("{typed}{typed}")),
        _ => None,
    }
}

/// True when typing `ch` should just step over the identical next character.
pub fn is_overtype(b: &Buffer, ch: char) -> bool {
    matches!(ch, ')' | ']' | '}' | '"' | '\'' | '`') && !b.has_selection() && b.line(b.caret.line).chars().nth(b.caret.col) == Some(ch)
}

// --- folding ---

/// Folding ranges from indentation: a line that is followed by deeper
/// lines folds them. Returns (first hidden line, last hidden line).
pub fn fold_ranges(b: &Buffer) -> Vec<(usize, usize)> {
    let n = b.line_count();
    let ind: Vec<Option<usize>> = b
        .lines()
        .iter()
        .map(|s| if s.trim().is_empty() { None } else { Some(s.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum()) })
        .collect();
    let mut out = Vec::new();
    for i in 0..n {
        let Some(base) = ind[i] else { continue };
        let mut last = None;
        let mut j = i + 1;
        while j < n {
            match ind[j] {
                None => {}
                Some(x) if x > base => last = Some(j),
                Some(_) => break,
            }
            j += 1;
        }
        if let Some(e) = last {
            out.push((i + 1, e));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SP4: Indent = Indent { use_spaces: true, width: 4 };

    fn sel(b: &mut Buffer, a: (usize, usize), z: (usize, usize)) {
        b.select_range(Pos::new(a.0, a.1), Pos::new(z.0, z.1));
    }

    #[test]
    fn duplicate_and_delete() {
        let mut b = Buffer::new("a\nb\nc");
        b.set_caret(Pos::new(1, 0), false);
        duplicate(&mut b);
        assert_eq!(b.text(), "a\nb\nb\nc");
        assert_eq!(b.caret.line, 2);
        delete_lines(&mut b);
        assert_eq!(b.text(), "a\nb\nc");
        b.set_caret(Pos::new(2, 0), false);
        delete_lines(&mut b);
        assert_eq!(b.text(), "a\nb");
    }

    #[test]
    fn move_up_down() {
        let mut b = Buffer::new("a\nb\nc");
        b.set_caret(Pos::new(2, 0), false);
        assert!(move_lines(&mut b, -1));
        assert_eq!(b.text(), "a\nc\nb");
        assert_eq!(b.caret.line, 1);
        assert!(!move_lines(&mut { let mut x = Buffer::new("a"); x }, 1));
        b.undo();
        assert_eq!(b.text(), "a\nb\nc");
    }

    #[test]
    fn join_and_split() {
        let mut b = Buffer::new("a\n  b\nc");
        sel(&mut b, (0, 0), (1, 3));
        join(&mut b);
        assert_eq!(b.text(), "a b\nc");
        let mut b = Buffer::new("aaa bbb ccc ddd");
        split_long(&mut b, 7);
        assert_eq!(b.text(), "aaa bbb\nccc ddd");
    }

    #[test]
    fn sorting_and_duplicates() {
        let mut b = Buffer::new("b\na\nc\na");
        sort(&mut b, SortKind::Ascending);
        assert_eq!(b.text(), "a\na\nb\nc");
        assert_eq!(remove_duplicates(&mut b), 1);
        assert_eq!(b.text(), "a\nb\nc");
        sort(&mut b, SortKind::Descending);
        assert_eq!(b.text(), "c\nb\na");
        let mut n = Buffer::new("10\n9\n100");
        sort(&mut n, SortKind::Numeric);
        assert_eq!(n.text(), "9\n10\n100");
    }

    #[test]
    fn trim_and_empty() {
        let mut b = Buffer::new("a  \n\nb\t");
        assert_eq!(trim_trailing(&mut b), 2);
        assert_eq!(b.text(), "a\n\nb");
        assert_eq!(remove_empty_lines(&mut b), 1);
        assert_eq!(b.text(), "a\nb");
    }

    #[test]
    fn case_changes_keep_selection() {
        let mut b = Buffer::new("hello world");
        sel(&mut b, (0, 0), (0, 5));
        change_case(&mut b, Case::Upper);
        assert_eq!(b.text(), "HELLO world");
        assert_eq!(b.selected_text(), "HELLO");
        b.select_all();
        change_case(&mut b, Case::Title);
        assert_eq!(b.text(), "Hello World");
    }

    #[test]
    fn enter_keeps_and_adds_indent() {
        let b = Buffer::new("    foo");
        let mut b2 = b.clone();
        b2.move_end(false);
        assert_eq!(newline_text(&b2, SP4).0, "\n    ");
        let mut c = Buffer::new("fn x() {");
        c.move_end(false);
        assert_eq!(newline_text(&c, SP4).0, "\n    ");
        let mut d = Buffer::new("{}");
        d.set_caret(Pos::new(0, 1), false);
        assert_eq!(newline_text(&d, SP4), ("\n    \n".to_string(), 5));
    }

    #[test]
    fn indent_and_outdent() {
        let mut b = Buffer::new("a\nb");
        b.select_all();
        indent_lines(&mut b, SP4, false);
        assert_eq!(b.text(), "    a\n    b");
        b.select_all();
        indent_lines(&mut b, SP4, true);
        assert_eq!(b.text(), "a\nb");
    }

    #[test]
    fn detect_indent_kinds() {
        let b = Buffer::new("a\n\tb\n\tc");
        assert!(!detect_indent(&b, SP4).use_spaces);
        let b = Buffer::new("a\n  b\n    c\n  d");
        let i = detect_indent(&b, SP4);
        assert!(i.use_spaces && i.width == 2);
        assert_eq!(detect_indent(&Buffer::new("x"), SP4), SP4);
    }

    #[test]
    fn comments_toggle_both_ways() {
        let mut b = Buffer::new("    a\n\n    b");
        b.select_all();
        assert!(toggle_comment(&mut b, "Rust"));
        assert_eq!(b.text(), "    // a\n\n    // b");
        b.select_all();
        toggle_comment(&mut b, "Rust");
        assert_eq!(b.text(), "    a\n\n    b");
        let mut h = Buffer::new("<p>x</p>");
        toggle_comment(&mut h, "HTML");
        assert_eq!(h.text(), "<!-- <p>x</p> -->");
        toggle_comment(&mut h, "HTML");
        assert_eq!(h.text(), "<p>x</p>");
        assert!(!toggle_comment(&mut b, "Plain text"));
    }

    #[test]
    fn brackets_match_across_lines() {
        let mut b = Buffer::new("fn f() {\n  (a[1])\n}");
        b.set_caret(Pos::new(0, 8), false); // just after '{'
        let (this, other) = matching_bracket(&b, 100).unwrap();
        assert_eq!((this, other), (Pos::new(0, 7), Pos::new(2, 0)));
        b.set_caret(Pos::new(2, 1), false);
        assert_eq!(matching_bracket(&b, 100).unwrap().1, Pos::new(0, 7));
        b.set_caret(Pos::new(1, 3), false);
        assert_eq!(matching_bracket(&b, 100).unwrap().1, Pos::new(1, 8 - 1));
        assert!(matching_bracket(&Buffer::new("(("), 10).is_none());
    }

    #[test]
    fn auto_close_rules() {
        let b = Buffer::new("");
        assert_eq!(auto_close(&b, '(').as_deref(), Some("()"));
        assert_eq!(auto_close(&b, '"').as_deref(), Some("\"\""));
        assert_eq!(auto_close(&b, 'x'), None);
        let mut w = Buffer::new("don");
        w.move_end(false);
        assert_eq!(auto_close(&w, '\''), None);
        let mut o = Buffer::new("()");
        o.set_caret(Pos::new(0, 1), false);
        assert!(is_overtype(&o, ')'));
    }

    #[test]
    fn folds_follow_indentation() {
        let b = Buffer::new("a\n  b\n  c\nd\n  e");
        assert_eq!(fold_ranges(&b), vec![(1, 2), (4, 4)]);
    }
}
