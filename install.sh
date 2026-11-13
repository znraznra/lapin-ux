#!/usr/bin/env bash
# One-step install of Lapin UX on Arch Linux (and Arch-based systems).
#
#   ./install.sh
#
# Installs the build tools, then builds and installs Lapin UX together with
# its compositor build; pacman pulls in every other dependency
# automatically. The font and icon sets ship inside this repository.
#
#   ./install.sh               use Lapin UX's sign-in screen and boot animation
#                              (asks first; Enter means yes, and unattended runs
#                              do the same)
#   ./install.sh --keep-login  keep your own sign-in screen and boot animation;
#                              Lapin UX is then one more session in your list
set -euo pipefail
cd "$(dirname "$0")"

use_lapin_login=1
for arg in "$@"; do
    case "$arg" in
    --keep-login) use_lapin_login=0 ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $arg (see --help)" >&2; exit 2 ;;
    esac
done

if ! command -v pacman >/dev/null; then
    echo "Lapin UX's installer is for Arch Linux (pacman was not found)." >&2
    exit 1
fi
if [[ $EUID -eq 0 ]]; then
    echo "Run this as your normal user; it asks for your password when it needs it." >&2
    exit 1
fi

# Asked first, so the long build below can run without anyone watching.
if [[ $use_lapin_login -eq 1 && -t 0 ]]; then
    echo "Use Lapin UX's sign-in screen and boot animation? Your current ones are kept and come"
    echo "back if you uninstall Lapin UX."
    read -r -p "Use Lapin UX's? [Y/n] " answer
    [[ $answer == [nN]* ]] && use_lapin_login=0
fi

tools=(base-devel git)
# Rust: keep rustup if it's already there, otherwise use Arch's rust package.
if ! command -v cargo >/dev/null; then
    tools+=(rust)
fi
# Ask for the password once, now, and keep it fresh for the whole build, so
# the install at the end doesn't stop and time out waiting for it.
echo "Lapin UX needs your password to install packages (asked once)."
sudo -v
( while kill -0 $$ 2>/dev/null; do sudo -n -v 2>/dev/null; sleep 50; done ) &
keepalive=$!
trap 'kill $keepalive 2>/dev/null' EXIT

echo "Installing build tools: ${tools[*]}"
sudo pacman -S --needed --noconfirm "${tools[@]}"

echo "Building Lapin UX (the first build takes a while)…"
# Icon themes Lapin UX ships that may already be on the system from a manual
# install (no package owns them, so pacman would refuse to overwrite them).
# Move those aside; lapin-ux-uninstall puts them back.
backup=/var/lib/lapin-ux/backup/icons
for theme in Win11 Win11-dark; do
    dir=/usr/share/icons/$theme
    [[ -d $dir ]] || continue
    pacman -Qqo "$dir" >/dev/null 2>&1 && continue
    echo "$dir was installed by hand (no package owns it); Lapin UX ships the same theme."
    echo "Moving it to $backup (uninstalling Lapin UX puts it back)."
    sudo mkdir -p "$backup"
    sudo rm -rf "${backup:?}/$theme"
    sudo mv "$dir" "$backup/"
done

cd packaging/arch
# makepkg writes the version it computes (pkgver() from git) back into the
# PKGBUILD's pkgver= line, which would block the next `git pull`. Put that one
# line back afterwards (even if the build fails); any other edit is left alone.
restore_pkgver() {
    local orig
    orig=$(git show HEAD:packaging/arch/PKGBUILD 2>/dev/null | grep -m1 '^pkgver=') || return 0
    [ -n "$orig" ] && sed -i "s/^pkgver=.*/$orig/" PKGBUILD
}
restore_pkgver
trap restore_pkgver EXIT
# Build (pulling in build dependencies), then install in one pacman step.
# Some of Lapin UX's packages stand in for COSMIC's own parts (its portal,
# greeter, OSD, touch keyboard, Settings). When the stock one is installed,
# pacman asks whether to replace it, and --noconfirm would answer "no" and
# stop everything (#49). Answering yes to that one question (--ask 4) swaps
# them in the same transaction, so nothing is ever left missing.
# The version this checkout builds as (same formula as pkgver() in the PKGBUILD).
ver="0.1.0.r$(git rev-list --count HEAD).g$(git rev-parse --short=7 HEAD)"
base=$(grep -m1 '^pkgver=' PKGBUILD | cut -d= -f2)
# makepkg --packagelist names the files (honouring PKGDEST); swap in that version.
mapfile -t want < <(makepkg --packagelist | sed "s/-${base//./\\.}-/-${ver}-/")
have=0
for f in "${want[@]}"; do [ -f "$f" ] && have=$((have + 1)); done
if [ ${#want[@]} -gt 0 ] && [ "$have" -eq ${#want[@]} ]; then
    # Already built from this exact commit (e.g. the install step was
    # interrupted last time): just install it.
    echo "Lapin UX $ver is already built; installing it."
else
    makepkg -s --noconfirm --needed -f
fi
restore_pkgver
built=()
for f in "${want[@]}"; do [ -f "$f" ] && built+=("$f"); done
if [ ${#built[@]} -eq 0 ]; then
    echo "The build didn't produce any packages." >&2
    exit 1
fi
replaced=()
for stock in cosmic-greeter cosmic-osk cosmic-osd cosmic-settings xdg-desktop-portal-cosmic; do
    # -Q matches installed package names only, not what Lapin's provide.
    pacman -Qq "$stock" >/dev/null 2>&1 && replaced+=("$stock")
done
if [ ${#replaced[@]} -gt 0 ]; then
    echo "Replacing COSMIC's own ${replaced[*]} with Lapin UX's versions (they come back if you uninstall Lapin UX)."
fi
sudo pacman -U --noconfirm --needed --ask 4 "${built[@]}"

if [[ $use_lapin_login -eq 1 ]]; then
    echo
    echo "Setting up Lapin UX's sign-in screen and boot animation (what you have now is saved first)."
    sudo lapin-ux-login use-lapin || echo "Couldn't set them up; your own are unchanged. Try again with: sudo lapin-ux-login use-lapin"
else
    echo
    echo "Keeping your own sign-in screen and boot animation. Lapin UX is in your session list."
    echo "To switch later: sudo lapin-ux-login use-lapin"
fi

# Optional extra mouse pointers. Their author (JepriCreations) doesn't
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
