# Lapin UX

A COSMIC-based DE written in Rust. Bringing the ease of use and familiarity
of Windows UX without the need of plugins and console dependency for everything.

A Linux desktop that feels familiar to anyone coming from Windows 11: a
centered taskbar, a Start menu with search, pinned apps and recent files,
toasts in the bottom-right corner, a system tray, and the keyboard shortcuts
people already know. Written in Rust with [libcosmic]/iced, running on the
COSMIC compositor. Arch Linux first.

> Lapin UX matches *behaviour, layout and feel*. It contains no Microsoft
> artwork, fonts, names or logos; the Start mark, wallpapers and icons in
> `data/` are original.

| | |
|---|---|
| ![Start menu](docs/screenshots/start-menu.png) | ![Search](docs/screenshots/start-search.png) |
| ![Jump list](docs/screenshots/taskbar-jumplist.png) | ![Window picker](docs/screenshots/taskbar-window-picker.png) |
| ![Calendar](docs/screenshots/calendar.png) | ![Tray menu](docs/screenshots/tray-menu.png) |

![Notifications](docs/screenshots/notifications.png)

## What's in the box (phase 1)

**Taskbar** (`lapin-ux-shell`), one per monitor
- Centered (or left-aligned) Start button, Search box, pinned and running apps.
- All windows of an app share a button. Click: open / focus / minimize;
  several windows: a window picker. Middle-click: new instance.
- Accent "pill" under the focused app, short dash for background apps.
- Right-click: the app's own jump-list actions (e.g. *New Private Window*),
  pin/unpin, close window(s).
- System tray (StatusNotifierItem): left-click activates, right-click opens
  the app's menu with checkmarks, disabled items and submenus. Discord,
  Steam, Nextcloud, etc. show up here.
- Clock with locale-aware time/date; click for a calendar.
- "Show desktop" sliver at the far right (click again to restore).

**Start menu**
- Type immediately to search. Ranking favours exact and prefix matches,
  initials (`vsc` → Visual Studio Code), generic names (`browser`), keywords
  and executables (`nautilus` → Files). Arrow keys + Enter work; Esc steps back.
- Pinned grid (6 × 3), *All apps* with A–Z sections, *Recommended* from recently
  opened files (`recently-used.xbel`) or recently launched apps.
- Search also finds **Settings pages** (Wi-Fi, Display, Dark mode…),
  **files and folders** in your Desktop, Documents, Downloads, Music, Pictures
  and Videos (own background index, hidden folders skipped), and offers
  **Search the web** (DuckDuckGo by default; `web_search` setting, `{}` = query,
  `hide_web_search` to turn it off). Results are grouped Best match / Apps /
  Settings / Folders / Documents / Web with a details card; files have
  *Open file location*.
- Right-click any app: pin/unpin to Start or taskbar, **Uninstall** (asks
  first; pacman packages through an admin prompt, Flatpaks directly; core
  desktop packages are refused). Newly installed or removed apps appear the
  next time Start opens.
- Account name/avatar, and a power menu: Lock, Sign out, Sleep, Restart, Shut down.

**Notifications** (`lapin-ux-notifyd`)
- Full `org.freedesktop.Notifications` server: actions, urgency, replace-id,
  transient, image-path, markup stripped to plain text.
- Up to 3 toasts stacked above the taskbar, 5 s default, critical ones stay,
  hover pauses the timer. Raw `image-data` pictures (chat avatars, photos) shown.
- **Notification centre** (click the clock or Super+N): notifications grouped
  by app with time, per-item and per-app dismiss, Clear all, Do not disturb,
  and a collapsible calendar underneath. Clicking a notification opens its app.
- The clock shows a bell with the unread count, or a muted bell during
  Do not disturb. `lapin-ux-notifyd center` toggles the centre from scripts;
  `dev.lapinux.Notifications` on the session bus exposes `Unread`,
  `DoNotDisturb`, `ToggleCenter` and `ClearAll`.

