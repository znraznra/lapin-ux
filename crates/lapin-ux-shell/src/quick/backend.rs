// SPDX-License-Identifier: GPL-3.0-only
//! System services behind quick settings: NetworkManager, BlueZ, UPower,
//! power-profiles-daemon and logind on the system bus; PipeWire/PulseAudio
//! through `wpctl` / `pactl`.
//!
//! Each service is watched the same way: take a snapshot, then re-take it
//! (debounced) whenever the service emits any signal or its bus name changes
//! owner. Snapshots are small, and this stays correct however the service's
//! object graph changes.

use std::collections::HashMap;
use std::time::Duration;

use cosmic::iced::{Subscription, stream};
use futures::{SinkExt, StreamExt};
use lapin_ux_core::quick::{
    AppStream, HOTSPOT_ID, Vpn, Battery, Bluetooth, BtDevice, Charge, Link, Network, Power, Sink, Volume, WifiNetwork, merge_networks,
    parse_pactl_mute, parse_pactl_sink_inputs, parse_pactl_sinks, parse_pactl_volume, parse_wpctl_volume,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream, Proxy};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const BLUEZ: &str = "org.bluez";
const UPOWER: &str = "org.freedesktop.UPower";
const PPD: [(&str, &str); 2] = [
    ("org.freedesktop.UPower.PowerProfiles", "/org/freedesktop/UPower/PowerProfiles"),
    ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
];

#[derive(Debug, Clone)]
pub enum Event {
    Network(Network),
    Bluetooth(Bluetooth),
    Power(Power),
    Volume(Volume),
    Sinks(Vec<Sink>),
    /// Apps playing sound (volume mixer).
    Streams(Vec<AppStream>),
}

async fn proxy<'a>(conn: &Connection, dest: &'a str, path: &'a str, iface: &'a str) -> zbus::Result<Proxy<'a>> {
    zbus::proxy::Builder::new(conn)
        .destination(dest)?
        .path(path)?
        .interface(iface)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

// ---- generic watcher -----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Service {
    Network,
    Bluetooth,
    Power,
}

impl Service {
    fn names(self) -> &'static [&'static str] {
        match self {
            Service::Network => &[NM],
            Service::Bluetooth => &[BLUEZ],
            Service::Power => &[UPOWER, PPD[0].0, PPD[1].0],
        }
    }

    async fn snapshot(self, c: &Connection) -> Event {
        match self {
            Service::Network => Event::Network(network(c).await.unwrap_or_default()),
            Service::Bluetooth => Event::Bluetooth(bluetooth(c).await.unwrap_or_default()),
            Service::Power => Event::Power(power(c).await),
        }
    }
}

/// Re-take the service's snapshot whenever it emits a signal or (dis)appears.
fn watch(service: Service) -> Subscription<Event> {
    Subscription::run_with(service, |service| {
        let service = *service;
        stream::channel(4, move |mut out: futures::channel::mpsc::Sender<Event>| async move {
            let conn = match Connection::system().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("{service:?}: no system bus: {e}");
                    return futures::future::pending().await;
                }
            };
            let mut streams = Vec::new();
            for s in service.names() {
                let rules = [
                    MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(*s).map(|b| b.build()),
                    MatchRule::builder()
                        .msg_type(zbus::message::Type::Signal)
                        .sender("org.freedesktop.DBus")
                        .and_then(|b| b.member("NameOwnerChanged"))
                        .and_then(|b| b.add_arg(*s))
                        .map(|b| b.build()),
                ];
                for rule in rules.into_iter().flatten() {
                    match MessageStream::for_match_rule(rule, &conn, Some(64)).await {
                        Ok(st) => streams.push(st),
                        Err(e) => tracing::debug!("{service:?}: match rule: {e}"),
                    }
                }
            }
            let mut signals = futures::stream::select_all(streams);
            loop {
                let _ = out.send(service.snapshot(&conn).await).await;
                if signals.next().await.is_none() {
                    return futures::future::pending().await;
                }
                // Changes come in bursts (a scan updates dozens of APs).
                tokio::time::sleep(Duration::from_millis(350)).await;
                while let Some(Some(_)) = signals.next().now_or_never() {}
            }
        })
    })
}

