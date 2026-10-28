# Building and Testing

## Install (Arch)
`./install.sh` asks for the password once, keeps sudo alive during the build, and moves a hand-installed Win11 icon theme aside. One split PKGBUILD (`packaging/arch`) builds the Lapin packages. Uninstall with `./uninstall.sh` (see [Coexistence and Uninstall](Coexistence-and-Uninstall)).

## Building
- Lapin: `cargo build` (debug; the test harness uses `target/debug`).
- Forks: `RUSTUP_TOOLCHAIN=stable cargo build --release` (remove their rust-toolchain pins; Settings needs `--locked`, ~16 min first time).
- Fork type-checks need system libs: libxkbcommon-dev, libwayland-dev, libpulse-dev, libpipewire-0.3-dev, libudev-dev, libinput-dev, libdbus-1-dev, clang.
- Fork patch workflow: rebuild the fork branch from `packaging/` first, edit, `git format-patch` into `packaging/`, confirm `git diff packaging/` shows only your change.

## Tests
- `lapin-ux-core`: 99 unit tests.
- Settings: `cargo test --release --locked -p cosmic-settings --bin cosmic-settings lapin` (13 passed).
- Uninstaller: `bash tools/test-uninstall/run.sh "$PWD/tools/lapin-ux-uninstall"` (55 checks, needs dconf and dbus-run-session; run as a normal user) plus `shellcheck`.

## Sandbox test harness
Run `dbus-run-session -- bash run.sh bash b9.sh`. Helpers: `S name [wait]` screenshot, `C x y wait [right]` click, `SH <subcommand>`, `W` wtype, `F app-id "Title"` foot window. Screenshots are 1600x900 with the nested screen at offset (160,50), 1280x800. `mocksys` mocks logind and systemd; `fakebin` stands in for checkupdates, flatpak, fwupdmgr, pkexec. `LAPIN_UX_ANIM_SCALE=10` slows animations for mid-frame captures.

## Previewing outside a Lapin session
Force switches: `LAPIN_OSD_PREVIEW=volume|brightness|admin`, `LAPIN_UX_OSK=1`, `LAPIN_UX_DIALOG=1`, `LAPIN_UX_SETTINGS=1`, `LAPIN_UX_TITLEBAR=1`. Lock/sign-in faces: cosmic-greeter's `examples/lapin_preview [signin]`. Settings pages: `cosmic-settings lapin <page-id>`.

## What the sandbox can't test
KMS-only paths, real locking, real modifier delivery and touch hardware. Those are retested by Tako on real Arch/CachyOS.

## Documentation upkeep
After each piece of work update the README (plain-words pitch, progress table, tour, roadmap), `docs/TECHNICAL.md`, and this wiki. Tour previews always come from approved gallery screens.
