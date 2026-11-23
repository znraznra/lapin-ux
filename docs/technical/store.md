# Store (`lapin-store`)

The Store (#77) is COSMIC Store with Lapin UX's names and layout, for Flatpak apps (Arch packages come later, see "Not yet"). It is a patch set on COSMIC Store at the epoch-1.11.0 commit (`16b1667`), in `packaging/lapin-ux-store/`. Rebuild it with the commands in that folder's `README`.

## First stage (this one)

- **Gating.** Everything Lapin is behind `lapin::enabled()` in `src/lapin.rs`: `XDG_CURRENT_DESKTOP` contains `LapinUX`, or `LAPIN_UX_STORE=1`. In COSMIC's own session the app is stock COSMIC Store, down to the window title.
- **Home and Library.** The sidebar calls the first page Home and the Installed page Library. The Updates page stays where people expect it.
- **No Applets page.** COSMIC panel applets are of no use in a Lapin session, so that page is hidden there.
- **Window title.** "Store" instead of "COSMIC Store".
- **Few call sites.** The Lapin code is one module (`src/lapin.rs`) with three call sites: the page names in `nav.rs`, the page list and the window title in `main.rs`. Strings are in `i18n/en/cosmic_store.ftl` under the `lapin-` prefix.

## Packaging

`lapin-store` is built by `packaging/arch/PKGBUILD` with its own cargo home (`cargo-store`) so Lapin's libcosmic title-bar patch applies to the Store's private libcosmic copy. It provides and replaces `cosmic-store`, and conflicts with `lapin-ux-cosmic-store` (the title-bar-only build from `packaging/arch-cosmic-apps`).

## Coexistence

The Store writes no COSMIC or GTK setting that Lapin UX adds, so nothing is new in `items()` of `tools/lapin-ux-uninstall`. `lapin-store` is in `LAPIN_PKGS` and its stock package is `cosmic-store` in `STOCK`, so when COSMIC is installed, or when something outside Lapin still needs `cosmic-store`, the uninstaller checks `pacman -Si cosmic-store` first and brings COSMIC Store back. Without either, it only removes `lapin-store`. `tools/test-uninstall/run.sh` scenario U checks both lists.

## Not yet

- Update code shared with the Update app (`lapin_ux_core::updates`). It needs a Lapin crate as a dependency of the fork, which the fork's locked `Cargo.lock` makes a change of its own.
- A Library page that joins Installed and Updates.
- Arch packages (pacman) next to Flatpak, and a Home with sections in Lapin's own design.
