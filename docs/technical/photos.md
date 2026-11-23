# Photos (`lapin-ux-photos`)

An offline picture app (#74). Nothing leaves the computer: no cloud, no accounts, no network.

## Two views

- **Gallery**: follows the person's light or dark theme (#200). A toolbar on top, an optional side bar (Gallery, Favorites, Recents, then Media types: Screenshots, Animated GIFs, and Hidden) and the grid.
  - Toolbar: side bar toggle, **Source** (all folders, or Pictures, Downloads, Desktop, Documents, shown only when they exist), **Group** (Years, Months, Days, All), a square or own-proportions grid toggle, a size slider, **Select**, and a More menu (Look for new pictures).
  - The size slider has six steps. The last step is a reel: one large photo with the others in a strip below; clicking the large photo opens the viewer.
  - **Select** shows a tick on each tile you click. Click selects, Shift+click selects a range, Ctrl+click adds one, Ctrl+A selects all, Esc clears. While photos are selected the toolbar offers Select all, favorite or unfavorite, and move to the Recycle Bin (asks first, uses `gio trash`).
  - **Recents** shows the last three days.
  - **Screenshots** shows pictures in a folder or with a file name starting "Screenshot". **Animated GIFs** shows `.gif` files.
  - **Hide** (in the selection toolbar) moves the selected photos out of every section except **Hidden**. In Hidden the same button reads **Unhide**. Hiding keeps the file where it is; it only changes what Photos shows.
- **Viewer**: one photo at a time with a floating toolbar (back, previous, next, favorite, file name) and a filmstrip of the nearby photos. Left and Right step through the list, Esc goes back to the gallery.

Opening a picture from Files (`lapin-ux-photos %f`) goes straight to the viewer, also when the picture is outside the scanned folders.

The side bar, source, group, grid style and size are remembered in `~/.config/dev.lapinux.Photos/settings.json`.

## How it works

- The scan walks the four folders in the background, up to 5 levels deep, skipping hidden folders, and stops at 20,000 pictures. Starred pictures outside those folders are added on top of that limit, since they are the person's own choice. An empty section still shows the read-error, stopped-early and could-not-save notes, and says "could not be read" only when a folder really failed. Formats: JPEG, PNG, WebP, BMP, GIF.
- Tiles are 100 to 250 px high (squares or the photo's own proportions, kept between 0.6 and 2 times the height), decoded four at a time off the UI thread, upright by their EXIF rotation. At most 400 tiles are loaded per section (about 46 MB). The grid is not virtualised yet.
- The full picture is decoded when opened and scaled down once if it is larger than 3840 px on either side.
- Favorites and hidden photos are saved as path lists in `~/.config/dev.lapinux.Photos/favorites.json` and `hidden.json`, each written under its own lock file and swapped in atomically.
- Favorites are saved in `favorites.json`. A failed save is logged and never ends the program (#174).

## Coexistence

Photos writes only its own `dev.lapinux.Photos` folder, so nothing is added to `items()` in `tools/lapin-ux-uninstall`. It replaces no COSMIC package.

## Not yet

Albums and Add to Album (separate pull requests), a lock or password for Hidden, slideshow, Add folder, lazy tile loading on scroll, rotate and edit.