use futures::FutureExt;

pub fn subscriptions() -> Subscription<Event> {
    Subscription::batch([watch(Service::Network), watch(Service::Bluetooth), watch(Service::Power), audio_subscription()])
}

// ---- NetworkManager --------------------------------------------------------------

type Settings = HashMap<String, HashMap<String, OwnedValue>>;

fn ssid_of(settings: &Settings) -> Option<String> {
    let v = settings.get("802-11-wireless")?.get("ssid")?;
    let bytes: Vec<u8> = v.try_clone().ok()?.try_into().ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Saved Wi-Fi connections: SSID → connection object path.
async fn saved_wifi(conn: &Connection) -> zbus::Result<HashMap<String, OwnedObjectPath>> {
    let settings = proxy(conn, NM, "/org/freedesktop/NetworkManager/Settings", "org.freedesktop.NetworkManager.Settings").await?;
    let paths: Vec<OwnedObjectPath> = settings.call("ListConnections", &()).await?;
    let mut out = HashMap::new();
    for p in paths {
        let c = proxy(conn, NM, p.as_str(), "org.freedesktop.NetworkManager.Settings.Connection").await?;
        if let Ok(s) = c.call::<_, _, Settings>("GetSettings", &()).await
            && let Some(ssid) = ssid_of(&s)
        {
            out.insert(ssid, p);
        }
    }
    Ok(out)
}

pub async fn network(conn: &Connection) -> zbus::Result<Network> {
    let nm = proxy(conn, NM, NM_PATH, NM).await?;
    let mut n = Network { available: true, ..Default::default() };
    n.wifi_enabled = nm.get_property("WirelessEnabled").await?;
    let devices: Vec<OwnedObjectPath> = nm.call("GetDevices", &()).await?;
    let mut active_ap = None;
    let mut aps = Vec::new();
    for d in &devices {
        let dev = proxy(conn, NM, d.as_str(), "org.freedesktop.NetworkManager.Device").await?;
        if dev.get_property::<u32>("DeviceType").await? != 2 || n.wifi_present {
            continue;
        }
        n.wifi_present = true;
        n.wifi_device = d.to_string();
        let wl = proxy(conn, NM, d.as_str(), "org.freedesktop.NetworkManager.Device.Wireless").await?;
        let active: OwnedObjectPath = wl.get_property("ActiveAccessPoint").await?;
        if active.as_str() != "/" {
            active_ap = Some(active.to_string());
        }
        aps = wl.call::<_, _, Vec<OwnedObjectPath>>("GetAllAccessPoints", &()).await.unwrap_or_default();
    }
    let saved = saved_wifi(conn).await.unwrap_or_default();
    let mut list = Vec::new();
    for ap in aps {
        let Ok(p) = proxy(conn, NM, ap.as_str(), "org.freedesktop.NetworkManager.AccessPoint").await else { continue };
        let Ok(ssid) = p.get_property::<Vec<u8>>("Ssid").await else { continue };
        let ssid = String::from_utf8_lossy(&ssid).into_owned();
        let strength: u8 = p.get_property("Strength").await.unwrap_or(0);
        let flags: u32 = p.get_property("Flags").await.unwrap_or(0);
        let wpa: u32 = p.get_property("WpaFlags").await.unwrap_or(0);
        let rsn: u32 = p.get_property("RsnFlags").await.unwrap_or(0);
        let active = active_ap.as_deref() == Some(ap.as_str());
        list.push(WifiNetwork {
            known: saved.contains_key(&ssid),
            ssid,
            strength,
            secured: flags & 1 != 0 || wpa != 0 || rsn != 0,
            active,
            ap_path: ap.to_string(),
        });
    }
    n.networks = merge_networks(list);
    let (vpns, hotspot) = vpns_and_hotspot(conn, &nm).await;
    n.vpns = vpns;
    n.hotspot = hotspot;

    let primary: OwnedObjectPath = nm.get_property("PrimaryConnection").await?;
    if primary.as_str() != "/" {
        let ac = proxy(conn, NM, primary.as_str(), "org.freedesktop.NetworkManager.Connection.Active").await?;
        let kind: String = ac.get_property("Type").await.unwrap_or_default();
        n.link_name = ac.get_property("Id").await.unwrap_or_default();
        n.link = if kind == "802-11-wireless" {
            let strength = n.networks.iter().find(|w| w.active).map_or(0, |w| w.strength);
            if let Some(w) = n.networks.iter().find(|w| w.active) {
                n.link_name = w.ssid.clone();
            }
            Link::Wifi { strength }
        } else {
            Link::Wired
        };
    }
    Ok(n)
}

/// Saved VPNs (with their active connection, if up) and whether our
/// hotspot is running.
async fn vpns_and_hotspot(conn: &Connection, nm: &Proxy<'_>) -> (Vec<Vpn>, bool) {
    // Saved connection path → active connection path.
    let mut active: HashMap<String, String> = HashMap::new();
    let mut hotspot = false;
    for a in nm.get_property::<Vec<OwnedObjectPath>>("ActiveConnections").await.unwrap_or_default() {
        let Ok(ac) = proxy(conn, NM, a.as_str(), "org.freedesktop.NetworkManager.Connection.Active").await else { continue };
        if ac.get_property::<String>("Id").await.is_ok_and(|id| id == HOTSPOT_ID) {
            hotspot = true;
        }
        if let Ok(c) = ac.get_property::<OwnedObjectPath>("Connection").await {
            active.insert(c.to_string(), a.to_string());
        }
    }
    let mut vpns = Vec::new();
    let Ok(settings) = proxy(conn, NM, "/org/freedesktop/NetworkManager/Settings", "org.freedesktop.NetworkManager.Settings").await else {
        return (vpns, hotspot);
    };
    for p in settings.call::<_, _, Vec<OwnedObjectPath>>("ListConnections", &()).await.unwrap_or_default() {
        let Ok(c) = proxy(conn, NM, p.as_str(), "org.freedesktop.NetworkManager.Settings.Connection").await else { continue };
        let Ok(s) = c.call::<_, _, Settings>("GetSettings", &()).await else { continue };
        let Some(base) = s.get("connection") else { continue };
        let kind: String = prop(base, "type").unwrap_or_default();
        if kind == "vpn" || kind == "wireguard" {
            vpns.push(Vpn { name: prop(base, "id").unwrap_or_default(), settings: p.to_string(), active: active.get(p.as_str()).cloned() });
        }
    }
    vpns.sort_by(|a, b| a.name.cmp(&b.name));
    (vpns, hotspot)
}

/// Connect or disconnect a VPN.
pub async fn vpn(settings: String, active: Option<String>) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    let nm = proxy(&conn, NM, NM_PATH, NM).await?;
    match active {
        Some(a) => nm.call::<_, _, ()>("DeactivateConnection", &(OwnedObjectPath::try_from(a.as_str())?,)).await,
        None => {
            let root = OwnedObjectPath::try_from("/")?;
            nm.call::<_, _, OwnedObjectPath>("ActivateConnection", &(OwnedObjectPath::try_from(settings.as_str())?, root.clone(), root))
                .await
                .map(|_| ())
        }
    }
}

