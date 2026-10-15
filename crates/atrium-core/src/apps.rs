// SPDX-License-Identifier: GPL-3.0-only
//! "All apps" list layout: alphabetical, grouped under letter headers, with
//! digits and symbols collected under "#" at the top.

use crate::search::normalize;

/// One header plus the indices of the items under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub header: String,
    pub items: Vec<usize>,
}

/// Group `names` (any order) into alphabetical sections. Returned indices
/// point back into `names`.
pub fn alphabet_sections(names: &[&str]) -> Vec<Section> {
    let mut order: Vec<usize> = (0..names.len()).collect();
    order.sort_by(|&a, &b| {
        normalize(names[a]).cmp(&normalize(names[b])).then_with(|| names[a].cmp(names[b]))
    });

    let mut sections: Vec<Section> = Vec::new();
    for idx in order {
        let header = header_for(names[idx]);
        match sections.iter_mut().find(|s| s.header == header) {
            Some(sec) => sec.items.push(idx),
            None => sections.push(Section { header, items: vec![idx] }),
        }
    }
    // "#" first, then letters A–Z, then any other scripts in sort order.
    sections.sort_by_key(|s| match s.header.as_str() {
        "#" => (0, String::new()),
        h if h.len() == 1 && h.as_bytes()[0].is_ascii_uppercase() => (1, h.to_string()),
        h => (2, h.to_string()),
    });
    sections
}

fn header_for(name: &str) -> String {
    let first = normalize(name).chars().find(|c| !c.is_whitespace());
    match first {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string(),
        Some(c) if c.is_alphabetic() => c.to_uppercase().to_string(),
        _ => "#".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_and_orders() {
        let names = ["firefox", "Files", "7-Zip", "Calculator", "Écran", "cal", "日記"];
        let s = alphabet_sections(&names);
        let headers: Vec<_> = s.iter().map(|s| s.header.as_str()).collect();
        assert_eq!(headers, ["#", "C", "E", "F", "日"]);
        let f: Vec<_> = s[3].items.iter().map(|&i| names[i]).collect();
        assert_eq!(f, ["Files", "firefox"]);
        let c: Vec<_> = s[1].items.iter().map(|&i| names[i]).collect();
        assert_eq!(c, ["cal", "Calculator"]);
    }

    #[test]
    fn empty() {
        assert!(alphabet_sections(&[]).is_empty());
    }
}
