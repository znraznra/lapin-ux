# Notes

[Back to the technical reference](../TECHNICAL.md)

Notes (`lapin-ux-notes`, app ID `dev.lapinux.Notes`, "Lapin Notes" in About) is
the text and code editor. Everything is local; nothing goes over the network.
It follows the user's light or dark theme.

## Shared code

Only through `lapin-ux-ui`: a menu bar widget (`menu_bar`, `MenuItem`) with
accelerators and screen reader names, and the existing `title_bar`. No fork
patches are reused or added.

## What it does

- Tabs with a close button, middle-click close and an unsaved dot. Window menu
  moves a tab left or right. Multiple windows (File > New window).
- Session restore: unsaved and untitled tabs come back after a restart. Only one
  window owns the session (`session.lock`); other windows start blank. Untitled
  tabs close without a "save changes?" prompt.
- Display: line numbers, current line, word wrap, whitespace and line end
  marks, zoom, folding, bracket matching.
- Status bar: line, column, selection, length, line ending, encoding, language,
  insert or overwrite.
- Search: Find, Replace, Find in Files, Go to line, regex, match case, whole
  word, count, mark all, incremental.
- Encodings: UTF-8 with or without BOM, UTF-16 LE/BE and legacy code pages
  (detected on open, convertible). Line endings LF, CRLF, CR, with convert.
- Syntax colours by extension (Language menu), auto-close, auto-indent,
  tab or space settings, comment toggle, line operations (duplicate, delete,
  move, join, split, sort, remove duplicates, trim, case).
- Read-only files, reload on external change, "Save as administrator"
  (`pkexec`, through a private temporary file).

## Files

`~/.config/dev.lapinux.Notes/settings.json`, `session.json`, `session.lock`.
The session file holds the user's own text, so uninstalling never deletes it.
Nothing Notes writes is shared with other apps or desktops, so nothing is added
to `items()` in `tools/lapin-ux-uninstall`.

## Default editor, Lapin session only

`/etc/xdg/lapinux-mimeapps.list` is a desktop-specific mime list. It is read
only when `XDG_CURRENT_DESKTOP` contains `LapinUX` (lower-cased to `lapinux`),
and `~/.config/mimeapps.list` always wins, so a choice made by the user is never
overwritten. Other desktops never read it, and no setting is written.

## Accessibility

Menus, tabs and buttons have names and sit in named groups. The text area is a
custom widget and is not exposed to screen readers yet.

## Not in this release

Print and print to PDF, dropping files into the window, tab drag reorder, the
full "Reopen with encoding" and Language lists, column selection and
multi-caret, bookmarks, minimap, function list, split view, macros, run
commands, a results panel for Find in Files beyond the basic list, diff, hex
view, auto-completion, a style configurator.
