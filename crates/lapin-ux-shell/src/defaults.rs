// SPDX-License-Identifier: GPL-3.0-only
//! First-run defaults for the rest of the desktop: theme, wallpaper and the
//! Super-key binding. Written once (when the shell's own config is first
//! created); after that the user's choices are never overwritten.
//!
//! These settings live in COSMIC's shared config namespaces because the
//! reused COSMIC services (theme daemon, wallpaper renderer, compositor)
//! read them from there.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cosmic::cosmic_config::{self, ConfigSet, CosmicConfigEntry};
use cosmic::cosmic_theme::palette::Srgb;
use cosmic::cosmic_theme::{CornerRadii, Theme, ThemeBuilder, ThemeMode};
use serde::Serialize;

/// Where packages install the default wallpapers.
pub const WALLPAPER_DIR: &str = "/usr/share/backgrounds/lapin-ux";

fn srgb(hex: u32) -> Srgb {
    Srgb::new(((hex >> 16) & 0xff) as f32 / 255.0, ((hex >> 8) & 0xff) as f32 / 255.0, (hex & 0xff) as f32 / 255.0)
}

/// Small radii on controls, 8 px on windows and panels.
fn radii() -> CornerRadii {
    CornerRadii {
        radius_0: [0.0; 4],
        radius_xs: [4.0; 4],
        radius_s: [4.0; 4],
        radius_m: [8.0; 4],
        radius_l: [8.0; 4],
        radius_xl: [8.0; 4],
    }
}

fn apply_theme_variant(dark: bool) -> Result<bool, cosmic_config::Error> {
    let (builder_cfg, theme_cfg) = if dark {
        (ThemeBuilder::dark_config()?, Theme::dark_config()?)
    } else {
        (ThemeBuilder::light_config()?, Theme::light_config()?)
    };
    // Someone who already styled COSMIC (accent or roundness) keeps their look.
    let id = if dark { "com.system76.CosmicTheme.Dark.Builder" } else { "com.system76.CosmicTheme.Light.Builder" };
    if ["accent", "corner_radii"].iter().any(|k| cfg_path(id).join(k).exists()) {
        tracing::info!("existing {} theme customisation found; leaving it", if dark { "dark" } else { "light" });
        return Ok(false);
    }
    let mut b = ThemeBuilder::get_entry(&builder_cfg).unwrap_or_else(|(_, b)| b);
    b.accent = Some(if dark { srgb(0x4ca0e8) } else { srgb(0x1a6fc4) });
    b.corner_radii = radii();
    // Floating windows sit edge to edge; tiles get a small gap.
    b.gaps = (0, 4);
    b.active_hint = 0;
    // Translucent, blurred shell surfaces.
    b.frosted_system_interface = true;
    b.frosted_panel = true;
    b.frosted_applets = true;
    b.write_entry(&builder_cfg)?;
    b.build().write_entry(&theme_cfg)?;
    Ok(true)
}

/// Mirrors cosmic-bg's `Entry` so it serialises identically.
#[derive(Serialize)]
struct BgEntry {
    output: String,
    source: BgSource,
    filter_by_theme: bool,
    rotation_frequency: u64,
    filter_method: BgFilter,
    scaling_mode: BgScaling,
    sampling_method: BgSampling,
}
#[derive(Serialize)]
enum BgSource {
    Path(PathBuf),
}
#[derive(Serialize)]
enum BgFilter {
    Lanczos,
}
#[derive(Serialize)]
enum BgScaling {
    Zoom,
}
#[derive(Serialize)]
enum BgSampling {
    Alphanumeric,
}

