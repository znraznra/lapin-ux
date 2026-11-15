# Lapin UX technical reference: Look and sound

[Back to the technical reference](../TECHNICAL.md)

**Text** (#40)
- Settings > Personalization > Fonts starts with *System font* and
  *Monospace font* (in the Lapin session they're not on Colors). The system
  font sets COSMIC's `interface_font`, GTK's `font-name`/`document-font-name`
  and Lapin's `system_font` (`""` = Selawik), which the shell keeps instead of
  resetting it to Selawik; Settings saves `system_font` first because the
  shell reacts to the font change. The monospace font sets COSMIC's
  `monospace_font` and GTK's `monospace-font-name`. All are swapped per
  session.
- *Text smoothing* (Sharper RGB / Sharper
  BGR / Standard) for GTK, Qt, Firefox and Electron: GTK's
  `font-antialiasing`, `font-rgba-order`, `font-hinting` (swapped per
  session; the shell sets RGB once) and `~/.config/lapin-ux/fonts.conf`,
  included last by the session font setup, whose own default is
  `rgba=rgb`, `lcdfilter=lcddefault`, `hintstyle=hintslight` (before the
  system's setup, so the user's or distribution's choice wins).
- *Text contrast* for Lapin's own apps: `packaging/cryoglyph` patches iced's
  glyph shader with an enhanced-contrast curve
  `a' = a(k+1)/(ak+1)`, k = contrast × (1 − ½·luminance of the text colour).
  Read at app start from `LAPIN_UX_TEXT_CONTRAST` or `dev.lapinux.Shell`
  `text_contrast` (default 0.5); 0 outside a Lapin session. Grayscale only:
  no subpixel rendering in Lapin's own apps yet.

**Sounds and pointers**
- System sounds from the bundled **Borealis** theme: sign-in, sign-out,
  notifications (apps can ask for silence or their own sound), alarms, volume
  changes, emptying the Recycle Bin.
- Settings > Personalization > Sounds: pick the scheme (config key
  `sound_theme` in `dev.lapinux.Shell`, empty = no sounds, read live by
  `lapin_ux_core::sound`), install schemes from an archive or folder into
  `~/.local/share/sounds` (tracked in `~/.local/state/lapin-ux/sounds-added`,
  only those can be removed there), and set a sound, silence or own file per
  event. Per-event choices live in the `lapin-ux-custom` theme, so an installed
  scheme is never edited. Uninstall removes `lapin-ux-custom` and leaves added
  schemes (listing them). Battery and device events are listed but Lapin does
  not play them yet.
- Optional pointers by JepriCreations, installed on request
  (`lapin-ux-cursors`, offered by the installer and Get Started; see below).
- Settings > Accessibility > Mouse pointer (#24): style (the extra pointers,
  light or dark, the system's, or any pointer theme installed on the
  computer (#57; folders with `cursors/` in the icon paths):
  `lapin-ux-cursors [--dark] --variant V`, `--system` or `--theme NAME`), the busy pointer's colour (the pack's variants) and size
  (24–80 px). The pack is kept in `~/.cache/lapin-ux/cursor-pack` after the
  first download. The size is Lapin's `pointer_size` (exported as
  `XCURSOR_SIZE` by `start-lapin-ux`, read by the compositor and apps at
  start) and GTK's `cursor-size` (swapped per session). Run from another
  desktop, `lapin-ux-cursors` only changes Lapin UX's own copy of the pointer
  settings (`~/.local/state/lapin-ux/lapin-desktop`).
  Changing the style shows at once (#57): `lapin-ux-cursors` writes the theme
  name to `~/.local/state/lapin-ux/pointer-theme` (Lapin's own file, written
  whole and moved into place), and the compositor (`lapin-ux-comp` 0012, only
  in the Lapin session) checks it once a second and reloads every seat's
  pointer from the new theme. GTK apps follow the gsettings key live; Qt and
  other apps that are already open still read the theme when they start, and
  Xwayland's pointer too.
  The pointer picked for Lapin UX, also one you added, is noted in
  `~/.local/state/lapin-ux/lapin-pointer` (#134), so `lapin-ux-desktop-settings`
  knows it's Lapin's and not the other desktops'. If it's found in the shared
  settings while Lapin's copy isn't in place (it was picked after the copies
  were handed back too early, #115), `leave` keeps it for Lapin UX and puts the
  other desktops' pointer back.
