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
#   dev/vm.sh check <name>    fs-auth, fs-audio, fs-bluez, fs-network, fs-upower
#                             (dev/check.sh, built on the rig, run in the guest)
#   dev/vm.sh greeter         dev/smoke-greeter.sh, this tree's build installed
#                             over the guest's shell
#   dev/vm.sh shell           a shell in a resumed slot, stopped on exit
#   dev/vm.sh hold <cmd...>   <cmd> with a slot held and FS_VM_SLOT set, the VM
#                             left as it is (dev/seams.sh)
#   dev/vm.sh wants <flags>   exits 0 when the flags name a leg_<n>_vm_only leg
#
# A saved slot goes stale when an apply changes its disk: `fspec vm stop`,
# `fspec vm start`, `fspec vm save` after `fspec apply --host fstestvm<n>`.
set -euo pipefail
cd "$(dirname "$0")/.."
repo_root="$(pwd)"
self="$repo_root/dev/vm.sh"

usage() {
  echo "usage: $0 {smoke <flags...>|check <name>|greeter|shell|hold <cmd...>|wants <flags...>}" >&2
  exit 1
}
[ $# -gt 0 ] || usage

host="${FS_G815_HOST:-g815}"
# The count of fstestvm hosts hosts/fstestvm/host.star declares.
slots="${FS_VM_SLOTS:-4}"
spec="${FS_VM_SPEC:-$HOME/.config/formalspec}"
fspec="${FSPEC:-fspec}"
ssh_opts=(-o BatchMode=yes -o ConnectTimeout=10 -o LogLevel=ERROR)

# The flag of every leg that declares leg_<n>_vm_only.
vm_flags() {
  local f leg
  for f in dev/smoke.d/*.sh; do
    leg=$(basename "$f" .sh)
    grep -q "^leg_${leg}_vm_only=" "$f" || continue
    sed -n "s/^leg_${leg}_flag=\"\(--[a-z0-9-]*\).*/\1/p" "$f"
  done
}

if [ "$1" = wants ]; then
  shift
  flags=" $(vm_flags | tr '\n' ' ')"
  for flag in "$@"; do
    case "$flags" in *" $flag "*) exit 0 ;; esac
  done
  exit 1
fi

verb=$1
shift
case "$verb" in
  smoke|greeter|shell) ;;
  hold) [ $# -gt 0 ] || usage ;;
  check)
    [ $# -eq 1 ] || usage
    case "$1" in fs-auth|fs-audio|fs-bluez|fs-network|fs-upower) ;; *) echo "$1: not a VM check (fs-auth, fs-audio, fs-bluez, fs-network, fs-upower)" >&2; exit 1 ;; esac
    ;;
  *) usage ;;
esac

if ! "$fspec" vm resume --help >/dev/null 2>&1; then
  echo "vm: $fspec has no \`vm resume\`; install FormalSpec's main (cargo install --locked --path crates/fspec)" >&2
  exit 1
fi

# The build, before a slot is taken: a rig slot builds it and shares it in
# g815's /tmp, where the copy into the guest reads it. A smoke-all's
# FS_RESULT is already that.
if [ -z "${FS_VM_RESULT:-}" ]; then
  if [ -n "${FS_RESULT:-}" ] && [ "$verb" != check ]; then
    export FS_VM_RESULT="$FS_RESULT"
  elif [ "$verb" != shell ] && [ "$verb" != hold ]; then
    shared="/tmp/formalshell-rig-result-$(date +%s)-$$"
    build_log=$(mktemp)
    case "$verb" in
      check) build="dev/check.sh build $1 $shared" ;;
      *) build="dev/build.sh && dev/share-result.sh $shared" ;;
    esac
    t0=$SECONDS
    if ! env -u FS_RIG_SLOT FS_RIG=g815 "$repo_root/dev/remote.sh" run "$build" > "$build_log" 2>&1; then
      tail -30 "$build_log" >&2
      echo "vm: build failed on the rig" >&2
      rm -f "$build_log"
      exit 1
    fi
    builder=$(sed -n 's/^g815: slot \([0-9]*\)$/\1/p' "$build_log" | head -1)
    rm -f "$build_log"
    echo "vm: built in $((SECONDS - t0))s on rig slot $builder"
    status=0
    FS_VM_RESULT="$shared" "$self" "$verb" "$@" || status=$?
    ssh "${ssh_opts[@]}" "fstest$builder@$host" "rm -rf $shared" || true
    exit "$status"
  fi
fi

# Slot locks live on the mac, as dev/remote.sh's do. The whole command holds
# its slot, from the resume to the stop.
if [ -z "${FS_VM_SLOT:-}" ]; then
  exec python3 -c '
import fcntl, os, subprocess, sys, time
slots, cmd = int(sys.argv[1]), sys.argv[2:]
began = time.time()
while True:
    for n in range(slots):
        lock = open("/tmp/formalshell-fstestvm-%d.lock" % n, "w")
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            lock.close()
            continue
        os.environ["FS_VM_SLOT"] = str(n)
        waited = int(time.time() - began)
        if waited:
            print("g815: waited %ds for a slot" % waited, file=sys.stderr)
        print("g815: slot vm%d" % n, file=sys.stderr)
        sys.exit(subprocess.call(cmd))
    time.sleep(3)
' "$slots" "$self" "$verb" "$@"
fi
case "$FS_VM_SLOT" in
  ''|*[!0-9]*) echo "FS_VM_SLOT must be a slot number, got '$FS_VM_SLOT'" >&2; exit 1 ;;
esac
[ "$verb" = hold ] && exec "$@"
vm="fstestvm$FS_VM_SLOT"
port=$((2222 + FS_VM_SLOT))

