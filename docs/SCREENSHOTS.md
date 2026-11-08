# Screenshots

[Back to the README](../README.md) · [Roadmap](ROADMAP.md) · [Technical reference](TECHNICAL.md)

Jump to: [The desktop](#the-desktop) · [The Settings app](#the-settings-app)

## The desktop

| | |
|---|---|
| ![Quick settings with Night light](tour/quick-settings.webp) **Quick settings** — Wi-Fi, Bluetooth, volume, brightness, Night light and more. | ![Notification centre with calendar](tour/notification-centre.webp) **Notification centre** — grouped notifications, calendar, today's agenda, Focus. |
| ![Snap layouts](tour/snap-layouts.webp) **Snap layouts** — Super+Z arranges windows in halves, thirds or quarters. | ![Task view with two desktops](tour/task-view.webp) **Task view** — all windows at a glance, plus virtual desktops. |
| ![Alt+Tab switcher](tour/alt-tab.webp) **Alt+Tab** — live window thumbnails, most recent first. | ![Emoji panel](tour/emoji-panel.webp) **Emoji and clipboard** — Super+. and Super+V. |
| ![Snipping overlay](tour/snipping.webp) **Snipping** — Super+Shift+S for a rectangle, window or full-screen snip. | ![Task Manager](tour/task-manager.webp) **Task Manager** — Ctrl+Shift+Esc: processes, performance, startup apps. |
| ![Control Panel](tour/control-panel.webp) **Control Panel** — the classic categories, opening the right settings. | ![Update in Settings](tour/update.webp) **Update** — system, app and firmware updates with one button, inside Settings. |
| ![Lock screen](tour/lock-screen.webp) **Lock screen** — big clock over your wallpaper. | ![Sign-in panel](tour/sign-in.webp) **Sign-in** — picture, name and password, centred; the gear picks which desktop to start. |
| ![Get Started](tour/get-started.webp) **Get Started** — first-sign-in setup: Wi-Fi, light or dark. | ![Open file dialog](tour/file-dialog.webp) **Open and Save dialogs** — the same tidy layout for every app. |
| ![Volume pop-up above the taskbar](tour/volume-popup.webp) **Volume and brightness pop-ups** — a small box above the taskbar when you press the keys. | ![Admin permission prompt](tour/admin-prompt.webp) **Admin prompt** — the screen dims and asks before an app changes your system. |
| ![Search preview of a text file](tour/search-preview.webp) **Search preview** — see a file's first lines or picture before opening it. | ![Task Manager Services tab](tour/task-manager-services.webp) **Task Manager: Users and Services** — who's signed in, and every background service. |
| ![Ctrl+Alt+Del screen](tour/ctrl-alt-del.webp) **Ctrl+Alt+Del** — Lock, Sign out, Change a password, Task Manager. | ![Shut Down dialog](tour/shut-down.webp) **Shut Down** — Alt+F4 on the desktop, with your Linux's name and logo. |
| ![Power-user menu](tour/winx-menu.webp) **Power-user menu** — Super+X; Installed apps, Power Options and System open Settings. | ![Run dialog](tour/run.webp) **Run** — Super+R opens programs, folders and websites. |
| ![Taskbar jump list](tour/jump-list.webp) **Jump lists** — right-click a taskbar app for recent files and actions. | ![Snap assist](tour/snap-assist.webp) **Snap assist** — after snapping a window, pick what fills the rest. |
| ![Snap group on the taskbar](tour/snap-group.webp) **Snap groups** — windows snapped together come back together. | ![Title bars](tour/title-bars.webp) **Title bars** — the app's icon and name on the left. |
| ![Touch keyboard](tour/touch-keyboard.webp) **Touch keyboard** — for tablets and touch screens. | ![Clipboard history](tour/clipboard-history.webp) **Clipboard history** — Super+V brings back what you copied earlier. |
| ![Task Manager performance](tour/task-manager-performance.webp) **Task Manager: Performance** — live CPU, memory, disk and network graphs. | |

## The Settings app

A Settings app laid out the way you'd expect (a sidebar of categories, a Home page, every setting its own card), built from COSMIC Settings so every setting keeps working the same way underneath. It was made in five stages, each one reviewed screen by screen before the next started.

| | |
|---|---|
| ![Settings Home](tour/settings-home.webp) **Home** — your account, Find a setting in the title bar, and the settings people change most. | ![Settings System page](tour/settings-system.webp) **System** — your wallpaper, PC name and Update at the top, then every System page. |
| ![Display settings](tour/settings-display.webp) **Display** — each setting is its own card with an icon. | ![Sound settings](tour/settings-sound.webp) **Sound** — round slider thumbs and On/Off beside every switch. |
| ![Power and battery settings](tour/settings-power.webp) **Power & battery** — a Power mode drop-down. | ![Colors settings](tour/settings-colors.webp) **Colors** — light or dark, accent colour and more. |
| ![Taskbar settings](tour/settings-taskbar.webp) **Taskbar** — every taskbar setting, applied the moment you flip them. | ![Notifications settings](tour/settings-notifications.webp) **Notifications** — Do not disturb, its schedule, and a switch for each app. |
| ![Night light settings](tour/settings-night-light.webp) **Night light** — warmer colours at night, with strength and a schedule. | ![Multitasking settings](tour/settings-multitasking.webp) **Multitasking** — Snap windows, with COSMIC's extra options under Advanced. |
| ![Installed apps](tour/settings-installed-apps.webp) **Installed apps** — every app with its size and date; uninstall from the … menu. | ![Storage settings](tour/settings-storage.webp) **Storage** — what's using your disk, and Temporary files to clean up. |
| ![Update in Settings](tour/settings-update.webp) **Update** — system, app and firmware updates, inside Settings. | ![Privacy and security](tour/settings-privacy.webp) **Privacy & security** — which apps may use your camera, microphone, location and more. |
| ![Lock screen settings](tour/settings-lock-screen.webp) **Lock screen** — your own lock screen picture, previewed with the clock. | ![Sign-in options](tour/settings-signin.webp) **Sign-in options** — face (Howdy), fingerprint and password. |
| ![Find a setting](tour/settings-search.webp) **Find a setting** — suggestions as you type, right under the box. | ![Search results](tour/settings-search-results.webp) **Search results** — every match, with where it lives. |
| ![About settings](tour/settings-about.webp) **About** — device and system specifications with Copy. | |

- [x] **Stage 0 — groundwork:** COSMIC Settings set up as Lapin's own package, pinned to COSMIC's stable release
- [x] **Stage 1 — the familiar frame:** sidebar with your account and the usual categories, Home, page titles like "System > Display", the System page header
- [x] **Stage 2 — the look inside pages:** setting cards with icons, On/Off switches, drop-downs and clean buttons, the About page
- [x] **Stage 3 — the page layout:** Taskbar, Start, Notifications, Night light and Multitasking pages; the old Panel and Dock pages removed; COSMIC-only options under "Advanced"
- [x] **Stage 4 — pages COSMIC doesn't have:** Installed apps, Storage, Sign-in options, Privacy & security, Lock screen, Clipboard, and Update moved inside Settings
- [x] **Stage 5 — joining it up:** Find a setting shows suggestions as you type and a Search results page; Start, Control Panel, Super+X, the taskbar menu, quick settings, the notification centre and Super+I open the right page

Under plain COSMIC the same app still looks like COSMIC Settings: the new layout switches on only in a Lapin UX session.