**Desktop icons** (the contents of ~/Desktop, on the main monitor)
- Click selects, double-click / Enter opens, drag moves an icon to another
  spot (remembered), F2 renames in place, Delete moves to the Trash
  (restorable from the file manager), picture files show thumbnails.
- Right-click an icon: Open, Copy as path, Rename, Delete. App launchers
  (`.desktop`) show the app's name and icon; one that isn't an installed app
  only runs after *Allow launching* (it could have come from a download).
- Right-click the desktop: View (Large/Medium/Small icons, Show desktop
  icons), Sort by (Name, Date modified, Item type), Refresh, New (Folder,
  Text Document), Display settings, Personalize, Open in Terminal.

**Clipboard history and emoji** (Super+V, Super+.)
- One panel with tabs: Emoji (search by name, categories, recently used),
  Kaomoji, Symbols, Clipboard.
- Clipboard history keeps the last 25 texts and pictures you copied
  (watched with `ext-data-control-v1`); pin items to keep them (pinned
  texts survive a restart), delete them, or Clear all. Copies marked as
  secrets by password managers are never recorded.
- Picking a history item puts it back on the clipboard and pastes it into
  the app you were using; picking an emoji or symbol types it there
  (virtual keyboard), without touching your clipboard.

**Quick settings** (network / volume / battery button left of the clock, or Super+A)
- Tiles: Wi-Fi, Bluetooth, Airplane mode, Energy saver, Do not disturb. Each
  appears only when its service exists (NetworkManager, BlueZ,
  power-profiles-daemon, the notification daemon).
- Wi-Fi page: nearby networks, connect to saved, open and password-protected
  networks, disconnect. Bluetooth page: connect/disconnect paired devices.
- Brightness slider (laptops; through logind, no root needed), volume slider
  with mute, sound output picker. Scroll over the taskbar button to change volume.
- Battery percentage and time remaining (UPower); the settings gear opens
  COSMIC Settings, and each page links to its settings page.
- Pairing new Bluetooth devices and advanced network setup (VPN, static IP)
  are in COSMIC Settings for now.

**Session** (`lapin-ux-session`, `start-lapin-ux`)
- Starts cosmic-comp in normal (not kiosk) mode using the same handshake as
  cosmic-session, exports the display to systemd/D-Bus, then starts and
  supervises every component with exponential back-off restarts.
- `dev.lapinux.Session.Exit` on the session bus for Sign out.
- XDG autostart ("startup apps") through `lapin-ux-session.target`.

**First-run defaults** (written once; your later changes are never overwritten)
- Theme: blue accent, 4 px control / 8 px window corners, frosted shell surfaces.
- Font: Inter for the whole interface (if you haven't already picked a font).
- Wallpaper: original light/dark artwork.
- Keyboard: Super opens Start, plus

| Keys | Action |
|---|---|
| Super | Start menu |
| Super + E | File manager |
| Super + D | Show desktop / restore |
| Super + X | Power-user menu |
| Super + R | Run dialog |
| Super + N | Notification centre |
| Super + A | Quick settings |
| Super + V | Clipboard history |
| Super + . | Emoji, kaomoji and symbols |
| Super + 1…9 | Open / switch to the n-th taskbar app |
| Super + L | Lock |
| Super + ↑ / ↓ | Maximize / minimize |
| Super + Shift + S | Screenshot |
| Alt + Tab | Switch windows |
| Ctrl + Alt + Del | Sign-out / power dialog |

## Architecture

```
display manager ──> start-lapin-ux ──> lapin-ux-session
                                      ├─ cosmic-comp            compositor (reused, unmodified)
                                      ├─ lapin-ux-shell           taskbar ×N, Start menu, tray   ← ours
                                      ├─ lapin-ux-notifyd         notification toasts + centre   ← ours
                                      ├─ cosmic-settings-daemon theme, media keys              (reused)
                                      ├─ cosmic-bg              wallpaper                      (reused)
                                      ├─ cosmic-osd             volume OSD, polkit dialogs     (reused)
                                      ├─ cosmic-idle            screen blanking                (reused)
                                      └─ cosmic-greeter         lock screen                    (reused)
```

| Crate | What it is | Depends on libcosmic |
|---|---|---|
| `lapin-ux-core` | Search ranking, taskbar model, toast queue, clock formats, recent files. 36 unit tests. | no |
| `lapin-ux-shell` | Taskbar, Start menu, tray, popups, first-run defaults | yes |
| `lapin-ux-notifyd` | Notification server and toasts | yes |
| `lapin-ux-session` | Compositor handshake and process supervisor. 2 tests. | no |

Window tracking supports two backends: cosmic-comp's protocols (full
control) and `wlr-foreign-toplevel-management` (sway, Hyprland, labwc,
niri…), so the shell also runs on wlroots compositors.

