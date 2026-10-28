# Architecture

Lapin UX is its own shell on top of cosmic-comp. Compositor changes stay in small patches with our own protocol; COSMIC programs are forked as patch files, never rewritten.

## Lapin crates
| Part | Role |
|---|---|
| lapin-ux-core | UI-free logic with 99 unit tests (search, taskbar model, notifications, quick settings, files, desktops filter, clipboard, pins, calendars, processes, snap layouts, displays, night light, updates incl. fwupd JSON, sounds, users/services, crash reporting) |
| lapin-ux-ui | Shared Windows-style widget styles (4 px push buttons, round slider thumbs, nav items, title bar) |
| lapin-ux-shell | Taskbar, Start, tray, quick settings, desktop icons, clipboard/emoji, Win+X, Run, Alt+Tab, Task view, snapping, snipping, Shut Down dialog, first-run defaults |
| lapin-ux-notifyd | Notification server, toasts, centre, agenda, Focus |
| lapin-ux-session | Starts compositor, supervises components, sign-in sound, exports `FONTCONFIG_FILE` |
| lapin-ux-taskmgr / -control / -update / -welcome | Task Manager, Control Panel, Update, Get Started |

## Forks (patches against pinned COSMIC epoch-1.11.0 commits)
| Package | Upstream pin | What the patches do |
|---|---|---|
| lapin-ux-comp (cosmic-comp 8cb1802) | 5 patches | `lapin_shell` protocol v1-v3 (snap, attention, peek, night light, gestures, open animation, Alt reporting), title bars, night light on the KMS path |
| lapin-ux-greeter (cosmic-greeter 80e8a10) | 0001-0007 | Windows 11 lock and sign-in faces, session gear, `/etc/pam.d/cosmic-greeter` |
| lapin-ux-osk (cosmic-osk 6b146a2) | 1 | Touch keyboard look |
| lapin-ux-osd (cosmic-osd 4f96e98) | 1 | Volume/brightness pop-ups, admin prompt (own code in `src/lapin.rs`) |
| lapin-ux-settings (cosmic-settings f1d00fc) | 0001-0013 | Settings app (code in `cosmic-settings/src/lapin/`) |
| lapin-ux-portal (xdg-desktop-portal-cosmic f448ddf + cosmic-files 3cc4b80) | patch | Windows-style Open/Save dialog |
| libcosmic 430f3d4 | 0001-0008 | Title bars, opt-in card lists, row icons, round slider thumbs, 4 px buttons |

Pinned libcosmic for COSMIC apps is 430f3d4 (comp uses d922ac3). A single split PKGBUILD (`packaging/arch`) builds lapin-ux-desktop-git, -comp, -greeter, -osk, -osd, -settings, -portal; `pkgver()` comes from git. Patches live in `packaging/lapin-ux-*/`.

## Reused COSMIC services
settings-daemon, bg, idle, and COSMIC Files (until File Explorer exists).

## Fork principles
- Lapin code in its own module with a handful of call sites.
- Every fork is gated on a Lapin session (`XDG_CURRENT_DESKTOP` contains `LapinUX`); COSMIC's original path remains in the else branch.
- Build with upstream Cargo.lock (`--locked`/`--frozen`); pin to COSMIC epoch tags so updates are LTS-style.
- Force switches for previews: `LAPIN_OSD_PREVIEW`, `LAPIN_UX_OSK=1`, `LAPIN_UX_DIALOG=1`, `LAPIN_UX_SETTINGS=1`, `LAPIN_UX_TITLEBAR=1`.

## Key mechanisms
- **Per-session settings swap**: `lapin-ux-desktop-settings enter|leave` gives Lapin and other desktops each their own copy of shared settings.
- **Own protocol**: `lapin_shell` lets the shell talk to the compositor (snap, peek, night light, gestures, Alt state).
- **Crash reporting**: `lapin_ux_core::crash` writes backtraces and the shell's last messages to `~/.local/state/lapin-ux/crash-<program>.txt`.
- **Desktops filter**: shared `lapin_ux_core::desktops` rule for Start's app list and per-session autostart masks.
