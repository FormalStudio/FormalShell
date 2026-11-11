#!/usr/bin/env bash
# The VM rig: the runs a g815 slot user cannot take, on the fstestvm guests.
#
# fstestvm0 to fstestvm3 are openSUSE Tumbleweed VMs on g815, declared in the
# owner's fspec spec (FormalSnake/dotfiles, hosts/fstestvm) and driven by
# `fspec vm` (FormalSpec's docs/formalshell-vm-rig.md). Each has a system bus
# of its own with NetworkManager against two hostapd radios, BlueZ with no
# controller, UPower, polkit, logind, PAM (`formalshell-lock`, password
# formalshell-test), PipeWire, greetd running the FormalShell greeter, and a
# vkms card dev/smoke.sh renders on.
#
# A run takes a free slot (a mac-side flock each, FS_VM_SLOTS of them),
# resumes it from its save (`fspec vm resume`, about 2s, on a throwaway
# overlay of its disk), copies the tree and a build in, runs, pulls the
# artifacts and stops it, which drops everything the run wrote: every run
# starts on the same clean machine. Nothing compiles in a guest. The build
# is a rig slot's (dev/remote.sh), or the FS_RESULT a smoke-all shares; the
# guest's own fspec formalshell package has the same fonts, tiles and helpers
# the build links to.
#
#   dev/vm.sh smoke <flags>   a run with a leg_<n>_vm_only leg in it; dev/remote.sh
#                             smoke hands those here, smoke-all's included
#   dev/vm.sh check <name>    a crate in dev/rig.sh's rig_vm_checks
#                             (dev/check.sh, built on the rig, run in the guest)
#   dev/vm.sh greeter         dev/smoke-greeter.sh, this tree's build installed
#                             over the guest's shell
#   dev/vm.sh shell           a shell in a resumed slot, stopped on exit
#   dev/vm.sh hold <cmd...>   <cmd> with a slot held and FS_VM_SLOT set, the VM
#                             left as it is (dev/seams.sh)
#   dev/vm.sh wants <flags>   exits 0 when the flags name a leg_<n>_vm_only leg
#
# A saved slot goes stale when an apply changes its disk: `fspec vm stop`,
# `fspec vm start`, `fspec vm save` after `fspec apply --host fstestvm<n>`,
# the boot's autologin session terminated first so the save sits at the
# greeter (CLAUDE.md, the VM rig).
set -euo pipefail
cd "$(dirname "$0")/.."
repo_root="$(pwd)"
self="$repo_root/dev/vm.sh"
. "$repo_root/dev/rig.sh"

usage() {
  echo "usage: $0 {smoke <flags...>|check <name>|greeter|shell|hold <cmd...>|wants <flags...>}" >&2
  exit 1
}
[ $# -gt 0 ] || usage

slots="${FS_VM_SLOTS:-$rig_vm_slots}"
spec=$rig_spec

# Every leg's declarations (dev/smoke.sh --manifest); dev/remote.sh hands
# them over in FS_MANIFEST.
[ -n "${FS_MANIFEST:-}" ] || FS_MANIFEST=$("$repo_root/dev/smoke.sh" --manifest | jq -c .)
export FS_MANIFEST

if [ "$1" = wants ]; then
  shift
  flags=" $(printf '%s' "$FS_MANIFEST" | jq -r '.legs[] | select(.vm_only) | .flag | split(" ")[0]' | tr '\n' ' ')"
  for flag in "$@"; do
    case "$flags" in *" $flag "*) exit 0 ;; esac
  done
  exit 1
fi

. "$repo_root/dev/perf-hold.sh"
perf_gate_wait vm.sh

verb=$1
shift
case "$verb" in
  smoke|greeter|shell) ;;
  hold) [ $# -gt 0 ] || usage ;;
  check)
    [ $# -eq 1 ] || usage
    rig_is_vm_check "$1" || { echo "$1: not a VM check ($rig_vm_checks)" >&2; exit 1; }
    ;;
  *) usage ;;
esac

if ! fspec vm resume --help >/dev/null 2>&1; then
  echo "vm: fspec has no \`vm resume\`; install FormalSpec's main (cargo install --locked --path crates/fspec)" >&2
  exit 1
