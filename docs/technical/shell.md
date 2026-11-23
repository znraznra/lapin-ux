# Lapin UX technical reference: The desktop shell

[Back to the technical reference](../TECHNICAL.md)

**Taskbar** (`lapin-ux-shell`), one per monitor
- Centered (or left-aligned) Start button, Search box, pinned and running apps.
- **Taskbar location** (#41): Settings > Personalization > Taskbar > *Taskbar
  location on screen* (Bottom, Top, Left, Right) or the taskbar's right-click
  menu. Key `position` in `dev.lapinux.Shell`, applied live. On the left or
  right the bar is 48 or 56 px wide: Start on top, icon-only Search, Task view,
  apps stacked, tray and clock at the bottom; the running marker sits on the
  inner edge and alignment Center/Left becomes Center/Top. Start, panels,
  pop-ups, toasts, auto-hide and the work area follow the edge. Start and
  panels on a side bar anchor to its bottom corner.
- The Search box is a real text field: click it to type on the taskbar (the
  taskbar takes the keyboard only meanwhile); Enter shows the results in
  Start; Esc or clicking away clears it.
- Start button picture: Settings > Personalization > Taskbar > Start button
  icon (the official Lapin UX logo, `docs/assets/lapin-ux-logo.svg`, as the
  default; Dots, Circle, the OS logo from os-release
  `LOGO=`, or a custom SVG/PNG copied to `~/.local/share/lapin-ux`);
  `start_icon` in `dev.lapinux.Shell` (`""` = the logo, which is also shown
  when a chosen picture is missing and is the app icon `dev.lapinux.Shell`,
  #56).
- Right-clicking an icon in the tray drawer (^) opens its menu beside it,
  growing upwards; the drawer stays open until a press outside both.
- Opaque by default, with no blur behind it (#55). Personalization > Colors >
  *Transparency effects* (`transparency_effects` in `dev.lapinux.Shell`,
  off by default; Lapin session only) makes it see-through (82 %) and
  blurred.
- Apps' single-colour tray icons (Steam's black `steam_tray_mono`,
  Electron apps' black icons, `-mono`/`-panel` variants, black SVGs or
  pixmaps) show the **app's own full-colour icon** instead (#52,
  `lapin_ux_core::tray_look`): the item's id, title and owning process
  (name, executable, command-line paths; Electron's generic
  `chrome_status_icon_N` ids are skipped) are matched to an installed app
  by desktop id, window class, executable or name. With no match they're
  tinted like the taskbar's own icons. Only items whose category is
  ApplicationStatus or Communications count as apps; status icons
  (SystemServices, Hardware: network, Bluetooth…) are drawn as sent.
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
- **Main display** (several monitors, #31): Settings > System > Display >
  *Make this my main display* (Lapin keys `main_display` and
  `main_display_model` in `dev.lapinux.Shell`: connector name, and make +
  model to find it after a port change). The main display has the tray, the
  desktop icons and notification toasts; other taskbars keep the clock. With
  none chosen (or the chosen one unplugged): the laptop panel, else the
  top-left monitor (`lapin_ux_core::display::pick_main`, shared by the shell,
  notifications and Settings). The OSD, admin prompt, lock screen and new
  windows don't follow it yet.
- System tray (StatusNotifierItem): left-click activates, right-click opens
  the app's menu with checkmarks, disabled items and submenus (or, for an
  item without a menu, asks the app for one with `ContextMenu`). Discord,
  Steam, Nextcloud, etc. show up here. Older X11 and Wine tray icons
  (XEmbed) show up too, through `lapin-ux-xembed` (#33): it owns
  `_NET_SYSTEM_TRAY_S0` on Xwayland, keeps each docked icon window
  off-screen, reads its picture through Composite every half second and
  publishes it as a StatusNotifierItem on its own D-Bus connection (so the
  item goes when the icon does); clicks and scrolling go back to the X window
  as button events.
- Clock with locale-aware date (seconds when turned on, on both taskbar
  sizes; the switch is in Taskbar behaviors and in Settings > Time &
  language > Date & time);
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
- **Start layout** (#198): Settings > Personalization > Start > Layout, or the
  welcome app's "Start layout" step (after Pointers). Setting `start_layout`
  in `dev.lapinux.Shell`: `Current` (default), `Classic` (narrow single
  column, accent strip on the left), `ClassicTwoColumns` and `ProgramsList`
  (600 px wide, user picture and places on the right). Code in
  `start_layouts.rs`; search, pins, all apps, other-desktop filtering and the
  power menu are shared with the Current layout. There are no flyout
  submenus: Programs switches the panel to the apps list (with a Back row),
  Shut Down's arrow opens the usual power menu. Applies the next time Start
  opens, only inside the Lapin session. Lapin's own setting, so nothing in
  `items()` of `lapin-ux-uninstall`.
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
- **Low disk space warning** (#174): every 30 s the daemon reads the free
  space of the root drive and the one holding the home folder (`df`, ten
  second limit). A drive is low under 5 percent free, never above 10 GB and
  always under 1 GB (`lapin_ux_core::disk`). A "Disk space is running low"
  notice appears once per drive when it crosses that line, and is armed again
  only after the drive has a fifth more than the limit free, so it never
  repeats while the space stays low. A switch in the notification settings
  (gear) turns it off; it is saved as `low_disk_warning` in
  `dev.lapinux.Notifications`, so it needs no entry in `items()`. The daemon
  only runs inside a Lapin session.
- The clock shows a bell with the unread count, or a muted bell during
  Do not disturb. `lapin-ux-notifyd center` toggles the centre from scripts;
  `dev.lapinux.Notifications` on the session bus exposes `Unread`,
  `DoNotDisturb`, `ToggleCenter` and `ClearAll`.

**Desktop icons** (the contents of ~/Desktop, on the main monitor)
- **Recycle Bin** icon (Open, Empty Recycle Bin); drop files on it to delete.
- Click selects; Ctrl-click, Shift-click and dragging a box select several.
  Double-click / Enter opens, drag moves icons (remembered), F2 renames in
  place, Delete moves to the Trash, picture files show thumbnails.
- Names are 13 px with tight lines (15 px), up to two lines under the icon,
  and the full name on hover/selection (#64). Cells: small 76×80, medium
  92×96, large 120×120.
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

**Windows reopen where they were** (`lapin-ux-comp` 0009, #42): in the Lapin
session the compositor notes each app's window size and position (with
the output) and saves them to `$XDG_STATE_HOME/lapin-ux/window-geometry`
when a window closes; the app's next window opens there (kept on screen,
centred on another output). Dialogs, maximized and snapped windows,
windows coming back from minimize and positions apps ask for are left alone.

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
- **Safely Remove Hardware** (#102): a tray icon left of the other tray icons,
  shown only while a USB stick, SD card or similar removable drive is attached
  (phones, network shares and internal disks are left out). Click it for one
  "Eject <drive>" row per drive with its volumes underneath, right-click it for
  Settings > Bluetooth & devices. Eject unmounts every volume (locking an
  encrypted one), then powers the drive off, or ejects it for card readers and
  discs, and says "Safe To Remove Hardware". If something is still using it,
  "Problem Ejecting" names the programs (found by scanning `/proc`). Unplugging
  without ejecting just hides the icon. It talks to UDisks2 on the system bus
  (`lapin-ux-shell/src/removable.rs`); the pure logic is in
  `lapin_ux_core::removable`. It reads and writes no shared COSMIC or GTK
  setting, and only exists in the Lapin session.
