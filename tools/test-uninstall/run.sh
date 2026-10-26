#!/usr/bin/env bash
# Tests lapin-ux-uninstall / lapin-ux-desktop-settings in throwaway homes.
# Needs dconf and dbus-run-session. Run as a normal user:
#   bash tools/test-uninstall/run.sh "$PWD/tools/lapin-ux-uninstall"
# Test lapin-ux-uninstall in fake homes. Usage: uninst-test.sh /path/to/lapin-ux-uninstall
set -u
U=$1
FAKEBIN=$(dirname "$0")/fakebin
fail=0
check() { if eval "$2"; then echo "  ok   $1"; else echo "  FAIL $1"; fail=1; fi; }
lapin_writes() {   # what Lapin UX's first sign-in does
  C=$HOME/.config/cosmic
  mkdir -p $C/com.system76.CosmicTk/v1 $C/com.system76.CosmicBackground/v1 $C/com.system76.CosmicSettings.Shortcuts/v1 \
           $C/com.system76.CosmicComp/v1 $C/com.system76.CosmicTheme.Dark.Builder/v1 $C/dev.lapinux.Shell/v1 $C/com.system76.CosmicTheme.Dark/v1
  echo '(family: "Selawik", weight: Normal)' > $C/com.system76.CosmicTk/v1/interface_font
  echo '"Lapin-UX-dark"' > $C/com.system76.CosmicTk/v1/icon_theme
  echo '(output: "all", source: Path("/usr/share/backgrounds/lapin-ux/lapin-ux-dark.jpg"))' > $C/com.system76.CosmicBackground/v1/all
  echo true > $C/com.system76.CosmicBackground/v1/same-on-all
  printf '%s\n{\n}\n' "// Managed by Lapin UX. Delete this line to keep your own edits from being replaced." > $C/com.system76.CosmicSettings.Shortcuts/v1/custom
  echo '{Launcher: "lapin-ux-shell start"}' > $C/com.system76.CosmicSettings.Shortcuts/v1/system_actions
  echo FocusIfActiveWorkspace > $C/com.system76.CosmicComp/v1/activation_policy
  echo 'Some(lapin)' > $C/com.system76.CosmicTheme.Dark.Builder/v1/accent
  echo '(0, 4)' > $C/com.system76.CosmicTheme.Dark.Builder/v1/gaps
  echo 0 > $C/com.system76.CosmicTheme.Dark.Builder/v1/active_hint
  echo 'lapin' > $C/com.system76.CosmicTheme.Dark/v1/accent
  echo 14 > $C/dev.lapinux.Shell/v1/defaults_version
  dconf write /org/gnome/desktop/interface/font-name "'Selawik 10'"
  dconf write /org/gnome/desktop/interface/icon-theme "'Lapin-UX-dark'"
  dconf write /org/gnome/desktop/interface/cursor-theme "'Cursor-Concept-W11-Tail-HDPi-by-JepriCreations'"
  mkdir -p $HOME/.local/share/icons/default $HOME/.local/share/icons/Cursor-Concept-W11-Tail-HDPi-by-JepriCreations $HOME/.cache/lapin-ux $HOME/.local/state/cosmic/dev.lapinux.Shell
  printf '[Icon Theme]\nName=Default\nInherits=Cursor-Concept-W11-Tail-HDPi-by-JepriCreations\n' > $HOME/.local/share/icons/default/index.theme
}
newhome() { :; }   # each scenario runs in its own home and D-Bus session (see the bottom)
rd() { dconf read /org/gnome/desktop/interface/$1; }

