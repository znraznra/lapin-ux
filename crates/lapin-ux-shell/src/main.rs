// SPDX-License-Identifier: GPL-3.0-only
//! Lapin UX shell: the taskbar (one per monitor) and the Start menu.
//!
//! Usage:
//!   lapin-ux-shell          run the shell (only one instance runs)
//!   lapin-ux-shell start    toggle the Start menu in the running instance
//!                         (bound to the Super key by the session)
//!   lapin-ux-shell desktop  show the desktop / restore windows (Super+D)
//!   lapin-ux-shell winx     power-user menu (Super+X)
//!   lapin-ux-shell quick    quick settings (Super+A)
//!   lapin-ux-shell notifications  notification centre (Super+N)
//!   lapin-ux-shell run      Run dialog (Super+R)
//!   lapin-ux-shell taskbar-item N   open/switch to the N-th taskbar app (Super+N)

mod account;
mod app;
mod autohide;
mod catalog;
mod catcher;
mod clipboard;
mod config;
mod defaults;
mod desktop;
mod dnd;
mod icons;
mod lapin_proto;
mod launcher_entry;
mod notify_client;
mod panels;
mod previews;
mod quick;
mod snap;
mod start_pins;
mod start_search;
mod style;
mod submenu;
mod start_view;
mod taskbar_extras;
mod taskbar_view;
mod taskview;
mod tray;
mod wayland;

use cosmic::app::{CosmicFlags, Settings};

#[derive(Debug, Clone, Default)]
pub struct Args {
    pub action: Option<String>,
    pub args: Vec<String>,
}

impl CosmicFlags for Args {
    type SubCommand = String;
    type Args = Vec<String>;

    fn action(&self) -> Option<&String> {
        self.action.as_ref()
    }

    fn args(&self) -> Vec<&str> {
        self.args.iter().map(String::as_str).collect()
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
    let args: Vec<String> = std::env::args().skip(2).collect();
    if matches!(action.as_deref(), Some("-h" | "--help")) {
        println!("usage: lapin-ux-shell [start | desktop | winx | run | quick | notifications | clipboard | emoji | alttab [prev] | taskview | snap | taskbar-item N]");
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
        Args { action, args },
    )
}
