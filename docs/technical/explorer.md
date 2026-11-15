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
The uninstaller lists the `lapin-explorer` package in `LAPIN_PKGS` and `STOCK`.

**File dialogs** (`lapin-ux-portal`, COSMIC's portal built with a COSMIC Files patch)
- The dialog's sidebar follows the Explorer's (#76): Home, Gallery (the Pictures
  folder, left out when the user has none), Recents, the pinned folders, then a *This PC* heading with the drives
  indented under it. Home and Gallery are plain folders here (the dialog has no
  Home page).
- Every app's Open/Save dialog ends with *File name* with the file
  type beside it (*Save as type* when saving), then Open/Save and Cancel.
