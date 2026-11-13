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
mod anim;
mod app;
mod autohide;
mod catalog;
mod catcher;
mod clipboard;
mod comp_extras;
mod config;
mod defaults;
mod desktop;
mod dnd;
mod icons;
mod input;
mod lapin_proto;
mod launcher_entry;
mod notify_client;
mod open_with;
mod panels;
mod previews;
mod properties;
mod quick;
mod pointer_gate;
mod redraw_debug;
mod screen_reader;
mod single;
mod snap;
mod snip;
mod start_pins;
mod start_search;
mod style;
mod submenu;
mod start_view;
mod taskbar_extras;
mod taskbar_fit;
mod taskbar_view;
mod system_ui;
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

pub(crate) fn in_lapin_ux_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|d| d.split(':').any(|n| n == lapin_ux_core::ids::DESKTOP_NAME))
}

fn main() -> cosmic::iced::Result {
    lapin_ux_core::crash::install("lapin-ux-shell");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    // Drag and drop diagnostics (#81): `touch
                    // ~/.local/state/lapin-ux/debug-dnd` (or LAPIN_UX_DEBUG_DND=1)
                    // before signing in logs every drag event the shell gets.
                    let dnd = std::env::var("LAPIN_UX_DEBUG_DND").is_ok_and(|v| v == "1")
                        || std::env::var_os("HOME")
                            .map(|h| std::path::PathBuf::from(h).join(".local/state/lapin-ux/debug-dnd"))
                            .is_some_and(|p| p.exists());
                    if dnd {
                        "warn,lapin_ux_shell=info,libcosmic::widget::dnd_destination=trace,iced::winit::program=trace".into()
                    } else {
                        "warn,lapin_ux_shell=info".into()
                    }
                }),
        )
        .init();
    // Redraw only the surfaces that move during an animation (#51);
    // LAPIN_UX_REDRAW_ALL=1 turns that off, for comparing.
    if std::env::var("LAPIN_UX_REDRAW_ALL").is_err() {
        anim::install_redraw_filter();
    }

    let action = std::env::args().nth(1);
    let mut args: Vec<String> = std::env::args().skip(2).collect();
    // Alt+Tab is slow (#65): say when the key press started this process, so
    // the running shell can log how long it took to get there.
    if action.as_deref() == Some("alttab")
        && let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
    {
        args.push(format!("--sent-ms={}", now.as_millis()));
    }
    if matches!(action.as_deref(), Some("-h" | "--help")) {
        println!("usage: lapin-ux-shell [start | desktop | winx | run | quick | notifications | clipboard | emoji | alttab [prev] | taskview | snap | taskbar-item N | power-button]");
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

    // Only the session starts the shell (#83): a shortcut's subcommand
    // never turns into a second one.
    match action.as_deref() {
        Some(a) => {
            if !single::shell_ready_for_command(a) {
                return Ok(());
            }
        }
        None => {
            if !single::take_shell_lock() {
                return Ok(());
            }
        }
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