scenA() {
echo "A: restore point taken before Lapin UX (fresh tester with their own setup)"
newhome
C=$HOME/.config/cosmic
mkdir -p $C/com.system76.CosmicTk/v1 $C/com.system76.CosmicTheme.Dark.Builder/v1
echo '(family: "Inter", weight: Normal)' > $C/com.system76.CosmicTk/v1/interface_font
echo 'Some(mine)' > $C/com.system76.CosmicTheme.Dark.Builder/v1/accent
dconf write /org/gnome/desktop/interface/font-name "'Cantarell 11'"
dconf write /org/gnome/desktop/interface/icon-theme "'Papirus'"
bash $U --snapshot
lapin_writes
# the tester changes the GTK icon theme themselves later, in another desktop
dconf write /org/gnome/desktop/interface/icon-theme "'Breeze'"
bash $U --settings-only --yes >$HOME/out.txt 2>&1; rc=$?
check "exit 0" "[ $rc = 0 ]"
check "GTK font back to Cantarell" "[ \"\$(rd font-name)\" = \"'Cantarell 11'\" ]"
check "GTK icons: tester's later choice kept" "[ \"\$(rd icon-theme)\" = \"'Breeze'\" ]"
check "GTK cursor reset (was unset)" "[ -z \"\$(rd cursor-theme)\" ]"
check "COSMIC font back to Inter" "grep -q Inter $C/com.system76.CosmicTk/v1/interface_font"
check "COSMIC icon theme back to default" "[ ! -e $C/com.system76.CosmicTk/v1/icon_theme ]"
check "wallpaper settings removed" "[ ! -e $C/com.system76.CosmicBackground/v1/all ] && [ ! -e $C/com.system76.CosmicBackground/v1/same-on-all ]"
check "shortcuts removed" "[ ! -e $C/com.system76.CosmicSettings.Shortcuts/v1/custom ] && [ ! -e $C/com.system76.CosmicSettings.Shortcuts/v1/system_actions ]"
check "focus policy removed" "[ ! -e $C/com.system76.CosmicComp/v1/activation_policy ]"
check "theme builder back to tester's" "grep -q mine $C/com.system76.CosmicTheme.Dark.Builder/v1/accent"
check "theme folder Lapin created removed" "[ ! -e $C/com.system76.CosmicTheme.Dark ]"
check "Lapin config removed" "[ ! -e $C/dev.lapinux.Shell ] && [ ! -e $HOME/.local/state/cosmic/dev.lapinux.Shell ]"
check "pointer files removed" "[ ! -e $HOME/.local/share/icons/default/index.theme ] && [ ! -e $HOME/.local/share/icons/Cursor-Concept-W11-Tail-HDPi-by-JepriCreations ]"
check "state + cache removed" "[ ! -e $HOME/.local/state/lapin-ux ] && [ ! -e $HOME/.cache/lapin-ux ]"
[ $fail = 1 ] && cat $HOME/out.txt
}

scenB() {
echo "B: installed before restore points existed (no originals)"
newhome; C=$HOME/.config/cosmic
lapin_writes
bash $U --snapshot
check "snapshot marked late" "[ -f $HOME/.local/state/lapin-ux/restore/late ]"
mkdir -p $C/com.system76.CosmicFiles/v1; echo x > $C/com.system76.CosmicFiles/v1/pref   # not Lapin's
bash $U --settings-only --yes >$HOME/out.txt 2>&1
check "GTK font reset" "[ -z \"\$(rd font-name)\" ]"
check "GTK icons reset" "[ -z \"\$(rd icon-theme)\" ]"
check "COSMIC font reset" "[ ! -e $C/com.system76.CosmicTk/v1/interface_font ]"
check "other COSMIC app prefs kept" "[ -f $C/com.system76.CosmicFiles/v1/pref ] || ! command -v pacman >/dev/null"
cat $HOME/out.txt | sed 's/^/    | /'
}

