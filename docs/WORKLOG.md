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
| Sun 22 Nov | The sign-in screen says plainly when the disk is full (#174), so a failed sign-in is no longer a silent mystery. | Retest |
| Sun 22 Nov | Warning before the disk is full (#174): a notice when a drive is almost full, shown once and not repeated while it stays low, with a switch in the notification settings. | Retest |
| Sun 22 Nov | Explorer: Home and This PC are per tab, right-click menus on Home and This PC, a tidier Recent list, and a details pane that follows the page (#168, #169, #170, #175). | Retest |
| Sun 22 Nov | Safely Remove Hardware tray icon with eject and a busy-drive message (#102). | Retest |
| Sun 22 Nov | Signing in on a nearly full disk (#174): a log line that could not be written ended the session script, and the crash reporter aborted the program. The session log is now optional, and a full disk no longer stops the session, the shell or the other programs. | Retest |
| Sun 22 Nov | The taskbar can sit on the left or right edge as well as the bottom and top (#41): a narrow icon-only bar, Start, panels, pop-ups, auto-hide and the work area all follow the edge. Chosen in Settings or from the taskbar menu. | Retest |
| Sun 22 Nov | Photos (#74), stage 1: an offline picture app with a dark gallery (by date, Favorites, your folders) and a light viewer with a filmstrip. | Retest |
| Sun 22 Nov | Explorer in a narrow window (#151): the details pane gives way first, then the navigation pane, and both return when the window has room. | Retest |
| Sun 22 Nov | Explorer pop-ups (new item, rename, delete, replace, errors) in Lapin's design, stage 1 (#163). | Retest |
| Sun 22 Nov | Sign-in screen matches the lock screen (#120): the same clock-to-panel fade and corner spacing. | Retest |
| Sun 22 Nov | Explorer: moving a folder into one that already exists no longer acts like a copy or ends in a "Directory not empty" error, and move errors are in plain words (#172). The folder-level question is still to come. | Retest |
| Sun 22 Nov | Explorer Properties window (#171): one fixed size, a tab bar like Explorer's, and only the page scrolls while OK, Cancel and Apply stay put. | Retest |
| Sun 22 Nov | Light mode by default (#159): a new install, a first Lapin session and the welcome app start light instead of dark. A choice made in Settings is never overwritten. | Retest |
| Sun 22 Nov | The file explorer is called Explorer everywhere in the Lapin session, and its About page says Lapin Explorer with credit to COSMIC Files (#173). | Retest |
| Sun 22 Nov | Code scanning (#190, part 2): CodeQL and Semgrep results now show in the Security tab. | Docs |
| Sun 22 Nov | Automated checks on every pull request (#190): the uninstall tests, shellcheck and a dependency and secret scan run in GitHub Actions. | Docs |
| Mon 23 Nov | Explorer Home (#168): the Recent and Favorites lists are real columns you can drag to resize and click to sort, long text ends in "…" with the full path as a tooltip, and Quick access, Favorites and Recent fold and unfold from their chevron. | Retest |
| Mon 23 Nov | Explorer right-click (#169): the menu on Home now opens at the pointer, right-click on empty space works (Refresh, Map network drive), drives can be pinned to Home, and This PC's details pane shows a summary when nothing is picked. | Retest |

## Phase dates

| Phase | Started | Finished |
|---|---|---|
| 1: Core desktop | 15 Oct | 20 Oct (reviewed) |
| 2: Settings and multitasking | 18 Oct | 21 Oct (Settings stage 5 approved) |
| 3: System apps | 19 Oct | In progress: File Explorer and the navigation pane remain |
| 4: Zero learning curve | 19 Oct | In progress: Store, snip markup editor and accessibility remain |
| Hardware retest | 22 Oct | In progress: 54 of 58 issues closed on 10 Nov |

Busiest stretch: 18 to 21 October, 92 commits in four days. After the first real-hardware test on 22 October, the work is mostly fixes and retests.
| Mon 23 Nov | Photos gallery layout (#203): toolbar with Source and Group menus, size slider with a reel step, square or proportional grid, multi-select with favorite and Recycle Bin, Recents. Follows the light or dark theme. | 4 |
| Mon 23 Nov | Photos Media types and Hidden (#203): Screenshots and Animated GIFs sections, Hide and Unhide for selected photos. | 1 |
| Mon 23 Nov | Photos Albums (#203): album list in the side bar, Add to album, New album, remove from album, delete album. | 1 |
| Mon 23 Nov | Photos slideshow (#203): play and pause in the viewer, More > Slideshow in the gallery. | 1 |
| Mon 23 Nov | Photos Add folder (#203): Add folder... in the folder drop-down, saved in `folders.json`, Stop looking in this folder. | 1 |
| Mon 23 Nov | Photos rename album (#203): More > Rename this album. | 1 |
| Mon 23 Nov | Photos Hidden password (#203): lock screen for Hidden, set, change and remove from More. | 1 |
| Mon 23 Nov | Photos slideshow speed and full screen (#203): speed in More, Full screen button in the viewer. | 1 |
| Mon 23 Nov | Photos tiles load as you scroll (#203). | 1 |
| Mon 23 Nov | Photos turn left and right in the viewer (#203). | 1 |
| Mon 23 Nov | Photos grid memory (#203): thumbnails far from the visible stretch are dropped, the grid restarts when the source folder changes, and a failed turn shows its note until the next try. | 1 |
| Mon 23 Nov | Photos edit (#203): brighter, darker, contrast, black and white and crops in the viewer, saved as a copy. | 1 |
| Tue 24 Nov | Photos Hidden lock hardening (#203): unreadable password or hidden list keeps Hidden closed, the password is re-read across windows, hidden photos are matched by canonical path. | 1 |
| Tue 24 Nov | Photos turn left and right now writes through an exclusively created temporary file (#203). | 1 |
| Tue 24 Nov | Photos edit copies are written through an exclusively created temporary file, never replace an existing file, and inherit Hidden from the original (#203). | 1 |
