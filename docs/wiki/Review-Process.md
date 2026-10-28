# Review Process

Every visible change goes through a screenshot gallery review before it counts as done.

## Workflow
1. Capture changed screens in the sandbox.
2. Add them to the gallery as a new group with new numbers ("was #N" in the title); show only screens still needing review.
3. Tako marks each screen right or change; changes are fixed and re-shot in the next round.
4. Hardware-only behaviour is retested by Tako on real Arch/CachyOS via GitHub issues.

## Round history (summary)
| Rounds | Screens | Outcome |
|---|---|---|
| Early builds | 1-63 | Taskbar, Start, notifications, quick settings, desktop, Task view, snapping refined and approved |
| 053be18 | 64-105 | 20 changes: round toggles/thumbs, slim clock, Shut Down dialog, Task Manager sidebar |
| a6a8fc9, 86597f1, 32c7d79, 967213a | 106-136 | Start never covers taskbar, title bars with app icon, Control Panel icons |
| 07c7359 (round 8) | 137-148 | All approved: pop-ups, admin prompt, Users/Services, preview pane, firmware |
| Settings stages 1-5 (rounds 9-18) | 149-255 | All approved by 2026-10-21; Settings app finished |
| Round 19 | 256-271 | 12 right, 4 change |
| Round 20 | 272-276 | Approved 2026-10-27 |
| Round 21 | 277-288 | 11 right; #288 changed (screen reader needs Orca) |
| Round 22 (Version 31) | 289-292 | Screen reader install flow (#18) and #29 crash-fix retest card |

Everything through #287 is approved; next round starts at #293. Hardware-retest items are shown as checklist cards.
