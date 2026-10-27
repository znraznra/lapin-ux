# Lapin UX — technical reference

> The friendly overview, roadmap and screenshots are in the [README](../README.md). This page is the full technical detail: every feature and setting, architecture, building, testing and credits.

A COSMIC-based DE written in Rust. Bringing the ease of use and familiarity
of Windows UX without the need of plugins and console dependency for everything.

A Linux desktop that feels familiar to anyone coming from Windows 11: a
centered taskbar, a Start menu with search, pinned apps and recent files,
toasts in the bottom-right corner, a system tray, and the keyboard shortcuts
people already know. Written in Rust with [libcosmic]/iced, running on the
COSMIC compositor. Arch Linux first.

> Lapin UX matches *behaviour, layout and feel*. Everything it ships is open
> source: its own code is GPL-3.0, and the third-party font and icons it uses
> by default (Selawik, Fluent UI System Icons, the Win11 icon theme) are used
> under their own open-source licences, with all trademarks belonging to their
> owners. Lapin UX is an independent project, not affiliated with or endorsed
> by Microsoft. See [Third-party assets and trademarks](#third-party-assets-and-trademarks).

| | |
|---|---|
| ![Start menu](screenshots/start-menu.png) | ![Search](screenshots/start-search.png) |
| ![Jump list](screenshots/taskbar-jumplist.png) | ![Window picker](screenshots/taskbar-window-picker.png) |
| ![Calendar](screenshots/calendar.png) | ![Tray menu](screenshots/tray-menu.png) |

![Notifications](screenshots/notifications.png)

## What's in the box (phase 1)

**Taskbar** (`lapin-ux-shell`), one per monitor
- Centered (or left-aligned) Start button, Search box, pinned and running apps.
- The Search box is a real text field: click it to type on the taskbar (the
  taskbar takes the keyboard only meanwhile); Enter shows the results in
  Start; Esc or clicking away clears it.
- Start button picture: Settings > Personalization > Taskbar > Start button
  icon (Lapin UX's, Dots, Circle, Rabbit, the OS logo from os-release
  `LOGO=`, or a custom SVG/PNG copied to `~/.local/share/lapin-ux`);
  `start_icon` in `dev.lapinux.Shell`.
- All windows of an app share a button. Click: open / focus / minimize;
  several windows: a window picker. Middle-click: new instance.
- Accent "pill" under the focused app, short dash for background apps.
- Right-click: recent files that app opened (jump list), the app's own
  actions (e.g. *New Private Window*), pin/unpin, **End task**, close window(s).
- Count badges and progress bars from apps (Unity LauncherEntry: Firefox,
  Thunderbird, Telegram, Discord, Steam…). A window that wants attention
  makes its button flash and stay lit (with `lapin-ux-comp`).
- Drag a file onto a running app's button to bring that app forward.
- **Task view** button (Super+Tab). Several monitors: *Show taskbar apps on*
  (all taskbars / main and where open / where open). Only the current
  desktop's windows are shown.
- System tray (StatusNotifierItem): left-click activates, right-click opens
  the app's menu with checkmarks, disabled items and submenus. Discord,
  Steam, Nextcloud, etc. show up here.
- Clock with locale-aware date (time only, no seconds, on the slim taskbar);
  12 or 24 hours as Settings' Time & language >
  Date & time > 24-hour time says (COSMIC's `military_time`, read only); click
  for a calendar.
- "Show desktop" sliver at the far right (click again to restore).

**Start menu**
- Type immediately to search. Ranking favours exact and prefix matches,
  initials (`vsc` → Visual Studio Code), generic names (`browser`), keywords
  and executables (`nautilus` → Files). Arrow keys + Enter work; Esc steps back.
- Pinned grid (6 × 3): drag to reorder, drop one app on another to make a
  **folder** (rename it, move apps out); pin files, folders and websites (drop
  them on the grid, or *Pin to Start* on a file result). *All apps* with A–Z sections, *Recommended* from recently
  opened files (`recently-used.xbel`) or recently launched apps.
- Search also finds **Settings pages** (Wi-Fi, Display, Dark mode…),
  **files and folders** in your Desktop, Documents, Downloads, Music, Pictures
  and Videos (own background index, hidden folders skipped), and offers
  **Search the web** (DuckDuckGo by default; `web_search` setting, `{}` = query,
  `hide_web_search` to turn it off). Results are grouped Best match / Apps /
  Settings / Folders / Documents / Web with a details card. For files the
  card is a preview pane: the first lines of a text file, the picture itself,
  or a folder's item count, then Open, *Open file location*, Copy path and
  Pin to Start, then size, date modified and location. With LocalSearch installed, **document contents** are
  searched too.
- Right-click any app: pin/unpin to Start or taskbar, **Uninstall** (asks
  first; pacman packages through an admin prompt, Flatpaks directly; core
  desktop packages are refused). Newly installed or removed apps appear the
  next time Start opens.
- Account button: Change account settings, **Change account picture**, Lock,
  Sign out, **Switch user** (LightDM, SDDM, GDM). Power menu: Lock, Sign out,
  Sleep, Restart, Shut down.

**Notifications** (`lapin-ux-notifyd`)
- Full `org.freedesktop.Notifications` server: actions, urgency, replace-id,
  transient, image-path, markup stripped to plain text.
- Up to 3 toasts stacked above the taskbar, 5 s default, critical ones stay,
  hover pauses the timer. Raw `image-data` pictures (chat avatars, photos) shown.
- **Notification centre** (click the clock or Super+N): notifications grouped
  by app with time, per-item and per-app dismiss, Clear all, Do not disturb,
  and a collapsible calendar underneath with **today's agenda** (local
  Evolution calendars, .ics files, calendar links; repeating events) and
  **Focus sessions**. Clicking a notification opens its app.
- Notification settings (gear): Do not disturb on a **schedule**, and per-app
  *Notifications* / *Show banners* switches.
- The clock shows a bell with the unread count, or a muted bell during
  Do not disturb. `lapin-ux-notifyd center` toggles the centre from scripts;
  `dev.lapinux.Notifications` on the session bus exposes `Unread`,
  `DoNotDisturb`, `ToggleCenter` and `ClearAll`.

**Desktop icons** (the contents of ~/Desktop, on the main monitor)
- **Recycle Bin** icon (Open, Empty Recycle Bin); drop files on it to delete.
- Click selects; Ctrl-click, Shift-click and dragging a box select several.
  Double-click / Enter opens, drag moves icons (remembered), F2 renames in
  place, Delete moves to the Trash, picture files show thumbnails.
- **Cut / Copy / Paste** (Ctrl+X/C/V, works with file managers), Ctrl+A.
- **Drag and drop**: icons drag into apps as files; files and links from
  apps drop onto the desktop (links become shortcuts) or onto folders.
- Right-click an icon: Open, Cut, Copy, Copy as path, Rename, Delete. App launchers
  (`.desktop`) show the app's name and icon; one that isn't an installed app
  only runs after *Allow launching* (it could have come from a download).
- Right-click the desktop: View (Large/Medium/Small icons, Show desktop
  icons), Sort by (Name, Date modified, Item type), Refresh, New (Folder,
  Text Document), Display settings, Personalize, Open in Terminal.

**Auto-hide** (right-click the taskbar → Automatically hide the taskbar):
the taskbar disappears and windows use the whole screen; touching the bottom
edge brings it back, and it stays while Start, a menu or a panel is open.

**Window previews**
- Hovering a taskbar button shows a live thumbnail of each of that app's
  windows (captured with `ext-image-copy-capture`); click to switch, × to close.
- Own Alt+Tab switcher: thumbnails in most-recently-used order; hold Alt and
  press Tab to move, release to switch; a quick tap flips between the last two
  windows; Alt+Shift+Tab goes back; Esc cancels.

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
- **Media controls** for what's playing (MPRIS).
- Tiles: Wi-Fi, Bluetooth, Airplane mode, Energy saver, Accessibility
  (magnifier, colour filters, screen reader), Mobile hotspot, Nearby sharing,
  Cast, Keyboard layout, VPN, Do not disturb. Each appears only when what it
  needs exists.
- Wi-Fi page: nearby networks, connect to saved, open and password-protected
  networks, disconnect. Bluetooth page: connect/disconnect, and **Add device**
  (pairing with PIN prompts; the shell is the Bluetooth agent).
- Brightness slider (laptops; through logind, no root needed), volume slider
  with mute, sound output picker and **per-app volume mixer**. Scroll over the taskbar button to change volume.
- Battery percentage and time remaining (UPower); the settings gear opens
  COSMIC Settings, and each page links to its settings page.
- **Night light** tile (warm screen filter in `lapin-ux-comp`; optional
  on/off schedule in the `night_light_schedule` setting) and **Rotation lock**
  (tablets and convertibles: the screen follows the accelerometer through
  iio-sensor-proxy unless locked).
- **Edit quick settings** (pencil): unpin tiles, drag them into a new order,
  add them back from the Add list.
- Notifications show **progress bars** and an **inline reply** box (KDE's
  inline-reply extension).

**Task view and snapping**
- Task view: windows of the current desktop as thumbnails, desktops below
  (hover to preview, click to switch, × to remove), New desktop, right-click a
  window to move it to another desktop.
- **Snap layouts** (Super+Z): halves, 2/3 + 1/3, thirds, half + quarters,
  quarters, 1/4-1/2-1/4 on wide screens; then **Snap assist** fills the other
  spaces from your windows. Uses `lapin-ux-comp`, which the installer builds.

- **Snap groups**: windows placed together with a snap layout show up as a
  Group card when you hover their taskbar button; clicking it brings them all back.
- Task view: right-click a desktop to **Rename** it or **Choose background**
  (each desktop can have its own wallpaper).
- **Desktop peek**: rest the pointer on the far right of the taskbar and the
  windows disappear until you move away (`lapin-ux-comp`).
- **Touchpad**: three fingers up for Task view, down to show the desktop,
  sideways to switch apps; four fingers sideways to switch desktops.
- New windows fade in as they open; minimize and restore animate to the taskbar.

**Settings** (`lapin-ux-settings`, COSMIC Settings at epoch-1.11.0 with
patches in `packaging/lapin-ux-settings`; in progress, all 5 stages done)
- Windows 11 layout: account card and colour category icons in the sidebar
  (Home, System, Bluetooth & devices, Network & internet, Personalization,
  Apps, Accounts, Time & language, Accessibility, Privacy & security,
  Update), Find a setting centred in the title bar, back arrow, breadcrumb
  page titles, category pages of rows, Home, and the System page header
  (wallpaper, PC name and model, Rename, Update's last check).
- COSMIC's pages are re-parented under Windows' categories at start-up
  (with Windows' names), not edited; old links (`cosmic-settings system`…)
  open the matching category.
- Inside pages: every setting its own card with an icon chosen from its
  title, grey descriptions, On/Off beside switches, Windows-style drop-downs,
  round slider thumbs, 4 px buttons; Power mode and the primary mouse button
  as drop-downs; Add buttons inside rows; About laid out like Windows with
  Device and OS specification cards (Copy, links).
- Windows' page layout (`lapin/settings.rs`): Personalization > Taskbar and
  Start, System > Notifications, Display > Night light and System >
  Multitasking are Lapin pages that read and write the shell's
  `dev.lapinux.Shell` and the notification service's
  `dev.lapinux.Notifications` cosmic-config (both watch them, so changes
  apply at once; Do not disturb goes over the notification service's
  D-Bus property). Panel and Dock are hidden; COSMIC-only pages (Window
  management, Workspaces, X11 compatibility) sit under an "Advanced"
  heading. Notifications lists apps the service has seen (`seen_apps`).
- Pages COSMIC doesn't have (stage 4): Apps > Installed apps (`lapin/apps.rs`:
  launchers + `pacman -Qo`/`-Qi` and `flatpak list` for version, size, date;
  Uninstall via `pkexec pacman -Rs` or `flatpak uninstall`, desktop packages
  protected), System > Storage (`lapin/storage.rs`: `df` + `du` per
  category; Temporary files removes Recycle Bin, thumbnails, app caches,
  Downloads, and old packages with `pkexec paccache -rk0`), System >
  Clipboard (shell keys `no_clipboard_history`, action `clipboard-clear`),
  Personalization > Lock screen (`lock_wallpaper`, `no_signin_picture`, read
  by the greeter patch 0002; greeter patch 0003 draws the lock and sign-in
  screens in Selawik by name, since the sign-in screen's own user has no
  font setting; patch 0004 gives the sign-in screen Windows-style corner
  buttons — accessibility, keyboard layout, a gear to pick the desktop
  session (every installed Wayland/X11 session, so you can switch back to
  KDE, GNOME…) and power, each with a flyout), Accounts > Sign-in options (`lapin/signin.rs`: facial
  recognition with Howdy through `tools/lapin-ux-howdy` — camera choice,
  `howdy -U user -y add/clear`, and Howdy's PAM line added first to
  /etc/pam.d/greetd and cosmic-greeter as `sufficient`, so the password
  still works — run with pkexec under the dev.lapinux.howdy polkit action;
  fprintd enrolment/removal; password + hint through AccountsService),
  Privacy & security (`lapin/privacy.rs`: device encryption from lsblk;
  Recent activity; per-app Location/Camera/Microphone/Screenshot/Background
  switches in the portal permission store), and Update (`lapin/update.rs`,
  same checks/installs as lapin-ux-update, plus Update history from
  pacman.log). `lapin-ux-update` without arguments now opens
  `cosmic-settings lapin lapin-update` (`LAPIN_UX_UPDATE_WINDOW=1` keeps its
  own window); any Lapin page opens with `cosmic-settings lapin <page-id>`.
- Stage 5: "Find a setting" (`lapin/search.rs`) shows up to 8 suggestions
  in a list under the box as you type (page names and descriptions,
  COSMIC's section titles and multi-word lines, Lapin's own setting names)
  and a "Search results" page on Enter or "Show all results"; COSMIC's
  in-page search isn't used in Lapin mode. Everything that opens Settings
  goes through `lapin_ux_core::system::settings_command(page)`: Lapin pages
  (`lapin-…`) open with `cosmic-settings lapin <id>`, falling back to the
  nearest COSMIC page when COSMIC's own Settings is installed. Start search,
  Control Panel, Win+X (Installed apps, Power Options, System, Network
  Connections, Settings), the taskbar menu's "Taskbar settings", desktop
  Display settings / Personalize, quick settings links, the notification
  centre's "More notification settings" and Win+I (Settings Home) all open
  the matching page.
- All Lapin code is in `cosmic-settings/src/lapin/`; COSMIC's files get small
  hooks. The look inside pages comes from opt-in libcosmic patches
  (`packaging/lapin-ux-libcosmic` 0002–0007), turned on only by Lapin
  Settings. The layout switches on only in a Lapin UX session
  (`XDG_CURRENT_DESKTOP` contains `LapinUX`, or `LAPIN_UX_SETTINGS=1`).

**Pop-ups and admin prompt** (`lapin-ux-osd`, a patch on COSMIC's cosmic-osd)
- Volume and brightness keys show a small rounded box 12 px above the
  taskbar: icon, thin bar, number.
- When an app needs admin rights, polkit's request shows as Windows'
  "Do you want to allow this app to make changes to your device?": the screen
  dims, a card names the program, *Show more details* lists the action and
  command, and the password box sits above Yes / No.
- `LAPIN_OSD_PREVIEW=volume|brightness|admin` shows one of them for checking.

**Title bars of COSMIC's own apps** (optional, `packaging/arch-cosmic-apps`)
- A small libcosmic patch (`packaging/lapin-ux-libcosmic`) puts the app icon
  and title at the left of COSMIC apps' title bars. It only acts in a Lapin UX
  session (`XDG_CURRENT_DESKTOP` contains `LapinUX`, or `LAPIN_UX_TITLEBAR=1`),
  so the same apps look stock under COSMIC. The opt-in PKGBUILD rebuilds COSMIC
  Settings, Files, Terminal, Text Editor and Store at COSMIC's stable tag with
  the patch applied to the build's private libcosmic copy (lockfiles stay
  frozen). GTK, Electron and Qt/KDE apps keep their own title bars.

**Snipping** (Win+Shift+S or Print Screen)
- The screen freezes and dims; pick Rectangle (drag), Window or Full screen
  from the toolbar. The snip is copied to the clipboard, saved in
  Pictures/Screenshots, and a notification shows it.

**System tools**
- **Task Manager** (Ctrl+Shift+Esc, Ctrl+Alt+Del, Win+X): Processes (apps with
  their child processes, background processes, CPU/memory/disk, End task, Run
  new task), Performance graphs, Startup apps (turn them on or off), Users
  (who is signed in, from logind, with their CPU and memory; Manage user
  accounts), Details, and Services (every systemd service with PID and status;
  Start, Stop and Restart go through polkit and the admin prompt).
- **Control Panel**: the classic category view and Large icons, opening the
  matching Settings page or tool.
- **Update**: checks pacman, Flatpak and firmware (fwupd, via
  `fwupdmgr get-updates --json`) updates, installs them with one
  button (polkit), shows progress and history, and offers a restart when the
  kernel or firmware changed. A background check notifies you when updates are waiting.
- **Get Started**: on the first sign-in, connect to Wi-Fi, choose light or
  dark mode, and optionally get the Windows 11-style pointers.
- Desktop icons: **Properties** (Alt+Enter), *This PC*, user folder, *Network*
  and *Control Panel* icons, *Open with ›* with **Choose another app** ("How do
  you want to open this file?"), *Compress to ZIP file*, *Show more options*.
- Win+P (*Project*), Ctrl+Alt+Del security screen, Alt+F4 on the desktop opens
  *Shut Down Windows*.

**Lock screen and sign-in** (`lapin-ux-greeter`, a patch on COSMIC's greeter)
- Lock screen: big clock and date over the wallpaper; click or type to get the
  sign-in panel (round picture, name, password; network, battery,
  accessibility and power bottom right). The login screen adds the user list
  bottom left. Fingerprint sign-in works through fprintd (optional).
- The lock screen checks the password through the `cosmic-greeter` PAM
  service; `lapin-ux-greeter` ships `/etc/pam.d/cosmic-greeter` in Arch's
  style (`include system-local-login`; Arch has none, and without it every
  unlock fails). Account pictures are cut to a circle when loaded.

**Typing**
- With several keyboard layouts or fcitx5 input methods, the taskbar shows
  the current one (**ENG / US**, or あ, 拼…); click to pick, **Win+Space** for the next.
- **Touch keyboard** (`lapin-ux-osk`, a patch on COSMIC's on-screen keyboard)
  in Windows 11's style, with emoji and clipboard buttons; right-click the
  taskbar → *Show touch keyboard button*.

**Accessibility**
- Quick settings → Accessibility: Magnifier, Colour filters, **Narrator**
  (Orca; Ctrl+Win+Enter). Contrast themes are turned off for now (Lapin UX
  has none of its own yet; COSMIC's high-contrast palette didn't work as
  one): no quick setting, no shortcut, hidden in Settings and on the lock
  screen; an upgrade turns a high-contrast palette off again.

**Sounds and pointers**
- System sounds from the bundled **Borealis** theme: sign-in, sign-out,
  notifications (apps can ask for silence or their own sound), alarms, volume
  changes, emptying the Recycle Bin.
- Windows 11-style pointers by JepriCreations, installed on request
  (`lapin-ux-cursors`, offered by the installer and Get Started; see below).

**File dialogs** (`lapin-ux-portal`, COSMIC's portal built with a COSMIC Files patch)
- Every app's Open/Save dialog ends like Windows': *File name* with the file
  type beside it (*Save as type* when saving), then Open/Save and Cancel.

**Session** (`lapin-ux-session`, `start-lapin-ux`)
- Starts `lapin-ux-comp` if installed (else cosmic-comp) in normal (not kiosk) mode using the same handshake as
  cosmic-session, exports the display to systemd/D-Bus, then starts and
  supervises every component with exponential back-off restarts.
- `dev.lapinux.Session.Exit` on the session bus for Sign out.
- Crash reports: every Lapin program installs `lapin_ux_core::crash`, which on
  a panic writes the message, its last ~40 handled messages (shell and
  notification service) and a backtrace to the session log and to
  `~/.local/state/lapin-ux/crash-<program>.txt`. Release builds keep line
  tables (`debug = "line-tables-only"`, PKGBUILD `!strip`) so backtraces
  name files and lines; `start-lapin-ux` sets `RUST_BACKTRACE=1` and logs
  the installed build.
- XDG autostart ("startup apps") through `lapin-ux-session.target`. Other
  desktops' own parts in /etc/xdg/autostart (settings daemons, polkit
  agents, panels, session-manager plumbing; rule in
  `lapin_ux_core::desktops`) are masked with `systemctl --user mask
  --runtime` for the session only, listed in
  `$XDG_RUNTIME_DIR/lapin-ux-autostart-masked` and unmasked when it ends
  (after a crash, by `lapin-ux-desktop-settings leave`). Your own
  ~/.config/autostart entries are never touched.
- The power button: Lapin UX's shortcuts bind `XF86PowerOff` to
  `lapin-ux-shell power-button` (Sleep with a battery, Shut down without),
  and the shell holds a logind `handle-power-key` inhibitor while it runs.
- Start, search and Run list apps, not other desktops' own parts (same
  rule; Settings > Start > "Show apps from other desktops" shows them).

**First-run defaults** (written once; your later changes are never overwritten)
- Theme: blue accent, 4 px control / 8 px window corners, frosted shell surfaces.
- Font: Selawik is the one interface font *inside Lapin UX*. COSMIC and GTK
  apps are set to it at every login and put straight back if anything
  changes it, and a fontconfig rule (read only by the Lapin UX session, see
  "Other desktops" below) makes it the font for `sans-serif`, `serif`, `system-ui`
  and the interface fonts other desktops default to (Inter, Cantarell, Noto
  Sans, DejaVu Sans…). Monospace (terminals, code) keeps a fixed-width font;
  characters Selawik doesn't have (Japanese, Chinese, Korean, other scripts)
  come from the Noto fonts.
- Emoji: Microsoft's Fluent Emoji everywhere, as the bundled colour font
  *Lapin Fluent Emoji*. The emoji panel draws only with it (and lists only
  emoji Fluent Emoji has), and the fontconfig rule makes it the emoji font
  for every app, even ones that ask for Noto Color Emoji or Segoe UI Emoji.
- **Other desktops are left alone.** The font rule isn't enabled
  system-wide: `start-lapin-ux` sets `FONTCONFIG_FILE=/usr/share/lapin-ux/fonts.conf`
  (the system's setup plus Lapin's rule) for its own session only. Settings
  Lapin UX shares with other desktops are kept per session: GTK font,
  document font, icons and pointer; COSMIC's font, icons, light/dark mode,
  wallpaper, colours and corners (the four theme folders), keyboard
  shortcuts, Super key and focus policy; the default X pointer.
  `lapin-ux-desktop-settings enter` (session start) stores the other
  desktops' copy in `~/.local/state/lapin-ux/other-desktops` and puts
  Lapin's (`lapin-desktop`) in place; `leave` (session end) does the
  reverse and drops `FONTCONFIG_FILE` from the systemd user session. A
  change made in COSMIC stays in COSMIC, one made in Lapin UX stays in
  Lapin UX. `lapin-active` marks Lapin's copy as the live one, so a missed
  `leave` is caught later: `/etc/xdg/autostart/lapin-ux-other-desktops.desktop`
  (NotShowIn=LapinUX) and the `lapin-ux-desktop-settings.service` user unit
  (enabled for everyone, `default.target` + `graphical-session.target`, so
  Sway, Hyprland and text consoles too) run `leave` at every login. It does
  nothing while Lapin UX runs or starts (`lapin-ux-shell` running, or
  `$XDG_RUNTIME_DIR/lapin-ux-session.pid` alive). Accounts set up before
  this existed: on the first `enter`, values that look like Lapin's stay
  Lapin's, and the other desktops get the restore point or their default.
- **Lapin's versions of COSMIC parts only look like Lapin inside Lapin UX.**
  The volume/brightness pop-ups and admin prompt (cosmic-osd fork), the
  touch keyboard (cosmic-osk fork), the Open/Save dialogs (cosmic-files
  patch in the portal), COSMIC Settings and COSMIC apps' title bars check
  `XDG_CURRENT_DESKTOP` for `LapinUX`; in COSMIC's own session they are
  COSMIC's. Force on for checks with `LAPIN_UX_OSD=1`, `LAPIN_UX_OSK=1`,
  `LAPIN_UX_DIALOG=1`, `LAPIN_UX_SETTINGS=1`, `LAPIN_UX_TITLEBAR=1`.
- Icons: the `Lapin-UX` / `Lapin-UX-dark` theme (follows light/dark mode).
  Apps, files, folders, shortcuts and desktop items use the full-colour
  Win11 icon theme; everything else (menus, tray, quick settings, buttons)
  uses Fluent UI System Icons. Only replaces COSMIC's default icon theme,
  never one you picked.
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
| Alt + Tab / Alt + Shift + Tab | Switch windows (thumbnails, most recent first) |
| Super + 1…9 | Open / switch to the n-th taskbar app |
| Super + L | Lock |
| Super + ↑ / ↓ | Maximize / minimize |
| Super + ← / → | Snap to the left / right half |
| Super + Shift + ← / → | Move the window to the next monitor |
| Super + Z | Snap layouts |
| Super + Tab | Task view |
| Ctrl + Super + ← / → | Previous / next desktop |
| Ctrl + Super + D | New desktop |
| Super + Shift + S | Screenshot |
| Ctrl + Alt + Del | Sign-out / power dialog |

## Architecture

```
display manager ──> start-lapin-ux ──> lapin-ux-session
                                      ├─ lapin-ux-comp / cosmic-comp  compositor (ours = upstream + 2 small patches)
                                      ├─ lapin-ux-shell           taskbar ×N, Start menu, tray   ← ours
                                      ├─ lapin-ux-notifyd         notification toasts + centre   ← ours
                                      ├─ cosmic-settings-daemon theme, media keys              (reused)
                                      ├─ cosmic-bg              wallpaper                      (reused)
                                      ├─ lapin-ux-osd           volume pop-ups, admin prompt   (COSMIC's + patch)
                                      ├─ cosmic-idle            screen blanking                (reused)
                                      └─ lapin-ux-greeter       lock screen (COSMIC's + patch)
```

| Crate | What it is | Depends on libcosmic |
|---|---|---|
| `lapin-ux-core` | Search ranking, taskbar model, toast queue, clock formats, recent files, pins, calendars, file operations, snap layouts, displays, night light, process and system monitoring, updates (incl. fwupd), sounds, OS name, users and services, search preview. 99 unit tests. | no |
| `lapin-ux-shell` | Taskbar, Start menu, tray, popups, first-run defaults | yes |
| `lapin-ux-notifyd` | Notification server and toasts | yes |
| `lapin-ux-session` | Compositor handshake and process supervisor. 2 tests. | no |
| `lapin-ux-taskmgr` | Task Manager | yes |
| `lapin-ux-control` | Control Panel | yes |
| `lapin-ux-update` | Update (pacman + Flatpak + firmware) | yes |
| `lapin-ux-welcome` | First-run setup | yes |
| `lapin-ux-ui` | Shared Windows-style widget styles (4 px buttons, round slider thumbs, sidebar items, icon + title for title bars) | yes |

`lapin-ux-comp` (`packaging/lapin-ux-comp`) is upstream cosmic-comp at a
pinned commit plus five patches adding `lapin_shell_v1` (`protocols/`, version 3): snap a
window into a fraction of the work area, tell the shell when a window asks
for attention, desktop peek, night light (also on the real-screen KMS path),
touchpad swipes for the shell, the Alt key's state for the shell's Alt+Tab
switcher (v3 `alt` event), and a window-open animation; plus title bars with
the app icon and a left-aligned title. Everything else works on stock cosmic-comp.

Five more COSMIC programs are built with small patches, each as its own
package that replaces the original: `lapin-ux-greeter` (lock and sign-in
screens), `lapin-ux-osk` (touch keyboard), `lapin-ux-osd` (volume/brightness
pop-ups and the admin prompt), `lapin-ux-settings` (Settings) and `lapin-ux-portal` (file dialogs; COSMIC's
portal built against a patched COSMIC Files). Each patch keeps Lapin's code
in its own file with a few call sites and is pinned to a COSMIC stable
(epoch) release, so it rebases easily when COSMIC updates.

Window tracking supports two backends: cosmic-comp's protocols (full
control) and `wlr-foreign-toplevel-management` (sway, Hyprland, labwc,
niri…), so the shell also runs on wlroots compositors.

All settings use cosmic-config and apply live:
`~/.config/cosmic/dev.lapinux.Shell/v1/` (`pinned_taskbar`, `pinned_start`,
`alignment` = `Center`/`Left`, `show_search`, `clock_24h`, `clock_seconds`,
`show_desktop_corner`, `tray_inline`, `web_search`, `hide_web_search`, `desktop_icon_size`, `desktop_sort`, `hide_desktop_icons`, `desktop_auto_arrange`, `auto_hide`, `multi_display`, `hotspot_password`).

## Install on Arch

```sh
git clone https://github.com/znraznra/lapin-ux.git
cd lapin-ux
./install.sh
```

That's all: the script installs the build tools, builds Lapin UX and its
compositor build (`lapin-ux-comp`, for snap layouts and taskbar flashing),
and pacman installs every other dependency automatically (COSMIC services,
NetworkManager, Bluetooth, sound, search, fonts for other scripts).
Nothing to install by hand. The Selawik font, the Fluent UI icons and the
Win11 icon theme all ship inside this repository (`data/` and
`third_party/`), so the build downloads nothing but Rust crates and the
pinned cosmic-comp source. The package replaces any separately installed
`win11-icon-theme`.

`sh tools/preflight.sh` shows what's already installed (optional).

Log out, pick **Lapin UX** in your display manager's session list, log in.
### Uninstall

Sign out of Lapin UX, log into another desktop (or a text console) and run
`lapin-ux-uninstall` (`./uninstall.sh` in the source folder is the same
script and still works once the packages are gone). `--dry-run` shows the
plan; `--settings-only` only restores settings (for someone who already ran
`pacman -R`).

- **Restore point.** `start-lapin-ux` (and `lapin-ux-cursors`) run
  `lapin-ux-uninstall --snapshot` before Lapin UX first changes anything:
  GTK `org.gnome.desktop.interface` font-name, document-font-name,
  icon-theme, cursor-theme (dconf); COSMIC's interface font, icon theme,
  wallpaper, shortcuts (`custom`, `system_actions`), focus policy and the
  four theme folders; `~/.local/share/icons/default/index.theme`. Saved once
  in `~/.local/state/lapin-ux/restore`. Accounts set up before this existed
  get a `late` marker instead (no originals).
- **Restore.** A value is only changed while it is still Lapin UX's
  (Selawik, Lapin-UX/Win11 icons, Lapin's pointer, wallpaper or shortcuts),
  so later choices made elsewhere stay. With a restore point it goes back to
  the saved value, otherwise to the desktop's default. Then
  `~/.config/cosmic/dev.lapinux.*`, `~/.local/state/cosmic/dev.lapinux.*`,
  `~/.cache/lapin-ux`, the pointer pack and `~/.local/state/lapin-ux` are
  deleted; `~/.config/cosmic` too (after asking) when no COSMIC program is
  left. Other accounts that used Lapin UX are restored the same way.
- **System.** Removes Lapin UX's Howdy line from the PAM files. Login
  screen: if greetd runs Lapin UX's sign-in screen, COSMIC's own
  (`cosmic-greeter`) goes back in its place, so the same login setup keeps
  working and lists every installed desktop. With COSMIC installed, every
  COSMIC part Lapin stood in for comes back (greeter, Settings, OSD, OSK,
  portal); otherwise those wanted by something outside Lapin. All of them
  must be installable first (`pacman -Si`), or nothing is changed; if the
  install fails, Lapin's packages stay. Then `pacman -Rn` the Lapin
  packages, and (without COSMIC) COSMIC parts that became orphans. Other new
  orphans (NetworkManager, Bluetooth, PipeWire…) are only listed.
- **Plain `pacman -R` is covered too.** The desktop package's `pre_remove`
  runs `lapin-ux-uninstall --settings-only --yes` for every account that
  used Lapin UX (skipping one still signed in, with a note). The greeter
  package's `post_remove` waits for pacman to finish; if no `cosmic-greeter`
  arrived and greetd would be left without a greeter, it switches to an
  installed login manager (sddm, gdm, plasmalogin, lightdm, lxdm, ly), or
  else sets greetd to a text login (`agreety`, old config kept as
  `config.toml.lapin-ux-bak`) and says how to get a graphical one back.
- Tests: `tools/test-uninstall/run.sh` (throwaway homes; needs dconf; 55
  checks, scenario I covers giving other desktops' skipped startup parts back).

## Try it without installing

From a terminal inside any Wayland session (COSMIC, GNOME, KDE, sway…) with
`cosmic-comp` installed:

```sh
cargo build
XDG_CONFIG_HOME="$(mktemp -d)" PATH="$PWD/target/debug:$PATH" XDG_CURRENT_DESKTOP=LapinUX:COSMIC \
  dbus-run-session -- target/debug/lapin-ux-session
```

To see the default font and icons in this mode, install them into your
home first with `tools/install-user-assets.sh` (undo with `--remove`).

cosmic-comp detects the parent session and opens as a window containing a
full Lapin UX desktop. Close the window (or `pkill lapin-ux-session`) to stop.

See [TESTING.md](../TESTING.md) for a step-by-step test checklist.

## Develop

```sh
cargo test --workspace     # unit tests (core logic, session handshake)
cargo clippy --workspace   # clean at the time of writing
RUST_LOG=lapin_ux_shell=debug target/debug/lapin-ux-shell   # run the shell alone in a session
target/debug/lapin-ux-shell start                          # toggle Start in the running shell
python3 tools/make-wallpaper.py                          # re-render wallpapers
python3 tools/make-fluent-icons.py <fluentui-system-icons checkout>   # regenerate data/icons/Lapin-UX*
python3 tools/make-fluent-emoji.py <fluentui-emoji checkout> <out>    # rebuild the emoji font (needs nanoemoji)
```

`lapin-ux-core` holds anything that can be tested without a display; put new
logic there first.

## Renaming

The working name is easy to change: `crates/lapin-ux-core/src/lib.rs` (`ids`),
`data/session/*`, `data/systemd/*`, `packaging/arch/PKGBUILD`.

## Not done yet

See the [roadmap in the README](../README.md#roadmap).

## License

Lapin UX's own code and original artwork (Start mark, wallpapers) are
GPL-3.0-only. Window tracking, the tray watcher/menus and the session start
script are adapted from System76's GPL-3.0 COSMIC projects; see file headers.

## Third-party assets and trademarks

Lapin UX uses the following open-source assets by default. Each is used under
its own licence, which is shipped with it; nothing here changes or adds to
those terms.

| Asset | Used for | Licence | Where | Source |
|---|---|---|---|---|
| Selawik 1.01, © Microsoft Corporation | Interface font | SIL Open Font License 1.1 | `data/fonts/selawik/` (unmodified, licence in `OFL.txt`) | [microsoft/Selawik](https://github.com/microsoft/Selawik) |
| Fluent UI System Icons, © Microsoft Corporation | Symbolic icons in `Lapin-UX` / `Lapin-UX-dark` | MIT | `data/icons/Lapin-UX/symbolic/` (renamed to freedesktop names and recoloured by `tools/make-fluent-icons.py`; licence in `LICENSE-fluentui-system-icons`) | [microsoft/fluentui-system-icons](https://github.com/microsoft/fluentui-system-icons) |
| Win11 icon theme by yeyushengfan258 (based on Ubuntu's Yaru) | App, folder, file, shortcut and desktop icons | GPL-3.0 | `third_party/win11-icon-theme/` (unmodified, pinned commit in `UPSTREAM.md`; licence in `COPYING`) | [yeyushengfan258/Win11-icon-theme](https://github.com/yeyushengfan258/Win11-icon-theme) |
| Fluent Emoji, © Microsoft Corporation | All emoji (built into the *Lapin Fluent Emoji* colour font) | MIT | `data/fonts/fluent-emoji/` (the "Color" artwork rendered into a colour-bitmap font by `tools/make-fluent-emoji.py`; licence in `LICENSE-fluentui-emoji`) | [microsoft/fluentui-emoji](https://github.com/microsoft/fluentui-emoji) |
| Noto fonts (Arch packages) | Only characters Selawik lacks (CJK, other scripts) | SIL Open Font License 1.1 | installed as dependencies | [notofonts](https://github.com/notofonts) |
| Borealis sound theme 0.9a, © 2004 Ivica Ico Bukvic | System sounds (sign-in, notifications, volume, Recycle Bin…) | Artistic License, with the author's note that commercial use is allowed only with GPL/LGPL software | `data/sounds/borealis/` (unmodified, from Ubuntu's `borealis-sound-theme` package; licence in `COPYING`, origin in `SOURCE`) | [ico.bukvic.net](http://ico.bukvic.net/Linux/Borealis_soundtheme/) |

**Mouse pointers.** The Windows 11-style pointers are *Cursor Concept W11 Tail
HDPi*, **designed by JepriCreations**
([DeviantArt](https://www.deviantart.com/jepricreations),
[jepricreations.com](https://jepricreations.com)). Their licence allows
personal use and modification but not redistribution, so they are **not**
included in this repository or its packages. `lapin-ux-cursors` (offered at
the end of `install.sh`) downloads the pack on your own computer from
[SullensCR's copy](https://github.com/SullensCR/Windows-11-Hdpi-Tail-Cursor-Concept-by-jepriCreations),
converts it to an X cursor theme for your own use under its author's name,
and makes it your pointer.

Installed packages put every licence in `/usr/share/licenses/lapin-ux-desktop-git/`.

**Trademarks.** Microsoft, Windows, Windows 11, Fluent and Selawik are
trademarks of Microsoft Corporation. "Selawik" is a Reserved Font Name under
the OFL, so the font is shipped unmodified under its own name; a modified
version would have to be renamed. Ubuntu and Yaru are trademarks of
Canonical Ltd. COSMIC is a trademark of System76, Inc. Other product names and
app icons in the Win11 theme belong to their respective owners. These names
are used only to identify the assets and what Lapin UX is compatible with or
inspired by; Lapin UX is an independent open-source project and is not
affiliated with, sponsored or endorsed by Microsoft, Canonical, System76 or
any other trademark owner. Lapin UX ships no Microsoft logos, Windows
artwork or proprietary fonts.

Don't want these defaults? Pick any other font or icon theme in COSMIC
Settings; Lapin UX never overwrites your choice.

[libcosmic]: https://github.com/pop-os/libcosmic
