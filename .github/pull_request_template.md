## What changed and why

<!-- A few plain sentences. Link the issue with "Refs #123" (not "Fixes"): bugs are closed after someone checks the fix on real hardware. -->

Refs #

## How I tried it

- [ ] Stage A (in a window)
- [ ] Stage B (installed, signed in as Lapin UX)
- [ ] Not tried on screen (docs or tooling only)

<!-- What did you click, type or run? What did you see? -->

## Screenshots

<!-- For anything people can see. Delete this section if nothing changed visually. -->

## What needs checking on real hardware

<!-- Things you can't check yourself: lock screen, multiple monitors, touchpad, graphics card, sound... Write "nothing" if there is nothing. -->

## Checklist

- [ ] `bash tools/test-uninstall/run.sh "$PWD/tools/lapin-ux-uninstall"` passes (if you touched settings, the uninstaller or the packages)
- [ ] `shellcheck -S warning tools/lapin-ux-uninstall` is quiet (if you touched it)
- [ ] A change to a COSMIC program is a patch in `packaging/lapin-ux-*/` rebuilt from the pinned commit, and `git diff packaging/` shows only my change
- [ ] Anything I added to a COSMIC program only applies inside a Lapin session
- [ ] A new write to a shared COSMIC or GTK setting is added to `items()` in `tools/lapin-ux-uninstall`
- [ ] Docs updated (README, `docs/TECHNICAL.md`, or `docs/wiki/`)
- [ ] My wording describes what the thing does, without comparing it to another operating system (see CONTRIBUTING.md)
- [ ] I read the [Code of Conduct](../CODE_OF_CONDUCT.md), and my change can be released under GPL-3.0-only
