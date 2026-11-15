# Lapin UX technical reference: Settings

[Back to the technical reference](../TECHNICAL.md)

**Settings** (`lapin-ux-settings`, COSMIC Settings at epoch-1.11.0 with
patches in `packaging/lapin-ux-settings`; in progress, all 5 stages done)
- Familiar layout: account card and colour category icons in the sidebar
  (Home, System, Bluetooth & devices, Network & internet, Personalization,
  Apps, Accounts, Time & language, Accessibility, Privacy & security,
  Update), Find a setting centred in the title bar, back arrow, breadcrumb
  page titles, category pages of rows, Home, and the System page header
  (wallpaper, PC name and model, Rename, Update's last check).
- COSMIC's pages are re-parented under the usual categories at start-up
  (with their usual names), not edited; old links (`cosmic-settings system`…)
  open the matching category.
- Inside pages: every setting its own card with an icon chosen from its
  title, grey descriptions, On/Off beside switches, tidy drop-downs,
  round slider thumbs, 4 px buttons; Power mode and the primary mouse button
  as drop-downs; Add buttons inside rows; About laid out as a name card with
  Device and OS specification cards (Copy, links), then an **About Lapin
  UX** card (#63): version and build from `/usr/share/lapin-ux/version`
  (the package's `pkgver`, its `.gHASH` links to the commit), the GPL-3.0
  licence, credits read from the table in `/usr/share/lapin-ux/CREDITS.md`
  (plus COSMIC, and the extra pointers when installed), the independence
  notice and project links. Search finds it as About Lapin UX, Credits and
  License.
- Page layout (`lapin/settings.rs`): Personalization > Taskbar and
  Start, System > Notifications, Display > Night light and System >
  Multitasking are Lapin pages that read and write the shell's
  `dev.lapinux.Shell` and the notification service's
  `dev.lapinux.Notifications` cosmic-config (both watch them, so changes
  apply at once; Do not disturb goes over the notification service's
  D-Bus property). Panel and Dock are hidden; COSMIC-only pages (Window
  management, Workspaces, X11 compatibility) sit under an "Advanced"
  heading. Notifications lists apps the service has seen (`seen_apps`).
- Pages COSMIC doesn't have (stage 4): Apps > Installed apps (`lapin/apps.rs`:
  launchers + `pacman -Qo`/`-Qi` and `flatpak list` for version, size, date;
  Uninstall via `pkexec pacman -Rs` or `flatpak uninstall`, desktop packages
  protected), System > Storage (`lapin/storage.rs`: `df` + `du` per
  category; Temporary files removes Recycle Bin, thumbnails, app caches,
  Downloads, and old packages with `pkexec paccache -rk0`), System >
  Clipboard (shell keys `no_clipboard_history`, action `clipboard-clear`),
  Personalization > Lock screen (`lock_wallpaper`, `no_signin_picture`, read
  by the greeter patch 0002; greeter patch 0003 draws the lock and sign-in
  screens in the system font by name (Selawik unless one is picked on
  Fonts: patch 0008 reads `system_font` from `dev.lapinux.Shell`, the
  user's own on the lock screen and the last user's on the sign-in screen,
  #62), since the sign-in screen's own user has no font setting; patch 0004 gives the sign-in screen rounded corner
  buttons — accessibility, keyboard layout, a gear to pick the desktop
  session (every installed Wayland/X11 session, so you can switch back to
  KDE, GNOME…) and power, each with a flyout), Accounts > Sign-in options (`lapin/signin.rs`: facial
  recognition with Howdy through `tools/lapin-ux-howdy` — camera choice,
  `howdy -U user -y add/clear`, and Howdy's PAM line added first to
  /etc/pam.d/greetd and cosmic-greeter as `sufficient`, so the password
  still works — run with pkexec under the dev.lapinux.howdy polkit action;
  fprintd enrolment/removal; password + hint through AccountsService),
  Privacy & security (`lapin/privacy.rs`: device encryption from lsblk;
  Recent activity; per-app Location/Camera/Microphone/Screenshot/Background
  switches in the portal permission store), and Update (`lapin/update.rs`,
  same checks/installs as lapin-ux-update, plus Update history from
  pacman.log). `lapin-ux-update` without arguments now opens
  `cosmic-settings lapin lapin-update` (`LAPIN_UX_UPDATE_WINDOW=1` keeps its
  own window); any Lapin page opens with `cosmic-settings lapin <page-id>`.
- Stage 5: "Find a setting" (`lapin/search.rs`) shows up to 8 suggestions
  in a list under the box as you type (page names and descriptions,
  COSMIC's section titles and multi-word lines, Lapin's own setting names)
  and a "Search results" page on Enter or "Show all results"; COSMIC's
  in-page search isn't used in Lapin mode. Everything that opens Settings
  goes through `lapin_ux_core::system::settings_command(page)`: Lapin pages
  (`lapin-…`) open with `cosmic-settings lapin <id>`, falling back to the
  nearest COSMIC page when COSMIC's own Settings is installed. Start search,
  Control Panel, Super+X (Installed apps, Power Options, System, Network
  Connections, Settings), the taskbar menu's "Taskbar settings", desktop
  Display settings / Personalize, quick settings links, the notification
  centre's "More notification settings" and Super+I (Settings Home) all open
  the matching page.
- All Lapin code is in `cosmic-settings/src/lapin/`; COSMIC's files get small
  hooks. The look inside pages comes from opt-in libcosmic patches
  (`packaging/lapin-ux-libcosmic` 0002–0007), turned on only by Lapin
  Settings. The layout switches on only in a Lapin UX session
  (`XDG_CURRENT_DESKTOP` contains `LapinUX`, or `LAPIN_UX_SETTINGS=1`).
- Start-up (#80): in the Lapin session `init` returns at once and the window
  shows "Loading…"; a 20 ms timer then sends `Message::Boot`, which builds the
  pages (`build_pages`) and opens the page asked for on the command line, or Home. Until then `update` ignores
  everything but `Boot`, and `subscription`, `dialog` and `on_app_exit` do
  nothing. COSMIC's own session builds the pages in `init` as before.
