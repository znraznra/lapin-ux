// SPDX-License-Identifier: GPL-3.0-only
//! Which tray icons are single-colour (#52), so the taskbar can tint them
//! like its own icons instead of drawing them black on a dark taskbar.
//!
//! Apps name such icons "…-symbolic" only sometimes: Steam ships
//! `steam_tray_mono`, others "-panel"/"-mono" variants, and plain black SVGs
//! or black pixmaps are common.

/// A name that says the icon is monochrome.
pub fn name_is_mono(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let stem = n.rsplit('/').next().unwrap_or(&n);
    let stem = stem.trim_end_matches(".svg").trim_end_matches(".png");
    ["-symbolic", "_symbolic", "-mono", "_mono", "-panel", "_panel", "-monochrome", "_monochrome"]
        .iter()
        .any(|s| stem.ends_with(s) || stem.contains(&format!("{s}-")) || stem.contains(&format!("{s}_")))
}

/// A file found in a theme's symbolic/status-symbolic folder.
pub fn path_is_mono(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        let c = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        c == "symbolic" || c.ends_with("-symbolic")
    }) || path.file_name().is_some_and(|f| name_is_mono(&f.to_string_lossy()))
}

/// An SVG drawn in one dark colour (or the default black): no gradients,
/// at most one colour, and that colour dark.
pub fn svg_is_mono(svg: &str) -> bool {
    if svg.contains("<linearGradient") || svg.contains("<radialGradient") || svg.contains("<image") {
        return false;
    }
    let mut colours: Vec<(u8, u8, u8)> = Vec::new();
    for key in ["fill", "stroke", "stop-color", "color"] {
        for sep in ["=\"", ":", "='"] {
            let pat = format!("{key}{sep}");
            let mut rest = svg;
            while let Some(i) = rest.find(&pat) {
                // "fill-opacity" etc. aren't colours; neither is the "fill"
                // inside "stroke-fill" (guarded by checking the char before).
                let before = rest[..i].chars().last();
                rest = &rest[i + pat.len()..];
                if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-') {
                    continue;
                }
                let value: String = rest.trim_start().chars().take_while(|c| !matches!(c, '"' | '\'' | ';' | ' ' | '}')).collect();
                if let Some(c) = parse_colour(&value)
                    && !colours.contains(&c)
                {
                    colours.push(c);
                }
            }
        }
    }
    match colours.as_slice() {
        [] => true,
        [c] => luminance(*c) < 0.35,
        _ => false,
    }
}

fn parse_colour(v: &str) -> Option<(u8, u8, u8)> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        let n = |s: &str| u8::from_str_radix(s, 16).ok();
        return match hex.len() {
            3 | 4 => {
                let d: Vec<String> = hex.chars().take(3).map(|c| format!("{c}{c}")).collect();
                Some((n(&d[0])?, n(&d[1])?, n(&d[2])?))
            }
            6 | 8 => Some((n(&hex[0..2])?, n(&hex[2..4])?, n(&hex[4..6])?)),
            _ => None,
        };
    }
    match v.as_str() {
        "black" => Some((0, 0, 0)),
        "white" => Some((255, 255, 255)),
        _ if v.starts_with("rgb(") => {
            let parts: Vec<u8> = v[4..].trim_end_matches(')').split(',').filter_map(|p| p.trim().parse().ok()).collect();
            (parts.len() == 3).then(|| (parts[0], parts[1], parts[2]))
        }
        // none, currentColor, url(#…) and named colours other than these:
        // not a fixed colour.
        _ => None,
    }
}

fn luminance((r, g, b): (u8, u8, u8)) -> f32 {
    (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0
}

/// An RGBA pixmap whose visible pixels are all near-black: drawn as-is it
/// disappears on a dark taskbar.
pub fn pixmap_is_dark_mono(rgba: &[u8]) -> bool {
    let mut seen = 0usize;
    for px in rgba.chunks_exact(4) {
        if px[3] < 32 {
            continue;
        }
        seen += 1;
        if px[0].max(px[1]).max(px[2]) > 60 {
            return false;
        }
    }
    seen > 0
}

/// Paint every visible pixel `colour`, keeping its transparency.
pub fn tint_pixmap(rgba: &mut [u8], (r, g, b): (u8, u8, u8)) {
    for px in rgba.chunks_exact_mut(4) {
        px[0] = r;
        px[1] = g;
        px[2] = b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(name_is_mono("steam_tray_mono"));
        assert!(name_is_mono("nm-signal-75-symbolic"));
        assert!(name_is_mono("nextcloud-panel"));
        assert!(name_is_mono("/home/u/.local/share/Steam/public/steam_tray_mono.png"));
        assert!(!name_is_mono("discord"));
        assert!(!name_is_mono("monolith"));
        assert!(path_is_mono(std::path::Path::new("/usr/share/icons/Adwaita/symbolic/status/x.svg")));
        assert!(!path_is_mono(std::path::Path::new("/usr/share/icons/hicolor/48x48/apps/steam.png")));
    }

    #[test]
    fn svgs() {
        assert!(svg_is_mono(r#"<svg><path d="M0 0h16v16z"/></svg>"#));
        assert!(svg_is_mono(r##"<svg><path fill="#212121" d=""/><path style="fill:#212121;fill-opacity:.5"/></svg>"##));
        assert!(svg_is_mono(r#"<svg><path fill="currentColor"/><path fill="none" stroke="black"/></svg>"#));
        assert!(!svg_is_mono(r##"<svg><path fill="#e74c3c"/><path fill="#2ecc71"/></svg>"##));
        assert!(!svg_is_mono(r##"<svg><path fill="#ffffff"/></svg>"##));
        assert!(!svg_is_mono(r#"<svg><linearGradient id="a"/><path fill="url(#a)"/></svg>"#));
    }

    #[test]
    fn pixmaps() {
        let black = [0, 0, 0, 255, 10, 10, 10, 200, 255, 255, 255, 0];
        assert!(pixmap_is_dark_mono(&black));
        assert!(!pixmap_is_dark_mono(&[0, 0, 0, 255, 200, 50, 50, 255]));
        assert!(!pixmap_is_dark_mono(&[0, 0, 0, 0]));
        let mut p = black;
        tint_pixmap(&mut p, (255, 255, 255));
        assert_eq!(&p[0..4], &[255, 255, 255, 255]);
        assert_eq!(p[11], 0);
    }
}
