#!/usr/bin/env bash
# The boot chain's seams on film: a VM rig slot (dev/vm.sh) recorded from
# power-on to the desktop, then out of it three ways, as PNG frames over
# QMP screendump, typed into over QMP send-key (`fspec vm record`, `type`,
# `key`, `frames`; FormalSpec's docs/formalshell-vm-rig.md).
#
#   dev/seams.sh      artifacts/seams/{boot,logout,reboot,shutdown}/frames/
#
# Frame n is n x 100 ms into its recording (the boot leg's n x 20 ms), so a
# file's number is its time.
# dev/seams-check.py reads them at the end (sheets and report.txt beside the
# frames) and the run exits with its verdict.
#   boot      power-on: OVMF, Limine, systemd-boot, the FormalOS splash, the
#             locked login's lock over the splash picture
#             (docs/superpowers/specs/2026-10-30-locked-login.md), the
#             password typed, the desktop with its bar
#   logout    SUPER+SHIFT+E out of the desktop and back to the greeter
#   reboot    the launcher's Reboot, through the shutdown splash, the
#             firmware and the boot again, to the lock
#   shutdown  the launcher's Shutdown, to the last frame before power-off
#
# The slot boots its applied disk, the guest's installed formalshell
# package. FS_SEAMS_TREE=1 films this tree's build instead: built on a rig
# slot, linked over the package on the guest's disk before the cold boot (a
# resume's overlay would not survive it), and the package's links put back
# before the save. A cold boot drops the slot's save, so the script saves
# it again at the end, at the greeter with no boot lock's marker left.
# FS_SEAMS_SCALE=1.25 films every leg with the guest's output at that
# scale (the monitor rule in its /etc/fstestvm/hyprland.lua, put back
# before the save): the boot lock's splash still has to land on plymouth's
# pixels.
set -euo pipefail
cd "$(dirname "$0")/.."
repo_root="$(pwd)"
. dev/rig.sh

if [ "${FS_SEAMS_TREE:-}" = 1 ] && [ -z "${FS_VM_RESULT:-}" ]; then
  status=0
  rig_with_build build -- "$0" "$@" || status=$?
  exit "$status"
fi

[ -n "${FS_VM_SLOT:-}" ] || exec "$repo_root/dev/vm.sh" hold "$0" "$@"

spec=$rig_spec
vm="fstestvm$FS_VM_SLOT"
out="$repo_root/artifacts/seams"
password=formalshell-test
result=$rig_vm_result
bins=$rig_greeter_bins

f() { fspec vm "$1" "$vm" "${@:2}" --spec "$spec"; }
guest() { ssh "${rig_ssh_opts[@]}" "$vm" "$@"; }
say() { echo "seams: $*"; }

# Polls until <what> answers true in the guest, at most <seconds>.
until_guest() {
  local what=$1 seconds=$2 cmd=$3 t0=$SECONDS
  until guest "$cmd" >/dev/null 2>&1; do
    [ $((SECONDS - t0)) -lt "$seconds" ] || { echo "seams: $what never came up in ${seconds}s" >&2; exit 1; }
    sleep 1
  done
  say "$what after $((SECONDS - t0))s"
}
greeter_up() { until_guest "the greeter" "$1" 'sudo sh -c "ls /run/formalshell-greeter/wayland-? >/dev/null" && pgrep -u greeter -f formalshell-rs'; sleep 3; }
lock_up() { until_guest "the boot lock" "$1" '/opt/fspec/current/bin/formalshell ipc --any-display call lock status | grep "\"boot\":true" | grep -q "\"secure\":true"'; sleep 3; }
desktop_up() { until_guest "the desktop's bar" "$1" 'hyprctl -j layers -i 0 | grep -q "\"namespace\": *\"formalshell:bar\""'; sleep 3; }

rm -rf "$out"
mkdir -p "$out"

f stop >/dev/null 2>&1 || true
if [ -n "${FS_VM_RESULT:-}" ]; then
  say "linking this tree's build over the guest's package"
  f start | tail -1
  until_guest "ssh" 120 true
  rig_guest_copy_result "$vm" "$((2222 + FS_VM_SLOT))"
  rig_guest_link_result "$vm" keep
  f stop | tail -1