/// Start or stop the mobile hotspot (Wi-Fi access point sharing this
/// computer's connection), creating its connection the first time.
pub async fn hotspot(device: String, on: bool, name: String, password: String) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    let nm = proxy(&conn, NM, NM_PATH, NM).await?;
    for a in nm.get_property::<Vec<OwnedObjectPath>>("ActiveConnections").await.unwrap_or_default() {
        let ac = proxy(&conn, NM, a.as_str(), "org.freedesktop.NetworkManager.Connection.Active").await?;
        if ac.get_property::<String>("Id").await.is_ok_and(|id| id == HOTSPOT_ID) {
            if !on {
                return nm.call::<_, _, ()>("DeactivateConnection", &(a,)).await;
            }
            return Ok(());
        }
    }
    if !on {
        return Ok(());
    }
    let dev = OwnedObjectPath::try_from(device.as_str())?;
    let root = OwnedObjectPath::try_from("/")?;
    // Reuse the saved hotspot connection if there is one.
    let settings = proxy(&conn, NM, "/org/freedesktop/NetworkManager/Settings", "org.freedesktop.NetworkManager.Settings").await?;
    for p in settings.call::<_, _, Vec<OwnedObjectPath>>("ListConnections", &()).await.unwrap_or_default() {
        let c = proxy(&conn, NM, p.as_str(), "org.freedesktop.NetworkManager.Settings.Connection").await?;
        if let Ok(s) = c.call::<_, _, Settings>("GetSettings", &()).await
            && s.get("connection").and_then(|b| prop::<String>(b, "id")).as_deref() == Some(HOTSPOT_ID)
        {
            nm.call::<_, _, OwnedObjectPath>("ActivateConnection", &(p, dev, root)).await?;
            return Ok(());
        }
    }
    let mut s: HashMap<&str, HashMap<&str, Value>> = HashMap::new();
    s.insert(
        "connection",
        HashMap::from([
            ("id", Value::from(HOTSPOT_ID)),
            ("type", Value::from("802-11-wireless")),
            ("autoconnect", Value::from(false)),
        ]),
    );
    s.insert("802-11-wireless", HashMap::from([("ssid", Value::from(name.into_bytes())), ("mode", Value::from("ap"))]));
    s.insert(
        "802-11-wireless-security",
        HashMap::from([("key-mgmt", Value::from("wpa-psk")), ("psk", Value::from(password))]),
    );
    s.insert("ipv4", HashMap::from([("method", Value::from("shared"))]));
    s.insert("ipv6", HashMap::from([("method", Value::from("ignore"))]));
    nm.call::<_, _, (OwnedObjectPath, OwnedObjectPath)>("AddAndActivateConnection", &(s, dev, root)).await?;
    Ok(())
}

