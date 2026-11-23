# Lapin UX technical reference: Explorer and file dialogs

[Back to the technical reference](../TECHNICAL.md)

**Explorer** (`lapin-explorer`, #71): COSMIC Files at the pinned commit with the
patches in `packaging/lapin-ux-files/` (after the dialog patch), installed in
place of `cosmic-files` (it provides and conflicts with it, and with the
optional `lapin-ux-cosmic-files`), so it stays the default file manager. Every
Lapin look is gated on `config::lapin_session()`; in COSMIC's own session it is
stock. First piece: a command bar (New folder, Cut, Copy, Paste, Rename, Delete,
view toggle). The sidebar starts with Home (the Home page, the only Home in
the list) and Gallery (the Pictures folder), then Recents and the pinned
folders, then This PC with the drives indented under it (its page shows each
drive with how full it is), then Networks and Trash. Pin icons, collapsing the
drives and the sidebar's own right-click entries: right-click This PC > Hide
drives folds them away and Show drives brings them back; a pinned folder's menu says Unpin from
Home and Properties (#72). Pin icons in the sidebar are still open on #72. The
menus (Sort, View and More, which holds the File and Edit entries the buttons
don't cover) sit in the command bar instead of the title bar, and the buttons
are icon-only with a tooltip so the row fits a narrow window; the menus fold
into one button when it is too narrow. The address bar under it
has no Back/Forward pair of its own any more (the command bar has them).
With the details pane docked the bar is narrower still, so it drops its
spacer and scrolls sideways.
The right-click entry is called Properties and opens a window of its own
("<name> Properties", with General and Security tabs, described below), #75; the
Details button docks a shorter details pane beside the files (its fields are
listed below too).
The tab bar is always shown, with a + that opens a new tab.
The Home page (#73) has Quick access (pinned folders with a pin to unpin
them, then up to 8 folders of your recent files), Favorites (pinned files) and
Recent (the 12 latest files). Right-click > Pin to Home pins a file or folder
(the same list as COSMIC's "Add to sidebar"); pinned files show on Home only,
pinned folders also in the sidebar.
File operations (#101): copying, moving, deleting and the like open a small
window titled with the overall percentage ("12% complete"), one section per
operation: what it does, a progress bar, Pause/Resume and Cancel. More details
adds a speed graph (one bar per half second, the last 30 s), the speed, the
item in hand, the time left and the items left; Fewer details hides them.
Operations that don't copy data (moving to the Recycle Bin, deleting) have no
speed line and an empty graph. The
numbers come from the operation's controller (bytes, items and the current
name are counted in the copy's progress callback), sampled twice a second.
The window closes when everything is done. Closing it by hand keeps the
operations going and shows COSMIC's own progress strip in the Explorer window
until they finish. Name conflicts still use COSMIC's Replace dialog.
Layout (#143), top to bottom: the tabs and a + in the title bar
(`header_start`); a navigation row with Back, Forward, Up, Refresh, the address
bar (the path as crumbs in a rounded box; click its blank part to type a path)
and an always-there search box ("Search <folder>", emptying it ends the
search); the command bar (New menu, Cut, Copy, Paste, Rename, Delete, then the
Sort, View and "…" menus, and Details with its label at the right); then the
navigation pane beside the files; then a status bar (item count, selection
count and size, and the list/grid buttons). The rows span the window, so the
view draws the navigation pane itself (`nav_pane`, the same `nav_bar` widget;
`nav_bar()` and `nav_model()` return nothing in the Lapin session, which also
keeps libcosmic's pane toggle out of the title bar). `Tab::location_view_part`
gives the address and the column headings separately. File dialogs and
COSMIC's own session are unchanged.
More of the familiar layout (#143). Menus: Sort (Name, Date modified, Size,
Ascending, Descending), View (Large icons, Details, Details pane, Hidden items,
folders first, zoom) and "…" (History, Select all, Open with, Pin to Home,
Properties, Options, About). The details view has a Type column ("File folder",
"PNG File"). This PC lists "Devices and drives" and "Network locations" as two
groups of tiles. The navigation pane is a tree drawn by the Explorer itself
(`lapin_tree_pane`): rows come from the same model and use the same handlers
as COSMIC's `nav_bar` (select, middle click, right-click entries, drops), plus
a chevron for This PC, the drives and their folders (`LapinTree`,
`lapin_expanded`; a folder's children are read when it is opened out), a pin
on pinned folders and an eject button on removable drives. The details pane
sits beside the files (not in COSMIC's drawer, which would cover the rows
above) and shows the name, then Type, Size, File location and the dates as
label and value pairs.
Properties and groups (#143). The Properties window has two tabs, General (the
name, Type, Size, Contains, File location, the dates, Opens with, and for a
drive its used and free space and capacity with a bar) and Security (the
permissions and the checksum), and an OK button. The details pane shows the
name, Type, Size, Contains, File location and the dates (no permissions, no
Opens with) and a Properties button. `tab::LAPIN_PROPERTIES_TAB` tells
`Item::preview_view` which of the three to build. Sorted by date, the list is
split into groups with a heading ("Today", "Yesterday", "Earlier this week",
"Last week", "Earlier this month", "Last month", "Earlier this year", "A long
time ago"); the headings take 32 px each in the row positions. The "…" menu
has Select none and Invert selection. A tab shows the icon of its place.
A group heading folds its group away and back (`Tab::lapin_collapsed`; folded
files have no row). Properties' General tab has a Read-only box (it clears
every write permission; unticking gives the owner's back) and the window has
Cancel beside OK (since the retest round below, OK applies the changes and closes the window, and Cancel drops them).
The details view keeps its columns until the file area is narrower than 480 px
(COSMIC switches to two-line rows under 600 px), so a normal window at 125%
scale still shows Name, Date modified, Type and Size.
Fixes from Tako's retest (#143, #101): the progress window closes while a
question is up (Replace or skip?) and comes back afterwards, and opens small
(150 px, 330 px with details); the tab strip takes the press, so dragging a tab
reorders tabs instead of moving the window (the title bar drags from presses
nothing took); the details pane only follows the Details setting, so Options or
History in COSMIC's drawer don't hide it; clicking the Type heading sorts by
type (folders, then by extension; again reverses), on top of the usual order
(`Tab::lapin_sort_type`, not saved).
Light mode, matched to Tako's reference (#143): the command bar, files and
status bar sit on their own layer (`lapin_layer_style`: plain white in light
mode, the window colour in dark) below the tabs and address row; the details
pane is light grey (`lapin_details_style`). Details rows are 32 px with 16 px
icons and no lines between them (24 px since the next round). The search box is the address's lightly
rounded box with no outline until focused (`lapin_box_input`), tabs are 240 px
each (a fixed-width box around the tab strip) and shrink when there are too
many, the open place in the pane is a soft grey box (`lapin_pane_selected`),
and Recents isn't in the pane (recent files are on Home).
In the details pane the picture sits on a grey band across the top
(`lapin_band_style`) and the rest is padded below it; the Properties button is
a lightly rounded box with a thin outline (`lapin_box_button`).
Tako's retest of that round (#143, #149, #101):
- Details rows are always 24 px (the Compact view toggle is gone).
- The tab strip is sized to the room left beside + and the window buttons
  (from the window's width), so + always shows; past that the tabs shrink.
- History, Options, About and Add network drive open in the details pane
  (`lapin_pane_page`, `lapin_pane_page_view`) with a title and a close button,
  not in COSMIC's drawer column, which never opens in the Lapin session. The
  pane goes back to the details when a file or folder is clicked or the place
  changes.
- On Home and This PC a click picks an item (the soft grey box) and a second
  click on it within half a second opens it (`LapinPageClick`).
- The Replace question offers Replace, Skip and Keep both, with Apply to all
  when there are several clashes (`ReplaceResult::KeepBoth(bool)`); a kept copy
  is named "name (1).ext".
- Date modified, Type and Size can be resized by dragging the handle just before
  each heading (`MouseArea::on_drag_delta`, 60 to 600 px, kept while Explorer
  is open).
- Properties' General tab follows the familiar layout (`lapin_general_view`):
  icon and an editable name, then sections split by lines: Type of file, Opens
  with (Change… lists the apps), Location, Size and Size on disk (Size and
  Contains for folders), the dates written out in full, and Read-only and
  Hidden. Changes wait in `lapin_props` until OK or Apply (greyed out until
  something changed); Cancel drops them. A new name or Hidden (a leading dot)
  goes through the usual rename operation.
- Home's headings look like the group headings elsewhere, Quick access uses the
  full-colour folder icons, and Favorites and Recent are lists with Name, Date
  accessed and File location columns. Home and This PC name their tab. The
  Network page is called Networks, and Empty trash and Add network drive are in
  the "…" menu instead of a bar under the files.
Map network drive (#153): "…" > Map network drive... opens in the details pane
with a folder address (`smb://server/share`) and an optional name. Finish
saves the drive (`tab::LapinMapped`, key `mapped_drives` in Lapin's own
`dev.lapinux.Explorer`, removed with the other `dev.lapinux.*` settings on
uninstall) and connects it through COSMIC's network-drive code. A mapped drive
is always listed under This PC, on the page and in the left pane, connected or
not; opening one that isn't connected connects it first (`LapinOpenMapped`),
and a whole server (`smb://server`) is browsed instead. "…" > Disconnect
network drive forgets the drive shown and disconnects it.
The uninstaller lists the `lapin-explorer` package in `LAPIN_PKGS` and `STOCK`.

**File dialogs** (`lapin-ux-portal`, COSMIC's portal built with a COSMIC Files patch)
- The dialog's sidebar follows the Explorer's (#76): Home, Gallery (the Pictures
  folder, left out when the user has none), Recents, the pinned folders, then a *This PC* heading with the drives
  indented under it. Home and Gallery are plain folders here (the dialog has no
  Home page).
- Every app's Open/Save dialog ends with *File name* with the file
  type beside it (*Save as type* when saving), then Open/Save and Cancel.

**Pages per tab and the details pane (#168, #169, #170, #175).** Home and This PC
are properties of each tab (the tab's location), not flags on the window, so two
tabs can show different pages and switching tabs no longer shows one tab's page in
the other. On both pages a click picks an item (the pane follows it: a drive, a
pinned folder or a file shows its details; nothing picked says "Select a file or
folder to see its details."), a double-click opens it, and right-click opens a
menu: Open, Open in new tab, Open item location, Copy path, Properties, and
Unpin from Home, Pin to Home (drives), Remove from recents or Disconnect where
they apply. Right-click on empty space opens the page's own menu (Refresh, and
Map network drive on This PC). There is one menu per page, wrapped around the
scrollable (an item picks itself on right-press, empty space clears the pick), so
the popup opens at the pointer instead of below the item (#169). With nothing
picked, This PC's pane shows a drive count. Not built yet: Eject, Format, View
(tiles or list) and Properties of This PC. Favorites and
Recent share one list (#168): fixed-width columns (Name, Date accessed, File
location) with the same row height and icons as a folder. Drag the right edge of
a heading to resize that column and click a heading to sort (the arrow shows the
direction; no choice keeps the most recently used first). Widths, the sort and
which sections are folded are kept in `dev.lapinux.Explorer` (`home_layout`).
Text that doesn't fit ends in "…" (cut by `lapin_ellipsize`, since iced text has
no ellipsis, and the cell clips as a safety net) and never runs into the next
column; the full path is the tooltip. Clicking the chevron or title of Quick
access, Favorites or Recent folds or unfolds its content, the chevron turns, and
the thin line stays. On Networks and Recycle Bin the pane follows the selected
item too, and the same empty message shows only while nothing is selected, instead
of a stale or blank pane (the same goes for any tab with nothing selected). This is Lapin-only
(`config::lapin_session()`); COSMIC's own session is stock.

**Narrow windows** (#151): the file list keeps at least 420 px. Below that
(window width under 700 px with the 280 px navigation pane) the navigation pane
hides, and the docked details pane (360 px) gives way first, under 1060 px. Both
return as soon as the window has room again, since the rule is worked out from
the current width every time the window is drawn, not from a stored flag. Pages
you opened yourself from the command bar (History, Options, About) stay. This is
Lapin-only (`config::lapin_session()`); COSMIC's own session is stock.

**Dialogs in Lapin's design** (#163, stage 1): Explorer's own pop-ups (New folder
or file, Rename, Delete, Move to Recycle Bin, Empty Recycle Bin, Replace or skip,
error and network messages) now share one layout: a plain light panel with the
message and a grey band underneath holding the buttons, the main action first and
the others after it, with 4 px corners and a smaller 32 px icon. Enter still
confirms, Esc cancels and the Tab order is unchanged. In COSMIC's own session the
same dialogs are built from COSMIC's own parts and look as before
(`config::lapin_session()`). Still to do in later stages: the Open and Save
dialogs, notifications, and the progress window's error and cancel states.

## Moving a folder into one that already exists (#172)

In a Lapin session a move that ends up as per-file moves now removes the
originals after a same-disk move. Before, drag and drop set `cross_device_copy`
for every move, so the originals stayed and the move acted like a copy. The flag
is now only honoured when a file really has to be copied across disks. A source
folder that still holds skipped items (a skipped clash, a hidden file) is left
as it is and logged, and the final folder cleanup no longer reports
"Directory not empty" as a failure. Errors from copy and move read as
`Couldn't move "from" to "to": reason` instead of a Rust debug dump. All of it is
Lapin-only (`config::lapin_session()`). Still open under #172: a folder-level
merge / keep both / skip question, and one shared conflict dialog with the desktop.

## Properties window size and tabs (#171)

In a Lapin session the Properties window opens at one fixed size (590 by 820,
minimum and maximum the same, not resizable), the usual way. The General and
Security tabs use the same tab bar as Explorer's own tabs (`lapin_props_tabs`,
a `segmented_button` model; `Message::LapinPropertiesTab` carries the entity).
Only the page scrolls: the tab bar and OK, Cancel and Apply stay in place,
because the Lapin arm of `view_window` returns early without the outer
scrollable that wraps every other window. COSMIC's own session keeps its
resizable window (`config::lapin_session()`). Patch:
`packaging/lapin-ux-files/0034`.

## Name and About (#173)

In a Lapin session the app is called **Explorer** everywhere: the window title
in `update_title`, and Start, search, the taskbar, Alt+Tab and Task Manager
through `lapin_names()` in `crates/lapin-ux-shell/src/catalog.rs` (the shared
`.desktop` file keeps "COSMIC Files", so COSMIC's own session is untouched).
The menu item reads "About Lapin Explorer" and the About page names Lapin
Explorer, its version (from `/usr/share/lapin-ux/version`), the Lapin UX and
support links, and credits COSMIC Files by System76 (GPL-3.0-only). No
developer contact is listed because the About widget turns those into mail
links. The Explorer app icon is not part of this change. Patch
`packaging/lapin-ux-files/0034-*`; outside a Lapin session both strings and the
About page are stock.

## Your folder and Home (#216)

"Home" is only the start page. The user's own folder is labelled with the
account's full name (the first field of the passwd GECOS entry), or the user
name when no full name is set (`config::lapin_user_label()`). It is used in the
sidebar tree, the address bar and tab titles (`folder_name`), the Open and Save
dialog sidebar, the quick-access tiles and the favourite's `display_name`.
Outside a Lapin session the folder stays "Home" as in stock COSMIC. Patch
`packaging/lapin-ux-files/0042-*`.
