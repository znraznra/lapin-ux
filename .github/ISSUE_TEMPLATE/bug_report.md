---
name: Bug report
about: Something in Lapin UX doesn't work or looks wrong
labels: bug
---

**Checklist item** (from TESTING.md, if any):

**What happened**

**What you expected**

**Steps to reproduce**
1.
2.

**Stage**
- [ ] A: in a window
- [ ] B: logged in as Lapin UX

**System**
- Lapin UX commit (`git rev-parse --short HEAD`):
- Graphics (`lspci | grep -i vga`):
- Monitors (number, resolution, scaling):
- Display manager (`systemctl status display-manager | head -1`):

**Screenshot**

**Log**
Attach `~/.local/state/lapin-ux/session.log` (Stage B) or `lapin-test.log` (Stage A).