fi
# The owner's hosts boot onto a wallpaper; the guest's user has none, and
# the lock's dark fallback under a dissolve films as black. One like
# --showcase's goes into the user's state.json on the guest's disk for the
# film and comes out again before the save.
say "a wallpaper for the film"
f start | tail -1
until_guest "ssh" 120 true
guest "set -e; mkdir -p ~/.local/state/formalshell; st=~/.local/state/formalshell/state.json
  magick -size 1280x800 xc:'#101428' \
    \( -size 900x900 radial-gradient:'#e0703a-none' \) -geometry +600+150 -composite \
    \( -size 1000x1000 radial-gradient:'#4a3fb0-none' \) -geometry -350-350 -composite \
    \( -size 750x750 radial-gradient:'#2a8f9a-none' \) -geometry +60+330 -composite \
    -blur 0x30 /var/tmp/seams-wallpaper.png
  test -e \$st.seams-orig || cp -a \$st \$st.seams-orig 2>/dev/null || echo '{}' > \$st.seams-orig
  jq '.wallpaper = \"/var/tmp/seams-wallpaper.png\"' \$st.seams-orig > \$st; sync"
if [ -n "${FS_SEAMS_SCALE:-}" ]; then
  say "the guest's output at scale $FS_SEAMS_SCALE"
  guest "set -e; c=/etc/fstestvm/hyprland.lua; test -e \$c.seams-orig || sudo cp -a \$c \$c.seams-orig
    sudo sed -i '/^hl.monitor/s/scale = [0-9.]*/scale = $FS_SEAMS_SCALE/' \$c; grep '^hl.monitor' \$c; sync"
fi
f stop | tail -1
say "boot, from power-on"
# At 20 ms a frame, since the locked login's bound is 100 ms of black.
f record --boot --every-ms 20 --seconds 60 | tail -1
lock_up 60
f type "$password" --enter
desktop_up 60
f frames "$out/boot" | tail -1
echo 20 > "$out/boot/every-ms"

say "logout"
f record --seconds 20 | tail -1
f key meta_l-shift-e
greeter_up 20
f frames "$out/logout" | tail -1
f type "$password" --enter
desktop_up 60

say "reboot, to the lock again"
f record --seconds 90 | tail -1
f key meta_l-spc
sleep 1
f type reboot
sleep 1
f key ret
sleep 1
f key ret
f frames "$out/reboot" | tail -1
lock_up 30
f type "$password" --enter
desktop_up 60

say "shutdown, to power-off"
f record --seconds 60 | tail -1
f key meta_l-spc
sleep 1
f type shutdown
sleep 1
f key ret
sleep 1
f key ret
f frames "$out/shutdown" | tail -1

for leg in boot logout reboot shutdown; do
  say "$leg: $(find "$out/$leg/frames" -name '*.png' | wc -l | tr -d ' ') frames in $out/$leg/frames"
done

say "saving $vm again, which the cold boot dropped"
f start | tail -1
until_guest "ssh" 120 true
if [ -n "${FS_VM_RESULT:-}" ]; then
  guest "set -e; for b in $bins; do sudo rm -f /opt/fspec/current/bin/\$b; test ! -e $result.orig/\$b -a ! -L $result.orig/\$b || sudo cp -a $result.orig/\$b /opt/fspec/current/bin/; done; sudo rm -rf $result $result.orig; sync"
  say "the package's links put back"
fi
guest "st=~/.local/state/formalshell/state.json; mv \$st.seams-orig \$st; rm -f /var/tmp/seams-wallpaper.png
  c=/etc/fstestvm/hyprland.lua; test ! -e \$c.seams-orig || sudo mv \$c.seams-orig \$c; sync"
say "the user's state.json and the guest's hyprland.lua put back"
# The autologin's boot lock leaves its marker in /run; saved with it, every
# resumed run's shell would raise the boot lock.
guest "sudo systemctl restart greetd"
sleep 10
guest "rm -f /run/user/1000/formalshell/login-lock"
f save | tail -1

dev/seams-check.py "$out"
