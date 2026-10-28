# Feature Set

Everything built so far, by area. The authoritative long version lives in `docs/TECHNICAL.md` in the repo.

## Taskbar (`lapin-ux-shell`)
- Centred or left icons, Search box (real text field; Enter shows results in Start), pinned and running apps, one taskbar per monitor.
- Grouped buttons: click opens/focuses/minimizes; several windows show a picker; middle-click opens a new instance.
- Accent pill under the focused app; jump lists, app actions, pin/unpin, **End task**.
- Badges and progress bars (Unity LauncherEntry); attention flash.
- Task view button, tray (StatusNotifierItem), clock (follows COSMIC's 24-hour switch; unset = 12 h), show-desktop sliver, auto-hide.
- Start button icon presets, OS logo or a custom picture.

## Start menu
- Type-to-search ranking (initials, generic names, keywords, executables); Settings pages, files, folders, document contents, web.
- Pinned grid with drag-to-reorder and folders; All apps (A-Z) with a slide transition; Recommended from recent files.
- Right-click Uninstall (core desktop packages refused). Other desktops' apps are hidden by default.
- Account and power menus; search preview pane (text, pictures, folder counts).

## Notifications (`lapin-ux-notifyd`)
Full `org.freedesktop.Notifications` server, toasts, notification centre with calendar and agenda, Focus sessions, Do not disturb schedule, per-app settings, inline reply and progress. D-Bus interface `dev.lapinux.Notifications`.

## Quick settings
Wi-Fi, Bluetooth with pairing, Airplane mode, Energy saver, accessibility, hotspot, nearby sharing, cast, keyboard layout, VPN, Do not disturb, media controls, brightness, volume with per-app mixer, Night light, rotation lock, editable tiles.

## Windows and multitasking
Alt+Tab with thumbnails, Task view, virtual desktops (renaming, per-desktop wallpaper), Super+arrow snapping, snap layouts (Super+Z), Snap assist, snap groups, desktop peek, touchpad gestures, open/minimize/restore animations.

## Desktop
Icons with Recycle Bin, multi-select, cut/copy/paste, drag and drop, Properties, This PC, Network, Control Panel icons, classic "Show more options" menu, Open with picker.

## Settings app (fork of COSMIC Settings)
Windows 11 frame (sidebar, Find a setting in the title bar, Home), cards for every setting, About laid out like Windows, plus Lapin pages: Taskbar, Start, Night light, Notifications, Multitasking, Storage, Clipboard, Lock screen, Installed apps, Sign-in options (fingerprint, Howdy face unlock, password), Privacy and security, Update (incl. firmware). Search across all pages. `cosmic-settings lapin <page-id>` opens any Lapin page.

## System apps
- **Task Manager**: Processes, Performance, Startup apps, Details, Users (logind), Services (systemd).
- **Control Panel**: category view and Large icons with Win11 colour icons.
- **Update**: pacman, Flatpak, fwupd firmware; history; restart prompt; background check.
- **Get Started**: Wi-Fi, light/dark, optional pointers.
- Snipping (Win+Shift+S): rectangle, window, full screen (no markup editor yet).
- Clipboard history and emoji (Super+V, Super+.).

## Sign-in
Lock and sign-in screens (`lapin-ux-greeter`): big clock, blurred panel, round picture, fingerprint via fprintd, Howdy face unlock, accessibility/keyboard/session/power flyouts, on-screen keyboard.

## Input and accessibility
Input indicator with fcitx5, Win+Space, touch keyboard (`lapin-ux-osk`), Magnifier, colour filters, Narrator via Orca (offers to install it). Contrast themes are currently turned off.

## Pop-ups and prompts (`lapin-ux-osd`)
Volume/brightness pop-ups and the admin (polkit) prompt in Windows 11's style.

## Dialogs (`lapin-ux-portal`)
Open/Save dialogs with a Windows-style footer and the asking app's icon in the title.

## Look and sound
Selawik (session-only font), Fluent UI System Icons, Win11 icon theme, Fluent Emoji, Borealis sounds, pointers (opt-in download).

## Keyboard shortcuts
Super (Start), +E, +D, +X, +R, +N, +A, +V, +., +Z, +Tab, +L, +1..9, +arrows, Alt+Tab, Ctrl+Alt+Del, Win+Shift+S, Win+I (Settings), Win+P.
