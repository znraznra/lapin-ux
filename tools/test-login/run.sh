#!/usr/bin/env bash
# Tests lapin-ux-login in throwaway roots with a fake systemctl and a fake
# plymouth-set-default-theme. Run as a normal user:
#   bash tools/test-login/run.sh "$PWD/tools/lapin-ux-login"
# rc and out are read inside the eval of check().
# shellcheck disable=SC2034
set -u
T=$1
FAKEBIN=$(cd "$(dirname "$0")" && pwd)/fakebin
fail=0
ROOTS=()
cleanup() { local r; for r in "${ROOTS[@]}"; do [ -n "$r" ] && rm -rf "$r"; done; }
trap cleanup EXIT
check() { if eval "$2"; then echo "  ok   $1"; else echo "  FAIL $1"; fail=1; fi; }

newroot() {   # [plymouth|noplymouth] [bgrt|nobgrt] [dm]
    ROOT=$(mktemp -d /tmp/lapin-login-test.XXXXXX)
    ROOTS+=("$ROOT")
    export LAPIN_LOGIN_ROOT=$ROOT PATH="$FAKEBIN:$ORIG_PATH"
    unset FAKE_ENABLE_FAILS FAKE_MISSING_UNIT LAPIN_LOGIN_FORCE_BGRT
    mkdir -p "$ROOT/etc/systemd/system/wants" "$ROOT/etc/greetd" "$ROOT/usr/bin" "$ROOT/usr/share/plymouth/themes/spinner" "$ROOT/usr/share/plymouth/themes/bgrt" "$ROOT/etc/plymouth" "$ROOT/sys/firmware/acpi/bgrt"
    : > "$ROOT/usr/share/plymouth/themes/spinner/animation-0001.png"
    : > "$ROOT/usr/share/plymouth/themes/spinner/watermark.png"
    printf '#!/bin/sh\n' > "$ROOT/usr/bin/cosmic-greeter-start"; chmod +x "$ROOT/usr/bin/cosmic-greeter-start"
    mkdir -p "$ROOT/usr/lib/systemd/system"
    printf '[Unit]\nDescription=Greeter daemon\nAfter=systemd-user-sessions.service plymouth-quit-wait.service\nAfter=getty@tty1.service\nConflicts=getty@tty1.service\n\n[Service]\nType=simple\nExecStart=greetd\nRestart=always\n\n[Install]\nAlias=display-manager.service\n' > "$ROOT/usr/lib/systemd/system/greetd.service"
    printf '[Daemon]\nTheme=bgrt\n' > "$ROOT/etc/plymouth/plymouthd.conf"
    if [ "${1:-plymouth}" = plymouth ]; then echo 'HOOKS=(base udev plymouth autodetect)' > "$ROOT/etc/mkinitcpio.conf"; fi
    if [ "${2:-bgrt}" = bgrt ]; then : > "$ROOT/sys/firmware/acpi/bgrt/image"; fi
    local dm=${3:-gdm}
    if [ "$dm" != none ]; then : > "$ROOT/etc/systemd/system/wants/$dm.service"; ln -sfn "/usr/lib/systemd/system/$dm.service" "$ROOT/etc/systemd/system/display-manager.service"; fi
}
ORIG_PATH=$PATH
dm_now() { basename "$(readlink "$ROOT/etc/systemd/system/display-manager.service" 2>/dev/null)" 2>/dev/null; }
theme_now() { sed -n 's/^Theme=//p' "$ROOT/etc/plymouth/plymouthd.conf"; }