fi

# The build, before a slot is taken: a rig slot builds it and shares it in
# g815's /tmp, where the copy into the guest reads it. A smoke-all's
# FS_RESULT is already that.
if [ -z "${FS_VM_RESULT:-}" ]; then
  if [ -n "${FS_RESULT:-}" ] && [ "$verb" != check ]; then
    export FS_VM_RESULT="$FS_RESULT"
  elif [ "$verb" != shell ] && [ "$verb" != hold ]; then
    status=0
    if [ "$verb" = check ]; then
      rig_with_build check "$1" -- "$self" "$verb" "$@" || status=$?
    else
      rig_with_build build -- "$self" "$verb" "$@" || status=$?
    fi
    exit "$status"
  fi
fi

# Slot locks live on the mac, as dev/remote.sh's do. The whole command holds
# its slot, from the resume to the stop.
if [ -z "${FS_VM_SLOT:-}" ]; then
  exec python3 "$repo_root/dev/rig_lock.py" vm "$slots" -- "$self" "$verb" "$@"
fi
case "$FS_VM_SLOT" in
  ''|*[!0-9]*) echo "FS_VM_SLOT must be a slot number, got '$FS_VM_SLOT'" >&2; exit 1 ;;
esac
[ "$verb" = hold ] && exec "$@"
vm="fstestvm$FS_VM_SLOT"
port=$(rig_vm_port "$FS_VM_SLOT")

stop_vm() { fspec vm stop "$vm" --spec "$spec" >/dev/null 2>&1 || true; }
# A slot someone left running is stopped first: a resume never joins a run.
stop_vm
trap 'stop_vm; exit 143' INT TERM HUP
trap stop_vm EXIT
fspec vm resume "$vm" --spec "$spec" | tail -1

guest_ssh() { ssh "${rig_ssh_opts[@]}" "$vm" "$@"; }

rig_sync_tree "$vm"

if [ -n "${FS_VM_RESULT:-}" ]; then
  rig_guest_copy_result "$vm" "$port"
fi

artifacts="${FS_ARTIFACTS:-$repo_root/artifacts}"

case "$verb" in
  shell)
    ssh -t "${rig_ssh_opts[@]}" "$vm" "cd ~/$rig_remote_dir; exec bash -l" || true
    ;;
  check)
    guest_ssh "cd ~/$rig_remote_dir && FS_CHECK_BIN=$rig_vm_result exec dev/check.sh $1"
    ;;
  greeter)
    # The greeter's session script and the desktop's config run the guest's
    # installed shell under /opt/fspec/current; on the overlay that is this
    # build for the length of the run.
    rig_guest_link_result "$vm"
    status=0
    out_file=$(mktemp)
    guest_ssh "cd ~/$rig_remote_dir && exec dev/smoke-greeter.sh" > "$out_file" 2>&1 || status=$?
    cat "$out_file"
    artifacts="$artifacts/greeter"
    for remote in $(grep -oE '^SMOKE_GREETER_[A-Z]+ [^[:space:]]+' "$out_file" | awk '{print $2}'); do
      rig_pull_one "$vm" "$rig_remote_dir/$remote" "$artifacts" || status=1
    done
    rm -f "$out_file"
    for remote in greeter-session.log greetd-journal.txt; do
      guest_ssh "test -e $rig_remote_dir/artifacts/greeter/$remote" || continue
      rig_pull_one "$vm" "$rig_remote_dir/artifacts/greeter/$remote" "$artifacts" || status=1
    done
    exit "$status"
    ;;
  smoke)
    out_file=$(mktemp)
    status=0
    guest_ssh "cd ~/$rig_remote_dir && $(rig_forward_env FS_CPU_QUOTA FS_CPU_QUOTA_PERIOD FS_SMOKE_SCALE)FS_RESULT=$rig_vm_result exec ./dev/smoke.sh $*" > "$out_file" 2>&1 || status=$?
    cat "$out_file"
    rig_pull_smoke "$vm" "$vm" "$artifacts" "$status" "$out_file" "$@" || status=$?
    rm -f "$out_file"
    exit "$status"
    ;;
esac