scenC() {
echo "C: refuses to run inside Lapin UX"
XDG_CURRENT_DESKTOP=LapinUX:COSMIC bash $U --settings-only --yes >/dev/null 2>&1; rc=$?
check "refused" "[ $rc = 1 ]"

}
scenD() {
echo "D: second run finds nothing"
lapin_writes; bash $U --settings-only --yes >/dev/null 2>&1
mkdir -p $HOME/.config/cosmic
bash $U --settings-only --yes > $HOME/out2.txt 2>&1
check "idempotent" "grep -q 'Nothing of Lapin UX' $HOME/out2.txt"
}
scenE() {
echo "E: Lapin's look only while Lapin runs (sign in, sign out, back to GNOME)"
C=$HOME/.config/cosmic; DS=$(dirname $U)/lapin-ux-desktop-settings
mkdir -p $C/com.system76.CosmicTk/v1
echo '(family: "Inter", weight: Normal)' > $C/com.system76.CosmicTk/v1/interface_font
dconf write /org/gnome/desktop/interface/font-name "'Cantarell 11'"
dconf write /org/gnome/desktop/interface/icon-theme "'Papirus'"
bash $U --snapshot; bash $DS enter
lapin_writes
bash $DS leave
check "GTK font back to Cantarell" "[ \"\$(rd font-name)\" = \"'Cantarell 11'\" ]"
check "GTK icons back to Papirus" "[ \"\$(rd icon-theme)\" = \"'Papirus'\" ]"
check "GTK pointer back to default" "[ -z \"\$(rd cursor-theme)\" ]"
check "COSMIC font back to Inter" "grep -q Inter $C/com.system76.CosmicTk/v1/interface_font"
check "COSMIC icons back to default" "[ ! -e $C/com.system76.CosmicTk/v1/icon_theme ]"
check "default pointer file removed" "[ ! -e $HOME/.local/share/icons/default/index.theme ]"
check "Lapin's own config untouched" "[ -e $C/dev.lapinux.Shell ]"
echo "   later, in GNOME the tester picks Adwaita icons, then signs into Lapin again"
dconf write /org/gnome/desktop/interface/icon-theme "'Adwaita'"
bash $DS enter
check "Lapin's icons back on entering Lapin" "[ \"\$(rd icon-theme)\" = \"'Lapin-UX-dark'\" ] && grep -q Lapin-UX $C/com.system76.CosmicTk/v1/icon_theme"
check "Lapin's pointer back on entering Lapin" "grep -q Inherits=Cursor $HOME/.local/share/icons/default/index.theme"
lapin_writes
echo "   Lapin session crashes (no leave); GNOME autostart runs leave"
bash $DS leave
check "GTK icons: the newer GNOME choice comes back" "[ \"\$(rd icon-theme)\" = \"'Adwaita'\" ]"
check "GTK font back to Cantarell" "[ \"\$(rd font-name)\" = \"'Cantarell 11'\" ]"
echo "   leave is skipped while Lapin's shell runs"
bash $DS enter
mkdir -p /tmp/ut-$$; cp /bin/sleep /tmp/ut-$$/lapin-ux-shell
/tmp/ut-$$/lapin-ux-shell 30 & sp=$!
sleep 0.3; bash $DS leave
check "nothing changed under a running Lapin" "[ \"\$(rd font-name)\" = \"'Selawik 10'\" ]"
kill $sp 2>/dev/null; wait $sp 2>/dev/null; rm -rf /tmp/ut-$$
echo "   tester picks Papirus inside Lapin; GNOME keeps Adwaita"
dconf write /org/gnome/desktop/interface/icon-theme "'Papirus'"
bash $DS leave
check "GNOME: Adwaita" "[ \"\$(rd icon-theme)\" = \"'Adwaita'\" ]"
bash $DS enter
check "Lapin: Papirus" "[ \"\$(rd icon-theme)\" = \"'Papirus'\" ]"
}
scenG() {
echo "G: real COSMIC keeps its own colours, wallpaper, shortcuts and dark mode"
C=$HOME/.config/cosmic; DS=$(dirname $U)/lapin-ux-desktop-settings
B=$C/com.system76.CosmicTheme.Dark.Builder/v1
mkdir -p $B $C/com.system76.CosmicBackground/v1 $C/com.system76.CosmicSettings.Shortcuts/v1 $C/com.system76.CosmicTheme.Mode/v1
echo 'Some(cosmic-purple)' > $B/accent; echo '(0, 8)' > $B/gaps; echo 3 > $B/active_hint
echo '(output: "all", source: Path("/home/me/Pictures/mountains.jpg"))' > $C/com.system76.CosmicBackground/v1/all
echo '{ (modifiers: [Super], key: "t"): Spawn("cosmic-term") }' > $C/com.system76.CosmicSettings.Shortcuts/v1/custom
echo false > $C/com.system76.CosmicTheme.Mode/v1/is_dark
bash $U --snapshot; bash $DS enter
# Lapin's first run (keeps a styled theme; here we pretend COSMIC was default) + Lapin choices
echo 'Some(lapin-blue)' > $B/accent; echo '(0, 4)' > $B/gaps; echo 0 > $B/active_hint
echo '(output: "all", source: Path("/usr/share/backgrounds/lapin-ux/lapin-ux-dark.jpg"))' > $C/com.system76.CosmicBackground/v1/all
printf '%s\n{}\n' "// Managed by Lapin UX. Delete this line to keep your own edits from being replaced." > $C/com.system76.CosmicSettings.Shortcuts/v1/custom
echo true > $C/com.system76.CosmicTheme.Mode/v1/is_dark
bash $DS leave
check "COSMIC accent back" "grep -q cosmic-purple $B/accent && grep -q '(0, 8)' $B/gaps"
check "COSMIC wallpaper back" "grep -q mountains $C/com.system76.CosmicBackground/v1/all"
check "COSMIC shortcuts back" "grep -q cosmic-term $C/com.system76.CosmicSettings.Shortcuts/v1/custom"
check "COSMIC light mode back" "grep -q false $C/com.system76.CosmicTheme.Mode/v1/is_dark"
echo "   in COSMIC the tester changes the accent; Lapin keeps its own"
echo 'Some(cosmic-green)' > $B/accent
bash $DS enter
check "Lapin's accent in Lapin" "grep -q lapin-blue $B/accent"
check "Lapin's wallpaper in Lapin" "grep -q lapin-ux $C/com.system76.CosmicBackground/v1/all"
check "Lapin dark in Lapin" "grep -q true $C/com.system76.CosmicTheme.Mode/v1/is_dark"
bash $DS leave
check "COSMIC's new green accent kept" "grep -q cosmic-green $B/accent"
echo "   uninstall"
bash $U --settings-only --yes > $HOME/out.txt 2>&1
check "after uninstall: COSMIC's own look" "grep -q cosmic-green $B/accent && grep -q mountains $C/com.system76.CosmicBackground/v1/all && grep -q cosmic-term $C/com.system76.CosmicSettings.Shortcuts/v1/custom"
}
scenH() {
echo "H: already installed before all this (no restore point, no copies)"
C=$HOME/.config/cosmic; DS=$(dirname $U)/lapin-ux-desktop-settings
lapin_writes
bash $U --snapshot; bash $DS enter
check "Lapin keeps its look on first entry" "grep -q Selawik $C/com.system76.CosmicTk/v1/interface_font && [ \"\$(rd font-name)\" = \"'Selawik 10'\" ]"
bash $DS leave
check "COSMIC font back to default" "[ ! -e $C/com.system76.CosmicTk/v1/interface_font ]"
check "COSMIC shortcuts back to default" "[ ! -e $C/com.system76.CosmicSettings.Shortcuts/v1/custom ] && [ ! -e $C/com.system76.CosmicSettings.Shortcuts/v1/system_actions ]"
check "COSMIC theme back to default" "[ ! -e $C/com.system76.CosmicTheme.Dark.Builder ]"
check "GTK font back to default" "[ -z \"\$(rd font-name)\" ]"
bash $DS enter
check "Lapin's look back in Lapin" "grep -q Selawik $C/com.system76.CosmicTk/v1/interface_font && grep -q '(0, 4)' $C/com.system76.CosmicTheme.Dark.Builder/v1/gaps && [ \"\$(rd font-name)\" = \"'Selawik 10'\" ]"
}
scenF() {
echo "F: login service vs. a Lapin session that's starting"
DS=$(dirname $U)/lapin-ux-desktop-settings
export XDG_RUNTIME_DIR=$HOME/run; mkdir -p $XDG_RUNTIME_DIR
bash $U --snapshot; bash $DS enter; lapin_writes
sleep 30 & sp=$!; echo $sp > $XDG_RUNTIME_DIR/lapin-ux-session.pid
bash $DS leave
check "skipped while start-lapin-ux is alive" "[ \"\$(rd font-name)\" = \"'Selawik 10'\" ]"
kill $sp; wait $sp 2>/dev/null
bash $DS leave
check "runs once that session is gone (stale mark)" "[ -z \"\$(rd font-name)\" ]"
}
scenI() {
echo "I: other desktops' startup parts a crashed Lapin session skipped come back"
DS=$(dirname $U)/lapin-ux-desktop-settings
export XDG_RUNTIME_DIR=$HOME/run; mkdir -p $XDG_RUNTIME_DIR
# Stand-in systemctl records what it's asked to do.
mkdir -p $HOME/bin
printf '#!/bin/sh\necho "$*" >> %s/systemctl.log\n' "$HOME" > $HOME/bin/systemctl; chmod +x $HOME/bin/systemctl
export PATH=$HOME/bin:$PATH
printf 'app-io.elementary.settings\\x2ddaemon@autostart.service\napp-xfsettingsd@autostart.service\n' > $XDG_RUNTIME_DIR/lapin-ux-autostart-masked
bash $DS leave
check "unmasked them (runtime)" "grep -q 'unmask --runtime app-io.elementary.settings.*app-xfsettingsd@autostart.service' $HOME/systemctl.log"
check "list removed" "[ ! -e $XDG_RUNTIME_DIR/lapin-ux-autostart-masked ]"
: > $HOME/systemctl.log
bash $DS leave
check "nothing to unmask the next time" "! grep -q unmask $HOME/systemctl.log"
}
if [ -n "${SCEN:-}" ]; then "scen$SCEN"; exit $fail; fi
for x in A B C D E F G H I; do
  h=$(mktemp -d)
  env -i HOME=$h USER=$USER PATH="$FAKEBIN:/usr/bin:/bin" XDG_DATA_DIRS=/usr/share SCEN=$x dbus-run-session -- bash "$0" "$U" > $h.log 2>&1 || fail=1
  grep -v -e dbus-daemon -e '^$' $h.log
done
exit $fail
