// SPDX-License-Identifier: GPL-3.0-only
//! Hex view: bytes as `offset  16 hex pairs  |text|` lines and back.

pub const PER_LINE: usize = 16;
/// Files bigger than this open as text only when asked.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

pub fn dump(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 4 + 16);
    for (i, chunk) in bytes.chunks(PER_LINE).enumerate() {
        s.push_str(&format!("{:08x} ", i * PER_LINE));
        for k in 0..PER_LINE {
            if k % 8 == 0 {
                s.push(' ');
            }
            match chunk.get(k) {
                Some(b) => s.push_str(&format!("{b:02x} ")),
                None => s.push_str("   "),
            }
        }
        s.push_str(" |");
        for &b in chunk {
            s.push(if (0x20..0x7f).contains(&b) { b as char } else { '.' });
        }
        s.push_str("|\n");
    }
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

/// Bytes from a (possibly edited) dump: reads the hex column of every line
/// (between the offset and the `|`), ignoring anything that isn't hex.
pub fn parse(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines() {
        let body = match line.split_once('|') {
            Some((h, _)) => h,
            None => line,
        };
        let body = body.get(8.min(body.len())..).unwrap_or("");
        let digits: Vec<u8> = body.bytes().filter(|b| b.is_ascii_hexdigit()).collect();
        for p in digits.chunks(2) {
            if p.len() == 2 {
                let h = |c: u8| (c as char).to_digit(16).unwrap() as u8;
                out.push(h(p[0]) << 4 | h(p[1]));
            }
        }
    }
    out
}

pub fn looks_binary(bytes: &[u8]) -> bool {
    let n = bytes.len().min(8192);
    bytes[..n].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let data: Vec<u8> = (0..=255u8).chain(0..7).collect();
        let d = dump(&data);
        assert_eq!(parse(&d), data);
        assert!(d.starts_with("00000000  00 01"));
    }

    #[test]
    fn edit_hex_column() {
        let d = dump(b"Hello");
        let e = d.replacen("48", "4a", 1);
        assert_eq!(parse(&e), b"Jello");
    }

    #[test]
    fn detects_binary() {
        assert!(looks_binary(&[1, 0, 2]));
        assert!(!looks_binary(b"text"));
    }
}
