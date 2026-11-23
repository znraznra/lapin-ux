//! Printing: the text becomes a PDF (A4, a fixed-width font, page numbers),
//! which is either saved ("Print to PDF") or handed to the print system
//! (`lp`). Writing the PDF ourselves keeps Notes free of a print toolkit and
//! gives the same pages on every printer.
//!
//! Only the built-in Courier font is used, so letters outside Western
//! European (Windows-1252) print as `?`. Long lines wrap; tabs are expanded.

use encoding_rs::WINDOWS_1252;

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 54.0;
const FONT: f32 = 10.0;
const LEADING: f32 = 12.5;
/// Courier is 0.6 em wide.
const CHAR_W: f32 = FONT * 0.6;

/// Characters that fit on one line.
fn columns() -> usize {
    ((PAGE_W - 2.0 * MARGIN) / CHAR_W).floor() as usize
}

/// Text lines that fit on one page (the last strip is the page number).
fn rows() -> usize {
    ((PAGE_H - 2.0 * MARGIN - 2.0 * LEADING) / LEADING).floor() as usize
}

/// Breaks `text` into printable lines: tabs expanded to `tab` columns, long
/// lines wrapped at the page width.
pub fn layout(text: &str, tab: usize) -> Vec<String> {
    let cols = columns();
    let tab = tab.clamp(1, 16);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let mut line = String::new();
        let mut n = 0;
        for ch in raw.chars() {
            if ch == '\t' {
                let pad = tab - n % tab;
                for _ in 0..pad {
                    if n == cols {
                        out.push(std::mem::take(&mut line));
                        n = 0;
                    }
                    line.push(' ');
                    n += 1;
                }
                continue;
            }
            if ch.is_control() {
                continue;
            }
            if n == cols {
                out.push(std::mem::take(&mut line));
                n = 0;
            }
            line.push(ch);
            n += 1;
        }
        out.push(line);
    }
    out
}

/// One character as a Windows-1252 byte, `?` when it has none.
fn win_ansi(ch: char) -> u8 {
    let mut b = [0u8; 4];
    let s = ch.encode_utf8(&mut b);
    let (bytes, _, bad) = WINDOWS_1252.encode(s);
    if bad || bytes.len() != 1 { b'?' } else { bytes[0] }
}

fn escape(s: &str, out: &mut Vec<u8>) {
    for ch in s.chars() {
        match win_ansi(ch) {
            c @ (b'(' | b')' | b'\\') => {
                out.push(b'\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
}

/// The finished PDF file for `text`. `title` is printed small at the bottom
/// of every page, next to the page number.
pub fn pdf(text: &str, title: &str, tab: usize) -> Vec<u8> {
    let lines = layout(text, tab);
    let per = rows();
    let pages: Vec<&[String]> = lines.chunks(per).collect();
    let pages = if pages.is_empty() { vec![&[][..]] } else { pages };
    let count = pages.len();

    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    // Object numbers: 1 catalog, 2 pages, 3 font, then page and content pairs.
    let mut obj = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        let n = offsets.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..count).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    obj(&mut out, format!("<< /Type /Pages /Kids [{}] /Count {count} >>", kids.join(" ")).as_bytes());
    obj(&mut out, b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>");
    for (i, page) in pages.iter().enumerate() {
        let content_no = 5 + 2 * i;
        obj(
            &mut out,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_no} 0 R >>"
            )
            .as_bytes(),
        );
        let mut s: Vec<u8> = Vec::new();
        s.extend_from_slice(format!("BT /F1 {FONT} Tf {LEADING} TL {MARGIN} {} Td\n", PAGE_H - MARGIN - FONT).as_bytes());
        for l in page.iter() {
            s.push(b'(');
            escape(l, &mut s);
            s.extend_from_slice(b") Tj T*\n");
        }
        s.extend_from_slice(b"ET\n");
        let foot = format!("{title}    {} / {count}", i + 1);
        s.extend_from_slice(format!("BT /F1 8 Tf {MARGIN} {} Td (", MARGIN - 10.0).as_bytes());
        escape(&foot, &mut s);
        s.extend_from_slice(b") Tj ET\n");
        let mut body = format!("<< /Length {} >>\nstream\n", s.len()).into_bytes();
        body.extend_from_slice(&s);
        body.extend_from_slice(b"endstream");
        obj(&mut out, &body);
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes(),
    );
    out
}

/// Printer names known to CUPS and the default one, from `lpstat`. Empty when
/// there is no print system.
pub fn printers() -> (Vec<String>, Option<String>) {
    let run = |args: &[&str]| {
        std::process::Command::new("lpstat")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let list: Vec<String> = run(&["-e"]).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    // "system default destination: NAME"
    let default = run(&["-d"]).and_then(|s| s.rsplit(':').next().map(|n| n.trim().to_string())).filter(|n| list.contains(n));
    (list, default)
}

/// Sends a PDF to `printer` with `copies` copies. Errors carry what `lp`
/// said.
pub fn send(pdf: &[u8], printer: &str, copies: u32, title: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("lp")
        .args(["-d", printer, "-n", &copies.max(1).to_string(), "-t", title, "-o", "media=A4"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("The print system is not available ({e})."))?;
    child.stdin.take().ok_or("Could not reach the print system.")?.write_all(pdf).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_lines_wrap_and_tabs_expand() {
        let long = "x".repeat(columns() * 2 + 5);
        let l = layout(&format!("{long}\nab\tc"), 4);
        assert_eq!(l.len(), 4);
        assert_eq!(l[0].chars().count(), columns());
        assert_eq!(l[3], "ab  c");
    }

    #[test]
    fn crlf_and_controls_are_dropped() {
        assert_eq!(layout("a\r\nb\u{7}c", 4), vec!["a".to_string(), "bc".to_string()]);
    }

    #[test]
    fn pdf_has_pages_and_a_valid_xref() {
        let text = (0..rows() + 3).map(|i| format!("line {i} (x) \\ \u{20ac} \u{4e2d}")).collect::<Vec<_>>().join("\n");
        let p = pdf(&text, "a.txt", 4);
        let s = String::from_utf8_lossy(&p);
        assert!(s.starts_with("%PDF-1.4"));
        assert!(s.contains("/Count 2"));
        assert!(s.contains("\\(x\\) \\\\"));
        assert!(s.trim_end().ends_with("%%EOF"));
        // Every xref entry points at "N 0 obj".
        let xref = p.windows(5).position(|w| w == b"xref\n").unwrap();
        let table = &p[xref..];
        let table = String::from_utf8_lossy(table);
        let offs: Vec<usize> = table.lines().skip(3).take_while(|l| l.ends_with(" n ")).map(|l| l[..10].parse().unwrap()).collect();
        assert_eq!(offs.len(), 3 + 2 * 2);
        for (i, o) in offs.iter().enumerate() {
            assert!(p[*o..].starts_with(format!("{} 0 obj", i + 1).as_bytes()));
        }
    }

    #[test]
    fn an_empty_document_is_one_blank_page() {
        let s = String::from_utf8_lossy(&pdf("", "x", 4)).into_owned();
        assert!(s.contains("/Count 1"));
    }

    #[test]
    fn euro_prints_and_chinese_becomes_a_question_mark() {
        assert_eq!(win_ansi('\u{20ac}'), 0x80);
        assert_eq!(win_ansi('\u{4e2d}'), b'?');
        assert_eq!(win_ansi('e'), b'e');
    }
}
