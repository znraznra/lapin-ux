// SPDX-License-Identifier: GPL-3.0-only
//! Removable drives on the system bus (UDisks2) for the taskbar's "Safely
//! Remove Hardware" icon (#102). The decisions (which drives are listed, names,
//! busy messages) live in `lapin_ux_core::removable`; this file talks to the bus
//! and reads `/proc`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use cosmic::iced::{Subscription, stream};
use futures::{FutureExt, SinkExt, StreamExt};
use lapin_ux_core::removable::{self as core, Drive, DriveProps, Proc, Remove, VolumeInfo};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream, Proxy};

const UDISKS: &str = "org.freedesktop.UDisks2";
const ROOT: &str = "/org/freedesktop/UDisks2";

#[derive(Debug, Clone)]
pub enum Event {
    Drives(Vec<Drive>),
}

type Props = HashMap<String, OwnedValue>;
type Managed = HashMap<OwnedObjectPath, HashMap<String, Props>>;

fn get<T: TryFrom<OwnedValue>>(p: &Props, key: &str) -> Option<T> {
    p.get(key)?.try_clone().ok()?.try_into().ok()
}

/// A NUL-terminated byte string (`ay`) as text.
fn bytes_text(b: Vec<u8>) -> String {
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

async fn proxy<'a>(conn: &Connection, path: &'a str, iface: &'a str) -> zbus::Result<Proxy<'a>> {
    zbus::proxy::Builder::new(conn)
        .destination(UDISKS)?
        .path(path)?
        .interface(iface)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

// ---- watching ------------------------------------------------------------------

pub fn subscription() -> Subscription<Event> {
    Subscription::run(|| {
        stream::channel(4, move |mut out: futures::channel::mpsc::Sender<Event>| async move {
            let conn = match Connection::system().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("removable: no system bus: {e}");
                    return futures::future::pending().await;
                }
            };
            let rules = [
                MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(UDISKS).map(|b| b.build()),
                MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .sender("org.freedesktop.DBus")
                    .and_then(|b| b.member("NameOwnerChanged"))
                    .and_then(|b| b.add_arg(UDISKS))
                    .map(|b| b.build()),
            ];
            let mut streams = Vec::new();
            for rule in rules.into_iter().flatten() {
                match MessageStream::for_match_rule(rule, &conn, Some(64)).await {
                    Ok(st) => streams.push(st),
                    Err(e) => tracing::debug!("removable: match rule: {e}"),
                }
            }
            let mut signals = futures::stream::select_all(streams);
            loop {
                // No UDisks2 (not running yet): keep the last list, try again on the next signal.
                if let Some(drives) = snapshot(&conn).await {
                    let _ = out.send(Event::Drives(drives)).await;
                }
                if signals.next().await.is_none() {
                    return futures::future::pending().await;
                }
                tokio::time::sleep(Duration::from_millis(350)).await;
                while let Some(Some(_)) = signals.next().now_or_never() {}
            }
        })
    })
}

async fn managed(conn: &Connection) -> Option<Managed> {
    let p = proxy(conn, ROOT, "org.freedesktop.DBus.ObjectManager").await.ok()?;
    p.call("GetManagedObjects", &()).await.ok()
}

async fn snapshot(conn: &Connection) -> Option<Vec<Drive>> {
    Some(drives_from(&managed(conn).await?))
}

fn drives_from(objs: &Managed) -> Vec<Drive> {
    let mut drives: Vec<Drive> = Vec::new();
    for (path, ifaces) in objs {
        let Some(d) = ifaces.get("org.freedesktop.UDisks2.Drive") else { continue };
        let compat: Vec<String> = get(d, "MediaCompatibility").unwrap_or_default();
        let props = DriveProps {
            vendor: get(d, "Vendor").unwrap_or_default(),
            model: get(d, "Model").unwrap_or_default(),
            bus: get(d, "ConnectionBus").unwrap_or_default(),
            removable: get(d, "Removable").unwrap_or(false),
            ejectable: get(d, "Ejectable").unwrap_or(false),
            can_power_off: get(d, "CanPowerOff").unwrap_or(false),
            media_removable: get(d, "MediaRemovable").unwrap_or(false),
            optical: compat.iter().any(|c| c.starts_with("optical_")),
        };
        if core::is_listed(&props) {
            drives.push(Drive { path: path.to_string(), props, volumes: Vec::new() });
        }
    }
    // The drive a block belongs to: its own, or (an open LUKS container's
    // cleartext device) the one of its backing device.
    let drive_of = |block: &Props| -> Option<String> {
        let own: Option<OwnedObjectPath> = get(block, "Drive");
        match own.filter(|p| p.as_str() != "/") {
            Some(p) => Some(p.to_string()),
            None => {
                let backing: OwnedObjectPath = get(block, "CryptoBackingDevice")?;
                let b = objs.get(&backing)?.get("org.freedesktop.UDisks2.Block")?;
                get::<OwnedObjectPath>(b, "Drive").map(|p| p.to_string())
            }
        }
    };
    let mut paths: Vec<&OwnedObjectPath> = objs.keys().collect();
    paths.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    for path in paths {
        let ifaces = &objs[path];
        let Some(block) = ifaces.get("org.freedesktop.UDisks2.Block") else { continue };
        let fs = ifaces.get("org.freedesktop.UDisks2.Filesystem");
        let enc = ifaces.get("org.freedesktop.UDisks2.Encrypted");
        if fs.is_none() && enc.is_none() {
            continue;
        }
        let Some(dp) = drive_of(block) else { continue };
        let Some(drive) = drives.iter_mut().find(|d| d.path == dp) else { continue };
        let mount_points: Vec<PathBuf> = fs
            .and_then(|f| get::<Vec<Vec<u8>>>(f, "MountPoints"))
            .unwrap_or_default()
            .into_iter()
            .map(|b| PathBuf::from(bytes_text(b)))
            .collect();
        let cleartext = enc.and_then(|e| get::<OwnedObjectPath>(e, "CleartextDevice")).filter(|p| p.as_str() != "/").map(|p| p.to_string());
        drive.volumes.push(VolumeInfo {
            path: path.to_string(),
            label: get(block, "IdLabel").unwrap_or_default(),
            size: get(block, "Size").unwrap_or(0),
            mount_points,
            encrypted: enc.is_some(),
            cleartext,
        });
    }
    drives.sort_by(|a, b| a.path.cmp(&b.path));
    drives
}

