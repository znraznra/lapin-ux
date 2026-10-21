#!/usr/bin/env bash
# One-step install of Lapin UX on Arch Linux (and Arch-based systems).
#
#   ./install.sh
#
# Installs the build tools, then builds and installs Lapin UX together with
# its compositor build; pacman pulls in every other dependency
# automatically. The font and icon sets ship inside this repository.
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v pacman >/dev/null; then
    echo "Lapin UX's installer is for Arch Linux (pacman was not found)." >&2
    exit 1
fi
if [[ $EUID -eq 0 ]]; then
    echo "Run this as your normal user; it asks for your password when it needs it." >&2
    exit 1
fi

tools=(base-devel git)
# Rust: keep rustup if it's already there, otherwise use Arch's rust package.
if ! command -v cargo >/dev/null; then
    tools+=(rust)
fi
echo "Installing build tools: ${tools[*]}"
sudo pacman -S --needed --noconfirm "${tools[@]}"

echo "Building Lapin UX (the first build takes a while)…"
cd packaging/arch
makepkg -si --noconfirm --needed

# Optional Windows 11-style pointers. Their author (JepriCreations) doesn't
# allow the files to be redistributed, so they're downloaded and converted
# on this computer, for this user, only if you say yes.
echo
echo "Optional: \"Cursor Concept W11 Tail HDPi\" mouse pointers, designed by JepriCreations"
echo "(https://www.deviantart.com/jepricreations). They're downloaded and converted for your"
echo "own use; Lapin UX can't ship them. You can also do this later with: lapin-ux-cursors"
if [[ -t 0 ]]; then
    read -r -p "Install them now? [y/N] " answer
    if [[ $answer == [yY]* ]]; then
        lapin-ux-cursors || echo "Couldn't install the pointers; try again later with lapin-ux-cursors."
    fi
fi

echo
echo "Done. Log out, choose \"Lapin UX\" in the session list and log in."
echo "To remove it later and get your old settings back: lapin-ux-uninstall (or ./uninstall.sh)."
