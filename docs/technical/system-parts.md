# Lapin UX technical reference: System parts

[Back to the technical reference](../TECHNICAL.md)

**Pop-ups and admin prompt** (`lapin-ux-osd`, a patch on COSMIC's cosmic-osd)
- Volume and brightness keys show a small rounded box 12 px above the
  taskbar: icon, thin bar, number.
- When an app needs admin rights, polkit's request shows as a clear
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

**Snipping** (Super+Shift+S or Print Screen)
- The screen freezes and dims; pick Rectangle (drag), Window or Full screen
  from the toolbar. The snip is copied to the clipboard, saved in
  Pictures/Screenshots, and a notification shows it.
- The picture is of the screen with the focused window (else the main
  display), and the overlay opens on that screen. COSMIC's own *Screenshot*
  system action also points at `lapin-ux-shell snip` (`system_actions`, swapped
  per session), so Print never falls back to `cosmic-screenshot` (#53). If
  no picture arrives within 3 s a notification says so; every press logs
  "screenshot requested".

**System tools**
- **Task Manager** (Ctrl+Shift+Esc, Ctrl+Alt+Del, Super+X): Processes (apps with
  their child processes, background processes, CPU/memory/disk, End task, Run
  new task), Performance graphs (CPU, memory, disk, network and one entry per graphics adapter, read from sysfs and DRM fdinfo), Startup apps (turn them on or off), Users
  (who is signed in, from logind, with their CPU and memory; Manage user
  accounts), Details, and Services (every systemd service with PID and status;
  Start, Stop and Restart go through polkit and the admin prompt).
- **Control Panel**: the classic category view and Large icons, opening the
  matching Settings page or tool.
  In Control Panel and Task Manager, typing with nothing focused goes into the
  search box (#96, #97): a key event no widget took is caught by a subscription,
  appended to the search text, and the box is focused. The focus asked for at
  start-up alone wasn't enough on the real session.
- **Update**: checks pacman, Flatpak and firmware (fwupd, via
  `fwupdmgr get-updates --json`) updates, installs them with one
  button (polkit), shows progress and history, and offers a restart when the
  kernel or firmware changed. A background check notifies you when updates are waiting.
- **Get Started**: on the first sign-in, connect to Wi-Fi, choose light or
  dark mode, and optionally install the extra mouse pointers.
- Desktop icons: **Properties** (Alt+Enter), *This PC*, user folder, *Network*
  and *Control Panel* icons, *Open with ›* with **Choose another app** ("How do
  you want to open this file?"), *Compress to ZIP file*, *Show more options*.
- Super+P (*Project*), Ctrl+Alt+Del security screen, Alt+F4 on the desktop opens
  *Shut Down* with your Linux's name.

**Lock screen and sign-in** (`lapin-ux-greeter`, a patch on COSMIC's greeter)
- Lock screen: big clock and date over the wallpaper; click or type to get the
  sign-in panel (round picture, name, password; network, battery,
  accessibility and power bottom right). The login screen adds the user list
  bottom left. Fingerprint sign-in works through fprintd (optional).
- The lock screen checks the password through the `cosmic-greeter` PAM
  service; `lapin-ux-greeter` ships `/etc/pam.d/cosmic-greeter` in Arch's
  style (`include system-local-login`; Arch has none, and without it every
  unlock fails). Account pictures are cut to a circle when loaded.
- Opening the sign-in panel is a 200 ms eased move (#54): the dark veil
  fades in as the panel rises 24 px into place, and Esc fades it back out
  over the clock face (greeter patch 0008, `lapin::Fade`). The compositor
  grows the blur behind it over the same 200 ms and back
  (`lapin-ux-comp` 0011, `LapinBlurFade`, lock surfaces in the Lapin
  session only).

**Typing**
- With several keyboard layouts or fcitx5 input methods, the taskbar shows
  the current one (**ENG / US**, or あ, 拼…); click to pick, **Super+Space** for the next.
- **Touch keyboard** (`lapin-ux-osk`, a patch on COSMIC's on-screen keyboard)
  with a familiar look, emoji and clipboard buttons; right-click the
  taskbar → *Show touch keyboard button*.

**Accessibility**
- **Narrator is paused** until further notice (`screen_reader::PAUSED` in the
  shell): no quick settings tile, Ctrl+Super+Enter does nothing, search
  doesn't offer it, Settings > Accessibility > Vision hides the screen reader
  rows in the Lapin session, and a running screen reader is turned off. What
  follows describes it for when it comes back.
- Quick settings → Accessibility: Magnifier, Colour filters, **Narrator**
  (Orca; Ctrl+Super+Enter). Contrast themes are turned off for now (Lapin UX
  has none of its own yet; COSMIC's high-contrast palette didn't work as
  one): no quick setting, no shortcut, hidden in Settings and on the lock
  screen; an upgrade turns a high-contrast palette off again.
- Narrator needs Orca and a voice for speech-dispatcher (`espeak-ng`, both
  optional dependencies; on Arch speech-dispatcher's voice is optional too,
  and without one Orca runs silently). Until both are there, quick settings
  leaves the Narrator switch out, and Settings → Accessibility → Vision →
  Screen reader shows **Install** (the admin prompt, then
  `pacman -S orca espeak-ng`) instead of a switch.
- One owner for Orca (#18): the AT-SPI flag `org.a11y.Status
  ScreenReaderEnabled` is the switch. Quick settings, Ctrl+Super+Enter and
  Settings only set it (and `IsEnabled`, so Chromium/Electron apps turn their
  accessibility on); the shell watches it and starts or stops Orca. at-spi
  keeps the flags in `org.gnome.desktop.a11y.applications
  screen-reader-enabled` and `org.gnome.desktop.interface
  toolkit-accessibility`, which are swapped per session like Lapin's other
  shared settings.
- What Orca reads (#44): Lapin's programs are built with libcosmic's `a11y`
  feature, and libcosmic's iced is patched (`packaging/libcosmic-a11y/`) so
  the widget tree actually reaches AT-SPI, window focus is reported and
  closed panels leave the tree. Each shell surface is a named group
  (Taskbar, Desktop, Start, Quick settings, menus…); icon-only buttons carry
  names ("Start", "Firefox - 2 running windows", "Wi-Fi, on"). Not read yet:
  text boxes (libcosmic's text input has no accessibility support), and
  keyboard focus inside a surface (Orca follows the window, not the focused
  button). Apps that were already running when Narrator came on need a
  restart to turn their own accessibility on.
