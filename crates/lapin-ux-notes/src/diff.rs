// SPDX-License-Identifier: GPL-3.0-only
//! Line diff between two texts (Myers-style LCS on lines), used by Compare.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Line exists only on this side.
    Added,
    /// Line exists only on the other side (shown on the neighbouring line).
    Removed,
    /// Line differs from its counterpart.
    Changed,
}

impl Mark {
    pub fn code(self) -> u8 {
        match self {
            Mark::Added => 1,
            Mark::Removed => 2,
            Mark::Changed => 3,
        }
    }
}

/// Marks for both sides: `(left, right)`, each a list of (line, mark).
/// Above this many lines per side the diff is skipped.
pub const MAX_LINES: usize = 20_000;

pub fn diff(a: &[&str], b: &[&str]) -> Option<(Vec<(usize, Mark)>, Vec<(usize, Mark)>)> {
    if a.len() > MAX_LINES || b.len() > MAX_LINES {
        return None;
    }
    // trim common head and tail
    let mut s = 0;
    while s < a.len() && s < b.len() && a[s] == b[s] {
        s += 1;
    }
    let mut ea = a.len();
    let mut eb = b.len();
    while ea > s && eb > s && a[ea - 1] == b[eb - 1] {
        ea -= 1;
        eb -= 1;
    }
    let (am, bm) = (&a[s..ea], &b[s..eb]);
    let (n, m) = (am.len(), bm.len());
    if n.saturating_mul(m) > 25_000_000 {
        return None;
    }
    // LCS table
    let w = m + 1;
    let mut t = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * w + j] = if am[i] == bm[j] { t[(i + 1) * w + j + 1] + 1 } else { t[(i + 1) * w + j].max(t[i * w + j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut left = Vec::new();
    let mut right = Vec::new();
    // collect runs of removed/added between matches
    let mut rem: Vec<usize> = Vec::new();
    let mut add: Vec<usize> = Vec::new();
    let flush = |rem: &mut Vec<usize>, add: &mut Vec<usize>, left: &mut Vec<(usize, Mark)>, right: &mut Vec<(usize, Mark)>, at_a: usize, at_b: usize| {
        let pair = rem.len().min(add.len());
        for k in 0..pair {
            left.push((rem[k], Mark::Changed));
            right.push((add[k], Mark::Changed));
        }
        for &r in &rem[pair..] {
            left.push((r, Mark::Added));
        }
        for &x in &add[pair..] {
            right.push((x, Mark::Added));
        }
        // a pure removal also marks the spot on the other side
        if add.is_empty() && !rem.is_empty() {
            right.push((at_b.min(b.len().saturating_sub(1)), Mark::Removed));
        }
        if rem.is_empty() && !add.is_empty() {
            left.push((at_a.min(a.len().saturating_sub(1)), Mark::Removed));
        }
        rem.clear();
        add.clear();
    };
    while i < n || j < m {
        if i < n && j < m && am[i] == bm[j] {
            flush(&mut rem, &mut add, &mut left, &mut right, s + i, s + j);
            i += 1;
            j += 1;
        } else if j >= m || (i < n && t[(i + 1) * w + j] >= t[i * w + j + 1]) {
            rem.push(s + i);
            i += 1;
        } else {
            add.push(s + j);
            j += 1;
        }
    }
    flush(&mut rem, &mut add, &mut left, &mut right, s + i, s + j);
    // a Removed marker must not hide a real mark on the same line
    for v in [&mut left, &mut right] {
        v.sort_by_key(|x| (x.0, x.1 == Mark::Removed));
        v.dedup_by_key(|x| x.0);
    }
    Some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_is_empty() {
        let (l, r) = diff(&["a", "b"], &["a", "b"]).unwrap();
        assert!(l.is_empty() && r.is_empty());
    }

    #[test]
    fn changed_added_removed() {
        let (l, r) = diff(&["a", "b", "c"], &["a", "x", "c", "d"]).unwrap();
        assert_eq!(l, vec![(1, Mark::Changed), (2, Mark::Removed)]);
        assert_eq!(r, vec![(1, Mark::Changed), (3, Mark::Added)]);
        let (l, r) = diff(&["a", "b", "c"], &["a", "c"]).unwrap();
        assert_eq!(l, vec![(1, Mark::Added)]);
        assert_eq!(r, vec![(1, Mark::Removed)]);
    }

    #[test]
    fn empty_sides() {
        let (l, r) = diff(&[], &["a"]).unwrap();
        assert!(l.is_empty() || l[0].1 == Mark::Removed);
        assert_eq!(r[0].1, Mark::Added);
    }
}
