// SPDX-License-Identifier: GPL-3.0-only
//! Quick settings model: what the status icons next to the clock and the
//! quick settings panel show. The D-Bus / command plumbing lives in the shell;
//! everything here is plain data and parsing so it can be unit-tested.

/// A Wi-Fi network as listed in the quick settings panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0–100.
    pub strength: u8,
    pub secured: bool,
    /// NetworkManager has a saved connection for it.
    pub known: bool,
    pub active: bool,
    /// Object path of the strongest access point with this SSID.
    pub ap_path: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    Offline,
    Wired,
    Wifi { strength: u8 },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Network {
    /// NetworkManager is running.
    pub available: bool,
    pub link: Link,
    /// Name of the active Wi-Fi network or wired connection.
    pub link_name: String,
    pub wifi_present: bool,
    pub wifi_enabled: bool,
    /// Wi-Fi device object path (for connect/scan).
    pub wifi_device: String,
    pub networks: Vec<WifiNetwork>,
    /// Saved VPN / WireGuard connections.
    pub vpns: Vec<Vpn>,
    /// Our mobile hotspot connection is up.
    pub hotspot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vpn {
    pub name: String,
    /// Saved connection object path.
    pub settings: String,
    /// Active connection object path while connected.
    pub active: Option<String>,
}

/// NetworkManager connection id of the mobile hotspot.
pub const HOTSPOT_ID: &str = "Mobile hotspot";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtDevice {
    pub path: String,
    pub name: String,
    pub icon: String,
    pub connected: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bluetooth {
    pub available: bool,
    pub adapter: String,
    pub powered: bool,
    /// Paired devices, connected first.
    pub devices: Vec<BtDevice>,
    /// The adapter is looking for new devices.
    pub discovering: bool,
    /// Devices found nearby that aren't paired yet (named ones only).
    pub nearby: Vec<BtDevice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Discharging,
    Full,
    /// Plugged in but not charging (charge limit reached, pending charge).
    PluggedIn,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Power {
    pub battery: Option<Battery>,
    /// Active power profile ("power-saver", "balanced", "performance"),
    /// when power-profiles-daemon (or tuned-ppd) is running.
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    pub percent: f64,
    pub state: Charge,
    /// Seconds until empty or full, when known.
    pub seconds_left: Option<i64>,
}

impl Charge {
    /// UPower `State`.
    pub fn from_upower(state: u32) -> Charge {
        match state {
            1 => Charge::Charging,
            4 => Charge::Full,
            5 => Charge::PluggedIn,
            _ => Charge::Discharging,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Volume {
    pub available: bool,
    /// 0.0–1.0 (can exceed 1.0 if the user boosted it elsewhere).
    pub level: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sink {
    pub name: String,
    pub description: String,
    pub default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Brightness {
    pub current: u32,
    pub max: u32,
}

impl Brightness {
    pub fn fraction(&self) -> f32 {
        if self.max == 0 { 0.0 } else { self.current as f32 / self.max as f32 }
    }
    /// Raw value for a 0–1 fraction; never fully dark (Windows keeps a floor too).
    pub fn raw_for(&self, fraction: f32) -> u32 {
        let min = (self.max as f32 * 0.01).ceil().max(1.0) as u32;
        ((fraction.clamp(0.0, 1.0) * self.max as f32).round() as u32).clamp(min.min(self.max), self.max)
    }
}

// ---- parsing -----------------------------------------------------------------

/// `wpctl get-volume @DEFAULT_AUDIO_SINK@` → "Volume: 0.40" / "Volume: 0.40 [MUTED]".
pub fn parse_wpctl_volume(out: &str) -> Option<Volume> {
    let rest = out.trim().strip_prefix("Volume:")?.trim();
    let mut parts = rest.split_whitespace();
    let level: f32 = parts.next()?.parse().ok()?;
    let muted = rest.contains("[MUTED]");
    Some(Volume { available: true, level, muted })
}

/// `pactl get-sink-volume @DEFAULT_SINK@`: the first percentage.
pub fn parse_pactl_volume(out: &str) -> Option<f32> {
    out.split('/')
        .map(str::trim)
        .find_map(|p| p.strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()))
        .map(|p| p / 100.0)
}

/// `pactl get-sink-mute @DEFAULT_SINK@` → "Mute: yes".
pub fn parse_pactl_mute(out: &str) -> bool {
    out.trim().strip_prefix("Mute:").is_some_and(|v| v.trim() == "yes")
}

/// `pactl list sinks` (long format): names and descriptions.
pub fn parse_pactl_sinks(out: &str, default: &str) -> Vec<Sink> {
    let mut sinks = Vec::new();
    let mut name: Option<String> = None;
    for line in out.lines() {
        let l = line.trim();
        if l.starts_with("Sink #") {
            name = None;
        } else if let Some(n) = l.strip_prefix("Name:") {
            name = Some(n.trim().to_string());
        } else if let Some(d) = l.strip_prefix("Description:")
            && let Some(n) = name.take()
        {
            let default = n == default;
            sinks.push(Sink { name: n, description: d.trim().to_string(), default });
        }
    }
    sinks
}

/// One app playing sound (a PulseAudio/PipeWire sink input).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppStream {
    pub index: u32,
    pub name: String,
    pub icon: String,
    /// Executable name, to find a better icon from installed apps.
    pub binary: String,
    pub level: f32,
    pub muted: bool,
}

/// Parse `pactl list sink-inputs`; streams of the same app are merged
/// (first one's index is used for its slider) and system sounds skipped.
pub fn parse_pactl_sink_inputs(out: &str) -> Vec<AppStream> {
    let mut list: Vec<AppStream> = Vec::new();
    let mut cur: Option<AppStream> = None;
    let mut role = String::new();
    let flush = |cur: &mut Option<AppStream>, role: &mut String, list: &mut Vec<AppStream>| {
        if let Some(s) = cur.take()
            && role != "event"
            && !s.name.is_empty()
            && !list.iter().any(|o| o.name == s.name)
        {
            list.push(s);
        }
        role.clear();
    };
    for line in out.lines() {
        let l = line.trim();
        if let Some(idx) = l.strip_prefix("Sink Input #") {
            flush(&mut cur, &mut role, &mut list);
            cur = idx.trim().parse().ok().map(|index| AppStream { index, ..Default::default() });
            continue;
        }
        let Some(s) = cur.as_mut() else { continue };
        if let Some(m) = l.strip_prefix("Mute:") {
            s.muted = m.trim() == "yes";
        } else if l.starts_with("Volume:") {
            if let Some(v) = parse_pactl_volume(l) {
                s.level = v;
            }
        } else if let Some((k, v)) = l.split_once(" = ") {
            let v = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "application.name" => s.name = v,
                "application.icon_name" => s.icon = v,
                "application.process.binary" => s.binary = v,
                "media.role" => role = v,
                _ => {}
            }
        }
    }
    flush(&mut cur, &mut role, &mut list);
    list
}

/// Keyboard layouts configured for cosmic-comp, from its `xkb_config` RON
/// file: `layout: "us,de"`, `variant: ",nodeadkeys"` → [("us",""), ("de","nodeadkeys")].
pub fn xkb_layouts(ron: &str) -> Vec<(String, String)> {
    let field = |name: &str| -> String {
        let Some(i) = ron.find(&format!("{name}:")) else { return String::new() };
        let rest = &ron[i + name.len() + 1..];
        let Some(a) = rest.find('"') else { return String::new() };
        let rest = &rest[a + 1..];
        rest[..rest.find('"').unwrap_or(rest.len())].to_string()
    };
    let layouts = field("layout");
    let variants = field("variant");
    let variants: Vec<&str> = variants.split(',').collect();
    layouts
        .split(',')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .enumerate()
        .map(|(i, l)| (l.to_string(), variants.get(i).map_or("", |v| v.trim()).to_string()))
        .collect()
}

/// Human name of a layout from xkb's `evdev.lst` ("us" → "English (US)").
pub fn xkb_description(lst: &str, layout: &str, variant: &str) -> Option<String> {
    let mut section = "";
    for line in lst.lines() {
        if let Some(s) = line.strip_prefix("! ") {
            section = s.trim();
            continue;
        }
        let l = line.trim();
        let Some((code, desc)) = l.split_once(char::is_whitespace) else { continue };
        let desc = desc.trim();
        if variant.is_empty() && section == "layout" && code == layout {
            return Some(desc.to_string());
        }
        // Variants read "nodeadkeys      de: German (no dead keys)".
        if !variant.is_empty() && section == "variant" && code == variant {
            if let Some(d) = desc.strip_prefix(&format!("{layout}: ")) {
                return Some(d.to_string());
            }
        }
    }
    None
}

/// Merge access points into one entry per SSID (strongest wins), hide
/// hidden networks, and sort: connected first, then known, then by strength.
pub fn merge_networks(mut aps: Vec<WifiNetwork>) -> Vec<WifiNetwork> {
    aps.retain(|a| !a.ssid.is_empty());
    aps.sort_by(|a, b| a.ssid.cmp(&b.ssid).then(b.active.cmp(&a.active)).then(b.strength.cmp(&a.strength)));
    aps.dedup_by(|later, first| {
        if later.ssid == first.ssid {
            first.active |= later.active;
            first.known |= later.known;
            true
        } else {
            false
        }
    });
    aps.sort_by(|a, b| b.active.cmp(&a.active).then(b.known.cmp(&a.known)).then(b.strength.cmp(&a.strength)).then(a.ssid.cmp(&b.ssid)));
    aps
}

// ---- icons and labels --------------------------------------------------------

pub fn wifi_icon(strength: u8) -> &'static str {
    match strength {
        0..=19 => "network-wireless-signal-none-symbolic",
        20..=39 => "network-wireless-signal-weak-symbolic",
        40..=59 => "network-wireless-signal-ok-symbolic",
        60..=79 => "network-wireless-signal-good-symbolic",
        _ => "network-wireless-signal-excellent-symbolic",
    }
}

pub fn network_icon(n: &Network) -> &'static str {
    match n.link {
        Link::Wired => "network-wired-symbolic",
        Link::Wifi { strength } => wifi_icon(strength),
        Link::Offline if n.wifi_present && !n.wifi_enabled => "network-wireless-offline-symbolic",
        Link::Offline => "network-offline-symbolic",
    }
}

pub fn volume_icon(v: &Volume) -> &'static str {
    if v.muted || v.level <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if v.level < 0.34 {
        "audio-volume-low-symbolic"
    } else if v.level < 0.67 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

pub fn battery_icon(b: &Battery) -> String {
    // Freedesktop names come in 10% steps: battery-level-40-charging-symbolic.
    let step = ((b.percent / 10.0).round() as i64 * 10).clamp(0, 100);
    match b.state {
        Charge::Full => "battery-level-100-charged-symbolic".into(),
        Charge::Charging | Charge::PluggedIn => format!("battery-level-{step}-charging-symbolic"),
        Charge::Discharging => format!("battery-level-{step}-symbolic"),
    }
}

/// "2 hr 15 min remaining" / "45 min until full".
pub fn battery_label(b: &Battery) -> String {
    let pct = format!("{:.0}%", b.percent);
    let hm = |s: i64| {
        let (h, m) = (s / 3600, (s % 3600) / 60);
        if h > 0 { format!("{h} hr {m} min") } else { format!("{m} min") }
    };
    match (b.state, b.seconds_left.filter(|s| *s > 0)) {
        (Charge::Full, _) => format!("{pct} · Fully charged"),
        (Charge::PluggedIn, _) => format!("{pct} · Plugged in"),
        (Charge::Charging, Some(s)) => format!("{pct} · {} until full", hm(s)),
        (Charge::Charging, None) => format!("{pct} · Charging"),
        (Charge::Discharging, Some(s)) => format!("{pct} · {} remaining", hm(s)),
        (Charge::Discharging, None) => pct,
    }
}

/// Tooltip-style one-liner for the network status icon.
pub fn network_label(n: &Network) -> String {
    match n.link {
        Link::Offline if !n.available => "Network status unavailable".into(),
        Link::Offline => "Not connected".into(),
        _ if n.link_name.is_empty() => "Connected".into(),
        _ => n.link_name.clone(),
    }
}

/// Quick settings tiles in the user's order: `order` lists tile ids as the
/// user arranged them, `hidden` the ones they removed. Tiles not mentioned
/// keep their default place after the arranged ones.
pub fn tiles_shown(available: &[&str], order: &[String], hidden: &[String]) -> Vec<String> {
    let mut shown: Vec<String> = order
        .iter()
        .filter(|id| available.contains(&id.as_str()) && !hidden.contains(id))
        .cloned()
        .collect();
    for id in available {
        if !shown.iter().any(|s| s == id) && !hidden.iter().any(|h| h == id) {
            shown.push(id.to_string());
        }
    }
    shown
}

/// Tiles that can be added back.
pub fn tiles_hidden(available: &[&str], hidden: &[String]) -> Vec<String> {
    available.iter().filter(|id| hidden.iter().any(|h| h == *id)).map(|s| s.to_string()).collect()
}

/// Move the shown tile at `from` to `to`; returns the new order.
pub fn move_tile(shown: &[String], from: usize, to: usize) -> Vec<String> {
    let mut v = shown.to_vec();
    if from < v.len() {
        let t = v.remove(from);
        v.insert(to.min(v.len()), t);
    }
    v
}

#[cfg(test)]
mod tile_tests {
    use super::*;

    #[test]
    fn order_hide_and_new_tiles() {
        let avail = ["wifi", "bt", "dnd", "vpn"];
        let order = vec!["dnd".to_string(), "wifi".into()];
        let hidden = vec!["bt".to_string()];
        assert_eq!(tiles_shown(&avail, &order, &hidden), vec!["dnd", "wifi", "vpn"]);
        assert_eq!(tiles_hidden(&avail, &hidden), vec!["bt"]);
        assert_eq!(move_tile(&["a".into(), "b".into(), "c".into()], 0, 2), vec!["b", "c", "a"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_layouts() {
        let ron = "(\n    rules: \"\",\n    model: \"pc104\",\n    layout: \"us,de\",\n    variant: \",nodeadkeys\",\n    options: None,\n)";
        assert_eq!(xkb_layouts(ron), vec![("us".into(), "".into()), ("de".into(), "nodeadkeys".into())]);
        let lst = "! model\n  pc104  Generic 104\n\n! layout\n  us              English (US)\n  de              German\n\n! variant\n  nodeadkeys      de: German (no dead keys)\n";
        assert_eq!(xkb_description(lst, "us", "").as_deref(), Some("English (US)"));
        assert_eq!(xkb_description(lst, "de", "nodeadkeys").as_deref(), Some("German (no dead keys)"));
        assert_eq!(xkb_description(lst, "fr", ""), None);
    }

    #[test]
    fn sink_inputs() {
        let out = "Sink Input #42\n\tMute: no\n\tVolume: front-left: 42598 /  65% / -11.23 dB,   front-right: 42598 /  65% / -11.23 dB\n\tProperties:\n\t\tapplication.name = \"Firefox\"\n\t\tapplication.process.binary = \"firefox\"\n\t\tapplication.icon_name = \"firefox\"\nSink Input #43\n\tMute: yes\n\tVolume: mono: 65536 / 100% / 0.00 dB\n\tProperties:\n\t\tapplication.name = \"Firefox\"\nSink Input #44\n\tMute: no\n\tVolume: mono: 65536 / 100%\n\tProperties:\n\t\tapplication.name = \"Spotify\"\nSink Input #45\n\tProperties:\n\t\tapplication.name = \"bell\"\n\t\tmedia.role = \"event\"\n";
        let s = parse_pactl_sink_inputs(out);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].index, s[0].name.as_str(), s[0].binary.as_str(), s[0].muted), (42, "Firefox", "firefox", false));
        assert!((s[0].level - 0.65).abs() < 0.01);
        assert_eq!((s[1].name.as_str(), s[1].level), ("Spotify", 1.0));
    }

    #[test]
    fn wpctl() {
        assert_eq!(parse_wpctl_volume("Volume: 0.40\n"), Some(Volume { available: true, level: 0.40, muted: false }));
        assert_eq!(parse_wpctl_volume("Volume: 0.15 [MUTED]"), Some(Volume { available: true, level: 0.15, muted: true }));
        assert_eq!(parse_wpctl_volume("garbage"), None);
    }

    #[test]
    fn pactl() {
        let v = "Volume: front-left: 26214 /  40% / -23.88 dB,   front-right: 26214 /  40% / -23.88 dB\n        balance 0.00";
        assert_eq!(parse_pactl_volume(v), Some(0.40));
        assert!(parse_pactl_mute("Mute: yes\n"));
        assert!(!parse_pactl_mute("Mute: no"));
        let list = "Sink #50\n\tState: RUNNING\n\tName: alsa_output.pci.analog-stereo\n\tDescription: Built-in Audio Analog Stereo\n\tDriver: PipeWire\nSink #61\n\tName: bluez_output.AA\n\tDescription: WH-1000XM4\n";
        let sinks = parse_pactl_sinks(list, "bluez_output.AA");
        assert_eq!(sinks.len(), 2);
        assert_eq!(sinks[0].description, "Built-in Audio Analog Stereo");
        assert!(!sinks[0].default);
        assert!(sinks[1].default);
    }

    fn ap(ssid: &str, strength: u8, known: bool, active: bool) -> WifiNetwork {
        WifiNetwork { ssid: ssid.into(), strength, secured: true, known, active, ap_path: format!("/{ssid}/{strength}") }
    }

    #[test]
    fn networks_merge_and_sort() {
        let merged = merge_networks(vec![
            ap("Cafe", 40, false, false),
            ap("Home", 50, true, false),
            ap("Home", 80, true, true),
            ap("", 99, false, false),
            ap("Neighbour", 90, false, false),
            ap("Cafe", 70, false, false),
        ]);
        let names: Vec<_> = merged.iter().map(|n| (n.ssid.as_str(), n.strength)).collect();
        assert_eq!(names, vec![("Home", 80), ("Neighbour", 90), ("Cafe", 70)]);
        assert!(merged[0].active && merged[0].known);
    }

    #[test]
    fn icons() {
        assert_eq!(wifi_icon(10), "network-wireless-signal-none-symbolic");
        assert_eq!(wifi_icon(65), "network-wireless-signal-good-symbolic");
        assert_eq!(volume_icon(&Volume { available: true, level: 0.5, muted: false }), "audio-volume-medium-symbolic");
        assert_eq!(volume_icon(&Volume { available: true, level: 0.5, muted: true }), "audio-volume-muted-symbolic");
        let b = Battery { percent: 47.0, state: Charge::Charging, seconds_left: Some(2700) };
        assert_eq!(battery_icon(&b), "battery-level-50-charging-symbolic");
        assert_eq!(battery_label(&b), "47% · 45 min until full");
        let b = Battery { percent: 81.4, state: Charge::Discharging, seconds_left: Some(8100) };
        assert_eq!(battery_icon(&b), "battery-level-80-symbolic");
        assert_eq!(battery_label(&b), "81% · 2 hr 15 min remaining");
    }

    #[test]
    fn brightness_floor() {
        let b = Brightness { current: 60, max: 120 };
        assert_eq!(b.fraction(), 0.5);
        assert_eq!(b.raw_for(0.0), 2);
        assert_eq!(b.raw_for(1.0), 120);
        assert_eq!(b.raw_for(0.25), 30);
    }
}
