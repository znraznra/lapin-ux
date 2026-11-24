// SPDX-License-Identifier: GPL-3.0-only
//! File encodings and line endings: detect on open, convert, write back.

use encoding_rs::{Encoding, UTF_8};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
    Cr,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::Crlf => "\r\n",
            Eol::Cr => "\r",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::Crlf => "CRLF",
            Eol::Cr => "CR",
        }
    }
    pub const ALL: [Eol; 3] = [Eol::Lf, Eol::Crlf, Eol::Cr];

    /// The most common ending in `text`; LF when there is none.
    pub fn detect(text: &str) -> Eol {
        let b = text.as_bytes();
        let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\r' if b.get(i + 1) == Some(&b'\n') => {
                    crlf += 1;
                    i += 1;
                }
                b'\r' => cr += 1,
                b'\n' => lf += 1,
                _ => {}
            }
            i += 1;
        }
        if crlf > lf && crlf >= cr {
            Eol::Crlf
        } else if cr > lf && cr > crlf {
            Eol::Cr
        } else {
            Eol::Lf
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enc {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Legacy(&'static Encoding),
}

/// What the Encoding menu offers besides the Unicode ones.
pub fn legacy_choices() -> Vec<(&'static str, &'static Encoding)> {
    use encoding_rs::*;
    vec![
        ("Western (Windows-1252)", WINDOWS_1252),
        ("Western (ISO-8859-15)", ISO_8859_15),
        ("Central European (Windows-1250)", WINDOWS_1250),
        ("Central European (ISO-8859-2)", ISO_8859_2),
        ("Cyrillic (Windows-1251)", WINDOWS_1251),
        ("Cyrillic (KOI8-R)", KOI8_R),
        ("Greek (Windows-1253)", WINDOWS_1253),
        ("Turkish (Windows-1254)", WINDOWS_1254),
        ("Japanese (Shift JIS)", SHIFT_JIS),
        ("Korean (EUC-KR)", EUC_KR),
        ("Chinese Simplified (GBK)", GBK),
        ("Chinese Traditional (Big5)", BIG5),
    ]
}

impl Enc {
    pub fn label(self) -> String {
        match self {
            Enc::Utf8 => "UTF-8".into(),
            Enc::Utf8Bom => "UTF-8 BOM".into(),
            Enc::Utf16Le => "UTF-16 LE".into(),
            Enc::Utf16Be => "UTF-16 BE".into(),
            Enc::Legacy(e) => e.name().to_string(),
        }
    }

    /// Reads `bytes`: a byte order mark decides, then valid UTF-8, then a
    /// guess among the legacy encodings.
    pub fn detect(bytes: &[u8]) -> Enc {
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            Enc::Utf8Bom
        } else if bytes.starts_with(&[0xFF, 0xFE]) {
            Enc::Utf16Le
        } else if bytes.starts_with(&[0xFE, 0xFF]) {
            Enc::Utf16Be
        } else if std::str::from_utf8(bytes).is_ok() {
            Enc::Utf8
        } else {
            let mut d = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
            d.feed(bytes, true);
            let e = d.guess(None, chardetng::Utf8Detection::Deny);
            if e == UTF_8 { Enc::Legacy(encoding_rs::WINDOWS_1252) } else { Enc::Legacy(e) }
        }
    }

    /// Text of `bytes` in this encoding. Bytes that do not fit become U+FFFD.
    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            Enc::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Enc::Utf8Bom => String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)).into_owned(),
            Enc::Utf16Le => {
                let b = bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes);
                let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&u)
            }
            Enc::Utf16Be => {
                let b = bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes);
                let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&u)
            }
            Enc::Legacy(e) => e.decode_without_bom_handling(bytes).0.into_owned(),
        }
    }

    /// The bytes to write. `lossy` is true when a character had no
    /// equivalent in a legacy encoding (it was written as `?`-like escapes).
    pub fn encode(self, text: &str) -> (Vec<u8>, bool) {
        match self {
            Enc::Utf8 => (text.as_bytes().to_vec(), false),
            Enc::Utf8Bom => {
                let mut v = vec![0xEF, 0xBB, 0xBF];
                v.extend_from_slice(text.as_bytes());
                (v, false)
            }
            Enc::Utf16Le => {
                let mut v = vec![0xFF, 0xFE];
                for u in text.encode_utf16() {
                    v.extend_from_slice(&u.to_le_bytes());
                }
                (v, false)
            }
            Enc::Utf16Be => {
                let mut v = vec![0xFE, 0xFF];
                for u in text.encode_utf16() {
                    v.extend_from_slice(&u.to_be_bytes());
                }
                (v, false)
            }
            Enc::Legacy(e) => {
                let (out, _, lossy) = e.encode(text);
                (out.into_owned(), lossy)
            }
        }
    }
}

/// True when `bytes` look like a binary file, not text.
pub fn looks_binary(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return false;
    }
    bytes.iter().take(8192).any(|b| *b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_boms_and_utf8() {
        assert_eq!(Enc::detect(b"\xEF\xBB\xBFhi"), Enc::Utf8Bom);
        assert_eq!(Enc::detect(b"\xFF\xFEh\0"), Enc::Utf16Le);
        assert_eq!(Enc::detect("héllo".as_bytes()), Enc::Utf8);
    }

    #[test]
    fn utf16_round_trip() {
        let (bytes, lossy) = Enc::Utf16Le.encode("aé€");
        assert!(!lossy);
        assert_eq!(Enc::detect(&bytes), Enc::Utf16Le);
        assert_eq!(Enc::Utf16Le.decode(&bytes), "aé€");
        let (bytes, _) = Enc::Utf16Be.encode("aé€");
        assert_eq!(Enc::Utf16Be.decode(&bytes), "aé€");
    }

    #[test]
    fn bom_is_dropped_and_restored() {
        let text = Enc::Utf8Bom.decode(b"\xEF\xBB\xBFabc");
        assert_eq!(text, "abc");
        assert_eq!(Enc::Utf8Bom.encode(&text).0, b"\xEF\xBB\xBFabc");
    }

    #[test]
    fn legacy_round_trip() {
        let bytes = b"caf\xE9 \x80";
        let enc = Enc::Legacy(encoding_rs::WINDOWS_1252);
        let text = enc.decode(bytes);
        assert_eq!(text, "café €");
        assert_eq!(enc.encode(&text).0, bytes);
        assert!(matches!(Enc::detect(bytes), Enc::Legacy(_)));
    }

    #[test]
    fn unencodable_is_reported() {
        let (_, lossy) = Enc::Legacy(encoding_rs::WINDOWS_1252).encode("日本");
        assert!(lossy);
    }

    #[test]
    fn eol_detection() {
        assert_eq!(Eol::detect("a\r\nb\r\nc\n"), Eol::Crlf);
        assert_eq!(Eol::detect("a\nb\n"), Eol::Lf);
        assert_eq!(Eol::detect("a\rb\rc"), Eol::Cr);
        assert_eq!(Eol::detect("none"), Eol::Lf);
    }

    #[test]
    fn binary_check() {
        assert!(looks_binary(b"ab\0cd"));
        assert!(!looks_binary(b"plain"));
    }
}
