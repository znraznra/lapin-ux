# Security Policy

Lapin UX is a desktop. It runs your sign-in, your lock screen and a few steps that need administrator rights, so security problems matter here. Thank you for helping keep it safe.

## Supported versions

Lapin UX is still a preview, and only the latest code is supported.

| Version | Supported |
|---|---|
| `main` branch (what `./install.sh` builds) | Yes |
| Older commits and older installed builds | No: update with `git pull && ./install.sh` |

Fixes go into `main`; there are no separate release branches yet.

## Report a vulnerability

**Please don't open a public issue for a security problem.** Report it privately:

1. Open the repository's **Security** tab.
2. Choose **Report a vulnerability**.
3. Describe what you found. Only the maintainer can see the report.

Helpful details:
- what is affected (for example the lock screen, Update, Settings, the installer)
- the steps to reproduce it, or a short proof of concept
- what an attacker could do with it, and what they need first (a local account, physical access, a malicious file)
- your Lapin UX build (the commit shown by `git log -1` in the folder you installed from) and your Linux distribution

A partial report is fine. Send what you have, and add to it later.

## What to expect

Lapin UX is maintained by volunteers, so these are goals, not guarantees:

- A first reply within **7 days**.
- A status update within **14 days**, saying whether it is confirmed and how serious we think it is.
- A fix for serious problems as soon as it can be made safely. Smaller ones may wait for the next round of work.
- Credit in the fix notes if you want it. Say so in your report, or ask to stay anonymous.

There is no bug bounty.

Please give us reasonable time to fix a problem before telling anyone else. We will keep you informed, and we will tell you when it is safe to publish.

## What counts

These parts run with extra rights or guard access to your session, so reports about them are especially welcome:

- the **lock screen and sign-in screen** (bypassing the password, leaking the password, reading a locked session)
- steps that ask for **administrator rights** through `pkexec`: Update, uninstalling apps, Storage cleanup and the face sign-in helper (`lapin-ux-howdy`), and the polkit rules that go with them
- the **admin prompt** (the dimmed "allow this app to make changes" screen)
- **PAM changes** made for face sign-in, and the installer and **uninstaller**, which run as root
- the notification service, clipboard history, the X11 tray bridge and other parts that handle data from other apps (for example a message that makes them run something, or shows another app's private data)
- anything that makes Lapin UX change another desktop's files or settings in a way that harms the user

## What doesn't

- Problems in software Lapin UX is built on, such as COSMIC, libcosmic, Orca, Howdy, fprintd, pacman, or the third-party assets credited in the README. Please report those to their own projects. If Lapin UX makes the problem worse, tell us as well.
- Things that need you to already have full administrator (root) access.
- Missing "hardening" with no way to exploit it.
- Everyday bugs, crashes and look problems that aren't a security risk. Please use the normal [issue tracker](https://github.com/znraznra/lapin-ux/issues) for those.

## Acting in good faith

If you look for problems carefully and follow this policy (test only on your own machines, don't touch other people's data, don't disrupt other people's use, and report privately), we will treat it as help, not as an attack, and we won't pursue it.

## Staying safe while it's a preview

- Install from this repository only.
- Keep your system updated, and `git pull && ./install.sh` for new Lapin UX builds.
- Keep a second desktop or a text login available. Lapin UX is designed to leave other desktops untouched and to uninstall cleanly with `lapin-ux-uninstall`, and a spare way in is still wise while it is a preview.