pub async fn set_wifi(enabled: bool) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    proxy(&conn, NM, NM_PATH, NM).await?.set_property("WirelessEnabled", enabled).await.map_err(Into::into)
}

pub async fn wifi_scan(device: String) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    let wl = proxy(&conn, NM, &device, "org.freedesktop.NetworkManager.Device.Wireless").await?;
    wl.call::<_, _, ()>("RequestScan", &(HashMap::<String, Value>::new(),)).await
}

/// Connect to `ssid`: re-use the saved connection when there is one,
/// otherwise create one (with `password` for secured networks).
pub async fn wifi_connect(device: String, ap: String, ssid: String, password: Option<String>) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    let nm = proxy(&conn, NM, NM_PATH, NM).await?;
    let dev = OwnedObjectPath::try_from(device.as_str())?;
    let ap = OwnedObjectPath::try_from(ap.as_str())?;
    if password.is_none()
        && let Some(saved) = saved_wifi(&conn).await?.remove(&ssid)
    {
        nm.call::<_, _, OwnedObjectPath>("ActivateConnection", &(saved, dev, ap)).await?;
        return Ok(());
    }
    let mut settings: HashMap<&str, HashMap<&str, Value>> = HashMap::new();
    if let Some(pw) = password {
        settings.insert(
            "802-11-wireless-security",
            HashMap::from([("key-mgmt", Value::from("wpa-psk")), ("psk", Value::from(pw))]),
        );
    }
    nm.call::<_, _, (OwnedObjectPath, OwnedObjectPath)>("AddAndActivateConnection", &(settings, dev, ap)).await?;
    Ok(())
}

pub async fn wifi_disconnect(device: String) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    proxy(&conn, NM, &device, "org.freedesktop.NetworkManager.Device").await?.call::<_, _, ()>("Disconnect", &()).await
}

// ---- BlueZ -----------------------------------------------------------------------

type Managed = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

fn prop<T: TryFrom<OwnedValue>>(props: &HashMap<String, OwnedValue>, key: &str) -> Option<T> {
    props.get(key).and_then(|v| v.try_clone().ok()).and_then(|v| T::try_from(v).ok())
}

