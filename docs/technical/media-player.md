# Media Player (`lapin-ux-media-player`)

An offline player for music, radio and video (#222). App ID `dev.lapinux.MediaPlayer`, package `lapin-ux-media-player`. Start search finds "Media Player"; About and Apps say "Lapin Media Player". Nothing leaves the computer until you play a station you added yourself.

## Engine

The player runs `mpv` as its own process and drives it over mpv's JSON IPC socket (`--input-ipc-server`). Reasons:

- No linking, so mpv's GPL stays on its side of the process boundary and the player carries no fork patches (`git diff packaging/` shows no change to any `*.patch`).
- A codec crash ends mpv, not the window. The player shows that playback stopped.
- No `libmpv` headers are needed to build. The package depends on `mpv`.
- mpv runs with `--config-dir=~/.config/dev.lapinux.MediaPlayer/mpv`, so `~/.config/mpv` and stock mpv are never read or changed.

Embedding libmpv's render output inside the window is a later stage, after a spike. Until then video opens in mpv's own window (`--wayland-app-id=dev.lapinux.MediaPlayer.Video`).

## Two views

- **Music library**: a sidebar with Listen Now, Artists, Albums, Songs, Videos, Radio and Made for You, a search box, and a transport bar (previous, play/pause, next, shuffle, repeat, seek, volume, queue). Cards show covers as coloured tiles.
- **Video mode**: Videos lists the video files found; playing one hands it to mpv's window while the transport bar keeps control of it.

The menu bar has File, Edit, Media, View, Controls, Playlist, Window and Help. The window follows the system light or dark theme and does not set a theme of its own.

## How it works

- The scan walks Music and Videos in the background and builds the library from file names and folders (artist and album from the folder layout). Tags and cover art are not read yet.
- Open and import/export ask for a path or address in a strip above the transport bar. `.m3u` and `.pls` playlists load; the queue exports as `.m3u`.
- Radio has no built-in stations. You add a name and a stream address, import `.m3u` or `.json`, and export both. No network call happens until you play a station.
- Play counts are kept in `~/.config/dev.lapinux.MediaPlayer/plays.json` and drive Listen Now and Made for You. Stations are saved in the same folder, written atomically.
- Icon-only buttons have accessible names and the surfaces are grouped with `lapin_ux_ui::a11y::group`.

## Coexistence

The player writes only its own `dev.lapinux.MediaPlayer` folder, so nothing is added to `items()` in `tools/lapin-ux-uninstall`. It replaces no COSMIC package, so nothing goes in `LAPIN_PKGS` or `STOCK`. Uninstalling never deletes the library data folder.

## Not yet

Video inside the window with an auto-hiding bar and playlist panel, media keys and MPRIS with a playback inhibit, tag and cover-art reading, favourite tracks, mini player, native file dialogs, output device choice, the full Settings pages, and setting it as the default player (a `mimeapps.list` write in the Lapin session only, with an `items()` entry and a `key_is_lapins` test).
