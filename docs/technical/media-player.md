# Media Player (`lapin-ux-media-player`)

An offline player for music, radio and video (#222). App ID `dev.lapinux.MediaPlayer`, package `lapin-ux-media-player`. Start search finds "Media Player"; About and Apps say "Lapin Media Player". Nothing leaves the computer until you play a station you added yourself or type a web address into Open.

## Engine

The player runs `mpv` as its own process and drives it over mpv's JSON IPC socket (`--input-ipc-server`). Reasons:

- No linking, so mpv's GPL stays on its side of the process boundary and the player carries no fork patches (`git diff packaging/` shows no change to any `*.patch`).
- A codec crash ends mpv, not the window. The player shows that playback stopped.
- Music and radio use this process only. Video is the one exception, see "Video in the window" below.
- mpv runs with `--config-dir=~/.config/dev.lapinux.MediaPlayer/mpv`, so `~/.config/mpv` and stock mpv are never read or changed.

When libmpv cannot start, video falls back to this process and opens in mpv's own window (`--wayland-app-id=dev.lapinux.MediaPlayer.Video`), with a notice.

## Two views

- **Music library**: a sidebar with Listen Now, Artists, Albums, Songs, Favourite Songs, Videos, Radio and Made for You, a search box, and a transport bar (previous, play/pause, next, shuffle, repeat, seek, volume, output device, queue). Cards show covers as coloured tiles.
- **Video mode**: Videos lists the video files found; playing one shows the picture inside the window (see below) with the playlist beside it.

The menu bar has File, Edit, Media, View, Controls, Playlist, Window and Help. The window follows the system light or dark theme and does not set a theme of its own.

## How it works

- The scan walks Music and Videos in the background and builds the library from file names and folders (artist and album from the folder layout). The library keeps the folder-derived names. Real tags (title, artist, album) are read from mpv while a file plays and shown in the transport bar and the desktop's media controls; reading tags for the whole library, and cover art, are not done yet.
- Open and import/export ask for a path or address in a strip above the transport bar. `.m3u` and `.pls` playlists load; the queue exports as `.m3u`.
- Open takes a path, `~/...`, a `file://` address (percent codes are decoded) or a web address. Relative paths are resolved against the current folder, and an exported playlist always holds absolute paths. Playlists keep `=` in plain `.m3u` paths, apply `.pls` rules only inside `[playlist]`, and drop entries with a web address that is not http, https, mms, rtmp or rtsp. A target with a line break is refused on export.
- Files with names that are not valid UTF-8 are skipped by the scan, and the player shows how many entries it could not read.
- A file that fails to open is skipped; when every queue item fails in a row playback stops with a notice instead of cycling. Repeat One does not replay a failed file. mpv's end-of-file event for the file being replaced is ignored, so changing tracks never skips one. A refused seek (for example in a live stream) puts the position back and shows a notice. Streams are not counted in play counts.
- If the stations file cannot be read, the player shows a notice and does not overwrite it; a missing file just means no stations yet.
- Without `XDG_RUNTIME_DIR` the mpv socket lives in a private (0700) folder under the temp directory, removed on exit. mpv is stopped if it does not start in time.
- Radio has no built-in stations. You add a name and a stream address, import `.m3u` or `.json`, and export both. No network call happens until you play a station.
- Play counts are kept in `~/.config/dev.lapinux.MediaPlayer/plays.json` and drive Listen Now and Made for You. Stations are saved in the same folder, written atomically.
- Icon-only buttons have accessible names and the surfaces are grouped with `lapin_ux_ui::a11y::group`.

## Media keys, MPRIS and sleep

`mpris.rs` serves `org.mpris.MediaPlayer2` and `.Player` on the session bus as `org.mpris.MediaPlayer2.dev.lapinux.MediaPlayer`. Media keys, the desktop's media controls and `playerctl` can play, pause, skip, stop, seek and raise the window. App state is published through a `watch` channel and commands return as messages, so the window stays the only owner of playback. While something plays the player holds a logind `idle:sleep` inhibitor (dropped on pause or stop), so the screen does not lock or sleep mid-song. With no session bus the player works as before and only logs a warning.

## Favourites, output device, mini player

- Favourite Songs: a star on every song row and on the transport card. The list is kept in `~/.config/dev.lapinux.MediaPlayer/favourites.json`.
- Output device: the speaker button (or View, Output device) lists the outputs mpv finds (`audio-device-list`) and switches with `audio-device`. The choice is saved in `output-device` in the player's own folder and applied when mpv starts. It never touches the desktop's sound settings.
- Mini player: View, Mini player shrinks the window to the transport bar with a "Full player" button to come back.

## Default player

The package ships `/usr/share/applications/lapinux-mimeapps.list`, the desktop-specific defaults file for `XDG_CURRENT_DESKTOP=LapinUX`. Only a Lapin session reads it, other desktops never do, and `~/.config/mimeapps.list` still wins over it. Nothing is written into the user's own `mimeapps.list`, so no `items()` entry is needed, and it is removed with the package.

## Coexistence

The player writes only its own `dev.lapinux.MediaPlayer` folder, so nothing is added to `items()` in `tools/lapin-ux-uninstall`. It replaces no COSMIC package, so nothing goes in `LAPIN_PKGS` or `STOCK`. Uninstalling never deletes the library data folder.

## Dialogs, tags, covers and Settings

- Open, Open folder, playlists and station import or export use the system file dialogs through the portal (`pick.rs`).
- Titles, artists and albums are read from the files with `ffprobe` (time-limited, four at a time) and cached in `tags.json` by path, size and modified time. Album pictures come from `cover`, `folder` or `front` images in the album folder, or from a picture inside the file via `ffmpeg`, scaled to 256 px and cached in `covers/`. A scan that was replaced by a newer one is ignored.
- Settings (`settings.json`, the last item in the sidebar): library folders, reading tags, showing album pictures, sound output and clearing the play history. Nothing here is shared with another app.

## Video in the window

- Video only runs through libmpv (`libmpv2-sys`, raw bindings, linked at build time). Music, radio, MPRIS, the queue and output device code are unchanged and still use the separate mpv process. The package depends on `mpv`, which provides `libmpv.so` on Arch.
- A thread named `lapin-video` owns the mpv handle and a software render context. It reports through the same `engine::Event` type, so the rest of the player does not care which engine plays. Frames are drawn into an RGBA buffer at the size of the picture area (at most 1920x1080) and shown as an image. At most two frames wait for the window; a newer frame replaces an older one that was not shown.
- `config=no` and an own config folder: `~/.config/mpv` is never read or changed. Nothing touches the network until you open a web address.
- **Hardware decoding** is a Settings switch, on by default: "Use the graphics card to play video". On means `hwdec=auto-safe`. Because the software render path cannot take pictures that stay on the card, methods that only work zero-copy (vaapi, nvdec, cuda, drm and similar) are changed to their `-copy` form, and if nothing worked `auto-copy-safe` is tried once. Off means software decoding. Changing the switch restarts the video engine and stops a playing video.
- **Zero-copy** (a dmabuf imported into a wgpu texture, or a Wayland subsurface under the window) is not done. It needs changes below libcosmic's iced renderer and a per-driver import path, which is not small or safe to add here. The copy back costs some CPU on 4K but 1080p is fine. It stays a later stage.
- The Video page has the picture, an auto-hiding control bar and the playlist beside it. The bar hides after about 3 s without mouse movement, shows on movement or any key, and never hides while paused. F or a double click enters full screen, Esc leaves it. The bar has a Chapters button (more than one chapter), a Video tracks button (more than one real video track; cover art is not counted), an Audio tracks button (more than one audio track) and a Subtitles button (any subtitle track), then an A-B loop button, a speed button and Adjust. Each list opens above the bar with the current choice highlighted; Subtitles also has Off. Tracks are read from libmpv's `track-list` and chosen with `aid`, `sid` and `vid`; chapters come from `chapter-list` and a choice sets `chapter`. Speed offers 0.25x to 2x (`speed`). The A-B loop button cycles with mpv's `ab-loop` command: first press sets the start (A), second the end (B) and loops, third clears; its name tells which press is next. All of these are cleared when a new file starts. An Adjust button opens a panel with Add subtitle file (the system file dialog, or a typed path if the dialog cannot open; sent to mpv as `sub-add` and selected at once), Subtitle delay and Sound delay (`sub-delay`, `audio-delay`, steps of 0.1 s up to 60 s either way) Subtitle size (`sub-scale`, steps of 10%, 30% to 300%), Subtitle position (`sub-pos`, steps of 5, top 0 to bottom 100) and Subtitle colour (`sub-color`: white, yellow, green, cyan). Pressing a value resets it, and each new file starts from the defaults. Nothing is saved. Icon-only buttons are named and surfaces are grouped for screen readers.
- If libmpv fails to start (missing library, no render context), the player says so and plays video in mpv's own window as before.

## Retest on hardware

Compiled and unit-tested only. Needs a real display with a video file: picture appears inside the window and resizes with it; audio and seek work; the bar hides after 3 s and returns on movement or a key, and stays while paused; F, double click and Esc; the playlist panel follows the playing file; switching the graphics card setting off and on; a file whose codec only decodes on the card; opening a file named on the command line; audio and subtitle menus on a file with two audio tracks and several subtitles (choose, Off, highlighted choice follows); Adjust panel: add an .srt next to a video, subtitle delay and sound delay change the timing as you press, subtitle size grows and shrinks the text, subtitle position moves the text up and down, the colour buttons recolour it, speed menu changes speed and the label on the bar follows, A-B loop (press three times) repeats the part between and clears, chapter menu jumps and highlights the current chapter on a file with chapters, video track menu on a file with two video tracks, reset by pressing the value, values back to normal on the next file; fallback window when libmpv is unavailable.

## Not yet

Zero-copy video (see above).
