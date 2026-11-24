# Photos (`lapin-ux-photos`)

An offline picture app (#74). Nothing leaves the computer: no cloud, no accounts, no network.

## Two views

- **Gallery**: follows the person's light or dark theme (#200). A toolbar on top, an optional side bar (Gallery, Favorites, Recents, then Media types: Screenshots, Animated GIFs, and Hidden, then your albums) and the grid.
  - Toolbar: side bar toggle, **Source** (all folders, or Pictures, Downloads, Desktop, Documents, shown only when they exist), **Group** (Years, Months, Days, All), a square or own-proportions grid toggle, a size slider, **Select**, and a More menu (Slideshow and Look for new pictures; **Stop looking in this folder** for an added folder; **Rename this album** and **Delete this album** inside an album).
  - The size slider has seven positions: six grid sizes, then a reel as the last one. The reel shows one large photo with the others in a strip below; clicking the large photo opens the viewer.
  - **Select** shows a tick on each tile you click. Click selects, Shift+click selects a range, Ctrl+click toggles one (adds it if it is not selected, removes it if it is), Ctrl+A selects every tile loaded so far (up to the current load limit, including ones scrolled out of view, and never a photo the list does not show, so nothing unseen can be moved to the Recycle Bin), Esc clears. While photos are selected the toolbar offers Select all, favorite or unfavorite, and move to the Recycle Bin (asks first, uses `gio trash`).
  - **Recents** shows the last three days.
  - **Screenshots** shows pictures in a folder or with a file name starting "Screenshot". **Animated GIFs** shows `.gif` files.
  - **Hide** (in the selection toolbar) moves the selected photos out of every section except **Hidden**. In Hidden the same button reads **Unhide**. Hiding keeps the file where it is; it only changes what Photos shows. Favorites, albums and Hidden are saved under each photo's real (canonical) name, so a symlink or `..` spelling of the same file matches the same entry. Locking or changing the Hidden lock clears the selection, and Hide, Unhide and delete do nothing while Hidden is locked.
- **Viewer**: one photo at a time with a floating toolbar (back, previous, next, favorite, slideshow play, full screen, file name) and a filmstrip of the nearby photos. Left and Right step through the list, Esc goes back to the gallery.

Opening a picture from Files (`lapin-ux-photos %f`) goes straight to the viewer, also when the picture is outside the scanned folders.

The side bar, source, group, grid style and size are remembered in `~/.config/dev.lapinux.Photos/settings.json` (the list of added folders is separate, in `folders.json`). Each one is saved on its own under a lock, so two open windows never write back an older value of a setting the other just changed.

**More actions** (#202): Rename (keeps the picture's place, thumbnail and star, and never replaces another file), Delete (asks first, then moves the picture to the Recycle Bin with `gio trash`, never a permanent delete; the viewer moves to the next photo, or the one before it, or back to the gallery when it was the last) and Open file location.

## How it works

- The scan walks the four folders in the background, up to 5 levels deep, skipping hidden folders, and stops at 20,000 pictures. Starred pictures outside those folders are added on top of that limit, since they are the person's own choice. A starred picture whose real file has no picture extension (a link named `a.jpg` pointing at `raw-file`) is kept when its first bytes are a supported format. An empty section still shows the read-error, stopped-early and could-not-save notes, and says "could not be read" only when a folder really failed. Formats: JPEG, PNG, WebP, BMP, GIF.
- Tiles are 100 to 250 px high (squares or the photo's own proportions, kept between 0.6 and 2 times the height), decoded four at a time off the UI thread, upright by their EXIF rotation. Tiles load as you scroll: 200 to start and 200 more each time you scroll near the bottom (about 92 MB of pixels for a full set of 240 x 240 RGBA tiles, and thumbnails already loaded stay in memory when you change section, so the limit is per view, not for the whole cache, which can grow with every section you open). Changing section starts again at 200. The grid is not virtualised yet.
- The full picture is decoded when opened and scaled down once if it is larger than 3840 px on either side.
  - **Albums**: the side bar lists them with **New album**. Select photos, then **Add to album** in the toolbar picks an album or makes a new one (a name box appears under the toolbar; Esc closes it). Inside an album, the remove button takes the selected photos out of the album only, never deletes files, and **More > Delete this album** removes the album and leaves the photos alone.
  - **Slideshow**: **More > Slideshow** in the gallery, or the play button in the viewer. It starts at the open photo (or the first selected one in gallery order, or the first shown). A photo opened from Files that is outside the section plays on its own, as a list of one. It follows what the current section shows, moves on every 4 seconds (More > Slideshow speed steps through 2, 4, 8 and 15, kept in `settings.json`) and starts over after the last photo. The button turns into pause, and Esc or Back to the gallery stops it. Left and Right still work while it plays.
  - **Add folder**: the folder drop-down in the toolbar has an **Add folder...** item, which opens the usual folder dialog. The folder is searched like Pictures, Downloads, Desktop and Documents (a newly added folder is scanned first and has its own budget, so a large Pictures folder cannot crowd it out), shows up in the drop-down under its own name, and is remembered in `folders.json`. **More > Stop looking in this folder** forgets it again (the folder itself is not touched).
  - **Rename this album**: in an album, **More > Rename this album** shows the name box with the current name. A name that another album already uses is refused.
  - **Password for Hidden**: in Hidden, **More > Set a password** (later **Change** or **Remove the password**). With a password set, Hidden shows a lock screen until it is typed, and locks again when you leave the section. It is a salted, repeated SHA-256 in `hidden-lock.json`. It keeps Hidden out of casual sight; the pictures are not encrypted and their files stay where they are. Only a missing `hidden-lock.json` means no password: a file that cannot be read or understood keeps Hidden locked and says why, instead of opening it. The saved password is read again whenever Hidden is entered, unlocked, set, changed or removed, so a change made in another Photos window takes effect here too and closes an already open Hidden. The same goes for `hidden.json`: if it cannot be read, no pictures are shown until it is fixed, rather than showing ones that were hidden.
  - **One name per picture**: hidden photos, the gallery filter and the scan's duplicate check use the picture's canonical path (symlinks and `..` resolved), so a second path to a hidden picture cannot show it. Entries saved under the old spelling still count.
- Favorites and hidden photos are saved as path lists in `~/.config/dev.lapinux.Photos/favorites.json` and `hidden.json`, each written under its own lock file and swapped in atomically. Albums are a name to path-list map in `albums.json`, saved the same way (`albums.lock`). Added folders are a path list in `folders.json` (`folders.lock`).
- Favorites are saved in `favorites.json`. A failed save is logged and never ends the program (#174).

- Turn left / Turn right in the viewer toolbar rewrite the file turned a quarter (JPEG, PNG, WebP, BMP; not GIF). The camera's rotation tag is baked in and dropped, the write goes through a temporary file that is created exclusively (`create_new`, a random name beside the picture, owner-only while it is written, so a leftover file or a planted link is never written through), given the picture's own permissions back, synced, renamed over the original and the folder synced so the rename survives a crash, and JPEG is compressed once more.
- Rotate (viewer): follows a symlink to the real file and replaces that, so the link stays a link and the picture keeps its name in Favorites, albums and Hidden. If the folder cannot be flushed after the swap, the new picture is still shown and a note says the disk has not confirmed it yet.

## Coexistence

Photos writes only its own `dev.lapinux.Photos` folder, so nothing is added to `items()` in `tools/lapin-ux-uninstall`. It replaces no COSMIC package.

## Not yet

Editing beyond turning (crop, brightness).
