// SPDX-License-Identifier: GPL-3.0-only
//! Lapin UX shell: the taskbar (one per monitor) and the Start menu.
//!
//! Usage:
//!   lapin-ux-shell          run the shell (only one instance runs)
//!   lapin-ux-shell start    toggle the Start menu in the running instance
//!                         (bound to the Super key by the session)
//!   lapin-ux-shell desktop  show the desktop / restore windows (Super+D)

mod app;
mod catalog;
mod config;
mod defaults;
mod style;
mod start_view;
mod taskbar_view;
mod tray;
mod wayland;

use cosmic::app::{CosmicFlags, Settings};

#[derive(Debug, Clone, Default)]
pub struct Args {
    pub action: Option<String>,
}

impl CosmicFlags for Args {
    type SubCommand = String;
    type Args = Vec<String>;

    fn action(&self) -> Option<&String> {
        self.action.as_ref()
    }
}

fn in_lapin_ux_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|d| d.split(':').any(|n| n == lapin_ux_core::ids::DESKTOP_NAME))
}

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,lapin_ux_shell=info".into()),
        )
        .init();

    let action = std::env::args().nth(1);
    if matches!(action.as_deref(), Some("-h" | "--help")) {
        println!("usage: lapin-ux-shell [start|desktop]");
        return Ok(());
    }

    // The Super-key binding is shared with COSMIC's config. Outside an
    // Lapin UX session, hand the key to COSMIC's own launcher.
    if action.as_deref() == Some("start") && !in_lapin_ux_session() {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new("cosmic-launcher").exec();
        eprintln!("lapin-ux-shell: not in an Lapin UX session and cosmic-launcher failed: {err}");
        return Ok(());
    }

    cosmic::app::run_single_instance::<app::Shell>(
        Settings::default()
            .antialiasing(true)
            .client_decorations(true)
            .default_text_size(14.0)
            .no_main_window(true)
            .exit_on_close(false),
        Args { action },
    )
}