// ---- ejecting --------------------------------------------------------------------

fn no_options() -> HashMap<&'static str, Value<'static>> {
    HashMap::new()
}

fn error_name(e: &zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(name, ..) => name.to_string(),
        _ => String::new(),
    }
}

/// Unmount every volume, lock LUKS containers, then power off or eject.
/// Errors are ready to show to the user.
pub async fn eject(drive: Drive) -> Result<(), String> {
    let name = core::drive_name(&drive.props);
    let conn = Connection::system().await.map_err(|e| format!("Couldn't reach the system bus: {e}"))?;
    let busy = |paths: &[PathBuf]| {
        let mounts: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
        let names = core::busy_programs(&mounts, &read_procs());
        core::busy_message(&name, &names)
    };
    let mounts: Vec<PathBuf> = core::mounted_paths(&drive).into_iter().map(|p| p.to_path_buf()).collect();
    for v in drive.volumes.iter().filter(|v| v.mounted()) {
        let p = proxy(&conn, &v.path, "org.freedesktop.UDisks2.Filesystem").await.map_err(|e| e.to_string())?;
        if let Err(e) = p.call::<_, _, ()>("Unmount", &(no_options(),)).await {
            let n = error_name(&e);
            if core::is_busy_error(&n) {
                return Err(busy(&mounts));
            }
            // Already unmounted by someone else is fine.
            if !n.ends_with("NotMounted") {
                return Err(format!("Couldn't eject {name}: {e}"));
            }
        }
    }
    for v in drive.volumes.iter().filter(|v| v.encrypted && v.cleartext.is_some()) {
        let p = proxy(&conn, &v.path, "org.freedesktop.UDisks2.Encrypted").await.map_err(|e| e.to_string())?;
        if let Err(e) = p.call::<_, _, ()>("Lock", &(no_options(),)).await {
            if core::is_busy_error(&error_name(&e)) {
                return Err(busy(&mounts));
            }
            return Err(format!("Couldn't lock {}: {e}", core::volume_name(v)));
        }
    }
    let (method, how) = match core::how_to_remove(&drive.props) {
        Remove::PowerOff => ("PowerOff", "power off"),
        Remove::Eject => ("Eject", "eject"),
    };
    let p = proxy(&conn, &drive.path, "org.freedesktop.UDisks2.Drive").await.map_err(|e| e.to_string())?;
    p.call::<_, _, ()>(method, &(no_options(),)).await.map_err(|e| {
        if core::is_busy_error(&error_name(&e)) { busy(&mounts) } else { format!("Couldn't {how} {name}: {e}") }
    })
}

/// Running programs and the places they hold open, from `/proc`. Only what we
/// are allowed to read (our own processes) shows up.
fn read_procs() -> Vec<Proc> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return out };
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let base = entry.path();
        let mut open: Vec<PathBuf> = Vec::new();
        if let Ok(cwd) = std::fs::read_link(base.join("cwd")) {
            open.push(cwd);
        }
        if let Ok(fds) = std::fs::read_dir(base.join("fd")) {
            open.extend(fds.flatten().filter_map(|f| std::fs::read_link(f.path()).ok()));
        }
        if open.is_empty() {
            continue;
        }
        let name = std::fs::read_to_string(base.join("comm")).unwrap_or_default().trim().to_string();
        out.push(Proc { pid, name, open });
    }
    out
}

/// A toast through the notification daemon.
pub async fn toast(summary: &str, body: &str) {
    let Ok(conn) = zbus::Connection::session().await else { return };
    let hints: HashMap<&str, Value> = HashMap::new();
    let _ = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Lapin UX", 0u32, "drive-removable-media-symbolic", summary, body, Vec::<&str>::new(), hints, -1i32),
        )
        .await;
}
