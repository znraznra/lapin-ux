# Real-Hardware Issues

Tako tests on real Arch/CachyOS (with Pantheon and real COSMIC installed next to Lapin UX). Bugs are GitHub issues: https://github.com/znraznra/lapin-ux/issues

Workflow: fix, comment what changed and what to retest, label `fixed: needs retest` (or `needs info`), reference as `Refs #N`. Tako closes after retesting.

## Round 1: issues #1-#22 (filed 2026-10-22, fixes pushed 2026-10-26)

| Issue | Problem | Cause / fix | Status |
|---|---|---|---|
| #1 | Lock screen: no password box, auth always fails | Missing `/etc/pam.d/cosmic-greeter`; stale blur box. Shipped PAM file | Closed after retest |
| #2 | OSK missing on lock screen | Locker starts cosmic-osk if not running | Closed |
| #3 | Lock screen battery icon blank | Standard `battery-level-N[-charging]` names | Closed |
| #4 / #8 | Round picture on lock and Start | Picture cut to a circle when loaded | Closed |
| #5 | Settings named "COSMIC Settings" | Renamed inside Lapin, gear icon in Lapin themes | Closed |
| #6 | Reverse scrolling | Windows wording on Mouse and Touchpad | Closed |
| #7 | Other desktops' apps in Start | `desktops` filter + Settings > Start switch | Closed |
| #9 | Night light/filters black screen on KMS | `warm` passed at KMS call sites | Closed |
| #10 | Pantheon owns power key | Logind inhibitor, autostart masked per session | Closed |
| #11 | Settings width/spacing | No 1000 px cap, no empty headings | Closed |
| #12 | See-through menus | Opaque Start/menus/dialogs | Closed |
| #13 | Start All apps slide | Slide with 220-260 ms curve | Closed |
| #14 | Get Started every login | Marker file + logged errors | Closed |
| #15 | Task Manager panel scrolls | Only the graph resizes | Closed |
| #16 | Taskbar search only opens Start | Real text field | Closed |
| #17 | Start button icon | Presets, OS logo, custom picture | Closed |
| #18 | Accessibility > Vision does nothing | Screen reader offers to install Orca (dca07a2) | Awaiting retest |
| #19 | Clock ignores 24-hour switch | Follows COSMIC `military_time` | Closed |
| #20 | Calendar grid never shows | No full-height blur; grid when expanded | Closed |
| #21 | Pointers fail | Pinned v2 commit b14dd9e, reason and Try again | Closed |
| #22 | Alt+Tab never steps | Compositor reports Alt (protocol v3) | Closed |

## Later issues
| Issue | Problem | Status |
|---|---|---|
| #23 | Other desktops get Lapin's icons | Not started |
| #24 | Pointer settings page (style, busy colour, size, download) | Not started |
| #25 | Lock screen blurred only behind the sign-in panel | Closed |
| #26 | Slim clock without seconds | Closed |
| #27 | Contrast themes turned off until real themes exist | Closed |
| #28 | X11 pop-ups over the taskbar | Not started (`_NET_WORKAREA`) |
| #29 | Shell crash "Downcast widget state" ~1 s after start | Fixed in 3e2dc63, awaiting retest |
| #30 | Blur behind sign-in panel only the first time | Fixed in 23d1e34, closed |

## Confirmed working on real hardware
Install via `./install.sh`, volume/brightness pop-ups, Open/Save dialogs (portal builds and works), touch keyboard in the session, firmware updates in Update.

## Not verified yet
Uninstaller on a real system, the admin (polkit) prompt, the arch-cosmic-apps PKGBUILD. Touch screens are untested (no device).
