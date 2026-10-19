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
tools/install-user-assets.sh    # optional: Selawik + Lapin UX/Win11 icons in ~/.local/share (undo: --remove)
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
- [ ] Hovering an open app's icon shows a live thumbnail of each of its windows after a moment; clicking one switches to it, × closes it; moving away hides it
- [ ] With two windows of one app, clicking its icon shows both thumbnails
- [ ] Middle-click an icon opens a new window of that app
- [ ] Right-click an icon: app actions (e.g. Firefox "New Private Window"), Pin/Unpin, Close
- [ ] Pin an app, log out and back in: it is still pinned
- [ ] Clicking the clock opens the notification centre with a calendar; arrows change month; clicking the clock again closes it
- [ ] The thin strip at the far right hides all windows; clicking again brings them back
- [ ] Drag a pinned icon left/right to reorder it; the order is kept after logging out
- [ ] Right-click empty taskbar space: alignment, Search box and desktop button options work (and Task Manager if one is installed)
- [ ] Right-click an app you've opened files with (e.g. a text editor): its recent files are listed at the top and open in that app
- [ ] Right-click an open app → End task: it closes even if it's frozen
- [ ] Apps with badges (Thunderbird, Telegram, Discord…) show a count; a download in Firefox shows a green progress bar
- [ ] Drag a file from the file manager over a running app's button and wait: that app comes forward so you can drop it in
- [ ] With two monitors: each taskbar shows its own windows per "Show taskbar apps on" in the taskbar's right-click menu
- [ ] (with lapin-ux-comp) An app on another desktop that wants attention (e.g. a chat message) lights up its taskbar button instead of switching desktops
- [ ] "Automatically hide the taskbar": it disappears and maximized windows grow to the bottom; pointer to the bottom edge shows it; it stays while you use it or Start is open; it hides again shortly after you move away

### System tray
- [ ] Apps with tray icons (Discord, Steam, Nextcloud, …) appear under the ^ button left of the clock
- [ ] Right-click a tray icon → "Show on taskbar" moves it next to the clock; "Hide in overflow menu" moves it back
- [ ] Left-click opens/activates the app
- [ ] Right-click shows the app's menu; submenus open; items work

### Desktop
- [ ] Files and folders in ~/Desktop show as icons top-left; pictures show a thumbnail
- [ ] Click selects; double-click opens (folders in the file manager, documents in their app)
- [ ] Drag an icon somewhere else: it stays there after logging out and in
- [ ] F2 (or right-click → Rename): type a name, Enter. Clicking elsewhere also keeps the name; Esc cancels
- [ ] Delete (or right-click → Delete) moves it to the Trash; you can restore it from the file manager's Trash
- [ ] Right-click empty space → New → Folder: a folder appears where you clicked, ready to be named
- [ ] View → Small/Medium/Large icons and Show desktop icons work; Sort by rearranges
- [ ] Display settings, Personalize, Open in Terminal open the right thing
- [ ] Copy a `.desktop` file of an installed app to the desktop: it shows the app's icon and starts it
- [ ] A downloaded `.desktop` file doesn't run until you choose "Allow launching"
- [ ] Files added to ~/Desktop by other apps appear within a couple of seconds
- [ ] Recycle Bin: delete a file, the bin looks full; Open shows it in the file manager; Empty Recycle Bin asks first, then empties it
- [ ] Ctrl-click, Shift-click and dragging a box on empty space select several icons; dragging one of them moves them all
- [ ] Ctrl+C a file, Ctrl+V: "name - Copy" appears where you last clicked; Ctrl+X then paste in the file manager moves it there; copying in the file manager and Paste on the desktop works too
- [ ] Drag a desktop file into an app (e.g. an email or chat): it's attached; drag a file from the file manager onto the desktop: it's copied/moved there; drag a link from the browser: a shortcut appears
- [ ] Drop files onto a desktop folder or the Recycle Bin

