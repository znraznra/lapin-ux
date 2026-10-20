# Lapin UX

**A Linux desktop that looks and works like Windows 11.**

Lapin UX gives Linux a Windows 11-style desktop: a centred taskbar, a Start menu with search, quick settings, notifications, snap layouts, Task Manager, Control Panel and the keyboard shortcuts you already know. No plugins to hunt down and no terminal needed for everyday tasks. It is written in Rust on top of [COSMIC](https://github.com/pop-os/cosmic-epoch), and it runs on Arch Linux first.

![The Start menu open on the Lapin UX desktop](docs/tour/start-menu.webp)

> **Status:** the everyday desktop is built and reviewed screen by screen. It has not been tested on a real Arch Linux machine yet, so treat it as a preview. Settings, File Explorer, a Store and an admin prompt are still to come (see the [roadmap](#roadmap)).
>
> Lapin UX copies how Windows 11 *behaves and looks*, not Microsoft's files. It ships no Microsoft logos, Windows artwork or proprietary fonts; everything it includes is open source under its own licence (see [Credits](#license)). It is an independent project, not affiliated with or endorsed by Microsoft.

## At a glance

| Phase | What it covers | Progress |
|---|---|---|
| 1. Core desktop | Taskbar, Start, notifications, tray, wallpaper and theme | `██████████` 12 of 12 |
| 2. Settings and multitasking | Quick settings, notification centre, Alt+Tab, Task view, snapping | `█████████░` 8 of 9 |
| 3. System apps | File Explorer, Update, Control Panel | `███████░░░` 4 of 6, 1 partly |
| 4. Road to a 1:1 Windows 11 | Lock and sign-in, Task Manager, snipping, touch keyboard, accessibility, sounds… | `████████░░` 16 of 19, 1 partly |

*Last updated: 20 October 2026 (build `14f2e42`).*

## Take the tour

| | |
|---|---|
| ![Quick settings with Night light](docs/tour/quick-settings.webp) **Quick settings** — Wi-Fi, Bluetooth, volume, brightness, Night light and more. | ![Notification centre with calendar](docs/tour/notification-centre.webp) **Notification centre** — grouped notifications, calendar, today's agenda, Focus. |
| ![Snap layouts](docs/tour/snap-layouts.webp) **Snap layouts** — Win+Z arranges windows in halves, thirds or quarters. | ![Task view with two desktops](docs/tour/task-view.webp) **Task view** — all windows at a glance, plus virtual desktops. |
| ![Alt+Tab switcher](docs/tour/alt-tab.webp) **Alt+Tab** — live window thumbnails, most recent first. | ![Emoji panel](docs/tour/emoji-panel.webp) **Emoji and clipboard** — Win+. and Win+V, as on Windows. |
| ![Snipping overlay](docs/tour/snipping.webp) **Snipping** — Win+Shift+S for a rectangle, window or full-screen snip. | ![Task Manager](docs/tour/task-manager.webp) **Task Manager** — Ctrl+Shift+Esc: processes, performance, startup apps. |
| ![Control Panel](docs/tour/control-panel.webp) **Control Panel** — the classic categories, opening the right settings. | ![Update](docs/tour/update.webp) **Update** — system and app updates with one button. |
| ![Lock screen](docs/tour/lock-screen.webp) **Lock screen** — big clock over your wallpaper. | ![Sign-in panel](docs/tour/sign-in.webp) **Sign-in** — picture, name and password, centred. |
| ![Get Started](docs/tour/get-started.webp) **Get Started** — first-sign-in setup: Wi-Fi, light or dark. | ![Open file dialog](docs/tour/file-dialog.webp) **Open and Save dialogs** — Windows-style for every app. |
| ![Volume pop-up above the taskbar](docs/tour/volume-popup.webp) **Volume and brightness pop-ups** — a small box above the taskbar when you press the keys. | ![Admin permission prompt](docs/tour/admin-prompt.webp) **Admin prompt** — the screen dims and asks before an app changes your system. |
| ![Search preview of a text file](docs/tour/search-preview.webp) **Search preview** — see a file's first lines or picture before opening it. | ![Task Manager Services tab](docs/tour/task-manager-services.webp) **Task Manager: Users and Services** — who's signed in, and every background service. |
| ![Ctrl+Alt+Del screen](docs/tour/ctrl-alt-del.webp) **Ctrl+Alt+Del** — Lock, Sign out, Change a password, Task Manager. | |

## Roadmap

The work is split into four phases. Green is done, yellow is partly done, grey is still to build.

```mermaid
flowchart LR
    P1["Phase 1<br/>Core desktop"]:::done --> P2["Phase 2<br/>Settings and multitasking"]:::mostly --> P3["Phase 3<br/>System apps"]:::partly --> P4["Phase 4<br/>Road to 1:1 Windows 11"]:::mostly
    P2 -.- S["Settings app"]:::todo
    P3 -.- FE["File Explorer"]:::todo
    P4 -.- ST["Store"]:::todo
    P4 -.- MK["Snip markup editor"]:::todo
    classDef done fill:#2e7d32,color:#fff,stroke:#1b5e20
    classDef mostly fill:#9e9d24,color:#fff,stroke:#827717
    classDef partly fill:#f9a825,color:#000,stroke:#f57f17
    classDef todo fill:#9e9e9e,color:#fff,stroke:#616161
```

### Phase 1 — core desktop (done)

- [x] Taskbar: centred or left icons, pinned and running apps, window picker, jump lists, End task, badges and progress, attention flash
- [x] Taskbar labels, smaller buttons, auto-hide, one taskbar per monitor, Show desktop corner
- [x] System tray with app menus
- [x] Clock with calendar
- [x] Start: type to search, pinned apps and folders, All apps, Recommended, account and power menus, Uninstall
- [x] Search: apps, Settings pages, files, text inside documents, the web
- [x] Start slides up as it opens, without touching the taskbar
- [x] Notifications: pop-ups, notification centre, agenda, Focus, Do not disturb on a schedule
- [x] Wallpaper and theme: accent colour, rounded corners, frosted panels
- [x] Look: Selawik font, Fluent Emoji, Win11-style icons
- [x] Session: starts everything and restarts anything that crashes
- [x] Arch package and one-step installer *(built, not yet tried on a real Arch machine)*

### Phase 2 — settings and multitasking (8 of 9)

- [x] Quick settings: Wi-Fi, Bluetooth, sound and per-app volume, media, brightness, Energy saver, hotspot, VPN, accessibility
- [x] Edit quick settings, Night light with a schedule, rotation lock
- [x] Notification centre with calendar
- [x] Alt+Tab and taskbar thumbnails
- [x] Task view, virtual desktops, names and wallpaper per desktop
- [x] Snapping, snap layouts, Snap assist, snap groups
- [x] Drag to reorder pinned apps
- [ ] **Settings app** in Windows 11's layout — *COSMIC Settings fills in for now*
- [x] Windows-style volume and brightness pop-ups

### Phase 3 — system apps (4 of 6, 1 partly)

- [ ] **File Explorer** in Windows 11's layout — *COSMIC Files fills in for now*
- [~] Open and Save dialogs — *Windows-style bottom row and title bar done; the side pane is still COSMIC's*
- [x] "How do you want to open this file?" picker
- [x] Update: system and Flatpak apps, history, restart prompt, background check
- [x] Firmware updates (BIOS, SSDs, docks) *(not yet tried with real firmware)*
- [x] Control Panel

### Phase 4 — road to a 1:1 Windows 11 (16 of 19, 1 partly)

- [x] Night light and desktop peek
- [x] Touchpad gestures *(untested on a real touchpad)*
- [x] Window open, minimize and restore animations
- [~] Windows 11 title bars (app icon, title on the left) — *done for most apps, Lapin's apps and (optionally) COSMIC's own apps; other apps that draw their own title bar keep theirs on purpose*
- [x] Win+X menu, Run, Win+P, Ctrl+Alt+Del, Alt+F4 Shut Down dialog
- [x] Lock screen, sign-in screen, fingerprint sign-in
- [x] Get Started (first-sign-in setup)
- [x] Desktop: Properties, This PC, Network and Control Panel icons, classic right-click menu
- [x] Task Manager: Processes, Performance, Startup apps, Details
- [x] Task Manager: Users and Services tabs
- [x] Snipping: rectangle, window, full screen
- [ ] Snip markup editor (pen, highlighter, crop)
- [x] Keyboard layout switcher and input methods (Japanese, Chinese…)
- [x] Touch keyboard
- [x] Narrator, Contrast themes, Magnifier, colour filters
- [x] System sounds and Windows 11-style pointers
- [ ] **Store**
- [x] Admin prompt in Windows 11's style
- [x] Richer file preview in search

*`[~]` = partly done.*

### Not planned

Left out on purpose: theming of GTK 3, GTK 4 and KDE (Qt) apps, the newer Start layout, search highlights, widgets and Copilot, and a closing animation for windows. Parked for later: live captions, Win+H voice typing and see-through (Mica) app windows. Some things can't be copied at all because they are Microsoft's own services or files: Microsoft account, OneDrive, Phone Link, Windows Hello face sign-in, and Segoe fonts and sounds (open look-alikes are used instead).

## How it fits together

```mermaid
flowchart TB
    subgraph always["Always running"]
        session["Session<br/>starts and watches everything"]
        comp["Compositor<br/>COSMIC's, with Lapin changes:<br/>windows, snapping, night light"]
        shell["Shell<br/>taskbar, Start, quick settings,<br/>desktop, Task view"]
        notify["Notifications"]
        session --> comp
        session --> shell
        session --> notify
        shell <-->|Lapin's own protocol| comp
    end
    subgraph opened["Opened when needed"]
        apps["Lapin apps<br/>Task Manager, Control Panel,<br/>Update, Get Started"]
        cosmic["COSMIC programs with Lapin changes<br/>lock and sign-in, touch keyboard,<br/>Open/Save dialogs, volume pop-ups,<br/>admin prompt, COSMIC app title bars"]
    end
    shared["Shared building blocks<br/>logic and Windows-style controls"]
    shell --> shared
    apps --> shared
```

Lapin UX is built on COSMIC, System76's Rust desktop. It writes its own taskbar, Start menu and apps, and reuses COSMIC's compositor and services, carrying small Lapin changes on top of a few COSMIC programs. Each change is kept in its own file and pinned to COSMIC's stable releases, so COSMIC updates don't break it. The [technical reference](docs/TECHNICAL.md) explains every part.

## Install (Arch Linux)

```sh
git clone https://github.com/znraznra/lapin-ux.git
cd lapin-ux
./install.sh
```

1. The script installs everything it needs; there is nothing to install by hand.
2. Log out, pick **Lapin UX** on the sign-in screen, and sign in.
3. To remove it: `sudo pacman -R lapin-ux-desktop-git`.

Want to try it first without installing? See [Try it without installing](docs/TECHNICAL.md#try-it-without-installing). Found something that doesn't work? The [test checklist](TESTING.md) lists what to check.

## Keyboard shortcuts

| Keys | What happens |
|---|---|
| Win | Start |
| Win + E / D / L | Files / show the desktop / lock |
| Win + A / N | Quick settings / notifications |
| Win + V / . | Clipboard history / emoji |
| Win + Tab | Task view |
| Win + Z, Win + arrows | Snap layouts, snap a window |
| Alt + Tab | Switch windows |
| Win + Shift + S | Snip part of the screen |
| Ctrl + Shift + Esc | Task Manager |
| Ctrl + Alt + Del | Lock, sign out, Task Manager |

The full list is in the [technical reference](docs/TECHNICAL.md).

## For developers

Building, testing, settings, the architecture in detail and how the COSMIC changes are kept as patches: [docs/TECHNICAL.md](docs/TECHNICAL.md) and [TESTING.md](TESTING.md).

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
