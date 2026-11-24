# Photos (`lapin-ux-photos`)

An offline picture app (#74). Nothing leaves the computer: no cloud, no accounts, no network.

## Two views

- **Gallery**: a sidebar with Gallery, Favorites and your folders (Pictures, Downloads, Desktop, Documents, shown only when they exist), and a grid of tiles grouped by date (Today, Yesterday, then the full date), newest first.
- **Viewer**: one photo at a time. The toolbar (Back with its label, file name, More actions, previous, next, favorite) sits above the picture and the filmstrip of nearby photos below it, so the picture is fitted into the space between them and never drawn under either bar (#201). Left and Right step through the list, Esc closes an open menu first and then goes back to the gallery.

**More actions** (#202): Rename (keeps the picture's place, thumbnail and star, and never replaces another file), Delete (asks first, then moves the picture to the Recycle Bin with `gio trash`, never a permanent delete; the viewer moves to the next photo, or the one before it, or back to the gallery when it was the last) and Open file location.

Opening a picture from Files (`lapin-ux-photos %f`) goes straight to the viewer, also when the picture is outside the scanned folders.

Both views follow the system theme, light by default (#200). There are no theme overrides in the app.

## How it works

- The scan walks the four folders in the background, up to 5 levels deep, skipping hidden folders, and stops at 20,000 pictures. Starred pictures outside those folders are added on top of that limit, since they are the person's own choice. An empty section still shows the read-error, stopped-early and could-not-save notes, and says "could not be read" only when a folder really failed. Formats: JPEG, PNG, WebP, BMP, GIF.
- Tiles are 170 px squares, decoded four at a time off the UI thread, upright by their EXIF rotation. At most 400 tiles are loaded per section (about 46 MB). The grid is not virtualised yet.
- The full picture is decoded when opened and scaled down once if it is larger than 3840 px on either side.
- Favorites are saved in `~/.config/dev.lapinux.Photos/favorites.json`. A failed save is logged and never ends the program (#174).

## Coexistence

Photos writes only its own `dev.lapinux.Photos` folder, so nothing is added to `items()` in `tools/lapin-ux-uninstall`. It replaces no COSMIC package.

## Not yet

Lazy tile loading on scroll, delete, rotate, slideshow. Edit opens the picture in the [snip editor](snip-editor.md).