stop_vm() { "$fspec" vm stop "$vm" --spec "$spec" >/dev/null 2>&1 || true; }
# A slot someone left running is stopped first: a resume never joins a run.
stop_vm
trap 'stop_vm; exit 143' INT TERM HUP
trap stop_vm EXIT
"$fspec" vm resume "$vm" --spec "$spec" | tail -1

guest_ssh() { ssh "${ssh_opts[@]}" "$vm" "$@"; }

# The tree, as dev/remote.sh syncs it to a slot user.
rsync -az --delete \
  --exclude-from="$repo_root/.gitignore" \
  --filter=':- .gitignore' \
  --exclude '/.git' \
  --exclude '/.claude/' \
  -e "ssh ${ssh_opts[*]}" \
  "$repo_root/" "$vm:formalshell/"

# The build goes from g815's /tmp to the guest without leaving g815, over
# the agent this mac forwards. /var/tmp, not the home, so the greeter
# account can read it. Its links name g815's fspec formalshell package; a
# guest saved at another rev of it has its own, so a link that does not
# resolve there is pointed at the same file in the guest's package.
result=/var/tmp/formalshell-result
if [ -n "${FS_VM_RESULT:-}" ]; then
  ssh "${ssh_opts[@]}" -A "$host" "rsync -a --delete -e 'ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -p $port' $(printf '%q' "$FS_VM_RESULT")/ kyan@127.0.0.1:$result/"
  guest_ssh "pkg=\$(readlink -f /opt/fspec/current/bin/formalshell) && pkg=\${pkg%/bin/formalshell} &&
    find $result -type l ! -exec test -e {} \\; -print | while read -r link; do
      to=\$(readlink \"\$link\"); ln -sfn \"\$pkg/\${to#*/formalshell/}\" \"\$link\"
    done"
fi

artifacts="${FS_ARTIFACTS:-$repo_root/artifacts}"
pull() {
  local remote=$1
  mkdir -p "$artifacts"
  rsync -a -e "ssh ${ssh_opts[*]}" "$vm:$remote" "$artifacts/$(basename "$remote")" \
    && echo "pulled: $artifacts/$(basename "$remote")"
}

case "$verb" in
  shell)
    ssh -t "${ssh_opts[@]}" "$vm" "cd ~/formalshell; exec bash -l" || true
    ;;
  check)
    guest_ssh "cd ~/formalshell && FS_CHECK_BIN=$result exec dev/check.sh $1"
    ;;
  greeter)
    # The greeter's session script and the desktop's config run the guest's
    # installed shell under /opt/fspec/current; on the overlay that is this
    # build for the length of the run.
    status=0
    out=$(guest_ssh "set -e
      printf '#!/bin/sh\nexec \"\$(dirname \"\$(readlink -f \"\$0\")\")/formalshell\" greeter \"\$@\"\n' > $result/bin/formalshell-greeter
      chmod 755 $result/bin/formalshell-greeter
      for f in formalshell formalshell-ipc formalshell-greeter; do sudo ln -sfn $result/bin/\$f /opt/fspec/current/bin/\$f; done
      cd ~/formalshell && exec dev/smoke-greeter.sh" 2>&1) || status=$?
    echo "$out"
    artifacts="$artifacts/greeter"
    for remote in $(printf '%s\n' "$out" | grep -oE '^SMOKE_GREETER_[A-Z]+ [^[:space:]]+' | awk '{print $2}'); do
      pull "formalshell/$remote"
    done
    for remote in greeter-session.log greetd-journal.txt; do
      guest_ssh "test -e formalshell/artifacts/greeter/$remote" && pull "formalshell/artifacts/greeter/$remote"
    done
    exit "$status"
    ;;
  smoke)
    forward=""
    for name in SMOKE_WALLPAPER_DITHER FS_CPU_QUOTA FS_CPU_QUOTA_PERIOD FS_DEPTH_PICTURE FS_SMOKE_SCALE; do
      [ -z "${!name:-}" ] || forward+="$name=$(printf '%q' "${!name}") "
    done
    status=0
    out=$(guest_ssh "cd ~/formalshell && ${forward}FS_RESULT=$result exec ./dev/smoke.sh $*" 2>&1) || status=$?
    echo "$out"
    ts=$(date +%Y%m%d-%H%M%S)
    ok=$(printf '%s\n' "$out" | grep -oE 'SMOKE_OK [^[:space:]]+' | tail -1 | awk '{print $2}')
    if [ -n "$ok" ]; then
      mkdir -p "$artifacts"
      rsync -a -e "ssh ${ssh_opts[*]}" "$vm:$ok" "$artifacts/smoke-$ts.png" && echo "pulled screenshot: $artifacts/smoke-$ts.png"
    fi
    for remote in $(printf '%s\n' "$out" | grep -oE '^SMOKE_[A-Z0-9_]+ [^[:space:]]+\.(png|log|tar)' | grep -v '^SMOKE_OK ' | awk '{print $2}'); do
      case "$remote" in
        *.tar) mkdir -p "$artifacts/$(basename "$remote" .tar)"; guest_ssh "cat $(printf '%q' "$remote")" | tar -xf - -C "$artifacts/$(basename "$remote" .tar)" ;;
        *) pull "$remote" ;;
      esac
    done
    if [ "$status" -ne 0 ]; then
      echo "$vm: smoke run failed (exit $status)" >&2
      exit "$status"
    fi
    [ -n "$ok" ] || { echo "$vm: no SMOKE_OK line in smoke output" >&2; exit 1; }
    ;;
esac
