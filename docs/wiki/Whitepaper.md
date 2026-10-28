# Lapin UX Whitepaper

*A Windows 11-familiar desktop for Linux, built by patching COSMIC rather than replacing it.*

Version of 2026-10-28, build `dca07a2`.

## Abstract

Many people who want to leave Windows are put off by the unfamiliarity of Linux desktops, the need for plugins and extensions to get a familiar layout, and frequent trips to the terminal. Lapin UX is a desktop environment written in Rust that reproduces Windows 11's behaviour, layout and look on Linux, with no plugins and as little command-line use as possible. It is built with libcosmic/iced on the COSMIC compositor and targets Arch Linux first. This paper describes the goals, the engineering approach (own shell plus small patches against pinned COSMIC releases), the safeguards that let it coexist with other desktops, the testing and review method, what has been delivered, and what remains.

## 1. Motivation

Windows 11's interface is the most widely learned desktop. Linux desktops offer strong technical foundations but ask newcomers to relearn basic habits (where the taskbar, Start menu, Settings and Task Manager are, how Alt+Tab or Win+X behave). Existing "Windows-like" Linux themes usually restyle another desktop with extensions or plugins, which break on updates and rarely match behaviour.

Lapin UX takes the opposite approach: match Windows 11's behaviour first, then its look, and do it as a real desktop environment, not a theme.

**Constraints**
- No Microsoft logos, Windows artwork or proprietary fonts. Open look-alikes only (Selawik, Fluent UI System Icons, Fluent Emoji, Borealis sounds).
- No plugins; as little command-line use as possible (one-step install, graphical uninstall paths, Settings pages instead of config files).
- Must never harm other desktops installed alongside it, and must uninstall cleanly.

## 2. Approach

### 2.1 Own shell, COSMIC underneath
COSMIC provides a modern Rust/Wayland compositor (cosmic-comp), a toolkit (libcosmic on iced) and services (settings daemon, background, idle). Lapin UX reuses them and adds:
- **lapin-ux-shell**: taskbar, Start, tray, quick settings, desktop icons, Task view, snapping, snipping, dialogs.
- **lapin-ux-notifyd**: notification server and centre.
- **lapin-ux-session**: starts and supervises everything.
- Own apps: Task Manager, Control Panel, Update, Get Started.
- **lapin-ux-core**: UI-free logic with unit tests, so most behaviour can be tested without a screen.

### 2.2 Forks as patch series
Large COSMIC programs (Settings about 35,000 lines, Files about 23,000, Store about 10,000) are not rewritten. Each is a patch series against a pinned COSMIC epoch commit, with Lapin code in its own module and a handful of call sites. Applied forks: cosmic-comp, greeter, OSK, OSD and admin prompt, Settings, Open/Save portal and Files dialog, and libcosmic itself.

Benefits: upstream bug fixes arrive with each epoch bump; patches can be dry-run before moving; the diff stays reviewable. The move from epoch-1.10.0 to 1.11.0 applied every patch cleanly.

### 2.3 Own compositor protocol
Features COSMIC doesn't expose (snap layouts, attention flashing, desktop peek, night light, touchpad gestures, window animations, Alt key state for Alt+Tab) go through a small `lapin_shell` Wayland protocol (v1-v3) implemented in five compositor patches. This keeps compositor changes minimal and optional.

### 2.4 Session gating
Every Lapin look inside a fork applies only in a Lapin session (`XDG_CURRENT_DESKTOP` contains `LapinUX`). COSMIC's own code path remains in the else branch, so a user running real COSMIC from the same install sees no difference.

## 3. Coexistence and clean removal

The first real Arch test showed that sharing a machine with COSMIC is the hardest part of "being a good guest". The resulting design:
- **Per-session settings swap.** Lapin and other desktops each keep their own copy of shared settings (fonts, icons, cursors, dark mode, wallpaper, shortcuts, theme). Entering and leaving a session swaps copies. Safety-net units run "leave" at every login, so a crash cannot leave another desktop looking like Lapin.
- **Fonts stay local.** Selawik is enabled only through a session-level `FONTCONFIG_FILE`, never system-wide.
- **Uninstaller as a first-class tool.** A restore point is taken; uninstall restores settings, brings back stock COSMIC parts, always leaves a working login, and checks package availability first. Plain `pacman -R` also leaves the system usable through package hooks. Fifty-five automated checks cover it.
- **Standing rules.** New writes to shared settings must be registered in the uninstaller; new forks must be gated; these are recorded in the repo's `CLAUDE.md`.