fn apply_wallpaper(dark: bool) -> Result<bool, cosmic_config::Error> {
    let file = Path::new(WALLPAPER_DIR).join(if dark { "lapin-ux-dark.jpg" } else { "lapin-ux-light.jpg" });
    if !file.exists() {
        tracing::info!(path = %file.display(), "default wallpaper not installed; leaving wallpaper unchanged");
        return Ok(false);
    }
    if cfg_path("com.system76.CosmicBackground").join("all").exists() {
        tracing::info!("a wallpaper is already configured; leaving it");
        return Ok(false);
    }
    let cfg = cosmic_config::Config::new("com.system76.CosmicBackground", 1)?;
    cfg.set("same-on-all", true)?;
    cfg.set(
        "all",
        BgEntry {
            output: "all".into(),
            source: BgSource::Path(file),
            filter_by_theme: false,
            rotation_frequency: 3600,
            filter_method: BgFilter::Lanczos,
            scaling_mode: BgScaling::Zoom,
            sampling_method: BgSampling::Alphanumeric,
        },
    )?;
    Ok(true)
}

/// Point the Super key (and Super+A) at our Start menu. `lapin-ux-shell start`
/// falls back to cosmic-launcher outside an Lapin UX session, so this is safe
/// for people who also log into COSMIC.
fn apply_shortcuts() -> Result<bool, cosmic_config::Error> {
    #[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
    enum System {
        AppLibrary,
        Launcher,
    }
    let cfg = cosmic_config::Config::new("com.system76.CosmicSettings.Shortcuts", 1)?;
    // The user file is merged over the system defaults by the compositor.
    // If the user already customised system actions, leave them alone.
    let user_file = cfg_path("com.system76.CosmicSettings.Shortcuts").join("system_actions");
    if std::fs::read_to_string(&user_file).is_ok_and(|s| !s.trim().is_empty()) {
        tracing::info!("user has custom system actions; not changing the Super key binding");
        return Ok(false);
    }
    let mut map = BTreeMap::new();
    map.insert(System::Launcher, "lapin-ux-shell start".to_string());
    map.insert(System::AppLibrary, "lapin-ux-shell start".to_string());
    cfg.set("system_actions", map)?;
    Ok(true)
}

/// Bump when the generated shortcuts (or other upgradeable defaults) change.
pub const DEFAULTS_VERSION: u32 = 4;

const MARKER: &str = "// Managed by Lapin UX. Delete this line to keep your own edits from being replaced.";

/// What the first release wrote, recognised so it can be upgraded in place.
const LEGACY_V1_SHORTCUTS: &str = r#"{
    (modifiers: [Super], key: "l"): System(LockScreen),
    (modifiers: [Super], key: "e"): System(HomeFolder),
    (modifiers: [Super], key: "d"): Spawn("lapin-ux-shell desktop"),
    (modifiers: [Super], key: "Up"): Maximize,
    (modifiers: [Super], key: "Down"): Minimize,
    (modifiers: [Super, Shift], key: "s"): System(Screenshot),
    (modifiers: [Ctrl, Alt], key: "Delete"): System(LogOut),
}"#;