All settings use cosmic-config and apply live:
`~/.config/cosmic/dev.lapinux.Shell/v1/` (`pinned_taskbar`, `pinned_start`,
`alignment` = `Center`/`Left`, `show_search`, `clock_24h`, `clock_seconds`,
`show_desktop_corner`, `tray_inline`, `web_search`, `hide_web_search`, `desktop_icon_size`, `desktop_sort`, `hide_desktop_icons`).

## Install on Arch

```sh
sudo pacman -S --needed base-devel rust python-numpy python-pillow \
    cosmic-comp cosmic-settings-daemon cosmic-bg cosmic-osd cosmic-idle cosmic-greeter \
    cosmic-launcher cosmic-screenshot cosmic-icon-theme xdg-desktop-portal-cosmic inter-font
cd packaging/arch
makepkg -si
```

Log out, pick **Lapin UX** in your display manager's session list, log in.
Uninstall with `sudo pacman -R lapin-ux-desktop-git`.

## Try it without installing

From a terminal inside any Wayland session (COSMIC, GNOME, KDE, sway…) with
`cosmic-comp` installed:

```sh
cargo build
XDG_CONFIG_HOME="$(mktemp -d)" PATH="$PWD/target/debug:$PATH" XDG_CURRENT_DESKTOP=LapinUX:COSMIC \
  dbus-run-session -- target/debug/lapin-ux-session
```

cosmic-comp detects the parent session and opens as a window containing a
full Lapin UX desktop. Close the window (or `pkill lapin-ux-session`) to stop.

See [TESTING.md](TESTING.md) for a step-by-step test checklist.

## Develop

```sh
cargo test --workspace     # unit tests (core logic, session handshake)
cargo clippy --workspace   # clean at the time of writing
RUST_LOG=lapin_ux_shell=debug target/debug/lapin-ux-shell   # run the shell alone in a session
target/debug/lapin-ux-shell start                          # toggle Start in the running shell
python3 tools/make-wallpaper.py                          # re-render wallpapers
```

`lapin-ux-core` holds anything that can be tested without a display; put new
logic there first.

## Renaming

The working name is easy to change: `crates/lapin-ux-core/src/lib.rs` (`ids`),
`data/session/*`, `data/systemd/*`, `packaging/arch/PKGBUILD`.

## Not done yet (next phases)

- **Settings app** (display, network, Bluetooth, sound, power, users, default
  apps); COSMIC Settings fills in until then.
- Own **Alt+Tab** switcher and **Task view**; until then Alt+Tab uses cosmic-launcher.
- **Snap** halves/quarters on Super+←/→ and snap layouts (needs compositor work).
- Hover **thumbnails** on taskbar buttons, badges/progress.
- File manager, update center (pacman + Flatpak + fwupd), control panel.

## License

GPL-3.0-only. Window tracking, the tray watcher/menus and the session start
script are adapted from System76's GPL-3.0 COSMIC projects; see file headers.

[libcosmic]: https://github.com/pop-os/libcosmic
