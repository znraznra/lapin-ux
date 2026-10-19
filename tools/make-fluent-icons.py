#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Build Lapin UX's symbolic icon theme from Microsoft's Fluent UI System
Icons (MIT): each freedesktop symbolic icon name below is drawn with a
Fluent glyph. Apps' own (full-colour) icons come from the Win11 theme,
which this theme inherits.

usage: make-fluent-icons.py <fluentui-system-icons checkout> <output dir>

Writes <out>/Lapin-UX and <out>/Lapin-UX-dark (identical glyphs; the dark
one inherits Win11-dark).
"""
import os
import re
import shutil
import sys

# freedesktop name (without -symbolic) -> (context, fluent name, style)
# style: "regular" (outline) or "filled".
MAP = {
    # actions
    "list-add": ("actions", "add", "regular"),
    "list-remove": ("actions", "subtract", "regular"),
    "window-close": ("actions", "dismiss", "regular"),
    "window-maximize": ("actions", "maximize", "regular"),
    "window-minimize": ("actions", "subtract", "regular"),
    "window-restore": ("actions", "square_multiple", "regular"),
    "window-new": ("actions", "window_new", "regular"),
    "go-next": ("actions", "chevron_right", "regular"),
    "go-previous": ("actions", "chevron_left", "regular"),
    "go-up": ("actions", "chevron_up", "regular"),
    "go-down": ("actions", "chevron_down", "regular"),
    "go-home": ("actions", "home", "regular"),
    "pan-end": ("actions", "chevron_right", "regular"),
    "pan-start": ("actions", "chevron_left", "regular"),
    "pan-up": ("actions", "chevron_up", "regular"),
    "pan-down": ("actions", "chevron_down", "regular"),
    "object-select": ("actions", "checkmark", "regular"),
    "edit-copy": ("actions", "copy", "regular"),
    "edit-cut": ("actions", "cut", "regular"),
    "edit-paste": ("actions", "clipboard_paste", "regular"),
    "edit-delete": ("actions", "delete", "regular"),
    "edit-clear": ("actions", "dismiss", "regular"),
    "edit": ("actions", "edit", "regular"),
    "document-edit": ("actions", "edit", "regular"),
    "edit-find": ("actions", "search", "regular"),
    "edit-undo": ("actions", "arrow_undo", "regular"),
    "edit-redo": ("actions", "arrow_redo", "regular"),
    "edit-select-all": ("actions", "select_all_on", "regular"),
    "document-open": ("actions", "open", "regular"),
    "document-save": ("actions", "save", "regular"),
    "document-save-as": ("actions", "save_edit", "regular"),
    "document-new": ("actions", "document_add", "regular"),
    "document-print": ("actions", "print", "regular"),
    "document-properties": ("actions", "info", "regular"),
    "folder-new": ("actions", "folder_add", "regular"),
    "view-refresh": ("actions", "arrow_clockwise", "regular"),
    "view-more": ("actions", "more_horizontal", "regular"),
    "view-more-horizontal": ("actions", "more_horizontal", "regular"),
    "open-menu": ("actions", "navigation", "regular"),
    "view-app-grid": ("actions", "apps", "regular"),
    "view-grid": ("actions", "grid", "regular"),
    "view-sort-ascending": ("actions", "arrow_sort", "regular"),
    "view-sort-descending": ("actions", "arrow_sort", "regular"),
    "view-list": ("actions", "text_bullet_list_ltr", "regular"),
    "view-paged": ("actions", "window_multiple", "regular"),
    "view-pin": ("actions", "pin", "regular"),
    "view-unpin": ("actions", "pin_off", "regular"),
    "pin": ("actions", "pin", "regular"),
    "view-reveal": ("actions", "eye", "regular"),
    "view-conceal": ("actions", "eye_off", "regular"),
    "view-fullscreen": ("actions", "full_screen_maximize", "regular"),
    "view-restore": ("actions", "full_screen_minimize", "regular"),
    "zoom-in": ("actions", "zoom_in", "regular"),
    "zoom-out": ("actions", "zoom_out", "regular"),
    "zoom-fit-best": ("actions", "zoom_fit", "regular"),
    "system-search": ("actions", "search", "regular"),
    "system-shutdown": ("actions", "power", "regular"),
    "system-reboot": ("actions", "arrow_clockwise", "regular"),
    "system-log-out": ("actions", "sign_out", "regular"),
    "system-lock-screen": ("actions", "lock_closed", "regular"),
    "system-switch-user": ("actions", "person_swap", "regular"),
    "system-run": ("actions", "play", "regular"),
    "process-stop": ("actions", "dismiss_circle", "regular"),
    "media-playback-start": ("actions", "play", "filled"),
    "media-playback-pause": ("actions", "pause", "filled"),
    "media-playback-stop": ("actions", "stop", "filled"),
    "media-skip-forward": ("actions", "next", "filled"),
    "media-skip-backward": ("actions", "previous", "filled"),
    "media-record": ("actions", "record", "filled"),
    "send-to": ("actions", "share", "regular"),
    "mail-send": ("actions", "send", "regular"),
    "help-about": ("actions", "info", "regular"),
    "help-browser": ("actions", "question_circle", "regular"),
    "find-location": ("actions", "location", "regular"),
    "insert-image": ("actions", "image_add", "regular"),
    "insert-link": ("actions", "link", "regular"),
    "insert-text": ("actions", "text_add", "regular"),
    "list-drag-handle": ("actions", "re_order_dots_vertical", "regular"),
    "tab-new": ("actions", "tab_add", "regular"),
    "selection-mode": ("actions", "checkbox_checked", "regular"),
    "star-new": ("actions", "star_add", "regular"),
    "bookmark-new": ("actions", "bookmark_add", "regular"),
    "mark-location": ("actions", "location", "regular"),
    "call-start": ("actions", "call", "regular"),
    "call-stop": ("actions", "call_end", "regular"),
    "content-loading": ("actions", "more_horizontal", "regular"),
    # status
    "network-wireless-signal-excellent": ("status", "wifi_1", "regular"),
    "network-wireless-signal-good": ("status", "wifi_2", "regular"),
    "network-wireless-signal-ok": ("status", "wifi_3", "regular"),
    "network-wireless-signal-weak": ("status", "wifi_4", "regular"),
    "network-wireless-signal-none": ("status", "wifi_4", "regular"),
    "network-wireless": ("status", "wifi_1", "regular"),
    "network-wireless-connected": ("status", "wifi_1", "regular"),
    "network-wireless-acquiring": ("status", "wifi_3", "regular"),
    "network-wireless-offline": ("status", "wifi_off", "regular"),
    "network-wireless-disconnected": ("status", "wifi_off", "regular"),
    "network-wireless-no-route": ("status", "wifi_warning", "regular"),
    "network-wireless-hotspot": ("status", "wifi_settings", "regular"),
    "network-wireless-secure-signal-excellent": ("status", "wifi_lock", "regular"),
    "network-wireless-secure-signal-good": ("status", "wifi_lock", "regular"),
    "network-wireless-secure-signal-ok": ("status", "wifi_lock", "regular"),
    "network-wireless-secure-signal-weak": ("status", "wifi_lock", "regular"),
    "network-wired": ("status", "desktop_signal", "regular"),
    "network-wired-disconnected": ("status", "plug_disconnected", "regular"),
    "network-offline": ("status", "globe_off", "regular"),
    "network-idle": ("status", "globe", "regular"),
    "network-transmit-receive": ("status", "arrow_sort", "regular"),
    "network-vpn": ("status", "shield_lock", "regular"),
    "network-cellular-signal-excellent": ("status", "cellular_data_1", "regular"),
    "network-cellular-signal-good": ("status", "cellular_data_2", "regular"),
    "network-cellular-signal-ok": ("status", "cellular_data_3", "regular"),
    "network-cellular-signal-weak": ("status", "cellular_data_4", "regular"),
    "network-cellular-offline": ("status", "cellular_off", "regular"),
    "airplane-mode": ("status", "airplane", "regular"),
    "audio-volume-high": ("status", "speaker_2", "regular"),
    "audio-volume-medium": ("status", "speaker_1", "regular"),
    "audio-volume-low": ("status", "speaker_0", "regular"),
    "audio-volume-muted": ("status", "speaker_mute", "regular"),
    "audio-volume-overamplified": ("status", "speaker_2", "filled"),
    "microphone-sensitivity-high": ("status", "mic", "regular"),
    "microphone-sensitivity-muted": ("status", "mic_off", "regular"),
    "microphone-disabled": ("status", "mic_off", "regular"),
    "bluetooth-active": ("status", "bluetooth", "regular"),
    "bluetooth-disabled": ("status", "bluetooth_disabled", "regular"),
    "bluetooth-disconnected": ("status", "bluetooth_disabled", "regular"),
    "bluetooth-acquiring": ("status", "bluetooth_searching", "regular"),
    "bluetooth-paired": ("status", "bluetooth_connected", "regular"),
    "display-brightness": ("status", "brightness_high", "regular"),
    "night-light": ("status", "weather_moon", "regular"),
    "rotation-allowed": ("status", "orientation", "regular"),
    "rotation-locked": ("status", "phone_lock", "regular"),
    "weather-clear-night": ("status", "weather_moon", "regular"),
    "weather-clear": ("status", "weather_sunny", "regular"),
    "notification": ("status", "alert", "regular"),
    "notification-new": ("status", "alert_badge", "regular"),
    "notification-disabled": ("status", "alert_off", "regular"),
    "preferences-system-notifications": ("status", "alert", "regular"),
    "dialog-information": ("status", "info", "regular"),
    "dialog-warning": ("status", "warning", "regular"),
    "dialog-error": ("status", "error_circle", "regular"),
    "dialog-question": ("status", "question_circle", "regular"),
    "dialog-password": ("status", "key", "regular"),
    "changes-prevent": ("status", "lock_closed", "regular"),
    "changes-allow": ("status", "lock_open", "regular"),
    "security-high": ("status", "shield_checkmark", "regular"),
    "security-medium": ("status", "shield", "regular"),
    "security-low": ("status", "shield_error", "regular"),
    "power-profile-power-saver": ("status", "leaf_one", "regular"),
    "power-profile-balanced": ("status", "scales", "regular"),
    "power-profile-performance": ("status", "flash", "regular"),
    "alarm": ("status", "clock_alarm", "regular"),
    "content-loading": ("status", "more_horizontal", "regular"),
    "image-missing": ("status", "image_off", "regular"),
    "avatar-default": ("status", "person", "regular"),
    "user-available": ("status", "presence_available", "regular"),
    "user-idle": ("status", "presence_away", "regular"),
    "user-busy": ("status", "presence_busy", "regular"),
    "user-offline": ("status", "presence_offline", "regular"),
    "user-trash-full": ("status", "delete_lines", "regular"),
    "mail-unread": ("status", "mail_unread", "regular"),
    "mail-read": ("status", "mail_read", "regular"),
    "media-playlist-shuffle": ("status", "arrow_shuffle", "regular"),
    "media-playlist-repeat": ("status", "arrow_repeat_all", "regular"),
    "keyboard-brightness": ("status", "keyboard", "regular"),
    "printer-printing": ("status", "print", "regular"),
    "checkbox-checked": ("status", "checkbox_checked", "regular"),
    "emblem-ok": ("emblems", "checkmark_circle", "regular"),
    "emblem-important": ("emblems", "important", "regular"),
    "emblem-favorite": ("emblems", "star", "filled"),
    "emblem-shared": ("emblems", "share", "regular"),
    "emblem-synchronizing": ("emblems", "arrow_sync", "regular"),
    "emblem-system": ("emblems", "settings", "regular"),
    "emblem-default": ("emblems", "checkmark_circle", "regular"),
    "emblem-documents": ("emblems", "document", "regular"),
    "emblem-photos": ("emblems", "image", "regular"),
    "emblem-music": ("emblems", "music_note_2", "regular"),
    "emblem-videos": ("emblems", "video", "regular"),
    # devices
    "audio-card": ("devices", "speaker_settings", "regular"),
    "audio-speakers": ("devices", "speaker_2", "regular"),
    "audio-headphones": ("devices", "headphones", "regular"),
    "audio-headset": ("devices", "headset", "regular"),
    "audio-input-microphone": ("devices", "mic", "regular"),
    "input-keyboard": ("devices", "keyboard", "regular"),
    "input-mouse": ("devices", "cursor", "regular"),
    "input-touchpad": ("devices", "tap_single", "regular"),
    "input-gaming": ("devices", "games", "regular"),
    "input-tablet": ("devices", "tablet", "regular"),
    "phone": ("devices", "phone", "regular"),
    "computer": ("devices", "desktop", "regular"),
    "computer-laptop": ("devices", "laptop", "regular"),
    "video-display": ("devices", "desktop", "regular"),
    "display": ("devices", "desktop", "regular"),
    "drive-harddisk": ("devices", "hard_drive", "regular"),
    "drive-removable-media": ("devices", "usb_stick", "regular"),
    "media-removable": ("devices", "usb_stick", "regular"),
    "drive-optical": ("devices", "record", "regular"),
    "camera-photo": ("devices", "camera", "regular"),
    "camera-web": ("devices", "camera", "regular"),
    "camera-video": ("devices", "video", "regular"),
    "printer": ("devices", "print", "regular"),
    "scanner": ("devices", "scan", "regular"),
    "battery": ("status", "battery_6", "regular"),
    "battery-good": ("status", "battery_8", "regular"),
    "battery-caution": ("status", "battery_2", "regular"),
    "battery-low": ("status", "battery_1", "regular"),
    "battery-empty": ("status", "battery_0", "regular"),
    "battery-full": ("status", "battery_10", "regular"),
    "battery-full-charged": ("status", "battery_checkmark", "regular"),
    "battery-missing": ("status", "battery_warning", "regular"),
    "multimedia-player": ("devices", "music_note_2", "regular"),
    # places
    "folder": ("places", "folder", "regular"),
    "folder-open": ("places", "folder_open", "regular"),
    "folder-documents": ("places", "document_folder", "regular"),
    "folder-download": ("places", "arrow_download", "regular"),
    "folder-music": ("places", "music_note_2", "regular"),
    "folder-pictures": ("places", "image", "regular"),
    "folder-videos": ("places", "video", "regular"),
    "folder-remote": ("places", "cloud", "regular"),
    "user-home": ("places", "home", "regular"),
    "user-desktop": ("places", "desktop", "regular"),
    "user-trash": ("places", "delete", "regular"),
    "user-bookmarks": ("places", "bookmark", "regular"),
    "network-workgroup": ("places", "globe_desktop", "regular"),
    "network-server": ("places", "server", "regular"),
    "starred": ("places", "star", "regular"),
    "document-open-recent": ("places", "history", "regular"),
    # mimes
    "text-x-generic": ("mimes", "document_text", "regular"),
    "image-x-generic": ("mimes", "image", "regular"),
    "audio-x-generic": ("mimes", "music_note_2", "regular"),
    "video-x-generic": ("mimes", "video", "regular"),
    "package-x-generic": ("mimes", "box", "regular"),
    "application-x-executable": ("mimes", "app_generic", "regular"),
    "x-office-document": ("mimes", "document", "regular"),
    "x-office-spreadsheet": ("mimes", "table", "regular"),
    "x-office-presentation": ("mimes", "slide_text", "regular"),
    "x-office-calendar": ("mimes", "calendar", "regular"),
    "x-office-address-book": ("mimes", "book_contacts", "regular"),
    "text-html": ("mimes", "globe", "regular"),
    # apps / categories (symbolic only; colour app icons come from Win11)
    "preferences-system": ("categories", "settings", "regular"),
    "preferences-desktop-accessibility": ("categories", "accessibility", "regular"),
    "preferences-desktop-appearance": ("categories", "paint_brush", "regular"),
    "preferences-desktop-wallpaper": ("categories", "image", "regular"),
    "preferences-desktop-display": ("categories", "desktop", "regular"),
    "preferences-desktop-locale": ("categories", "local_language", "regular"),
    "preferences-desktop-keyboard": ("categories", "keyboard", "regular"),
    "preferences-desktop-default-applications": ("categories", "apps", "regular"),
    "preferences-system-time": ("categories", "clock", "regular"),
    "preferences-system-windows": ("categories", "window_multiple", "regular"),
    "preferences-system-network": ("categories", "globe", "regular"),
    "preferences-system-privacy": ("categories", "shield", "regular"),
    "preferences-color": ("categories", "color", "regular"),
    "color-select": ("categories", "color", "regular"),
    "applications-system": ("categories", "settings", "regular"),
    "applications-utilities": ("categories", "toolbox", "regular"),
    "applications-internet": ("categories", "globe", "regular"),
    "applications-multimedia": ("categories", "play_circle", "regular"),
    "applications-graphics": ("categories", "paint_brush", "regular"),
    "applications-games": ("categories", "games", "regular"),
    "applications-engineering": ("categories", "wrench", "regular"),
    "applications-science": ("categories", "beaker", "regular"),
    "applications-office": ("categories", "briefcase", "regular"),
    "applications-development": ("categories", "code", "regular"),
    "system-users": ("apps", "people", "regular"),
    "system-file-manager": ("apps", "folder", "regular"),
    "system-software-install": ("apps", "arrow_download", "regular"),
    "system-software-update": ("apps", "arrow_sync", "regular"),
    "utilities-terminal": ("apps", "window_console", "regular"),
    "utilities-system-monitor": ("apps", "data_area", "regular"),
    "web-browser": ("apps", "globe", "regular"),
    "help-contents": ("apps", "question_circle", "regular"),
    "accessories-calculator": ("apps", "calculator", "regular"),
    "accessories-text-editor": ("apps", "document_edit", "regular"),
    "accessories-screenshot": ("apps", "screenshot", "regular"),
    "org.gnome.Settings": ("apps", "settings", "regular"),
}

SYMBOLIC_GREY = "#bebebe"


def index_files(root):
    files = {}
    for dirpath, _, names in os.walk(os.path.join(root, "assets")):
        for n in names:
            m = re.match(r"ic_fluent_(.+)_(\d+)_(regular|filled)\.svg$", n)
            if m:
                files[(m.group(1), int(m.group(2)), m.group(3))] = os.path.join(dirpath, n)
    return files


def pick(files, name, style):
    for size in (16, 20, 24):
        for st in (style, "regular" if style == "filled" else "filled"):
            f = files.get((name, size, st))
            if f:
                return f
    return None


def recolor(svg):
    # Fluent glyphs are single-colour; make them the symbolic grey that
    # GTK and libcosmic recolour to the text colour.
    svg = re.sub(r'fill="#[0-9a-fA-F]{3,6}"', f'fill="{SYMBOLIC_GREY}"', svg)
    svg = svg.replace('fill="currentColor"', f'fill="{SYMBOLIC_GREY}"')
    if "fill=" not in svg:
        svg = svg.replace("<svg ", f'<svg fill="{SYMBOLIC_GREY}" ', 1)
    return svg


def write_theme(out, name, inherits, contexts, comment):
    with open(os.path.join(out, name, "index.theme"), "w") as f:
        dirs = [f"symbolic/{c}" for c in sorted(contexts)]
        f.write("[Icon Theme]\n")
        f.write(f"Name={name.replace('-', ' ')}\n")
        f.write(f"Comment={comment}\n")
        f.write(f"Inherits={inherits}\n")
        f.write("Example=folder\n\n")
        f.write(f"Directories={','.join(dirs)}\n\n")
        ctx_names = {"actions": "Actions", "status": "Status", "devices": "Devices", "places": "Places",
                     "mimes": "MimeTypes", "apps": "Applications", "categories": "Categories", "emblems": "Emblems"}
        for d, c in zip(dirs, sorted(contexts)):
            f.write(f"[{d}]\nContext={ctx_names[c]}\nSize=16\nMinSize=8\nMaxSize=512\nType=Scalable\n\n")


def main():
    src, out = sys.argv[1], sys.argv[2]
    files = index_files(src)
    missing = []
    base = os.path.join(out, "Lapin-UX")
    shutil.rmtree(base, ignore_errors=True)
    contexts = set()
    for fd, (ctx, fl, style) in sorted(MAP.items()):
        f = pick(files, fl, style)
        if not f:
            missing.append(f"{fd} -> {fl}")
            continue
        d = os.path.join(base, "symbolic", ctx)
        os.makedirs(d, exist_ok=True)
        with open(f) as fh:
            svg = recolor(fh.read())
        with open(os.path.join(d, f"{fd}-symbolic.svg"), "w") as fh:
            fh.write(svg)
        contexts.add(ctx)
    # Battery levels: 0..100 in steps of 10, charging and not.
    d = os.path.join(base, "symbolic", "status")
    for level in range(0, 101, 10):
        for charging in (False, True):
            fl = f"battery_charge_{level // 10}" if charging else f"battery_{level // 10}"
            if charging and level == 100:
                fl = "battery_checkmark"
            f = pick(files, fl, "regular")
            if not f:
                missing.append(fl)
                continue
            suffix = "-charging" if charging and level < 100 else ("-charged" if charging else "")
            with open(f) as fh:
                svg = recolor(fh.read())
            with open(os.path.join(d, f"battery-level-{level}{suffix}-symbolic.svg"), "w") as fh:
                fh.write(svg)
    comment = "Lapin UX: Fluent UI System Icons (MIT, Microsoft) for symbolic icons; app icons from Win11"
    write_theme(out, "Lapin-UX", "Win11,hicolor,breeze,Adwaita", contexts, comment)
    # Dark variant shares the glyphs.
    dark = os.path.join(out, "Lapin-UX-dark")
    shutil.rmtree(dark, ignore_errors=True)
    os.makedirs(dark)
    os.symlink("../Lapin-UX/symbolic", os.path.join(dark, "symbolic"))
    write_theme(out, "Lapin-UX-dark", "Win11-dark,Win11,hicolor,breeze,Adwaita", contexts, comment)
    lic = os.path.join(src, "LICENSE")
    for t in ("Lapin-UX", "Lapin-UX-dark"):
        shutil.copy(lic, os.path.join(out, t, "LICENSE-fluentui-system-icons"))
    count = sum(len(fs) for _, _, fs in os.walk(os.path.join(base, "symbolic")))
    print(f"{count} icons written")
    if missing:
        print("missing:", ", ".join(missing))
        sys.exit(1)


if __name__ == "__main__":
    main()
