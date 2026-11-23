# Photos (`lapin-ux-photos`)

An offline picture app (#74). Nothing leaves the computer: no cloud, no accounts, no network.

## Two views

- **Gallery**: follows the person's light or dark theme (#200). A toolbar on top, an optional side bar (Gallery, Favorites, Recents, then Media types: Screenshots, Animated GIFs, and Hidden, then your albums) and the grid.
  - Toolbar: side bar toggle, **Source** (all folders, or Pictures, Downloads, Desktop, Documents, shown only when they exist), **Group** (Years, Months, Days, All), a square or own-proportions grid toggle, a size slider, **Select**, and a More menu (Look for new pictures).
  - The size slider has six steps. The last step is a reel: one large photo with the others in a strip below; clicking the large photo opens the viewer.
  - **Select** shows a tick on each tile you click. Click selects, Shift+click selects a range, Ctrl+click adds one, Ctrl+A selects all, Esc clears. While photos are selected the toolbar offers Select all, favorite or unfavorite, and move to the Recycle Bin (asks first, uses `gio trash`).
  - **Recents** shows the last three days.
  - **Screenshots** shows pictures in a folder or with a file name starting "Screenshot". **Animated GIFs** shows `.gif` files.
  - **Hide** (in the selection toolbar) moves the selected photos out of every section except **Hidden**. In Hidden the same button reads **Unhide**. Without a password, hiding keeps the file where it is and only changes what Photos shows. With a password (see below), hidden pictures are encrypted.
- **Viewer**: one photo at a time with a floating toolbar (back, previous, next, favorite, file name) and a filmstrip of the nearby photos. Left and Right step through the list, Esc goes back to the gallery.

Opening a picture from Files (`lapin-ux-photos %f`) goes straight to the viewer, also when the picture is outside the scanned folders.

The side bar, source, group, grid style and size are remembered in `~/.config/dev.lapinux.Photos/settings.json`.

## How it works

- The scan walks the four folders in the background, up to 5 levels deep, skipping hidden folders, and stops at 20,000 pictures. Starred pictures outside those folders are added on top of that limit, since they are the person's own choice. An empty section still shows the read-error, stopped-early and could-not-save notes, and says "could not be read" only when a folder really failed. Formats: JPEG, PNG, WebP, BMP, GIF.
- Tiles are 100 to 250 px high (squares or the photo's own proportions, kept between 0.6 and 2 times the height), decoded four at a time off the UI thread, upright by their EXIF rotation. Thumbnails far from the visible stretch are dropped from memory (see "Tiles load as you scroll").
- The full picture is decoded when opened and scaled down once if it is larger than 3840 px on either side.
  - **Albums**: the side bar lists them with **New album**. Select photos, then **Add to album** in the toolbar picks an album or makes a new one (a name box appears under the toolbar; Esc closes it). Inside an album, the remove button takes the selected photos out of the album only, never deletes files, and **More > Delete this album** removes the album and leaves the photos alone.
  - **Slideshow**: **More > Slideshow** in the gallery, or the play button in the viewer. It starts at the open photo (or the first selected one, or the first shown), follows what the current section shows, moves on every 4 seconds and starts over after the last photo. The button turns into pause, and Esc or Back to the gallery stops it. Left and Right still work while it plays.
  - **Add folder**: the folder drop-down in the toolbar has an **Add folder...** item, which opens the usual folder dialog. The folder is searched like Pictures, Downloads, Desktop and Documents, shows up in the drop-down under its own name, and is remembered in `folders.json`. **More > Stop looking in this folder** forgets it again (the folder itself is not touched).
  - **Rename this album**: in an album, **More > Rename this album** shows the name box with the current name. A name that another album already uses is refused.
  - **Password for Hidden (the vault)**: in Hidden, **More > Set a password** (later **Change** or **Remove the password**). Setting a password makes an encrypted vault and moves the pictures already hidden into it. From then on Hide moves a picture into the vault (the original file is deleted once the encrypted copy and the index are saved) and Unhide puts it back in the Pictures folder under its old name (never over a file that is there). With a password set, Hidden shows a lock screen until it is typed and locks again when you leave the section. A forgotten password cannot be recovered: the set-password box says so. Inside the vault there are no favorites, albums, Recycle Bin, turn or edit; the only way out is Unhide. **Remove the password** (needs Hidden open) takes everything out, puts the pictures back on the plain Hidden list and deletes the vault; if one picture cannot come out, the vault and password stay.
    - Crypto (`vault.rs`): Argon2id (19 MiB, 2 passes, 1 lane, a random salt) turns the password into a key that seals a random data key (ChaCha20-Poly1305). Each picture is its own ChaCha20-Poly1305 file, `<id>.bin`, with the id as associated data, so a file swapped or renamed is refused. The list of pictures (name, date) is sealed the same way, so file names are not readable on disk. A cost stored in a changed key file is bounded before it is used. Pictures are decrypted in memory only, for thumbnails and the viewer; nothing readable is written to disk. The key is held in memory after the first unlock so Hide works from other sections and is dropped when the password is removed or the program ends. It does not protect against someone who can read the memory of the running program.
    - Files: `~/.local/share/dev.lapinux.Photos/vault/` (key file, sealed index, `<id>.bin`). Kept in the data folder, not the config folder, so a settings reset never destroys the pictures.
- Favorites and hidden photos (those hidden without a password) are saved as path lists in `~/.config/dev.lapinux.Photos/favorites.json` and `hidden.json`, each written under its own lock file and swapped in atomically. Albums are a name to path-list map in `albums.json`, saved the same way (`albums.lock`). Added folders are a path list in `folders.json` (`folders.lock`).
- Favorites are saved in `favorites.json`. A failed save is logged and never ends the program (#174).

## Coexistence

Photos writes only its own `dev.lapinux.Photos` folder, so nothing is added to `items()` in `tools/lapin-ux-uninstall`. It replaces no COSMIC package. The Hidden vault lives in `~/.local/share/dev.lapinux.Photos/vault/`; `lapin-ux-uninstall` never touches that folder, because it holds the person's own pictures, and uninstalling Lapin UX must not delete them. (If the Photos app is removed the pictures stay encrypted there.)
- Slideshow speed: More > "Slideshow speed" steps 2, 4, 8, 15 seconds, saved as `slide_secs` in `settings.json`. The viewer toolbar has a Full screen button; leaving the viewer leaves full screen.
- Tiles load as you scroll: the grid starts with 200 and adds 200 more each time you scroll near the bottom, so the old "Showing the newest 400" line is gone. Changing section starts again at 200. Only the tiles near what you are looking at (about 150 either side) keep their thumbnail in memory; the others are dropped and decoded again when you scroll back, so a very long scroll no longer grows memory. Changing the source folder also starts the grid again at 200.
- Turn left / Turn right in the viewer toolbar rewrite the file turned a quarter (a failed turn shows a note under the toolbar, cleared on the next try) (JPEG, PNG, WebP, BMP; not GIF). The camera's rotation tag is baked in and dropped, the write goes through a temporary file, and JPEG is compressed once more.
- Edit in the viewer: the pencil button shows Brighter, Darker, More contrast, Less contrast, Black and white, and Crop square, 16:9 or 4:3 (the middle of the picture is kept). Every edit is saved as a copy, "name (edited).ext" beside the original (or "(edited 2)" and so on, never over an existing file), and the copy opens. Editing a copy edits it in place, so repeated tweaks do not pile up files. JPEG, PNG, WebP and BMP; not GIF. The write goes through a temporary file.

## Not yet

Free-hand crop (dragging a box), undo for edits, and a brightness slider.
