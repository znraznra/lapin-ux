# Lapin UX: rules for anyone (or any Claude session) working in this repo

Project context lives in the claude.ai Project docs (`claude/lapin-overview.md`
first). This file holds the rules that keep already-shipped fixes from being
undone. They apply to every change.

## Before touching anything

1. `git pull` first. Other sessions push to `main` too.
2. The patch files in `packaging/lapin-ux-*/` are the source of truth for every
   COSMIC fork, not any fork checkout lying around in a sandbox. Before editing
   a fork, rebuild its branch from the patches:
   ```sh
   git -C ~/src/cosmic-osd checkout -B lapin-ux 4f96e98   # pinned commit from packaging/arch/PKGBUILD
   git -C ~/src/cosmic-osd am ~/lapin-ux/packaging/lapin-ux-osd/*.patch
   ```
   Same for cosmic-osk (6b146a2), cosmic-files (3cc4b80, patch in
   `packaging/lapin-ux-portal/0001-*`), greeter, comp, settings, libcosmic.
   Regenerating patches (`git format-patch`) from an older checkout silently
   throws away later fixes. After regenerating, `git diff packaging/` must only
   show the change you meant to make.

## Things that must stay true (shipped 2026-10-21, commits 2ca65a3..4dad559)

**Lapin UX never changes other desktops.**
- Selawik/Fluent Emoji only inside the Lapin session: the fontconfig rule is
  NOT enabled system-wide. `start-lapin-ux` sets
  `FONTCONFIG_FILE=/usr/share/lapin-ux/fonts.conf`. Never add a
  `/etc/fonts/conf.d` or `conf.default` link back.
- Settings Lapin shares with other desktops are swapped per session by
  `tools/lapin-ux-uninstall` (also installed as `lapin-ux-desktop-settings
  enter|leave`): GTK font/document font/icons/pointer (dconf), COSMIC font,
  icons, light/dark mode, wallpaper, the four theme folders, keyboard
  shortcuts, Super key, focus policy, the default X pointer.
  **Any new code that writes a COSMIC (`~/.config/cosmic/com.system76.*`) or
  GTK/dconf setting must add that setting to `items()` in
  `tools/lapin-ux-uninstall`** (with a `key_is_lapins` test if Lapin's value
  is recognisable). Lapin's own settings go in `dev.lapinux.*` and need
  nothing.
- Every Lapin look inside a COSMIC fork is gated on the Lapin session
  (`XDG_CURRENT_DESKTOP` contains `LapinUX`): OSD pop-ups + admin prompt
  (`lapin::enabled()`, `LAPIN_UX_OSD=1`), touch keyboard (`lapin()`,
  `LAPIN_UX_OSK=1`), file dialogs (`config::lapin_session()`,
  `LAPIN_UX_DIALOG=1`), Settings (`LAPIN_UX_SETTINGS=1`), libcosmic title bars
  (`LAPIN_UX_TITLEBAR=1`). COSMIC's own code path must stay intact in the
  `else` branch. **New forks (File Explorer, Store…) follow the same rule**:
  in COSMIC's own session they behave exactly like stock COSMIC.

**Uninstalling is clean and never leaves the user stuck.**
- `lapin-ux-uninstall` / `./uninstall.sh`: restore point
  (`--snapshot`, taken by `start-lapin-ux` and `lapin-ux-cursors` before
  Lapin first changes anything), per-session copies, Howdy PAM line removed,
  stock COSMIC parts reinstalled (always `cosmic-greeter` when greetd uses it;
  every replaced part when COSMIC is installed), checked with `pacman -Si`
  before anything changes. **New Lapin packages that replace a COSMIC package
  go in `LAPIN_PKGS` and `STOCK` there.**
- Plain `pacman -R` is covered: `lapin-ux.install` `pre_remove` restores
  every account's settings; `lapin-ux-greeter.install` `post_remove` keeps a
  working login (another login manager, or a text login).
- `lapin-ux-desktop-settings.service` (user unit, default.target +
  graphical-session.target) and `/etc/xdg/autostart/lapin-ux-other-desktops.desktop`
  hand settings back after a crash; `$XDG_RUNTIME_DIR/lapin-ux-session.pid`
  keeps them away from a Lapin session that's starting.

**Building.**
- `pkgver()` comes from git, so `git pull && ./install.sh` always installs the
  new build. The root `Cargo.toml` excludes `packaging/` from the workspace;
  `prepare()` runs `git reset --hard && git clean -fdq` before patching.

## Checks before pushing

- `bash tools/test-uninstall/run.sh "$PWD/tools/lapin-ux-uninstall"` (needs
  dconf): all checks must pass. Add a scenario for any new shared setting.
- `shellcheck -S warning tools/lapin-ux-uninstall`.
- Forks you touched: `cargo check --locked` (or a build) at the pinned commit
  with all patches applied.
