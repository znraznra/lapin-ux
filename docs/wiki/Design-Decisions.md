# Design Decisions

Standing decisions from the project plan and review rounds.

## Principles
- Own shell on top of cosmic-comp; compositor changes in small patches with our own protocol.
- Fork COSMIC programs as patch files; every fork keeps COSMIC's behaviour outside a Lapin session.
- One-step install; every dependency declared. Assets ship in the repo, except the Windows 11-style pointer pack (licence forbids redistribution), which `lapin-ux-cursors` downloads on request, credited to JepriCreations.
- Uninstall must never leave the user stuck.
- Lapin UX and other desktops keep their own copies of shared settings.

## Look
- Selawik is the only interface font inside Lapin UX (session-only fontconfig); the lock screen and all system surfaces use Selawik by name.
- Win11 icon theme for apps/files/Control Panel; Fluent UI System Icons for everything else; Fluent Emoji; Borealis sounds.
- Theme radii: largest radius fully round (toggles, sliders, progress bars); 8 px windows/panels; Lapin apps use 4 px push buttons via lapin-ux-ui. Side effect: unforked COSMIC apps show pill buttons.
- Round toggles and slider thumbs (blue circles); slim taskbar clock shows time only (no seconds).
- Title bars = app icon + left-aligned title.
- Start never covers the taskbar: opens 4 px above, rests 12 px above; nothing cut off.
- Menus, Start, Win+X and dialogs are opaque.
- Shut Down dialog: fixed size, OS banner, "Shut Down <OS>". Sign-in panel dead centre. Dialog button labels centred. Ctrl+Alt+Del regular weight with Cancel under the list. Task Manager sidebar narrow and collapsible.
- Snipping overlay lives in the shell, not in a cosmic-screenshot patch.

## Settings app (rounds 9-18)
- "Find a setting" is centred in the title bar.
- Sidebar icons come only from the Win11 icon theme; Update uses the blue round-arrows refresh icon.
- Home groups have no background; only rows with a chevron do.
- System page top section: wallpaper in a small screen, name, model, Rename; Update + last checked at right.
- Every setting is its own card with uniform 4 px gaps, same width and left edge on all pages and search.
- Update lives inside Settings as the last category. Gaming left out. COSMIC-only settings sit under "Advanced".
- Face sign-in offered through Howdy; the sign-in screen has a gear to choose the desktop session (any installed Wayland/X11 desktop).
- About follows Windows (name card + Device/OS specification expanders with Copy).

## Left out (don't build or propose)
GTK 3, GTK 4 and KDE (Qt) theming, newer Start layout, search highlights, widgets/Copilot, window close animation. Parked: live captions, Win+H, Mica on app windows.