/// Familiar keyboard shortcuts, layered over the compositor defaults.
pub fn windows_style_shortcuts() -> String {
    let mut lines = vec![
        r#"(modifiers: [Super], key: "l"): System(LockScreen),"#.to_string(),
        r#"(modifiers: [Super], key: "e"): System(HomeFolder),"#.to_string(),
        r#"(modifiers: [Super], key: "d"): Spawn("lapin-ux-shell desktop"),"#.to_string(),
        r#"(modifiers: [Super], key: "x"): Spawn("lapin-ux-shell winx"),"#.to_string(),
        r#"(modifiers: [Super], key: "r"): Spawn("lapin-ux-shell run"),"#.to_string(),
        r#"(modifiers: [Super], key: "n"): Spawn("lapin-ux-shell notifications"),"#.to_string(),
        r#"(modifiers: [Super], key: "a"): Spawn("lapin-ux-shell quick"),"#.to_string(),
        r#"(modifiers: [Super], key: "Up"): Maximize,"#.to_string(),
        r#"(modifiers: [Super], key: "Down"): Minimize,"#.to_string(),
        r#"(modifiers: [Super, Shift], key: "s"): System(Screenshot),"#.to_string(),
        r#"(modifiers: [Ctrl, Alt], key: "Delete"): System(LogOut),"#.to_string(),
    ];
    // Super+1…9 open the n-th taskbar app (these keys switch workspaces in COSMIC).
    for n in 1..=9 {
        lines.push(format!(r#"(modifiers: [Super], key: "{n}"): Spawn("lapin-ux-shell taskbar-item {n}"),"#));
    }
    let body: Vec<String> = lines.into_iter().map(|l| format!("    {l}")).collect();
    format!("{MARKER}\n{{\n{}\n}}\n", body.join("\n"))
}

fn apply_keybindings() -> Result<bool, cosmic_config::Error> {
    let dir = cfg_path("com.system76.CosmicSettings.Shortcuts");
    let file = dir.join("custom");
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let ours = existing.trim().is_empty()
        || existing.lines().next().is_some_and(|l| l.trim() == MARKER)
        || existing.trim() == LEGACY_V1_SHORTCUTS.trim();
    if !ours {
        tracing::info!("user has their own custom shortcuts; not changing them");
        return Ok(false);
    }
    let new = windows_style_shortcuts();
    if existing == new {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir).map_err(cosmic_config::Error::Io)?;
    std::fs::write(&file, new).map_err(cosmic_config::Error::Io)?;
    Ok(true)
}

/// Defaults that newer releases add for people who already ran an older one.
pub fn apply_upgrade(from: u32) {
    tracing::info!(from, to = DEFAULTS_VERSION, "upgrading defaults");
    match apply_keybindings() {
        Ok(true) => tracing::info!("updated keyboard shortcuts"),
        Ok(false) => {}
        Err(e) => tracing::warn!("could not update keyboard shortcuts: {e}"),
    }
}

/// Interface font for every libcosmic app (shell, settings, files…).
pub const UI_FONT: &str = "Inter";

fn font_installed(family: &str) -> bool {
    std::process::Command::new("fc-list")
        .arg(format!(":family={family}"))
        .arg("family")
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}

/// Switch the interface font to Inter, unless it isn't installed or the
/// user already picked a font of their own.
fn apply_font() -> Result<bool, cosmic_config::Error> {
    use cosmic::config::CosmicTk;
    if !font_installed(UI_FONT) {
        tracing::info!("{UI_FONT} is not installed; keeping the default interface font");
        return Ok(false);
    }
    let cfg = CosmicTk::config()?;
    let mut tk = CosmicTk::get_entry(&cfg).unwrap_or_else(|(_, tk)| tk);
    if tk.interface_font.family != CosmicTk::default().interface_font.family {
        tracing::info!(family = %tk.interface_font.family, "user chose an interface font; leaving it");
        return Ok(false);
    }
    tk.interface_font.family = UI_FONT.to_string();
    cfg.set("interface_font", tk.interface_font)?;
    Ok(true)
}

fn cfg_path(id: &str) -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default();
    base.join("cosmic").join(id).join("v1")
}

/// Make sure a light/dark preference exists; without it every libcosmic
/// app falls back to its built-in theme and ignores ours.
fn ensure_mode() -> bool {
    let Ok(cfg) = ThemeMode::config() else { return true };
    match ThemeMode::is_dark(&cfg) {
        Ok(dark) => dark,
        Err(_) => {
            if let Err(e) = cfg.set("is_dark", true) {
                tracing::warn!("could not set theme mode: {e}");
            }
            true
        }
    }
}

pub fn apply_first_run() {
    let dark = ensure_mode();
    for (what, res) in [
        ("dark theme", apply_theme_variant(true)),
        ("light theme", apply_theme_variant(false)),
        ("wallpaper", apply_wallpaper(dark)),
        ("interface font", apply_font()),
        ("Super key", apply_shortcuts()),
        ("keyboard shortcuts", apply_keybindings()),
    ] {
        match res {
            Ok(true) => tracing::info!("applied default {what}"),
            Ok(false) => tracing::info!("kept existing {what}"),
            Err(e) => tracing::warn!("could not apply default {what}: {e}"),
        }
    }
}
