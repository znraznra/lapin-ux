# Lapin UX technical reference: Session and first-run defaults

[Back to the technical reference](../TECHNICAL.md)

**Session** (`lapin-ux-session`, `start-lapin-ux`)
- Starts `lapin-ux-comp` if installed (else cosmic-comp) in normal (not kiosk) mode using the same handshake as
  cosmic-session, exports the display to systemd/D-Bus, then starts and
  supervises every component with exponential back-off restarts.
- `dev.lapinux.Session.Exit` on the session bus for Sign out.
- `start-lapin-ux` opens `session.log` on its first lines, so a failure while
  starting is recorded with the last step it reached (#100). The log is a
  convenience, never a requirement (#174): on a full disk, a read-only home or
  a broken state folder the session starts without it (the file is test-opened
  in a subshell first, since a failed redirection ends a POSIX shell), and
  every log line goes through `say`, which ignores a write error that `set -e`
  would turn into the end of the script. It waits up to 5 s
  for the user's systemd manager (greetd starts the session before it is
  ready, GDM later) and treats the `systemctl --user` calls as best effort. It
  also turns a `;`-separated `DesktopNames` that cosmic-greeter passes into
  the `:`-separated `XDG_CURRENT_DESKTOP` the session gate looks for. The
  session file itself now says `DesktopNames=LapinUX:COSMIC`, because under
  greetd the `;` form sent the sign-in back to the greeter (#100); GDM and the
  other login managers read it the same way.
- `lapin-ux-desktop-settings leave` (the login service) also asks logind
  whether one of the user's sessions has `LapinUX` as its desktop, since under
  greetd the service runs before `start-lapin-ux` has written its pid file and
  would otherwise swap the settings to the other desktops' copy as a Lapin
  session starts (the wallpaper reset, #115).
- Crash reports: every Lapin program installs `lapin_ux_core::crash`, which on
  a panic writes the message, its last ~40 handled messages (shell and
  notification service) and a backtrace to the session log and to
  `~/.local/state/lapin-ux/crash-<program>.txt`. Release builds keep line
  tables (`debug = "line-tables-only"`, PKGBUILD `!strip`) so backtraces
  name files and lines; `start-lapin-ux` sets `RUST_BACKTRACE=1` and logs
  the installed build. Writing the report never panics (#174): the crash hook
  writes to stderr with `writeln!` and ignores the result, and every program's
  `tracing_subscriber` is built with `log_internal_errors(false)`, because the
  default prints a failed log write with `eprintln!`, which panics when stderr
  is broken too, and a panic inside the hook aborts the process.
- Redraw diagnostics (#32, #34, #51): `touch ~/.local/state/lapin-ux/debug-redraw`
  (or `LAPIN_UX_DEBUG_REDRAW=1`) before signing in makes the shell log to
  `session.log` its message count every 5 s (with the busiest message kinds,
  the view build time per surface and the redraws done and skipped), every
  tracked position change,
  every pop-up/Start/catcher opened or closed (with its anchor), and which
  surface each click lands on. Delete the file to turn it off.
- Hover previews (#34): moving along the taskbar switches previews at once, but
  not within 350 ms of the last one opening (`SWITCH_GAP` in `previews.rs`):
  closing a pop-up that isn't created yet destroys nothing and leaves a stray,
  which `sweep_strays` then removes. Fewer strays means fewer "destroying a
  pop-up the shell had lost track of" lines in the log.
- Drag and drop diagnostics (#81): `touch ~/.local/state/lapin-ux/debug-dnd`
  (or `LAPIN_UX_DEBUG_DND=1`) before signing in makes the shell log libcosmic's
  drop-target lines and iced's "Dnd event" lines to `session.log` (verbose, so
  delete the file (and unset `LAPIN_UX_DEBUG_DND` if you used it), then sign
  out and in again; it applies to the
  whole session).
- Desktop drag and drop (#81) was broken by the screen reader wrapper
  (`lapin_ux_ui::a11y::group`, #44): the wrapper passed everything to its
  content except `drag_destinations`, so the desktop's drop target was never
  reported to the compositor and every drop over the desktop was a "leave".
  The wrapper now passes `drag_destinations`, `id` and `set_id` on too. Checked
  in the nested compositor: a file dragged from Explorer onto the desktop is
  moved to the Desktop folder. smithay-clipboard, under iced, also only knew
  which seat to read a drop from after a click or key press on one of the
  shell's surfaces, so the first drag right after signing in didn't land;
  `packaging/smithay-clipboard/0001` reads it from the seat that has the drag
  offer instead (applied to `cargo-lapin` by `_patch_smithay_clipboard`).
  Checked in the nested compositor with no click on the shell before the drag.
- Less work per frame (#51): every message makes iced rebuild and redraw
  every shell surface, so the shell keeps messages few. Pointer motion is a
  message only while the left button is held or at the Show desktop corner
  (`pointer_gate`); otherwise the position is stored and handled just before
  the next message. A batch of only animation frames (and position updates)
  redraws only the surfaces that move, through the hook added by
  `packaging/libcosmic-a11y/0002` (`iced::window::REDRAW_FILTER`);
  `LAPIN_UX_REDRAW_ALL=1` turns that off.
- Graphics: on Intel Arc (PCI vendor 0x8086 with device 0x56xx/0xe2xx, or the
  `xe` driver) `start-lapin-ux` sets `WGPU_BACKEND=gl` (#34);
  `LAPIN_UX_WGPU_BACKEND` overrides it.
- Repair at every start (#46): the shell puts back Lapin's icon theme,
  wallpaper, shortcuts, Super key and focus policy when they're unset or
  empty, and the per-session swap never applies or saves an empty copy.
- XDG autostart ("startup apps") through `lapin-ux-session.target`. Other
  desktops' own parts in /etc/xdg/autostart (settings daemons, polkit
  agents, panels, session-manager plumbing; rule in
  `lapin_ux_core::desktops`) are masked with `systemctl --user mask
  --runtime` for the session only, listed in
  `$XDG_RUNTIME_DIR/lapin-ux-autostart-masked` and unmasked when it ends
  (after a crash, by `lapin-ux-desktop-settings leave`). Your own
  ~/.config/autostart entries are never touched.
- Session start order (#82): after the compositor is ready the session exports
  the environment, then starts the components (shell, wallpaper…) at once. The
  masking above (it makes systemd reload, which can take seconds) and the start
  of `lapin-ux-session.target` run beside them, in that order, and each logs the
  milliseconds since the export ("other desktops' autostart masked", "session
  target started") in `session.log`.
- The power button: Lapin UX's shortcuts bind `XF86PowerOff` to
  `lapin-ux-shell power-button` (Sleep with a battery, Shut down without),
  and the shell holds a logind `handle-power-key` inhibitor while it runs.
- Start, search and Run list apps, not other desktops' own parts (same
  rule; Settings > Start > "Show apps from other desktops" shows them).
A second switch, `~/.local/state/lapin-ux/debug-wayland`, makes the shell print
every Wayland message it sends and gets (`WAYLAND_DEBUG=1`, set first thing in
`main`) to the session log, to see whether the compositor offers a drag to the
shell's surfaces at all (`wl_data_device.enter`). The shell reads it once, when
it starts: create the file, then sign out and in; after the test delete it and
sign out and in again (deleting it alone doesn't stop the running shell's
trace). Apps the shell starts don't inherit the switch. It is very chatty, so
use it for one test.

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
- Light/dark mode: Lapin UX starts light (#159). The shell (`defaults::ensure_mode`)
  and the welcome app write `is_dark = false` only when the key is missing, so a
  choice made in Settings is never overwritten. The key is swapped per session
  like the other shared settings, so another desktop keeps its own mode. An
  existing Lapin copy of `is_dark` is kept as it is, since a chosen dark mode
  can't be told from an old default.
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
| Super + R | Run dialog (shows "Can't find …" when the name is not an app, program, file, folder or address) |
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

**Boot splash to sign-in screen (#100).** Tako's rule: signing in should feel
like unlocking, with no break. When Lapin UX's sign-in screen is in use,
`lapin-ux-login` writes `/etc/systemd/system/greetd.service`, a full copy of
the installed unit (systemd can't drop an `After=` from a drop-in). `refresh`,
run by the desktop package's `post_upgrade` and, when greetd itself is
updated, by the pacman hook `lapin-ux-greetd.hook` (so the copy keeps up with
greetd's own unit), writes it for people who already use Lapin's sign-in
screen. An existing copy is refreshed even while another login manager is on,
so switching greetd back on directly with `systemctl` doesn't bring back an
old copy. The file is generated: edits made to it are replaced on the next
refresh, so local changes belong in a drop-in
(`/etc/systemd/system/greetd.service.d/*.conf`), which still applies on top. A `greetd.service` of your own in `/etc` is saved
first and comes back with `restore`. It works the way display managers such as
GDM do:
- `plymouth-quit-wait.service` is taken out of greetd's `After=`, and
  `After=plymouth-start.service systemd-logind.service`,
  `Conflicts=plymouth-quit.service` (so Plymouth isn't closed early) and
  `OnFailure=plymouth-quit.service` (still closed if greetd fails) are added.
- `ExecStartPre=plymouth quit --retain-splash` leaves the splash's last picture
  on screen until the sign-in screen draws over it.
- The boot text on tty1 is cleared and its cursor hidden (`setterm`), so the
  moment between the sign-in screen closing and a session drawing, Lapin UX's
  or another desktop's, is plain black instead of text.

`restore` removes it (and marks the sign-in screen as given back, so `refresh` leaves it alone even when the record stays for the boot animation). It can't be checked in the sandbox (no KMS or
Plymouth), so it needs Tako's hardware. Tests: scenarios L to O in
`tools/test-login/run.sh`.
