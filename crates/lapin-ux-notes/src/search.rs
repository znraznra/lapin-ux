// SPDX-License-Identifier: GPL-3.0-only
//! Find, replace, find in files. Matches never span lines.

use crate::buffer::{Buffer, Pos};
use regex::Regex;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub regex: bool,
    pub match_case: bool,
    pub whole_word: bool,
}

impl Query {
    /// `Err` holds the regex error text, for the user to read.
    pub fn compile(&self) -> Result<Option<Regex>, String> {
        if self.text.is_empty() {
            return Ok(None);
        }
        let mut pat = if self.regex { self.text.clone() } else { regex::escape(&self.text) };
        if self.whole_word {
            pat = format!(r"\b(?:{pat})\b");
        }
        if !self.match_case {
            pat = format!("(?i){pat}");
        }
        regex::RegexBuilder::new(&pat).size_limit(10 << 20).build().map(Some).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// Character column of every byte offset asked for, in one forward pass.
/// `offs` must be sorted ascending and sit on char boundaries.
fn cols_for(line: &str, offs: &[usize]) -> Vec<usize> {
    let mut out = Vec::with_capacity(offs.len());
    let (mut byte, mut col) = (0usize, 0usize);
    for &o in offs {
        col += line[byte..o].chars().count();
        byte = o;
        out.push(col);
    }
    out
}

pub fn matches_in_line(re: &Regex, line: &str, idx: usize) -> Vec<Match> {
    let found: Vec<(usize, usize)> =
        re.find_iter(line).filter(|m| !m.as_str().is_empty()).map(|m| (m.start(), m.end())).collect();
    let offs: Vec<usize> = found.iter().flat_map(|&(s, e)| [s, e]).collect();
    let cols = cols_for(line, &offs);
    found.iter().enumerate().map(|(i, _)| Match { line: idx, start: cols[2 * i], end: cols[2 * i + 1] }).collect()
}

pub fn count_all(b: &Buffer, re: &Regex) -> usize {
    b.lines().iter().enumerate().map(|(i, l)| matches_in_line(re, l, i).len()).sum()
}

pub fn all_matches(b: &Buffer, re: &Regex, limit: usize) -> Vec<Match> {
    let mut out = Vec::new();
    for (i, l) in b.lines().iter().enumerate() {
        out.extend(matches_in_line(re, l, i));
        if out.len() >= limit {
            out.truncate(limit);
            break;
        }
    }
    out
}

/// The next match after `from` (or before it, going back), wrapping round.
pub fn find(b: &Buffer, re: &Regex, from: Pos, forward: bool) -> Option<Match> {
    let n = b.line_count();
    if forward {
        for k in 0..=n {
            let i = (from.line + k) % n;
            for m in matches_in_line(re, b.line(i), i) {
                // The first and last visit are the start line: only the part
                // after (or before) `from` counts on each.
                let ok = if k == 0 { m.start >= from.col } else if k == n { m.start < from.col } else { true };
                if ok {
                    return Some(m);
                }
            }
        }
    } else {
        for k in 0..=n {
            let i = (from.line + n - (k % n)) % n;
            let ms = matches_in_line(re, b.line(i), i);
            for m in ms.into_iter().rev() {
                let ok = if k == 0 { m.end <= from.col } else if k == n { m.end > from.col } else { true };
                if ok {
                    return Some(m);
                }
            }
        }
    }
    None
}

/// Replacement text for one match: `$1` style groups when `regex` is on.
/// Groups are read against the whole line (so anchors and look-around see the
/// same text the search saw), and only when the match found there really is `m`.
pub fn replacement_for(re: &Regex, line: &str, m: Match, with: &str, regex: bool) -> String {
    if !regex {
        return with.to_string();
    }
    let bs = line.char_indices().nth(m.start).map(|(i, _)| i).unwrap_or(line.len());
    let be = line.char_indices().nth(m.end).map(|(i, _)| i).unwrap_or(line.len());
    match re.captures_at(line, bs) {
        Some(c) if c.get(0).map(|g| (g.start(), g.end())) == Some((bs, be)) => {
            let mut out = String::new();
            c.expand(with, &mut out);
            out
        }
        _ => with.to_string(),
    }
}

/// Replaces every match; returns how many. The count and the edit come from
/// the same list of non-empty matches, so they always agree.
pub fn replace_all(b: &mut Buffer, re: &Regex, with: &str, regex: bool) -> usize {
    let mut count = 0;
    let mut out = Vec::with_capacity(b.line_count());
    for l in b.lines() {
        let mut res = String::new();
        let mut last = 0;
        let mut n = 0;
        for c in re.captures_iter(l) {
            let Some(m) = c.get(0) else { continue };
            if m.as_str().is_empty() {
                continue;
            }
            res.push_str(&l[last..m.start()]);
            if regex {
                c.expand(with, &mut res);
            } else {
                res.push_str(with);
            }
            last = m.end();
            n += 1;
        }
        if n == 0 {
            out.push(l.clone());
            continue;
        }
        res.push_str(&l[last..]);
        count += n;
        out.push(res);
    }
    if count > 0 {
        b.replace_all(&out.join("\n"));
    }
    count
}

// --- Find in files ---

#[derive(Debug, Clone)]
pub struct FileHit {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub text: String,
}

const SKIP_DIRS: &[&str] = &["target", "node_modules", ".git", ".hg", ".svn"];
const MAX_FILE: u64 = 5 << 20;

/// Searches the text files under `dir`. Skips hidden folders, build output
/// and binary files; stops at `limit` hits.
pub fn find_in_files(dir: &Path, re: &Regex, filter: &str, limit: usize) -> Vec<FileHit> {
    let mut hits = Vec::new();
    let exts: Vec<String> = filter
        .split([',', ';', ' '])
        .map(|s| s.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase())
        .filter(|s| !s.is_empty() && s != "*")
        .collect();
    walk(dir, re, &exts, limit, &mut hits, 0);
    hits
}

fn walk(dir: &Path, re: &Regex, exts: &[String], limit: usize, hits: &mut Vec<FileHit>, depth: usize) {
    if depth > 24 || hits.len() >= limit {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if hits.len() >= limit {
            return;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(ft) = e.file_type() else { continue };
        let path = e.path();
        if ft.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(&path, re, exts, limit, hits, depth + 1);
        } else if ft.is_file() {
            if !exts.is_empty() {
                let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                if !exts.contains(&ext) {
                    continue;
                }
            }
            if e.metadata().map(|m| m.len() > MAX_FILE).unwrap_or(true) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            if crate::encoding::looks_binary(&bytes) {
                continue;
            }
            let text = crate::encoding::Enc::detect(&bytes).decode(&bytes);
            for (i, l) in crate::buffer::split_lines(&text).iter().enumerate() {
                for m in matches_in_line(re, l, i) {
                    hits.push(FileHit { path: path.clone(), line: i, col: m.start, text: l.trim().chars().take(200).collect() });
                    if hits.len() >= limit {
                        return;
                    }
                    break; // one hit per line is enough to list
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(t: &str) -> Query {
        Query { text: t.into(), ..Default::default() }
    }

    #[test]
    fn plain_case_and_word() {
        let b = Buffer::new("Foo foo food\nFOO");
        let re = q("foo").compile().unwrap().unwrap();
        assert_eq!(count_all(&b, &re), 4);
        let re = Query { match_case: true, ..q("foo") }.compile().unwrap().unwrap();
        assert_eq!(count_all(&b, &re), 2);
        let re = Query { whole_word: true, ..q("foo") }.compile().unwrap().unwrap();
        assert_eq!(count_all(&b, &re), 3);
    }

    #[test]
    fn plain_text_is_not_a_regex() {
        let b = Buffer::new("a.b axb");
        let re = q("a.b").compile().unwrap().unwrap();
        assert_eq!(count_all(&b, &re), 1);
        assert!(Query { regex: true, text: "(".into(), ..Default::default() }.compile().is_err());
    }

    #[test]
    fn find_wraps_both_ways() {
        let b = Buffer::new("x a\nb\na x");
        let re = q("a").compile().unwrap().unwrap();
        let m = find(&b, &re, Pos::new(0, 3), true).unwrap();
        assert_eq!((m.line, m.start), (2, 0));
        let m = find(&b, &re, Pos::new(2, 1), true).unwrap();
        assert_eq!((m.line, m.start), (0, 2));
        let m = find(&b, &re, Pos::new(0, 2), false).unwrap();
        assert_eq!((m.line, m.start), (2, 0));
        let m = find(&b, &re, Pos::new(2, 1), false).unwrap();
        assert_eq!((m.line, m.start), (2, 0));
    }

    #[test]
    fn columns_are_characters() {
        let b = Buffer::new("héé x");
        let re = q("x").compile().unwrap().unwrap();
        let m = find(&b, &re, Pos::new(0, 0), true).unwrap();
        assert_eq!((m.start, m.end), (4, 5));
    }

    #[test]
    fn replace_all_plain_and_groups() {
        let mut b = Buffer::new("a1 a2\na3");
        let re = Query { regex: true, text: r"a(\d)".into(), ..Default::default() }.compile().unwrap().unwrap();
        assert_eq!(replace_all(&mut b, &re, "<$1>", true), 3);
        assert_eq!(b.text(), "<1> <2>\n<3>");
        b.undo();
        assert_eq!(b.text(), "a1 a2\na3");
        let re = q("a$").compile().unwrap().unwrap();
        let mut b = Buffer::new("a$ a$");
        assert_eq!(replace_all(&mut b, &re, "$1", false), 2);
        assert_eq!(b.text(), "$1 $1");
    }

    #[test]
    fn replace_all_skips_empty_matches_and_reads_the_whole_line() {
        let re = Query { regex: true, text: "^|z".into(), ..Default::default() }.compile().unwrap().unwrap();
        let mut b = Buffer::new("az");
        assert_eq!(replace_all(&mut b, &re, "_", true), 1);
        assert_eq!(b.text(), "a_");
        // Groups see the whole line, so a look-behind free anchor still works.
        let re = Query { regex: true, text: r"\bx(\d)".into(), ..Default::default() }.compile().unwrap().unwrap();
        let b = Buffer::new("x1 x2");
        let ms = matches_in_line(&re, b.line(0), 0);
        assert_eq!(ms.len(), 2);
        assert_eq!(replacement_for(&re, b.line(0), ms[1], "<$1>", true), "<2>");
    }
}
