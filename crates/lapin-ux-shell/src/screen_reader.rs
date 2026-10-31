// SPDX-License-Identifier: GPL-3.0-only
//! Narrator (Orca), with one owner (#18).
//!
//! The AT-SPI "screen reader on" flag (`org.a11y.Status.ScreenReaderEnabled`
//! on the session bus) is the switch. Quick settings, Ctrl+Win+Enter and
//! Settings only set that flag; the shell watches it and starts or stops
//! Orca to match, like COSMIC's and GNOME's sessions do. Before this, Lapin
//! started Orca directly in some places and only set the flag in others,
//! so Orca (which quits when the flag reads off) and the switches fell out
//! of step: opening Settings' Vision page stopped Orca, and the switch there
//! couldn't start it again.
//!
//! Turning the screen reader on also sets `IsEnabled`, the flag apps check
//! to switch their own accessibility on (Chromium/Electron apps such as
//! Claude, #44). at-spi keeps both flags in the GNOME settings
//! `org.gnome.desktop.a11y.applications screen-reader-enabled` and
//! `org.gnome.desktop.interface toolkit-accessibility`, which Lapin swaps
//! per session (tools/lapin-ux-uninstall).

use cosmic::iced::{self, Subscription};
use futures::{SinkExt, StreamExt};

#[zbus::proxy(interface = "org.a11y.Status", default_service = "org.a11y.Bus", default_path = "/org/a11y/bus")]
trait Status {
    #[zbus(property)]
    fn screen_reader_enabled(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_screen_reader_enabled(&self, on: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn is_enabled(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_is_enabled(&self, on: bool) -> zbus::Result<()>;
}

/// Whether an `orca` process is running.
pub fn orca_running() -> bool {
    std::fs::read_dir("/proc").into_iter().flatten().flatten().any(|e| {
        std::fs::read(e.path().join("cmdline"))
            .is_ok_and(|c| c.split(|b| *b == 0).take(2).any(|a| a.ends_with(b"/orca") || a == b"orca"))
    })
}

/// Set the screen reader flag. Returns false when it couldn't be set (no
/// accessibility bus).
pub async fn set(on: bool) -> bool {
    let res: zbus::Result<()> = async {
        let conn = zbus::Connection::session().await?;
        let status = StatusProxy::new(&conn).await?;
        if on {
            // Apps switch their accessibility on when this is set.
            status.set_is_enabled(true).await?;
        }
        status.set_screen_reader_enabled(on).await
    }
    .await;
    match res {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("screen reader: couldn't set the AT-SPI flag: {e}");
            false
        }
    }
}

/// Start or stop Orca to match `on` (the watcher's job).
pub fn follow(on: bool) {
    let running = orca_running();
    let args: &[&str] = match (on, running) {
        (true, false) => &[],
        (false, true) => &["--quit"],
        _ => return,
    };
    tracing::info!(on, "screen reader: {}", if on { "starting Orca" } else { "stopping Orca" });
    match std::process::Command::new("orca").args(args).spawn() {
        // Reap it when it ends, so no finished Orca lingers as a zombie.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => tracing::warn!("orca: {e}"),
    }
}

/// The AT-SPI screen reader flag: its value at start, then every change.
pub fn subscription() -> Subscription<bool> {
    Subscription::run_with("lapin-ux-screen-reader", |_| {
        iced::stream::channel(8, |mut tx: futures::channel::mpsc::Sender<bool>| async move {
            let status = async {
                let conn = zbus::Connection::session().await?;
                StatusProxy::new(&conn).await
            }
            .await;
            let status = match status {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("screen reader: no accessibility bus: {e}");
                    return futures::future::pending().await;
                }
            };
            let mut changes = status.receive_screen_reader_enabled_changed().await;
            if let Ok(on) = status.screen_reader_enabled().await {
                let _ = tx.send(on).await;
            }
            while let Some(change) = changes.next().await {
                if let Ok(on) = change.get().await {
                    let _ = tx.send(on).await;
                }
            }
            futures::future::pending().await
        })
    })
}
