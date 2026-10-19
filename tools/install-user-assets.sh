#!/usr/bin/env bash
# Install Lapin UX's look into your home folder without the package: the
# Selawik font, the Lapin UX icon themes (Fluent UI System Icons) and the
# Win11 icon theme (app icons). Handy for the "try it in a window" test.
# Remove again with:  tools/install-user-assets.sh --remove
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
win11_commit=a5b460a407da143b32f19a503d7fcebb3edf2371

if [[ ${1:-} == --remove ]]; then
    rm -rf "$data/fonts/lapin-ux-selawik" "$data/icons/Lapin-UX" "$data/icons/Lapin-UX-dark" "$data/icons/Win11" "$data/icons/Win11-dark"
    fc-cache -f >/dev/null 2>&1 || true
    echo "Removed."
    exit 0
fi

mkdir -p "$data/fonts/lapin-ux-selawik" "$data/icons"
cp "$repo"/data/fonts/selawik/*.ttf "$repo/data/fonts/selawik/OFL.txt" "$data/fonts/lapin-ux-selawik/"
fc-cache -f >/dev/null 2>&1 || true
rm -rf "$data/icons/Lapin-UX" "$data/icons/Lapin-UX-dark"
cp -a "$repo/data/icons/Lapin-UX" "$repo/data/icons/Lapin-UX-dark" "$data/icons/"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
git clone -q https://github.com/yeyushengfan258/Win11-icon-theme.git "$tmp/win11"
git -C "$tmp/win11" checkout -q "$win11_commit"
(cd "$tmp/win11" && ./install.sh -d "$data/icons" >/dev/null)
echo "Installed Selawik, the Lapin UX icon themes and the Win11 icon theme into $data."
