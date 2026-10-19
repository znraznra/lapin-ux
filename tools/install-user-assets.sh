#!/usr/bin/env bash
# Install Lapin UX's look into your home folder without the package: the
# Selawik font (and the fontconfig rule making it the one system font), the
# Lapin UX icon themes (Fluent UI System Icons) and the Win11 icon theme
# (app, folder and file icons). Everything comes from this repository.
# Handy for the "try it in a window" test.
# Remove again with:  tools/install-user-assets.sh --remove
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
fcdir="${XDG_CONFIG_HOME:-$HOME/.config}/fontconfig/conf.d"
fc=50-lapin-ux-selawik.conf

if [[ ${1:-} == --remove ]]; then
    rm -rf "$data/fonts/lapin-ux-selawik" "$data/icons/Lapin-UX" "$data/icons/Lapin-UX-dark" "$data/icons/Win11" "$data/icons/Win11-dark"
    rm -f "$fcdir/$fc"
    fc-cache -f >/dev/null 2>&1 || true
    echo "Removed."
    exit 0
fi

mkdir -p "$data/fonts/lapin-ux-selawik" "$data/icons" "$fcdir"
cp "$repo"/data/fonts/selawik/*.ttf "$repo/data/fonts/selawik/OFL.txt" "$data/fonts/lapin-ux-selawik/"
cp "$repo/data/fontconfig/$fc" "$fcdir/"
fc-cache -f >/dev/null 2>&1 || true
rm -rf "$data/icons/Lapin-UX" "$data/icons/Lapin-UX-dark"
cp -a "$repo/data/icons/Lapin-UX" "$repo/data/icons/Lapin-UX-dark" "$data/icons/"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cp -a "$repo/third_party/win11-icon-theme" "$tmp/win11"
(cd "$tmp/win11" && ./install.sh -d "$data/icons" >/dev/null)
echo "Installed Selawik, the Lapin UX icon themes and the Win11 icon theme into $data."