### Clipboard and emoji
- [ ] Copy a few texts and a picture (e.g. a screenshot) in apps, press Super+V: they're listed, newest first
- [ ] Click an older item: it's pasted into the app you were typing in
- [ ] Pin an item, log out and in: it's still there; Clear all keeps pinned items
- [ ] A password copied from a password manager (KeePassXC etc.) does not appear
- [ ] Super+. in a text field: type "heart", click one: it's inserted; it shows under "Most recently used" next time
- [ ] Kaomoji and Symbols tabs insert what you click

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
- [ ] Play music or a video: the media card shows it; previous / pause / next work
- [ ] Volume arrow → Volume mixer: each app playing sound has its own slider and mute
- [ ] Bluetooth → Add device: put a device in pairing mode; it appears; click it; confirm the PIN (or type it); it connects
- [ ] Accessibility tile: Magnifier zooms in, Color filters turn the screen grey (and Screen reader with Orca installed)
- [ ] Mobile hotspot: turn it on, connect a phone with the name and password shown
- [ ] VPN tile (with a VPN set up in Settings) connects and disconnects
- [ ] With two keyboard layouts set up, the Keyboard layout tile switches between them

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
- [ ] Drag a pinned app onto another: a folder appears; open it, rename it, right-click an app → Move out of folder
- [ ] Drag pinned apps to reorder them; drop a file or a link from the browser onto the pinned grid to pin it
- [ ] With LocalSearch installed, search for a word inside a document: the document is found
- [ ] Click your name: Change account picture lets you pick a photo and it shows up; Switch user goes to the login screen (if your login screen supports it)

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
- [ ] Events from your calendar (Evolution/GNOME Calendar local calendars, or an .ics link added in the centre's settings) show under the calendar for the selected day
- [ ] Start a 5-minute Focus session: Do not disturb turns on, the timer counts down, and a notice appears when it ends
- [ ] Settings (gear): a Do not disturb schedule covering now turns it on; turning an app's Notifications off stops its notifications; Show banners off keeps them in the centre only

### Look and feel
- [ ] Interface text uses the Selawik font (Inter if Selawik isn't installed)
- [ ] Apps, folders and files use the Win11-style icons; tray, quick settings and menu glyphs are Fluent outline icons
- [ ] Switching light/dark switches the icon theme between Lapin-UX and Lapin-UX-dark
- [ ] An icon theme you picked yourself in COSMIC Settings is kept
- [ ] Blue accent, rounded corners, Lapin UX wallpaper (first login only)
- [ ] Light/dark switching (if you change it) updates the taskbar and Start

### Keyboard (Stage B only)
- [ ] Super opens/closes Start
- [ ] Super+E file manager · Super+D show desktop · Super+L lock
- [ ] Super+↑ maximise · Super+↓ minimise · Super+Shift+S screenshot
- [ ] Super+X opens the power-user menu above Start; arrow keys + Enter work; "Shut down or sign out" shows the power options
- [ ] Super+N opens/closes the notification centre
- [ ] Super+A opens/closes Quick settings
- [ ] Super+V opens clipboard history, Super+. the emoji panel
- [ ] Super+R opens Run: try `firefox`, `~/Documents`, `https://example.com`
- [ ] Super+1, Super+2… open or switch to the taskbar apps in order; pressing again on an app with several windows cycles them
- [ ] Alt+Tab: hold Alt, press Tab repeatedly: thumbnails of all windows, most recent first; releasing Alt switches to the highlighted one (and you can type in it right away)
- [ ] A quick Alt+Tab tap flips between the last two windows; Alt+Shift+Tab goes backwards; Esc cancels
- [ ] Ctrl+Alt+Del shows the sign-out/power dialog
- [ ] Super+← / Super+→ snap a window to the left/right half; Super+Shift+← / → move it to the other monitor
- [ ] Super+Tab (or the Task view button) shows Task view; click a window to switch; New desktop adds one; right-click a window → Move to Desktop 2; Ctrl+Super+← / → switch desktops
- [ ] (with lapin-ux-comp) Super+Z: pick the left third of the thirds layout: the window goes there and Snap assist offers your other windows for the remaining spaces

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