echo "A: GDM user, Plymouth set up: use Lapin's, then restore"
newroot plymouth bgrt gdm
bash "$T" use-lapin >/dev/null
check "greetd is now the display manager" '[ "$(dm_now)" = greetd.service ]'
check "gdm is disabled" '[ ! -e "$ROOT/etc/systemd/system/wants/gdm.service" ]'
check "greetd runs Lapin's sign-in screen" 'grep -q cosmic-greeter-start "$ROOT/etc/greetd/config.toml"'
check "Lapin's boot theme is set" '[ "$(theme_now)" = lapin-ux ]'
check "the theme has a plymouth file" '[ -f "$ROOT/usr/share/plymouth/themes/lapin-ux/lapin-ux.plymouth" ]'
check "with the firmware logo on" 'grep -q "UseFirmwareBackground=true" "$ROOT/usr/share/plymouth/themes/lapin-ux/lapin-ux.plymouth"'
check "the restore point is saved" '[ -f "$ROOT/var/lib/lapin-ux/login/complete" ]'
bash "$T" use-lapin >/dev/null
check "a second use-lapin keeps the first record" '[ "$(cat "$ROOT/var/lib/lapin-ux/login/dm")" = gdm.service ] && [ "$(cat "$ROOT/var/lib/lapin-ux/login/plymouth-theme")" = bgrt ]'
bash "$T" restore >/dev/null
check "gdm is the display manager again" '[ "$(dm_now)" = gdm.service ]'
check "greetd is off again" '[ ! -e "$ROOT/etc/systemd/system/wants/greetd.service" ]'
check "no greetd config is left (there was none)" '[ ! -e "$ROOT/etc/greetd/config.toml" ]'
check "the old boot theme is back" '[ "$(theme_now)" = bgrt ]'
check "Lapin's theme folder is gone" '[ ! -d "$ROOT/usr/share/plymouth/themes/lapin-ux" ]'
check "the record is removed" '[ ! -e "$ROOT/var/lib/lapin-ux/login" ]'

echo "B: Plymouth not set up: the login changes, the boot animation is left alone"
newroot noplymouth bgrt gdm
out=$(bash "$T" use-lapin 2>&1); rc=$?
check "succeeds" '[ $rc = 0 ]'
check "greetd is the display manager" '[ "$(dm_now)" = greetd.service ]'
check "the boot theme is untouched" '[ "$(theme_now)" = bgrt ]'
check "it says so" 'echo "$out" | grep -q "Plymouth isn.t set up"'
bash "$T" restore >/dev/null
check "restore brings gdm back" '[ "$(dm_now)" = gdm.service ]'

echo "C: greetd can't be enabled: nothing is left changed"
newroot plymouth bgrt sddm
export FAKE_ENABLE_FAILS=1
bash "$T" use-lapin >/dev/null 2>&1; rc=$?
check "fails" '[ $rc != 0 ]'
unset FAKE_ENABLE_FAILS
check "sddm is the display manager again" '[ "$(dm_now)" = sddm.service ]'
check "no Lapin greetd config left" '[ ! -e "$ROOT/etc/greetd/config.toml" ]'

echo "D: COSMIC's greetd user keeps their own config file on restore"
newroot plymouth bgrt greetd
echo 'command = "cosmic-greeter-start-mine"' > "$ROOT/etc/greetd/config.toml"
bash "$T" use-lapin >/dev/null
check "config is Lapin's meanwhile" '! grep -q mine "$ROOT/etc/greetd/config.toml"'
bash "$T" restore >/dev/null
check "own config is back" 'grep -q mine "$ROOT/etc/greetd/config.toml"'
check "greetd is still the display manager" '[ "$(dm_now)" = greetd.service ]'

echo "E: no firmware logo: the system's own logo is the watermark"
newroot plymouth nobgrt gdm
mkdir -p "$ROOT/usr/share/pixmaps"; printf 'LOGO=archlinux-logo\n' > "$ROOT/etc/os-release"; printf 'DISTROPNG' > "$ROOT/usr/share/pixmaps/archlinux-logo.png"
bash "$T" use-lapin >/dev/null
check "firmware background off" 'grep -q "UseFirmwareBackground=false" "$ROOT/usr/share/plymouth/themes/lapin-ux/lapin-ux.plymouth"'
check "watermark is the distro logo" 'grep -q DISTROPNG "$ROOT/usr/share/plymouth/themes/lapin-ux/watermark.png"'

