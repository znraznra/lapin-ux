#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Checks an Arch Linux machine is ready to build and run Lapin UX.
# Read-only: it installs nothing and changes nothing. ./install.sh installs
# everything listed here automatically; this only tells you what's there.
#
#   sh tools/preflight.sh

ok=0; warn=0; bad=0
pass() { printf '  \033[32mOK\033[0m    %s\n' "$1"; ok=$((ok + 1)); }
note() { printf '  \033[33mWARN\033[0m  %s\n' "$1"; warn=$((warn + 1)); }
fail() { printf '  \033[31mMISS\033[0m  %s\n' "$1"; bad=$((bad + 1)); }

echo "System"
if [ -r /etc/os-release ] && grep -qi '^ID=arch\|^ID_LIKE=.*arch' /etc/os-release; then
    pass "Arch Linux (or Arch-based)"
else
    note "not Arch Linux; the PKGBUILD will not work, but the nested test may"
fi
case "${XDG_SESSION_TYPE:-}" in
    wayland) pass "running a Wayland session (nested test possible)" ;;
    x11) note "running X11; the nested test needs cosmic-comp's X11 backend, which usually works" ;;
    *) note "no graphical session detected (XDG_SESSION_TYPE unset)" ;;
esac

echo "Runtime packages (./install.sh installs any that are missing)"
missing=""
for p in cosmic-comp cosmic-settings-daemon cosmic-settings cosmic-bg cosmic-osd cosmic-idle cosmic-greeter \
         cosmic-launcher cosmic-screenshot cosmic-icon-theme adwaita-icon-theme xdg-desktop-portal-cosmic xdg-desktop-portal-gtk \
         fontconfig noto-fonts noto-fonts-cjk noto-fonts-emoji libxkbcommon wayland dbus systemd-libs xdg-utils glib2 \
         networkmanager upower wireplumber libpulse bluez bluez-utils power-profiles-daemon accountsservice localsearch gnome-keyring; do
    if pacman -Qq "$p" >/dev/null 2>&1; then pass "$p"; else fail "$p"; missing="$missing $p"; fi
done
systemctl is-active -q NetworkManager 2>/dev/null && pass "NetworkManager is running" || note "NetworkManager is not running: Wi-Fi won't appear in quick settings (sudo systemctl enable --now NetworkManager)"

echo "Build tools"
for p in base-devel git python-numpy python-pillow; do
    if pacman -Qq "$p" >/dev/null 2>&1 || pacman -Qg "$p" >/dev/null 2>&1; then pass "$p"; else fail "$p"; missing="$missing $p"; fi
done
if command -v cargo >/dev/null 2>&1; then
    v=$(rustc --version 2>/dev/null | awk '{print $2}')
    major=$(echo "$v" | cut -d. -f1); minor=$(echo "$v" | cut -d. -f2)
    if [ "${major:-0}" -gt 1 ] || [ "${minor:-0}" -ge 93 ]; then pass "Rust $v (need 1.93+)"; else fail "Rust $v is too old (need 1.93+)"; fi
else
    fail "cargo (install the 'rust' or 'rustup' package)"; missing="$missing rust"
fi

echo "Display manager"
dm=$(systemctl status display-manager 2>/dev/null | sed -n '1s/^. \([^ ]*\).*/\1/p')
if [ -n "$dm" ]; then pass "display manager: $dm"; else note "no display manager service found; you'd start Lapin UX from a TTY with: start-lapin-ux"; fi

echo "Other desktops"
for d in cosmic-session plasma-workspace gnome-shell; do
    pacman -Qq "$d" >/dev/null 2>&1 && note "$d is installed: Lapin UX shares COSMIC's theme/wallpaper/shortcut settings, see TESTING.md"
done
[ -e /usr/share/wayland-sessions/lapin-ux.desktop ] && pass "Lapin UX is installed" || note "Lapin UX is not installed yet (fine for the nested test)"

echo
echo "$ok OK, $warn warnings, $bad missing"
if [ -n "$missing" ]; then
    echo "./install.sh installs these for you. To install them by hand:"
    echo "  sudo pacman -S --needed$missing"
    exit 1
fi
