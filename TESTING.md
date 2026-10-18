# Testing Lapin UX on your machine

Two stages. Stage A runs Lapin UX in a window inside your current desktop:
nothing is installed and your settings are untouched. Stage B installs it as
a real login session. Do A first.

## 0. Pre-flight (1 minute)

```sh
git clone https://github.com/znraznra/lapin-ux
cd lapin-ux
sh tools/preflight.sh
```

It only reads your system. If it lists missing packages it prints the
`pacman` command to install them. Re-run until it shows no `MISS` lines.

## Stage A: in a window (no install, no logout)

```sh
cargo build                     # first build takes 10–20 minutes
export LAPIN_TEST="$(mktemp -d)"
XDG_CONFIG_HOME="$LAPIN_TEST/config" XDG_STATE_HOME="$LAPIN_TEST/state" \
XDG_CURRENT_DESKTOP=LapinUX:COSMIC PATH="$PWD/target/debug:$PATH" \
  dbus-run-session -- target/debug/lapin-ux-session 2>&1 | tee lapin-test.log
```

The temporary config folder keeps Lapin UX from changing your real theme,
wallpaper or shortcuts, and `dbus-run-session` gives it a private message bus
so its notifications and tray don't mix with your desktop's. A window opens with the Lapin UX desktop inside it.
Close it (or press Ctrl+C in the terminal) to stop.

In a window your own desktop keeps the Super key and most shortcuts, so in
Stage A test with the mouse. Keyboard shortcuts are tested in Stage B.

Apps you start from Lapin UX's Start menu open inside the window. To test
notifications, open a terminal from that Start menu and run `notify-send`
there; commands typed in your normal terminal go to your normal desktop.
Everything renders a little slower in a window; that is expected.

## Stage B: as your desktop

```sh
cd packaging/arch
makepkg -si
```

Log out, choose **Lapin UX** in the session list on the login screen, log in.

**Getting back:** log out (Start → power → Sign out) and pick your usual
session. If the desktop is unusable, press Ctrl+Alt+F3, log in on the text
console and run `sudo systemctl restart display-manager`, or remove it with
`sudo pacman -R lapin-ux-desktop-git`.

If you also use the regular COSMIC desktop: Lapin UX shares COSMIC's theme,
wallpaper and shortcut settings. On first login it only fills in settings you
haven't customised (your COSMIC theme and wallpaper are kept). The Super key
still opens COSMIC's own launcher when you are in COSMIC.

## Checklist

Mark each item ✅ / ❌. For ❌ note what happened instead.

### Taskbar
- [ ] Taskbar appears at the bottom of every monitor; icons are centred
- [ ] Clock shows the right time and date in your format (12/24 h)
- [ ] Pinned apps (file manager, browser, store if installed) open on click
- [ ] An open app shows a short dash under its icon; the focused one a longer blue pill
- [ ] Clicking the focused app's icon minimises it; clicking again restores it
- [ ] With two windows of one app, clicking its icon shows a list of both
- [ ] Middle-click an icon opens a new window of that app
- [ ] Right-click an icon: app actions (e.g. Firefox "New Private Window"), Pin/Unpin, Close
- [ ] Pin an app, log out and back in: it is still pinned
- [ ] Clicking the clock opens the notification centre with a calendar; arrows change month; clicking the clock again closes it
- [ ] The thin strip at the far right hides all windows; clicking again brings them back
- [ ] Drag a pinned icon left/right to reorder it; the order is kept after logging out
- [ ] Right-click empty taskbar space: alignment, Search box and desktop button options work (and Task Manager if one is installed)

### System tray
- [ ] Apps with tray icons (Discord, Steam, Nextcloud, …) appear under the ^ button left of the clock
- [ ] Right-click a tray icon → "Show on taskbar" moves it next to the clock; "Hide in overflow menu" moves it back
- [ ] Left-click opens/activates the app
- [ ] Right-click shows the app's menu; submenus open; items work