pub async fn bluetooth(conn: &Connection) -> zbus::Result<Bluetooth> {
    let om = proxy(conn, BLUEZ, "/", "org.freedesktop.DBus.ObjectManager").await?;
    let objects: Managed = om.call("GetManagedObjects", &()).await?;
    let mut bt = Bluetooth { available: true, ..Default::default() };
    let mut paths: Vec<_> = objects.keys().collect();
    paths.sort_by_key(|p| p.as_str());
    for path in paths {
        let ifaces = &objects[path];
        if let Some(a) = ifaces.get("org.bluez.Adapter1")
            && bt.adapter.is_empty()
        {
            bt.adapter = path.to_string();
            bt.powered = prop::<bool>(a, "Powered").unwrap_or(false);
        }
        if let Some(d) = ifaces.get("org.bluez.Device1")
            && prop::<bool>(d, "Paired").unwrap_or(false)
        {
            bt.devices.push(BtDevice {
                path: path.to_string(),
                name: prop::<String>(d, "Alias").or_else(|| prop(d, "Name")).unwrap_or_else(|| "Unknown device".into()),
                icon: prop::<String>(d, "Icon").unwrap_or_default(),
                connected: prop::<bool>(d, "Connected").unwrap_or(false),
            });
        }
    }
    if bt.adapter.is_empty() {
        bt.available = false;
    }
    bt.devices.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.cmp(&b.name)));
    Ok(bt)
}

pub async fn set_bluetooth(adapter: String, on: bool) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    proxy(&conn, BLUEZ, &adapter, "org.bluez.Adapter1").await?.set_property("Powered", on).await.map_err(Into::into)
}

pub async fn bluetooth_connect(device: String, connect: bool) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    let d = proxy(&conn, BLUEZ, &device, "org.bluez.Device1").await?;
    d.call::<_, _, ()>(if connect { "Connect" } else { "Disconnect" }, &()).await
}

// ---- UPower / power profiles -------------------------------------------------------

pub async fn power(conn: &Connection) -> Power {
    let mut p = Power::default();
    if let Ok(d) = proxy(conn, UPOWER, "/org/freedesktop/UPower/devices/DisplayDevice", "org.freedesktop.UPower.Device").await
        && d.get_property::<bool>("IsPresent").await.unwrap_or(false)
        && d.get_property::<u32>("Type").await.unwrap_or(0) == 2
    {
        let state = Charge::from_upower(d.get_property("State").await.unwrap_or(0));
        let left: i64 = match state {
            Charge::Charging => d.get_property("TimeToFull").await.unwrap_or(0),
            _ => d.get_property("TimeToEmpty").await.unwrap_or(0),
        };
        p.battery = Some(Battery {
            percent: d.get_property("Percentage").await.unwrap_or(0.0),
            state,
            seconds_left: (left > 0).then_some(left),
        });
    }
    for (name, path) in PPD {
        if let Ok(pp) = proxy(conn, name, path, name).await
            && let Ok(active) = pp.get_property::<String>("ActiveProfile").await
        {
            p.profile = Some(active);
            break;
        }
    }
    p
}

pub async fn set_profile(profile: &'static str) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    let mut last = None;
    for (name, path) in PPD {
        match proxy(&conn, name, path, name).await?.set_property("ActiveProfile", profile).await {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.map_or(zbus::Error::Failure("no power profiles service".into()), Into::into))
}

// ---- backlight ---------------------------------------------------------------------

/// First backlight device (firmware/platform interfaces are preferred over raw).
/// `LAPIN_UX_BACKLIGHT_DIR` overrides `/sys/class/backlight` for testing.
pub fn backlight() -> Option<(String, lapin_ux_core::quick::Brightness)> {
    let dir = std::env::var("LAPIN_UX_BACKLIGHT_DIR").unwrap_or_else(|_| "/sys/class/backlight".into());
    let mut devs: Vec<_> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).collect();
    let rank = |p: &std::path::PathBuf| match std::fs::read_to_string(p.join("type")).unwrap_or_default().trim() {
        "firmware" => 0,
        "platform" => 1,
        _ => 2,
    };
    devs.sort_by_key(rank);
    let p = devs.first()?;
    let read = |f: &str| std::fs::read_to_string(p.join(f)).ok()?.trim().parse::<u32>().ok();
    let b = lapin_ux_core::quick::Brightness { current: read("brightness")?, max: read("max_brightness")? };
    Some((p.file_name()?.to_string_lossy().into_owned(), b))
}

