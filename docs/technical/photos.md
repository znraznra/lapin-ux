# Photos (`lapin-ux-photos`)

An offline picture app (#74). Nothing leaves the computer: no cloud, no accounts, no network.

## Two views

- **Gallery** (dark): a sidebar with Gallery, Favorites and your folders (Pictures, Downloads, Desktop, Documents, shown only when they exist), and a grid of tiles grouped by date (Today, Yesterday, then the full date), newest first.
- **Viewer** (light): one photo at a time with a floating toolbar (back, previous, next, favorite, file name) and a filmstrip of the nearby photos. Left and Right step through the list, Esc goes back to the gallery.

Opening a picture from Files (`lapin-ux-photos %f`) goes straight to the viewer, also when the picture is outside the scanned folders.

Each view sets its own theme on its content (`iced::widget::themer`). The title bar stays in the system theme.

## How it works

- The scan walks the four folders in the background, up to 5 levels deep, skipping hidden folders, and stops at 20,000 pictures. Starred pictures outside those folders are added on top of that limit, since they are the person's own choice. An empty section still shows the read-error, stopped-early and could-not-save notes, and says "could not be read" only when a folder really failed. Formats: JPEG, PNG, WebP, BMP, GIF.
- Tiles are 170 px squares, decoded four at a time off the UI thread, upright by their EXIF rotation. At most 400 tiles are loaded per section (about 46 MB). The grid is not virtualised yet.
- The full picture is decoded when opened and scaled down once if it is larger than 3840 px on either side.
- Favorites are saved in `~/.config/dev.lapinux.Photos/favorites.json`. A failed save is logged and never ends the program (#174).

## Coexistence

Photos writes only its own `dev.lapinux.Photos` folder, so nothing is added to `items()` in `tools/lapin-ux-uninstall`. It replaces no COSMIC package.

## Not yet

Lazy tile loading on scroll, delete, rotate, slideshow. Edit opens the picture in the [snip editor](snip-editor.md).