### Quick settings
- [ ] Network, volume and (on laptops) battery icons show left of the clock and match reality
- [ ] Clicking them opens Quick settings bottom-right; clicking outside or Esc closes it
- [ ] Wi-Fi tile turns Wi-Fi off/on; the arrow opens the network list
- [ ] Connect to a saved network, an open one, and one with a password (type it, Enter); Disconnect works
- [ ] Bluetooth tile turns Bluetooth off/on; its arrow lists paired devices; clicking one connects/disconnects
- [ ] Airplane mode turns Wi-Fi and Bluetooth off; turning it off brings them back
- [ ] Energy saver (if power-profiles-daemon is installed) toggles the power mode
- [ ] Brightness slider changes screen brightness (laptops)
- [ ] Volume slider and the speaker (mute) button work; the arrow (when you have several outputs) switches output, e.g. to Bluetooth headphones
- [ ] Scrolling over the taskbar icons changes the volume
- [ ] Battery shows % and time remaining; the gear opens Settings
- [ ] Opening Quick settings closes the notification centre and vice versa

### Start menu
- [ ] Start button and Search box both open it; clicking outside it (desktop or a window) or Esc closes it
- [ ] Typing searches immediately; Enter opens the top result
- [ ] Try: `firefox`, `browser`, `files`, part of a name, initials (`vsc`)
- [ ] Arrow keys move the highlight
- [ ] "All" shows every app A–Z; Back returns
- [ ] Recommended shows recently opened files (open a document in an app first)
- [ ] Right-click an app: Pin/Unpin to Start, Pin/Unpin to taskbar
- [ ] Type part of a file or folder name from Documents/Downloads/Pictures: it shows under Documents/Folders; Open and "Open file location" work
- [ ] Type `wifi`, `display`, `dark mode`: the matching Settings page is offered and opens
- [ ] Any search ends with "Search the web", which opens your browser
- [ ] Esc clears the search and you can type a new one straight away
- [ ] Right-click an app you installed (pacman or Flatpak) → Uninstall → confirm: an admin prompt appears for pacman apps; a notification says it was uninstalled; it disappears from Start
- [ ] Install an app (e.g. `flatpak install` or from the store), open Start: it's listed
- [ ] Your name (and photo, if set) appear at the bottom
- [ ] Power menu: Lock, Sign out, Sleep, Restart, Shut down each do what they say

### Notifications
- [ ] `notify-send "Hello" "Test"` shows a toast bottom-right that disappears after ~5 s
- [ ] Hovering a toast keeps it open
- [ ] `notify-send -u critical "Stays" "until closed"` stays until you click ×
- [ ] Notifications from real apps (browser, chat) look right and their buttons work
- [ ] A bell with a number appears next to the clock after notifications arrive; opening the centre clears the number
- [ ] The centre lists notifications grouped by app with "Just now"/"5m ago"; × removes one, × on the app header removes the group, "Clear all" empties it
- [ ] Clicking a notification from a real app (e.g. a chat message) opens that app
- [ ] Do not disturb switch: new notifications no longer pop up (they still appear in the centre); the bell next to the clock is crossed out; it stays on after logging out and in
- [ ] The arrow next to the date collapses/expands the calendar; the choice is remembered
- [ ] Clicking anywhere outside the centre (or Esc) closes it

### Look and feel
- [ ] Interface text uses the Inter font
- [ ] Blue accent, rounded corners, Lapin UX wallpaper (first login only)
- [ ] Light/dark switching (if you change it) updates the taskbar and Start

### Keyboard (Stage B only)
- [ ] Super opens/closes Start
- [ ] Super+E file manager · Super+D show desktop · Super+L lock
- [ ] Super+↑ maximise · Super+↓ minimise · Super+Shift+S screenshot
- [ ] Super+X opens the power-user menu above Start; arrow keys + Enter work; "Shut down or sign out" shows the power options
- [ ] Super+N opens/closes the notification centre
- [ ] Super+A opens/closes Quick settings
- [ ] Super+R opens Run: try `firefox`, `~/Documents`, `https://example.com`
- [ ] Super+1, Super+2… open or switch to the taskbar apps in order; pressing again on an app with several windows cycles them
- [ ] Alt+Tab switches windows · Ctrl+Alt+Del shows the sign-out/power dialog

### Session
- [ ] Apps set to start at login (autostart) start
- [ ] Sign out returns to the login screen cleanly
- [ ] Laptop: closing the lid / idle locks the screen; the lock screen accepts your password

## Reporting a problem

1. Note the checklist item and what you saw.
2. Take a screenshot if it's visual (Super+Shift+S).
3. Attach the log:
   - Stage A: `lapin-test.log` in the repo folder
   - Stage B: `~/.local/state/lapin-ux/session.log`
     (the previous session's log is `session.previous.log`)
4. Open an issue on GitHub using the "Bug report" template, or paste it to me.