## 4. Engineering and verification method

- **Sandbox testing.** A nested-compositor test harness with mock system services (logind, systemd, fprintd, permission store) and stand-in tools takes reproducible screenshots of every screen, including lock and sign-in previews.
- **Review gallery.** Each visible change is shown in a numbered screenshot gallery; the owner marks screens right or change; revisions come back as new rounds. Over 290 screens have been reviewed in 22 rounds.
- **Real-hardware loop.** Behaviour the sandbox cannot show (KMS rendering, real locking, real modifier delivery, touch) is retested on real Arch and CachyOS machines through GitHub issues with a fixed workflow: fix, comment what to retest, label, owner closes.
- **Crash reporting.** The shell installs a panic hook that records a backtrace and its recent messages, which let an otherwise unreproducible start-up crash (#29) be traced to a widget-state mismatch.
- **Documentation upkeep.** README, technical doc, roadmap and this wiki are updated after every piece of work.

## 5. Results to date

**Delivered (2026-10-28):**
- A complete shell: taskbar, Start with search and preview, tray, notification centre, quick settings, Task view, snap layouts and groups, clipboard and emoji, Run, Win+X, Ctrl+Alt+Del, snipping.
- A full Settings app with Windows 11's layout, forked from COSMIC Settings through five reviewed stages, plus Lapin pages (Taskbar, Start, Storage, Installed apps, Sign-in options, Privacy, Update).
- Lock and sign-in screens, touch keyboard, volume/brightness pop-ups, admin prompt, Open/Save dialogs, Update (pacman, Flatpak, firmware), Task Manager with Users and Services, Control Panel.
- A one-step installer and clean uninstaller.

**Phase progress:** Core desktop 12/12; Settings and multitasking 9/9; System apps 4 of 6 (1 partial); Road to 1:1 16 of 19 (1 partial).

**Confirmed on real hardware:** install, pop-ups, Open/Save dialogs, touch keyboard in the session, firmware updates; of 30 filed hardware issues, 25 are fixed and closed, 2 are fixed and awaiting retest (#18, #29), and 3 are not started (#23, #24, #28).

## 6. Limitations and risks

- The sandbox cannot reproduce KMS paths, locking or touch; fixes in those areas depend on the owner's retests.
- Every fork is a patch to rebase when COSMIC changes. Dry runs reduce, but don't remove, that cost.
- Apps outside COSMIC (GTK, Electron, Qt) keep their own title bars and theming by choice.
- Segoe UI Variable and Segoe Fluent Icons are proprietary; Selawik and Fluent UI System Icons are close but not identical, and Selawik has no optical sizes.
- Linux-specific constraints remain visible, for example administrator prompts still ask for a password and Orca differs from Narrator.
- No touch-device testing; the OOBE depends on the Arch installer for language and account setup.

## 7. Future work

1. **File Explorer**: fork COSMIC Files with a Windows 11 command bar, breadcrumb bar, Home, Gallery and details pane; add the Open/Save navigation pane.
2. **Store**: fork COSMIC Store into a Microsoft Store-style Home, sections and Library, sharing update code with Update.
3. **Snip markup editor** (pen, highlighter, crop).
4. **Snap-group shared-edge resize** in the compositor.
5. Pointer settings, icon scoping for other desktops, X11 work-area support (issues #23, #24, #28).
6. Real contrast themes; possibly live captions and voice typing (parked).

## 8. Explicitly out of scope

GTK and Qt app theming, widgets and Copilot, the newer Start layout, search highlights, window close animation, running Windows apps natively, and Microsoft account services.

## 9. Conclusion

Lapin UX shows that a Windows 11-familiar desktop can be built on Linux as a real desktop environment: own shell, small patches on a pinned upstream, strict separation from other desktops, and an evidence-based review loop. The remaining work is the two large forks (Explorer and Store) and hardware verification of the most recent fixes.

## Appendix: Key numbers

| Item | Value |
|---|---|
| Unit tests (lapin-ux-core) | 99 |
| Uninstall checks | 55 |
| Settings unit tests | 13 |
| Review rounds / screens | 22 / 292 |
| Hardware issues filed / open fixes awaiting retest | 30 / 2 (#18, #29) plus 3 not started (#23, #24, #28) |
| Pinned COSMIC release | epoch-1.11.0 |
