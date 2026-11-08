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

// ---- Apps' own icons instead of black tray icons (#52, 8 Nov) ------------

/// Status-notifier categories that belong to apps (Steam, Discord, chat
/// and Electron apps). "SystemServices" and "Hardware" items (network,
/// Bluetooth, battery…) are status icons and keep their own pictures.
pub fn category_is_app(category: Option<&str>) -> bool {
    matches!(category, None | Some("ApplicationStatus") | Some("Communications"))
}

/// Words that say nothing about which app it is.
fn generic(word: &str) -> bool {
    word.len() < 3
        || word.starts_with("chrome_status_icon")
        || word.starts_with("status_icon")
        || [
            "electron", "python", "python3", "java", "node", "bash", "sh", "chrome", "chromium", "app", "app.asar",
            "resources", "usr", "lib", "lib64", "bin", "opt", "share", "local", "home", "wine", "wine64",
            "wine-preloader", "wine64-preloader", "flatpak", "bwrap", "appimage", "tray", "systray", "indicator",
        ]
        .contains(&word)
}

/// Clues about a tray item's app, lowercased: its tray id and title, the
/// owning process's name, executable and the path parts of its command
/// line (Electron apps run as "electron /usr/lib/claude-desktop/app.asar").
pub fn clues(id: Option<&str>, title: Option<&str>, process: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |w: &str| {
        let w = w.trim().to_ascii_lowercase();
        let w = w.trim_end_matches(".exe").trim_end_matches(".asar").trim_end_matches(".appimage").to_string();
        if !generic(&w) && !out.contains(&w) {
            out.push(w);
        }
    };
    for s in [id, title].into_iter().flatten() {
        push(s);
    }
    for p in process {
        for part in p.split('/') {
            push(part);
        }
    }
    out
}

/// An installed app, as far as matching goes.
pub struct AppKey<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub wm_class: Option<&'a str>,
    pub exec: Option<&'a str>,
}

/// The installed app a tray item belongs to: the first clue that names an
/// app by desktop id (or its last part, "com.valvesoftware.Steam" →
/// "steam"), window class, executable or name.
pub fn match_app(clues: &[String], apps: &[AppKey]) -> Option<usize> {
    let exec_name = |e: &str| {
        let first = e.split_whitespace().find(|t| !t.contains('=') && *t != "env").unwrap_or("");
        first.rsplit('/').next().unwrap_or(first).to_ascii_lowercase()
    };
    for clue in clues {
        let found = apps.iter().position(|a| {
            let id = a.id.to_ascii_lowercase();
            id == *clue
                || id.rsplit('.').next() == Some(clue.as_str())
                || a.wm_class.is_some_and(|w| w.eq_ignore_ascii_case(clue))
                || a.exec.is_some_and(|e| exec_name(e) == *clue)
                || a.name.eq_ignore_ascii_case(clue)
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

#[cfg(test)]
mod app_tests {
    use super::*;

    fn apps() -> Vec<AppKey<'static>> {
        vec![
            AppKey { id: "firefox", name: "Firefox", wm_class: Some("firefox"), exec: Some("firefox %u") },
            AppKey { id: "steam", name: "Steam", wm_class: Some("steam"), exec: Some("/usr/bin/steam-runtime %U") },
            AppKey { id: "claude-desktop", name: "Claude", wm_class: Some("Claude"), exec: Some("claude-desktop %u") },
            AppKey { id: "com.discordapp.Discord", name: "Discord", wm_class: None, exec: Some("flatpak run com.discordapp.Discord") },
        ]
    }

    #[test]
    fn finds_the_app() {
        let a = apps();
        assert_eq!(match_app(&clues(Some("steam"), Some("Steam"), &[]), &a), Some(1));
        // Electron: generic tray id, the app shows in the command line.
        let c = clues(Some("chrome_status_icon_1"), None, &["/usr/lib/electron/electron".into(), "/usr/lib/claude-desktop/app.asar".into()]);
        assert_eq!(match_app(&c, &a), Some(2));
        // By window class / name, case-insensitive.
        assert_eq!(match_app(&clues(None, Some("claude"), &[]), &a), Some(2));
        // Flatpak id's last part.
        assert_eq!(match_app(&clues(Some("discord"), None, &[]), &a), Some(3));
        assert_eq!(match_app(&clues(Some("nm-applet"), None, &["/usr/bin/nm-applet".into()]), &a), None);
    }

    #[test]
    fn categories() {
        assert!(category_is_app(Some("ApplicationStatus")));
        assert!(category_is_app(Some("Communications")));
        assert!(category_is_app(None));
        assert!(!category_is_app(Some("SystemServices")));
        assert!(!category_is_app(Some("Hardware")));
    }
}
