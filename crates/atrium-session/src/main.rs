// SPDX-License-Identifier: GPL-3.0-only
//! Atrium session manager.
//!
//! 1. Starts the compositor (cosmic-comp) with a socket it uses to tell us
//!    which `WAYLAND_DISPLAY` / `DISPLAY` it created. This is the same
//!    handshake cosmic-session uses, so the compositor runs in normal mode
//!    (keyboard shortcuts enabled) rather than kiosk mode.
//! 2. Exports that environment to systemd and D-Bus so portals and
//!    D-Bus-activated apps can reach the display.
//! 3. Starts every component in `components()` and restarts any that crash,
//!    backing off if one keeps failing.
//! 4. Exposes `dev.atrium.Session.Exit()` on the session bus; the Start
//!    menu's "Sign out" calls it.

mod handshake;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::watch;
use tracing::{error, info, warn};

/// A program the session keeps running.
#[derive(Debug, Clone)]
struct Component {
    exec: &'static str,
    args: &'static [&'static str],
    /// Missing optional components are logged and skipped.
    required: bool,
}

const fn c(exec: &'static str, required: bool) -> Component {
    Component { exec, args: &[], required }
}

fn components() -> Vec<Component> {
    vec![
        // Ours
        c("atrium-shell", true),
        c("atrium-notifyd", true),
        // Reused, unmodified COSMIC services (no visible branding):
        // applies theme/fonts, media keys, night light
        c("cosmic-settings-daemon", false),
        // wallpaper renderer; Atrium writes its config
        c("cosmic-bg", false),
        // volume/brightness OSD and the polkit password dialog
        c("cosmic-osd", false),
        // screen blanking / suspend on idle
        c("cosmic-idle", false),
        // run as the logged-in user this is the lock screen (Win+L,
        // "Lock" in Start, lid close), not the login greeter
        c("cosmic-greeter", false),
    ]
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let compositor = std::env::args().nth(1).unwrap_or_else(|| "cosmic-comp".into());
    info!(%compositor, "starting Atrium session");

    let (mut comp, env) = handshake::start_compositor(&compositor).await?;
    info!(?env, "compositor is ready");
    for (k, v) in &env {
        // SAFETY: single-threaded at this point in the sense that matters:
        // nothing else reads the environment concurrently.
        unsafe { std::env::set_var(k, v) };
    }
    export_environment(&env).await;
    systemctl(&["start", "--no-block", "atrium-session.target"]).await;

    let (stop_tx, stop_rx) = watch::channel(false);
    let mut supervisors = Vec::new();
    for comp in components() {
        if !on_path(comp.exec) {
            if comp.required {
                error!(exec = comp.exec, "required component not installed");
            } else {
                info!(exec = comp.exec, "optional component not installed, skipping");
            }
            continue;
        }
        supervisors.push(tokio::spawn(supervise(comp, stop_rx.clone())));
    }

    let dbus = serve_dbus(stop_tx.clone()).await;
    if let Err(e) = &dbus {
        warn!("session D-Bus interface unavailable: {e:#}");
    }

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut stop_wait = stop_rx.clone();
    let code = tokio::select! {
        status = comp.wait() => {
            let status = status?;
            info!(?status, "compositor exited");
            status.code().unwrap_or(1)
        }
        _ = sigterm.recv() => { info!("SIGTERM"); 0 }
        _ = tokio::signal::ctrl_c() => { info!("SIGINT"); 0 }
        _ = stop_wait.wait_for(|s| *s) => { info!("sign-out requested"); 0 }
    };

    let _ = stop_tx.send(true);
    for s in supervisors {
        let _ = tokio::time::timeout(Duration::from_secs(3), s).await;
    }
    terminate(&mut comp).await;
    systemctl(&["stop", "--no-block", "atrium-session.target"]).await;
    std::process::exit(code);
}

/// Run one component forever, restarting it with exponential backoff.
async fn supervise(comp: Component, mut stop: watch::Receiver<bool>) {
    let mut backoff = Duration::from_millis(250);
    loop {
        let started = Instant::now();
        let mut child = match Command::new(comp.exec)
            .args(comp.args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                error!(exec = comp.exec, "failed to start: {e}");
                return;
            }
        };
        info!(exec = comp.exec, pid = child.id(), "started");

        tokio::select! {
            status = child.wait() => {
                match status {
                    Ok(s) if s.success() => info!(exec = comp.exec, "exited cleanly"),
                    Ok(s) => warn!(exec = comp.exec, ?s, "crashed"),
                    Err(e) => warn!(exec = comp.exec, "wait failed: {e}"),
                }
            }
            _ = stopped(&mut stop) => {
                terminate(&mut child).await;
                return;
            }
        }

        // A component that ran for a while gets a fresh backoff.
        if started.elapsed() > Duration::from_secs(30) {
            backoff = Duration::from_millis(250);
        } else {
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = stopped(&mut stop) => return,
        }
    }
}

/// Resolves once a stop has been requested. (Wrapping `wait_for` keeps its
/// non-`Send` guard out of the spawned future.)
async fn stopped(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|s| *s).await;
}

/// SIGTERM, then SIGKILL after a grace period.
async fn terminate(child: &mut Child) {
    if let Some(pid) = child.id().and_then(|p| rustix::process::Pid::from_raw(p as i32)) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
        if tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_ok() {
            return;
        }
    }
    let _ = child.kill().await;
}

fn on_path(exec: &str) -> bool {
    if exec.contains('/') {
        return std::path::Path::new(exec).exists();
    }
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(exec).is_file()))
        .unwrap_or(false)
}

/// Make the display reachable from systemd user units and D-Bus activated
/// services (portals, file choosers, apps started from notifications).
async fn export_environment(env: &HashMap<String, String>) {
    let mut names: Vec<&str> = env.keys().map(String::as_str).collect();
    names.extend(["XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE", "XDG_SESSION_DESKTOP"]);
    let mut args = vec!["import-environment"];
    args.extend(&names);
    systemctl(&args).await;

    let mut dbus_args = vec!["--systemd".to_string()];
    for n in &names {
        if let Ok(v) = std::env::var(n) {
            dbus_args.push(format!("{n}={v}"));
        }
    }
    run_quiet("dbus-update-activation-environment", &dbus_args).await;
}

async fn systemctl(args: &[&str]) {
    let mut full = vec!["--user"];
    full.extend(args);
    run_quiet("systemctl", &full).await;
}

async fn run_quiet<S: AsRef<std::ffi::OsStr>>(prog: &str, args: &[S]) {
    if !on_path(prog) {
        return;
    }
    match Command::new(prog).args(args).stdout(Stdio::null()).status().await {
        Ok(s) if s.success() => {}
        Ok(s) => warn!(prog, ?s, "command failed"),
        Err(e) => warn!(prog, "command failed: {e}"),
    }
}

struct SessionIface {
    stop: watch::Sender<bool>,
}

#[zbus::interface(name = "dev.atrium.Session")]
impl SessionIface {
    /// End the session (sign out).
    fn exit(&self) {
        let _ = self.stop.send(true);
    }
}

async fn serve_dbus(stop: watch::Sender<bool>) -> Result<zbus::Connection> {
    zbus::connection::Builder::session()?
        .name("dev.atrium.Session")?
        .serve_at("/dev/atrium/Session", SessionIface { stop })?
        .build()
        .await
        .context("connecting to the session bus")
}
