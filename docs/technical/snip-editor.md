# Snip editor (`lapin-ux-snip-editor`)

A small app to mark up a screenshot (#78): pen, highlighter, crop, undo and redo, then Copy or Save. COSMIC has no editor for snips, so this is a new app (app id `dev.lapinux.SnipEditor`, modelled on Photos).

**Ways in**
- After a snip, the notification has an **Edit** button (`snip.rs` in the shell). It opens the editor on the saved file. The button stays on the notification in the notification centre.
- Photos' viewer toolbar has an **Edit** button for the picture on screen.
- `lapin-ux-snip-editor <picture>` from a terminal or a launcher entry. Without a picture it says how to get one.
- Not done: an Edit entry in Explorer's right-click menu. That is a patch in the Files fork, so it is its own piece of work.

**Tools**
- **Pen**: a solid line in one of six colours, with a size slider.
- **Highlighter**: the same colours, wide and see-through. A line that crosses itself stays one colour, because each stroke is blended once.
- **Crop**: drag the edges, corners or the inside of the frame, then Apply crop. Applying keeps what is drawn so far in the picture and can be undone.
- Ctrl+Z undo, Ctrl+Shift+Z or Ctrl+Y redo, Ctrl+C copy, Ctrl+S save.
- Line widths follow the picture size, so a mark looks the same on a small and a large screen.

**Copy and Save**
- Copy puts a PNG on the clipboard (`wl-copy`), like the snipping bar.
- Save writes a new PNG in Pictures/Screenshots (`Screenshot <date> <time> edited.png`). It never overwrites the original snip.

**How it is built**
- `model.rs` holds all the maths with no UI in it: stroke drawing on a pixel buffer, crop handles and limits, fitting the picture to the window, mapping the pointer back to picture pixels, and the undo history (50 steps). It has unit tests.
- `main.rs` is the libcosmic app. The window shows a scaled-down preview (at most 1600 px) redrawn after every change; Copy and Save draw onto the full-size picture.
- Icon-only buttons are named for screen readers, and the window is one group (`lapin_ux_ui::a11y`).

**Coexistence**
The editor writes no COSMIC or GTK setting and nothing else outside Pictures/Screenshots, so nothing is added to `items()` in `tools/lapin-ux-uninstall`. It replaces no COSMIC package, so it is not in `LAPIN_PKGS` or `STOCK`. It has no session gating, like Photos.
