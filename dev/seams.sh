#!/usr/bin/env bash
# The boot chain's seams on film: a VM rig slot (dev/vm.sh) recorded from
# power-on to the desktop, then out of it three ways, as PNG frames over
# QMP screendump, typed into over QMP send-key (`fspec vm record`, `type`,
# `key`, `frames`; FormalSpec's docs/formalshell-vm-rig.md).
#
#   dev/seams.sh      artifacts/seams/{boot,logout,reboot,shutdown}/frames/
#
# Frame n is n x 100 ms into its recording, so a file's number is its time.
#   boot      power-on: OVMF, Limine, systemd-boot, the FormalOS splash, the
#             greeter, the password typed, the desktop with its bar
#   logout    SUPER+SHIFT+E out of the desktop and back to the greeter
#   reboot    the launcher's Reboot, through the shutdown splash, the
#             firmware and the boot again, to the greeter
#   shutdown  the launcher's Shutdown, to the last frame before power-off
#
# The slot boots its applied disk, the guest's installed formalshell
# package, not this tree's build: the seams are the boot chain's, and a
# build installed over the package would outlive the run on that disk. A
# cold boot drops the slot's save, so the script saves it again at the end.
set -euo pipefail
cd "$(dirname "$0")/.."
repo_root="$(pwd)"

[ -n "${FS_VM_SLOT:-}" ] || exec "$repo_root/dev/vm.sh" hold "$0" "$@"

spec="${FS_VM_SPEC:-$HOME/.config/formalspec}"
fspec="${FSPEC:-fspec}"
vm="fstestvm$FS_VM_SLOT"
out="$repo_root/artifacts/seams"
password=formalshell-test
ssh_opts=(-o BatchMode=yes -o ConnectTimeout=10 -o LogLevel=ERROR)

f() { "$fspec" vm "$1" "$vm" "${@:2}" --spec "$spec"; }
guest() { ssh "${ssh_opts[@]}" "$vm" "$@"; }
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
desktop_up() { until_guest "the desktop's bar" "$1" 'hyprctl -j layers -i 0 | grep -q "\"namespace\": *\"formalshell:bar\""'; sleep 3; }

rm -rf "$out"
mkdir -p "$out"

f stop >/dev/null 2>&1 || true
say "boot, from power-on"
f record --boot --seconds 60 | tail -1
greeter_up 60
f type "$password" --enter
desktop_up 60
f frames "$out/boot" | tail -1

say "logout"
f record --seconds 20 | tail -1
f key meta_l-shift-e
greeter_up 20
f frames "$out/logout" | tail -1
f type "$password" --enter
desktop_up 60

say "reboot, to the greeter again"
f record --seconds 90 | tail -1
f key meta_l-spc
sleep 1
f type reboot
sleep 1
f key ret
sleep 1
f key ret
f frames "$out/reboot" | tail -1
greeter_up 30
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
f save | tail -1
