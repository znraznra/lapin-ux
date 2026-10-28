# COSMIC Upgrades

Lapin UX moved from COSMIC epoch-1.10.0 to **epoch-1.11.0** on 2026-10-21 (commits b8f9869, 269b1e3), after a dry run applying every patch in order with `git am --3way`.

## Dry-run result
| Fork | Old pin | 1.11 commit | Patches |
|---|---|---|---|
| cosmic-comp | 742b800 | 8cb1802 | 0001-0003 clean |
| cosmic-osk | 3a4800a | 6b146a2 | clean |
| cosmic-osd | 9b14ee0 | 4f96e98 | clean |
| cosmic-greeter | 5b912ba | 80e8a10 | 0001-0004 clean |
| cosmic-files (dialog) | f767979 | 3cc4b80 | clean |
| xdg-desktop-portal-cosmic | 9d35e63 | f448ddf | clean |
| cosmic-settings | 58aeb65 | f1d00fc | 0001-0009 clean |
| libcosmic | 5a8bd94 / e1f2cd2 | 430f3d4 | 0001-0007 clean |

## Verification
- Settings fork builds on 1.11; Sound, Mouse, Power, Startup apps and Display pages are pixel-identical to approved shots; others differ by at most 0.13% (clock text).
- Compositor, greeter preview and touch keyboard rebuilt; lock/sign-in and touch keyboard identical to approved screens.
- Not rebuilt in the sandbox: cosmic-osd (only version/translation changes) and the portal (can't be built; gitlab.freedesktop.org is blocked).

## What 1.11 gave us
Escape no longer closes COSMIC Files/Text Editor dialogs and search boxes (including our Open/Save dialogs); input-method/virtual-keyboard loop fix (touch keyboard + fcitx5); cosmic-randr KDL escaping of monitor serials (Win+P). Lapin's taskbar volume scrolling was aligned with COSMIC's whole-step behaviour.

## Procedure for the next bump
1. Dry-run all patches on the new epoch commits.
2. Change pins in `packaging/arch/PKGBUILD`, `packaging/lapin-ux-comp/PKGBUILD` and `packaging/arch-cosmic-apps`.
3. Regenerate patches, add Lapin icon-table entries for new Settings rows.
4. Rebuild comp, greeter preview, osk; re-shoot touch keyboard and lock/sign-in.
5. Move Lapin's own crates to the new libcosmic if useful (build against an unpatched copy).

Arch's extra repo still had cosmic-settings 1.10.0 on 2026-10-21; bump to stay in step when Arch moves.
