// SPDX-License-Identifier: GPL-3.0-only
//! Task Manager's system-bus calls: who is signed in (logind) and the
//! system services (systemd). Both are stable, documented D-Bus APIs, so
//! this keeps working across systemd and COSMIC updates. Starting or
//! stopping a service is checked by polkit, which shows the admin prompt.

use lapin_ux_core::services::Service;
use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const SYSTEMD: &str = "org.freedesktop.systemd1";
const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";
const SYSTEMD_MANAGER: &str = "org.freedesktop.systemd1.Manager";

/// The uids with a session, from logind; `None` when logind can't be asked.
pub async fn signed_in_uids() -> Option<Vec<u32>> {
    let conn = Connection::system().await.ok()?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "ListUsers",
            &(),
        )
        .await
        .ok()?;
    let users: Vec<(u32, String, OwnedObjectPath)> = reply.body().deserialize().ok()?;
    Some(users.into_iter().map(|(uid, ..)| uid).collect())
}

type UnitRow = (String, String, String, String, String, String, OwnedObjectPath, u32, String, OwnedObjectPath);

async fn main_pid(conn: &Connection, path: &OwnedObjectPath) -> Option<u32> {
    let reply = conn
        .call_method(
            Some(SYSTEMD),
            path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.systemd1.Service", "MainPID"),
        )
        .await
        .ok()?;
    let v: OwnedValue = reply.body().deserialize().ok()?;
    u32::try_from(v).ok().filter(|p| *p > 0)
}

/// Every loaded system service, running or not.
pub async fn services() -> Result<Vec<Service>, String> {
    let conn = Connection::system().await.map_err(|e| e.to_string())?;
    let reply = conn
        .call_method(
            Some(SYSTEMD),
            SYSTEMD_PATH,
            Some(SYSTEMD_MANAGER),
            "ListUnitsByPatterns",
            &(Vec::<&str>::new(), vec!["*.service"]),
        )
        .await
        .map_err(|e| format!("systemd isn't available ({e})"))?;
    let rows: Vec<UnitRow> = reply.body().deserialize().map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(rows.len());
    for (unit, description, _load, active, sub, _following, path, ..) in rows {
        let pid = if active == "active" { main_pid(&conn, &path).await } else { None };
        out.push(Service { unit, description, active, sub, pid });
    }
    Ok(lapin_ux_core::services::tidy_services(out))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Start,
    Stop,
    Restart,
}

/// Start, stop or restart a service. polkit asks for an admin password.
pub async fn service_action(unit: String, verb: Verb) -> Result<(), String> {
    let conn = Connection::system().await.map_err(|e| e.to_string())?;
    let method = match verb {
        Verb::Start => "StartUnit",
        Verb::Stop => "StopUnit",
        Verb::Restart => "RestartUnit",
    };
    conn.call_method(Some(SYSTEMD), SYSTEMD_PATH, Some(SYSTEMD_MANAGER), method, &(unit.as_str(), "replace"))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
