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
- [ ] Clicking the clock opens a calendar; arrows change month
- [ ] The thin strip at the far right hides all windows; clicking again brings them back

### System tray
- [ ] Apps with tray icons (Discord, Steam, Nextcloud, …) show them left of the clock
- [ ] Left-click opens/activates the app
- [ ] Right-click shows the app's menu; submenus open; items work

### Start menu
- [ ] Start button and Search box both open it; click outside or Esc closes it
- [ ] Typing searches immediately; Enter opens the top result
- [ ] Try: `firefox`, `browser`, `files`, part of a name, initials (`vsc`)
- [ ] Arrow keys move the highlight
- [ ] "All" shows every app A–Z; Back returns
- [ ] Recommended shows recently opened files (open a document in an app first)
- [ ] Right-click an app: Pin/Unpin to Start, Pin/Unpin to taskbar
- [ ] Your name (and photo, if set) appear at the bottom
- [ ] Power menu: Lock, Sign out, Sleep, Restart, Shut down each do what they say

### Notifications
- [ ] `notify-send "Hello" "Test"` shows a toast bottom-right that disappears after ~5 s
- [ ] Hovering a toast keeps it open
- [ ] `notify-send -u critical "Stays" "until closed"` stays until you click ×
- [ ] Notifications from real apps (browser, chat) look right and their buttons work

### Look and feel
- [ ] Interface text uses the Inter font
- [ ] Blue accent, rounded corners, Lapin UX wallpaper (first login only)
- [ ] Light/dark switching (if you change it) updates the taskbar and Start

### Keyboard (Stage B only)
- [ ] Super opens/closes Start
- [ ] Super+E file manager · Super+D show desktop · Super+L lock
- [ ] Super+↑ maximise · Super+↓ minimise · Super+Shift+S screenshot
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