echo "F: nothing to restore"
newroot plymouth bgrt gdm
out=$(bash "$T" restore 2>&1); rc=$?
check "says so and succeeds" '[ $rc = 0 ] && echo "$out" | grep -q "Nothing to put back"'
check "gdm untouched" '[ "$(dm_now)" = gdm.service ]'

echo "G: a commented-out plymouth hook doesn't count as set up"
newroot plymouth bgrt gdm
echo 'HOOKS=(base udev) # add plymouth later' > "$ROOT/etc/mkinitcpio.conf"
out=$(bash "$T" use-lapin 2>&1)
check "Plymouth is left alone" 'echo "$out" | grep -q "Plymouth isn.t set up" && [ "$(theme_now)" = bgrt ]'

echo "H: the saved display manager is gone: restore keeps greetd and the record"
newroot plymouth bgrt gdm
bash "$T" use-lapin >/dev/null
export FAKE_MISSING_UNIT=gdm.service
out=$(bash "$T" restore 2>&1); rc=$?
check "fails and says so" '[ $rc != 0 ] && echo "$out" | grep -q "greetd stays on"'
check "greetd is still the display manager" '[ "$(dm_now)" = greetd.service ]'
check "greetd still has Lapin's config" 'grep -q cosmic-greeter-start "$ROOT/etc/greetd/config.toml"'
check "the record is kept" '[ -f "$ROOT/var/lib/lapin-ux/login/complete" ]'
unset FAKE_MISSING_UNIT
bash "$T" restore >/dev/null
check "a later restore works" '[ "$(dm_now)" = gdm.service ] && [ ! -e "$ROOT/var/lib/lapin-ux/login" ]'

echo "I: Lapin's sign-in screen isn't installed: no record, and it says so"
newroot noplymouth bgrt gdm
rm -f "$ROOT/usr/bin/cosmic-greeter-start"
out=$(bash "$T" use-lapin 2>&1); rc=$?
check "exits with 2" '[ $rc = 2 ]'
check "gdm untouched and nothing recorded" '[ "$(dm_now)" = gdm.service ] && [ ! -e "$ROOT/var/lib/lapin-ux/login/complete" ]'

echo "J: Plymouth fails to set the theme: its config is put back"
newroot plymouth bgrt gdm
export FAKE_PLYMOUTH_FAILS=1
bash "$T" use-lapin >/dev/null 2>&1
unset FAKE_PLYMOUTH_FAILS
check "plymouthd.conf is as it was" '[ "$(theme_now)" = bgrt ]'
check "no theme folder is left" '[ ! -d "$ROOT/usr/share/plymouth/themes/lapin-ux" ]'

echo "K: a later failed use-lapin keeps the first restore point"
newroot noplymouth bgrt gdm
bash "$T" use-lapin >/dev/null 2>&1
rm -f "$ROOT/usr/bin/cosmic-greeter-start"
bash "$T" use-lapin >/dev/null 2>&1; rc=$?
check "reports 'not set up'" '[ $rc = 2 ]'
check "the first record is still there" '[ "$(cat "$ROOT/var/lib/lapin-ux/login/dm")" = gdm.service ]'
bash "$T" restore >/dev/null 2>&1
check "restore still brings gdm back" '[ "$(dm_now)" = gdm.service ]'