/// Through logind, so no root or udev rule is needed.
pub async fn set_brightness(device: String, value: u32) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    let s = proxy(&conn, "org.freedesktop.login1", "/org/freedesktop/login1/session/auto", "org.freedesktop.login1.Session").await?;
    s.call::<_, _, ()>("SetBrightness", &("backlight", device.as_str(), value)).await
}

// ---- audio -------------------------------------------------------------------------

async fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new(cmd).args(args).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub async fn volume() -> Volume {
    if let Some(v) = output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]).await.as_deref().and_then(parse_wpctl_volume) {
        return v;
    }
    let level = output("pactl", &["get-sink-volume", "@DEFAULT_SINK@"]).await.as_deref().and_then(parse_pactl_volume);
    match level {
        Some(level) => {
            let muted = output("pactl", &["get-sink-mute", "@DEFAULT_SINK@"]).await.is_some_and(|m| parse_pactl_mute(&m));
            Volume { available: true, level, muted }
        }
        None => Volume::default(),
    }
}

pub async fn sinks() -> Vec<Sink> {
    let default = output("pactl", &["get-default-sink"]).await.unwrap_or_default();
    match output("pactl", &["list", "sinks"]).await {
        Some(list) => parse_pactl_sinks(&list, default.trim()),
        None => Vec::new(),
    }
}

async fn run(cmd: &str, args: &[&str]) -> bool {
    tokio::process::Command::new(cmd).args(args).status().await.is_ok_and(|s| s.success())
}

pub async fn set_volume(level: f32) {
    let l = format!("{:.2}", level.clamp(0.0, 1.0));
    if !run("wpctl", &["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", &l]).await {
        let pct = format!("{}%", (level.clamp(0.0, 1.0) * 100.0).round());
        run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &pct]).await;
    }
}

pub async fn set_mute(muted: bool) {
    let m = if muted { "1" } else { "0" };
    if !run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", m]).await {
        run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", m]).await;
    }
}

pub async fn streams() -> Vec<AppStream> {
    output("pactl", &["list", "sink-inputs"]).await.as_deref().map(parse_pactl_sink_inputs).unwrap_or_default()
}

pub async fn set_stream_volume(index: u32, level: f32) {
    let pct = format!("{}%", (level.clamp(0.0, 1.0) * 100.0).round());
    run("pactl", &["set-sink-input-volume", &index.to_string(), &pct]).await;
}

pub async fn set_stream_mute(index: u32, muted: bool) {
    run("pactl", &["set-sink-input-mute", &index.to_string(), if muted { "1" } else { "0" }]).await;
}

pub async fn set_default_sink(name: String) {
    run("pactl", &["set-default-sink", &name]).await;
}

/// Volume and outputs: follow `pactl subscribe` (works with PipeWire's pulse
/// server); without pactl, poll every few seconds.
fn audio_subscription() -> Subscription<Event> {
    Subscription::run_with("lapin-ux-quick-audio", |_| {
        stream::channel(4, |mut out: futures::channel::mpsc::Sender<Event>| async move {
            use tokio::io::AsyncBufReadExt;
            loop {
                let _ = out.send(Event::Volume(volume().await)).await;
                let _ = out.send(Event::Sinks(sinks().await)).await;
                let _ = out.send(Event::Streams(streams().await)).await;
                let child = tokio::process::Command::new("pactl")
                    .arg("subscribe")
                    .stdout(std::process::Stdio::piped())
                    .kill_on_drop(true)
                    .spawn();
                let Ok(mut child) = child else {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                };
                let Some(stdout) = child.stdout.take() else { continue };
                let mut lines = tokio::io::BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.contains("sink-input") {
                        tokio::time::sleep(Duration::from_millis(60)).await;
                        let _ = out.send(Event::Streams(streams().await)).await;
                        continue;
                    }
                    if !(line.contains("sink") || line.contains("server")) {
                        continue;
                    }
                    tokio::time::sleep(Duration::from_millis(60)).await;
                    let _ = out.send(Event::Volume(volume().await)).await;
                    if line.contains("'new'") || line.contains("'remove'") || line.contains("server") {
                        let _ = out.send(Event::Sinks(sinks().await)).await;
                    }
                }
                // pactl exited (sound server restarted); start over shortly.
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        })
    })
}
