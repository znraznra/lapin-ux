# Work log

[Back to the README](../README.md) · [Roadmap](ROADMAP.md)

The days Lapin UX was actually worked on, taken from the commit history of `main`. Eighteen working days between 15 October and 13 November 2026, about 166 commits. Days with no commits are left out.

| Day | What moved | Phase |
|---|---|---|
| Thu 15 Oct | First commit. Taskbar, Start menu, notifications and session. Project named Lapin UX. Ready for real machines. | 1 |
| Sun 18 Oct | Tray overflow, Run, notification centre, quick settings, Start search, desktop icons, clipboard history and emoji, Alt+Tab, auto-hide. | 1, 2 |
| Mon 19 Oct | The biggest day. Jump lists, Start folders, Recycle Bin, Task view, snapping, snap layouts and groups, night light, Task Manager, Control Panel, Update, lock and sign-in screens, first-run setup, snipping, touch keyboard, Open/Save dialogs, sounds. Selawik, Fluent icons and Fluent Emoji. | 1, 2, 3, 4 |
| Tue 20 Oct | Review fixes. Volume and brightness pop-ups, admin prompt, Task Manager Users and Services, search preview, firmware updates, COSMIC app title bars. Settings stages 0 to 2. README rewritten for everyone. | 2, 3, 4 |
| Wed 21 Oct | Settings stages 3 to 5: the Settings app is finished and Phase 2 closes. Moved to COSMIC 1.11. Clean uninstaller, and the rules that keep other desktops untouched. | 2 |
| Thu 22 Oct | Installer asks for the password once. First real-hardware test on CachyOS: issues #1 to #22 filed. | Retest |
| Mon 26 Oct | Fixes for all 22 hardware issues. Round 19 review. | Retest |
| Tue 27 Oct | Crash reports and the start-up crash fix (#29). Lock screen blur. Screen reader install prompt (#18). | Retest |
| Wed 28 Oct | Wiki and white paper written, with the sync to GitHub. | Docs |
| Sat 31 Oct | Closing a window from its preview no longer kills the shell (#45). Narrator ownership. | Retest |
| Sun 1 Nov | Fixes for issues #23 to #45: X11 and Wine tray icons, mouse pointer page, 2-line desktop icon names, Task Manager runs once, menus close on outside click, window work area. README repositioned. | Retest |
| Mon 2 Nov | Main display choice (#31), sharper text (#40). Code of Conduct. Narrator paused. | Retest, 4 |
| Sat 7 Nov | Fixes for the lost-look bug (#46), Fonts page, install over stock COSMIC parts, windows open at their own size. | Retest |
| Sun 8 Nov | Print Screen, a faster shell, tray icon tints, opaque taskbar, rabbit Start icon. Security Policy, contributing guide and templates. README restructured, logo added. Slogan changed. | Retest, Docs |
| Mon 9 Nov | The official logo is the one Lapin UX icon. Lock screen follows the chosen font and its blur fades. About Lapin UX card in Settings. | Retest |
| Tue 10 Nov | Milestones, tracking issues and sub-issues created on GitHub. | Docs |
| Thu 12 Nov | Work now goes through pull requests. CodeRabbit reviews them using `.coderabbit.yaml`, and `main` is protected against deletion, force pushes and direct pushes. | Docs |
| Fri 13 Nov | The taskbar dead zone (#34): a hover preview closed too early was left behind as an invisible pop-up that swallowed clicks. The shell now notices pop-ups it no longer owns and destroys them. | Retest |

## Phase dates

| Phase | Started | Finished |
|---|---|---|
| 1: Core desktop | 15 Oct | 20 Oct (reviewed) |
| 2: Settings and multitasking | 18 Oct | 21 Oct (Settings stage 5 approved) |
| 3: System apps | 19 Oct | In progress: File Explorer and the navigation pane remain |
| 4: Zero learning curve | 19 Oct | In progress: Store, snip markup editor and accessibility remain |
| Hardware retest | 22 Oct | In progress: 54 of 58 issues closed on 10 Nov |

Busiest stretch: 18 to 21 October, 92 commits in four days. After the first real-hardware test on 22 October, the work is mostly fixes and retests.
