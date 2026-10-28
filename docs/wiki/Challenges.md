# Challenges

Roadblocks met so far and how each was handled.

## Scale and maintainability
- **Forking big apps.** COSMIC Settings is about 35,000 lines, Files about 23,000, Store about 10,000. Rewriting is out of the question, so each is a patch series on a pinned epoch tag, with Lapin code in its own module and a few hook call sites.
- **Upstream churn.** COSMIC changes often. Pinning to epoch tags, building with upstream lockfiles and dry-running every patch on the next epoch (see [COSMIC Upgrades](COSMIC-Upgrades)) keeps rebasing cheap. 1.10 to 1.11 applied cleanly.
- **libcosmic shared checkout.** In the sandbox the shared cargo checkout of libcosmic gets patched for Settings/greeter/osk builds, but Lapin's own crates must build against an unpatched copy. Reset and rebuild in the right order.

## Proprietary Windows assets
Segoe UI, Segoe Fluent Icons, Windows sounds and pointers can't be shipped. Replaced by Selawik, Fluent UI System Icons, Fluent Emoji, Borealis sounds, and the JepriCreations pointer pack, which can't be redistributed so `lapin-ux-cursors` downloads and converts it on the user's machine.

## Testing without the hardware
The sandbox only runs the nested (winit) compositor backend. That can't reproduce KMS-only code (#9), real session locking (#1), real keyboard modifier delivery (#22) or touch hardware. These need careful code reading and Tako's retest on real machines.

## Coexistence with real COSMIC
First real-Arch test (2026-10-21) showed Lapin changing shared settings and replacing COSMIC parts. Fixed with per-session settings swap, gating every fork on the Lapin session, and a safe uninstaller. See [Coexistence and Uninstall](Coexistence-and-Uninstall).

## Toolkit limits
- iced has only basic animation support; every animation (Start slide, taskbar hop, window open) is hand-built.
- libcosmic's default theme radii produced square slider thumbs and pill buttons; fixed with opt-in card-list patches and 4 px push buttons (side effect: unforked COSMIC apps show pill buttons).
- A widget-state bug (#29): libcosmic's `RectangleTrackingContainer` forwards content state but keeps a stateless tag, so two different tracked widgets swapping places matched on tag and hit a downcast panic. Fix: always wrap tracked content in a one-child row. Rule: never pass a stateful widget straight into `tracker.container()`.

## Compositor patches
- Night light on KMS: the `warm` shader uniform was only set in the winit render path, not at the two KMS post-process call sites, so filters turned the screen black (#9).
- Alt+Tab (#22): the switcher couldn't see Alt held; the compositor now reports Alt via `lapin_shell` v3.
- Shared-edge snap-group resize would touch COSMIC's grab code, which changes upstream often; not built.

## Authentication and lock screen
Arch ships no `/etc/pam.d/cosmic-greeter`, so PAM fell back to deny-all and the lock screen never accepted a password (#1). Lapin now ships one. The sign-in layer also had stale blur geometry after output info updates.

## Platform gaps
- Orca differs from Narrator; Screen reader now offers to install Orca.
- Pantheon's autostart parts took over the power key (#10); Lapin binds it via a logind inhibitor and masks other desktops' autostart per session.
- X11 pop-ups appear over the taskbar (#28): needs `_NET_WORKAREA` in cosmic-comp's X11 WM.

## Disk and build time
Release builds are large (Settings about 16 minutes, 2.3 GB target). The sandbox disk is tight, so `target/release` and incremental caches get deleted as needed, with preview binaries kept aside.
