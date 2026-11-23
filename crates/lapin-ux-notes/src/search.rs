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

fn cols(line: &str, byte_start: usize, byte_end: usize) -> (usize, usize) {
    let s = line[..byte_start].chars().count();
    (s, s + line[byte_start..byte_end].chars().count())
}

pub fn matches_in_line(re: &Regex, line: &str, idx: usize) -> Vec<Match> {
    re.find_iter(line)
        .filter(|m| !m.as_str().is_empty())
        .map(|m| {
            let (start, end) = cols(line, m.start(), m.end());
            Match { line: idx, start, end }
        })
        .collect()
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
pub fn replacement_for(re: &Regex, line: &str, m: Match, with: &str, regex: bool) -> String {
    let bs = line.char_indices().nth(m.start).map(|(i, _)| i).unwrap_or(line.len());
    let be = line.char_indices().nth(m.end).map(|(i, _)| i).unwrap_or(line.len());
    if !regex {
        return with.to_string();
    }
    match re.captures(&line[bs..be]) {
        Some(c) => {
            let mut out = String::new();
            c.expand(with, &mut out);
            out
        }
        None => with.to_string(),
    }
}

/// Replaces every match; returns how many.
pub fn replace_all(b: &mut Buffer, re: &Regex, with: &str, regex: bool) -> usize {
    let mut count = 0;
    let mut out = Vec::with_capacity(b.line_count());
    for l in b.lines() {
        let n = if regex {
            re.find_iter(l).filter(|m| !m.as_str().is_empty()).count()
        } else {
            re.find_iter(l).filter(|m| !m.as_str().is_empty()).count()
        };
        if n == 0 {
            out.push(l.clone());
            continue;
        }
        count += n;
        let r = if regex { re.replace_all(l, with).into_owned() } else { re.replace_all(l, regex::NoExpand(with)).into_owned() };
        out.push(r);
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
}
