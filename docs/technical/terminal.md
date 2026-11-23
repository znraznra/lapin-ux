# Terminal (`lapin-ux-terminal`)

Lapin Terminal is a fork of [Alacritty](https://github.com/alacritty/alacritty) (v0.17.0, pinned
commit `94e7c88`), shown to people as **Terminal**. It is the real application, not just its
engine crate, carried as a patch set in `packaging/lapin-ux-terminal/` and built by
`packaging/arch-terminal/PKGBUILD` (optional: `cd packaging/arch-terminal && makepkg -si`).

## Names
Own names everywhere, so nothing collides with a user's own `alacritty` package:

| | Lapin Terminal | Alacritty |
|---|---|---|
| Binary | `lapin-ux-terminal` | `alacritty` |
| App ID / window class | `dev.lapinux.Terminal` | `Alacritty` |
| Window title | Terminal | Alacritty |
| Config | `~/.config/dev.lapinux.Terminal/terminal.toml` | `alacritty.toml` and its fallbacks |
| IPC socket, log file | `dev.lapinux.Terminal-*` | `Alacritty-*` |
| `TERM` | `xterm-256color` | `alacritty` when its terminfo exists |

Alacritty's config locations (`~/.config/alacritty`, `~/.alacritty.toml`, `/etc/alacritty`) are
not read, so a user's own Alacritty setup is never picked up or changed. `TERM` is always
`xterm-256color`, since the package does not ship Alacritty's terminfo.

## Session gating
`alacritty/src/lapin.rs` holds the gate: `lapin::enabled()` is true when `XDG_CURRENT_DESKTOP`
contains `LapinUX`, or `LAPIN_UX_TERMINAL=1` forces it. The first patch only renames, so
the app behaves like stock Alacritty under its own names; later Lapin looks go behind that gate.

## Default terminal
`start-lapin-ux` sets `x-scheme-handler/terminal=dev.lapinux.Terminal.desktop` in the Lapin
session, only when the package is installed and no default is set (the user's own choice wins).
Explorer's "Open in Terminal" asks for that handler, so it opens Lapin Terminal there. The entry
lives in `mimeapps.list`, which `lapin-ux-desktop-settings` swaps per session (#223, PR #226),
so other desktops never see it. COSMIC's own session is unchanged and keeps COSMIC Terminal.

## Coexistence
- `lapin-ux-cosmic-term` (COSMIC Terminal with familiar title bars) stays installable: Lapin
  Terminal does not replace a COSMIC package, so it is in `LAPIN_PKGS` (removed on uninstall)
  but not in `STOCK`.
- Uninstall: `pacman -R lapin-ux-terminal` or `./uninstall.sh`; nothing outside
  `~/.config/dev.lapinux.Terminal` is written.

## Rebuilding and upstream
```sh
git -C ~/src/alacritty checkout -B lapin-ux 94e7c88
git -C ~/src/alacritty am ~/lapin-ux/packaging/lapin-ux-terminal/*.patch
```
`.github/workflows/terminal-upstream.yml` tries the patches weekly on Alacritty's latest release
and opens an issue when they no longer apply.