echo "L: the splash hands over to the sign-in screen, and restore takes that away (#100)"
newroot plymouth bgrt gdm
bash "$T" use-lapin >/dev/null 2>&1
D=$ROOT/etc/systemd/system/greetd.service
check "a full greetd unit is written in /etc" '[ -f "$D" ] && grep -q "^ExecStart=greetd$" "$D" && grep -q "^Alias=display-manager.service$" "$D"'
check "greetd no longer waits for plymouth-quit-wait" '! grep -q plymouth-quit-wait "$D" && grep -q "^After=systemd-user-sessions.service$" "$D" && grep -q "^After=getty@tty1.service$" "$D"'
check "plymouth-quit is kept from closing the splash early" 'grep -qx "Conflicts=plymouth-quit.service" "$D" && grep -qx "OnFailure=plymouth-quit.service" "$D"'
check "the splash stays on screen until the sign-in screen draws" 'awk "/^\\[Service\\]/{s=1} s&&/plymouth quit --retain-splash/{f=1} END{exit !f}" "$D"'
check "tty1 is cleared with a quoted shell line" 'grep -q "^ExecStartPre=-/bin/sh -c '"'"'command -v setterm" "$D"'
rm -f "$D"
bash "$T" refresh >/dev/null 2>&1
check "refresh (after an upgrade) writes it back" '[ -f "$D" ]'
bash "$T" restore >/dev/null 2>&1
check "restore removes it" '[ ! -e "$D" ]'
bash "$T" refresh >/dev/null 2>&1
check "refresh does nothing when Lapin's sign-in screen isn't used" '[ ! -e "$D" ]'

echo "M: your own greetd.service override is kept and comes back"
newroot plymouth bgrt gdm
D=$ROOT/etc/systemd/system/greetd.service
printf '[Unit]\nDescription=mine\n' > "$D"
bash "$T" use-lapin >/dev/null 2>&1
check "ours replaces it while in use" 'grep -q "Lapin UX: hand the boot splash" "$D"'
bash "$T" restore >/dev/null 2>&1
check "yours is back after restore" 'grep -qx "Description=mine" "$D"'

echo "O: refresh leaves greetd alone when it runs another greeter"
newroot plymouth bgrt gdm
bash "$T" use-lapin >/dev/null 2>&1
D=$ROOT/etc/systemd/system/greetd.service
rm -f "$D"
printf '[default_session]\ncommand = "tuigreet"\n' > "$ROOT/etc/greetd/config.toml"
bash "$T" refresh >/dev/null 2>&1
check "no override for someone else's greeter" '[ ! -e "$D" ]'
printf "[terminal]\nvt = 1\n\n[default_session]  # the sign-in screen\ncommand = 'cosmic-greeter-start'   # mine\nuser = \"cosmic-greeter\"\n" > "$ROOT/etc/greetd/config.toml"
bash "$T" refresh >/dev/null 2>&1
check "single quotes and a comment still count as Lapin's" '[ -f "$D" ]'
rm -f "$D"
printf '[initial_session]\ncommand = "cosmic-greeter-start"\n\n[default_session]\ncommand = "tuigreet"\n' > "$ROOT/etc/greetd/config.toml"
bash "$T" refresh >/dev/null 2>&1
check "only [default_session]'s command counts" '[ ! -e "$D" ]'

echo "P: Lapin's copy is kept current while another login manager is on"
newroot plymouth bgrt gdm
bash "$T" use-lapin >/dev/null 2>&1
D=$ROOT/etc/systemd/system/greetd.service
ln -sfn /usr/lib/systemd/system/gdm.service "$ROOT/etc/systemd/system/display-manager.service"
sed -i 's/^ExecStart=greetd$/ExecStart=greetd --new/' "$ROOT/usr/lib/systemd/system/greetd.service"
bash "$T" refresh >/dev/null 2>&1
check "the copy follows greetd's updated unit" 'grep -qx "ExecStart=greetd --new" "$D"'

echo "N: a restore that keeps the record (boot animation failed) stops refresh"
newroot plymouth bgrt gdm
bash "$T" use-lapin >/dev/null 2>&1
D=$ROOT/etc/systemd/system/greetd.service
FAKE_PLYMOUTH_FAILS=1 bash "$T" restore >/dev/null 2>&1
unset FAKE_PLYMOUTH_FAILS
ln -sfn /usr/lib/systemd/system/greetd.service "$ROOT/etc/systemd/system/display-manager.service"
bash "$T" refresh >/dev/null 2>&1
check "the override isn't put back" '[ ! -e "$D" ]'

[ "$fail" = 0 ] && echo "All checks passed." || { echo "Some checks FAILED."; exit 1; }
