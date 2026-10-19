// SPDX-License-Identifier: GPL-3.0-only
//! Client side of lapin_shell_v1, offered by Lapin UX's build of cosmic-comp
//! (`lapin-ux-comp`): snapping windows into layouts, and attention requests.

#![allow(non_upper_case_globals, non_camel_case_types, clippy::all)]

pub use generated::lapin_shell_manager_v1;

mod generated {
    use wayland_client;

    pub mod __interfaces {
        wayland_scanner::generate_interfaces!("../../protocols/lapin-shell-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("../../protocols/lapin-shell-v1.xml");
}
