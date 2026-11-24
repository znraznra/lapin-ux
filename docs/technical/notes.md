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
- Every encoding Notes knows is in the Encoding menu (convert, and "Reopen
  with" for a file that was read wrongly) and every syntax in the Language
  menu. Long menus scroll.
- Dropping files from the file manager onto the window opens them
  (`text/uri-list` through libcosmic's `dnd_destination`). Tabs can be dragged
  along the strip to reorder them.
- Print (Ctrl+P) and Save as PDF, both from File. The text becomes an A4 PDF
  written by `print.rs` (fixed-width font, tabs expanded, long lines wrapped,
  file name and page number in the footer). Print sends it to the chosen
  printer with `lp` (printers from `lpstat`, copies 1 to 99); with no print
  system the panel still offers Save as PDF. The built-in PDF font only has
  Western European letters, so other scripts print as `?`.
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

## Also in this release

- Function list (Search > Function list, Ctrl+Alt+L): definitions found per
  language by `outline.rs`; clicking one jumps to its line.
- Word completion (Search > Complete word, Ctrl+Space): a strip of words from
  the file and the language's keywords; clicking one replaces the word before
  the caret.
- Compare with next tab: `diff.rs` marks added, removed and changed lines in
  the gutter of both tabs; Clear comparison removes the marks.
- Hex dump in new tab and Hex dump back to text, via `hex.rs` (8 MB limit).

## Not in this release

Split view, macros, run commands, a results panel for Find in Files beyond the
basic list, a style configurator. Printing other scripts than Western European
ones needs an embedded font.
