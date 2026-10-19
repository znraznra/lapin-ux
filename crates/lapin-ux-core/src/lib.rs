// SPDX-License-Identifier: GPL-3.0-only
//! Logic shared by the Lapin UX shell components that does not need a GUI.
//!
//! Everything here is plain Rust so it can be unit-tested without a Wayland
//! session: app search ranking, the "All apps" alphabet grouping, the taskbar
//! button model (pinned + running windows), the notification toast queue and
//! clock formatting.

pub mod apps;
pub mod badge;
pub mod clip;
pub mod clock;
pub mod desktop;
pub mod display;
pub mod dnd;
pub mod files;
pub mod ics;
pub mod notify;
pub mod pins;
pub mod procs;
pub mod nightlight;
pub mod props;
pub mod quick;
pub mod recent;
pub mod search;
pub mod snap;
pub mod sound;
pub mod sysmon;
pub mod system;
pub mod taskbar;
pub mod thumb;
pub mod updates;

/// Reverse-DNS identifiers. Change these in one place when the project gets
/// its final name / domain.
pub mod ids {
    pub const SHELL_APP_ID: &str = "dev.lapinux.Shell";
    pub const NOTIFYD_APP_ID: &str = "dev.lapinux.Notifications";
    pub const DESKTOP_NAME: &str = "LapinUX";
}
