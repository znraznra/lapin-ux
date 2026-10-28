# Roadmap

Status words: **Done** = built, sandbox-tested, approved in review. **Partial**. **Not started**. **Left out** = deliberately excluded (don't build or propose). **Parked** = optional, later. **Out of reach** = needs Microsoft services/files.

Last updated 2026-10-28, build `dca07a2`.

## Phase 1: Core desktop (12/12 done)
- Taskbar (centred/left, pinning, grouped buttons, jump lists, End task, badges, progress, attention flash, labels, small buttons, auto-hide, one per monitor, show-desktop sliver)
- System tray with overflow and app menus (apps using only the old XEmbed tray don't show)
- Clock and calendar
- Start: search, pinned grid with folders, All apps, Recommended, account/power menus, Uninstall
- Start search: apps, Settings pages, files, document contents (needs LocalSearch), web
- Start slide-up animation (opens 4 px above the taskbar, rests 12 px above)
- Notifications: server, toasts, centre, agenda, Focus, Do not disturb schedule, per-app settings
- Wallpaper (original light/dark artwork), theme (accent, corners, frosted surfaces, Selawik, Fluent Emoji, Win11 + Fluent icons)
- Session supervision and sign out
- Arch packaging and one-step installer (verified on real hardware)

## Phase 2: Settings and multitasking (9/9 done)
- Quick settings (all tiles, edit tiles, Night light, rotation lock)
- Notification centre with agenda
- Alt+Tab with thumbnails, taskbar hover previews, Task view, virtual desktops
- Super+arrow snapping, snap layouts, Snap assist, snap groups
- Drag to reorder pins
- **Settings app** (fork of COSMIC Settings, 5 stages, all approved 2026-10-21)
- Windows 11-style volume/brightness pop-ups (cosmic-osd fork)

## Phase 3: System apps (4 done, 1 partial, 1 not started)
- Done: "Open with" picker, Update (pacman + Flatpak + fwupd firmware), Control Panel
- Partial: Open/Save dialogs (Windows-style footer and title done; navigation pane is still COSMIC's)
- **Not started: File Explorer** (fork of COSMIC Files; command bar, Home, Gallery, details pane)

## Phase 4: Road to 1:1 Windows 11 (16 done, 1 partial, 2 not started)
- Done: Night light and desktop peek, touchpad gestures, open/minimize/restore animations, Win+X/Run/Win+P/Ctrl+Alt+Del/Alt+F4, lock and sign-in screens, first-run setup, desktop icons/properties, Task Manager (incl. Users and Services), snipping tool, input indicator, touch keyboard, accessibility, sounds and pointers, admin prompt, search preview pane
- Partial: title bars (GTK, Electron, Qt apps keep theirs by choice)
- Not started: **Store** (fork of COSMIC Store), **snip markup editor**

## What's left (suggested order)
1. Tako's hardware retest of fixed issues (see [Real-Hardware Issues](Real-Hardware-Issues))
2. #23 other desktops get Lapin's icons; #24 mouse pointer settings page; #28 X11 pop-ups over the taskbar (needs `_NET_WORKAREA` in cosmic-comp's X11 WM)
3. File Explorer (+ dialog navigation pane), gated on the Lapin session
4. Store, gated on the Lapin session; add to `LAPIN_PKGS`/`STOCK` in the uninstaller if it replaces cosmic-store
5. Snip markup editor (COSMIC has none: own small editor)
6. Snap-group shared-edge resize (new grab in COSMIC's compositor; changes often upstream)

## Left out (confirmed 2026-10-20, don't build or propose)
GTK 3 / GTK 4 / KDE (Qt) app theming, newer Start layout, search highlights, widgets and Copilot, window close animation, wallpaper slideshow/daily picture.

## Parked
Live captions, Win+H voice typing, Mica/acrylic on app windows.

## Out of reach
Microsoft account, OneDrive, Phone Link (use Nextcloud, KDE Connect); Segoe UI, Segoe Fluent Icons, Windows sounds and pointers (replaced by Selawik, Fluent UI System Icons, Borealis, JepriCreations); Windows Hello face sign-in (Howdy offered instead); running Windows apps natively (Wine/Proton is a separate project).

## Cross-cutting roadblocks
The sandbox only runs the nested compositor backend, so KMS-only paths, real locking and real keyboard modifier delivery can only be verified by Tako on hardware. Touch screens are untested (no device). Every COSMIC fork is a patch to rebase when COSMIC updates.
