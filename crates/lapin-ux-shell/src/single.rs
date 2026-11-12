// SPDX-License-Identifier: GPL-3.0-only
//! Only the session starts the shell (#83).
//!
//! Every keyboard shortcut runs `lapin-ux-shell <subcommand>`, which hands the
//! request to the running shell over D-Bus and exits. When no shell had
//! registered yet (a slow sign-in, #82) the subcommand used to become a shell
//! itself, so two shortcuts pressed while waiting gave three taskbars.
//!
//! - A subcommand waits a few seconds for the shell to register, then gives
//!   up instead of starting one.
//! - A shell started without a subcommand takes a lock in the runtime
//!   directory, so a second one can never run next to it.

use std::time::{Duration, Instant};

/// How long a shortcut waits for a shell that is still starting.
const WAIT_FOR_SHELL: Duration = Duration::from_secs(5);
/// How long a new shell waits for the previous one to let go of the lock
/// (the session restarts a crashed shell while the old process is exiting).
const WAIT_FOR_LOCK: Duration = Duration::from_secs(3);

/// Whether a shell has registered its D-Bus name.
fn shell_is_running() -> bool {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return false };
    rt.block_on(async {
        let Ok(conn) = zbus::Connection::session().await else { return false };
        let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await else { return false };
        let Ok(name) = zbus::names::BusName::try_from(lapin_ux_core::ids::SHELL_APP_ID) else { return false };
        proxy.name_has_owner(name).await.unwrap_or(false)
    })
}

/// A subcommand (`start`, `alttab`, `snip`, ...): true when a shell is there
/// to receive it, waiting briefly for one that is still starting.
pub fn shell_ready_for_command(action: &str) -> bool {
    let started = Instant::now();
    loop {
        if shell_is_running() {
            return true;
        }
        if started.elapsed() >= WAIT_FOR_SHELL {
            tracing::error!(action, "no shell is running, so the request is dropped (it would otherwise start a second shell)");
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The shell itself: true when this is the only one. The lock lives as long
/// as the process.
pub fn take_shell_lock() -> bool {
    use std::os::fd::AsRawFd;
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let path = dir.join("lapin-ux-shell.lock");
    let file = match std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            // No lock is better than no shell.
            tracing::warn!(?path, "can't open the shell lock file: {e}");
            return true;
        }
    };
    let started = Instant::now();
    loop {
        // SAFETY: flock on a file descriptor this function owns.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if locked {
            // Kept for the life of the process.
            std::mem::forget(file);
            return true;
        }
        if started.elapsed() >= WAIT_FOR_LOCK {
            tracing::error!(?path, "another lapin-ux-shell is already running, so this one exits");
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
