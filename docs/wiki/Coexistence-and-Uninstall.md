# Coexistence and Uninstall

Requirements set on 2026-10-21 after the first real Arch run: Lapin UX must (1) never affect other installed desktops, including real COSMIC, each keeping its own settings, and (2) uninstall without harm (no blank login, nothing to reinstall). These are **standing rules for all new work**. The repo's `CLAUDE.md` is the version to keep in sync.

## Rules for contributors
1. `git pull` before starting. Never regenerate a fork patch from an old checkout: rebuild the fork branch from `packaging/lapin-ux-*/` first (`git checkout -B lapin-ux <pinned commit> && git am packaging/.../*.patch`), edit, then check `git diff packaging/` shows only your change.
2. Every Lapin look inside a COSMIC fork (OSD, OSK, file dialog, Settings, libcosmic title bars, future File Explorer/Store forks) applies only in a Lapin session; COSMIC's own path stays intact.
3. Any new write to a shared COSMIC (`com.system76.*`) or GTK/dconf setting must be added to `items()` in `tools/lapin-ux-uninstall`. New packages replacing a COSMIC package go in `LAPIN_PKGS`/`STOCK` there.
4. Selawik is never enabled system-wide (session-only `FONTCONFIG_FILE`).
5. Run `bash tools/test-uninstall/run.sh "$PWD/tools/lapin-ux-uninstall"` (55 checks) and `shellcheck` before pushing.
6. Real-hardware bugs are GitHub issues with the fix/comment/label workflow.

## How it works
- **Per-session swap**: `lapin-ux-desktop-settings enter|leave` swaps GTK font/icon/cursor settings, COSMIC interface font and icon theme, dark mode, wallpaper, shortcuts, activation policy and theme folders. Copies live in `~/.local/state/lapin-ux/{other-desktops,lapin-desktop}`.
- **Safety nets**: a user unit and an XDG autostart entry run `leave` at every login unless a Lapin session is running, so a crash never leaves another desktop with Lapin's look.
- **Fork gating**: osd, osk, file dialog, Settings and title bars check the session (or a `LAPIN_UX_*` force switch).
- **Uninstaller** (`lapin-ux-uninstall`, `--dry-run`, `--settings-only`): restores settings, removes Lapin config/state, restores stock COSMIC parts, always keeps a working login (stock cosmic-greeter), checks `pacman -Si` first, and removes Lapin packages. Orphans are removed only when COSMIC isn't installed.
- **Plain `pacman -R`**: package hooks restore settings per account; the greeter package switches to another display manager (sddm, gdm, lightdm...) or greetd with `agreety` so the user is never stuck.

## History (2026-10-21)
2ca65a3 build fixes, eb0a9d9 uninstaller, 75d1a82 never restore Lapin's pointer, d49e8af session-only Selawik, 8234996 user unit, addaa76 git `pkgver()`, 4dad559 per-session copies and gating (52 checks), 3e0a85c `CLAUDE.md`.

## Not verified yet
Real Arch run of the uninstaller and hooks; gated forks on screen; greetd's handling of the agreety fallback.
