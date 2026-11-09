# shellcheck shell=bash
# The rig's plumbing, sourced by dev/remote.sh, dev/vm.sh, dev/seams.sh,
# dev/check.sh, dev/lint.sh, dev/mac-vm.sh and dev/tarball.sh. Sourcing it
# runs nothing; a caller that uses a function sets `repo_root` first when it
# needs one of the tree-relative ones.
#
# Slot locking is dev/rig_lock.py, which hands the slot back through
# FS_SLOT_FILE (a value, not stderr text).

rig_dev=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
rig_root=$(dirname "$rig_dev")
rig_ssh_opts=(-o BatchMode=yes -o ConnectTimeout=10 -o LogLevel=ERROR)
rig_spec="$HOME/.config/formalspec"
# The tree on a slot user or a guest, relative to its home.
rig_remote_dir=formalshell
rig_vm_host=g815
# The admin account of the VM rig's guests and of the mac rig's.
rig_guest_admin=kyan
rig_vm_result=/var/tmp/formalshell-result
rig_result_prefix=/tmp/formalshell-rig-result-
rig_greeter_bins="formalshell formalshell-ipc formalshell-greeter"
# The service crates' checks: the first four on a private bus on the rig,
# the others on real daemons in a VM rig guest. dev/lint.sh tests the rest.
rig_session_checks="fs-tray fs-notifd fs-mpris fs-dbus"
rig_vm_checks="fs-auth fs-audio fs-bluez fs-network fs-upower"

rig_is_session_check() { case " $rig_session_checks " in *" $1 "*) return 0 ;; esac; return 1; }
rig_is_vm_check() { case " $rig_vm_checks " in *" $1 "*) return 0 ;; esac; return 1; }

rig_host() {
  case "$1" in
    g815) echo g815 ;;
    mac) echo fstestmac ;;
    *) echo "FS_RIG must be g815 or mac, got '$1'" >&2; return 1 ;;
  esac
}

# A directory every slot user can read, for one build shared by a list of runs.
rig_result_name() { echo "$rig_result_prefix$(date +%s)-$$"; }
rig_result_ok() { case "$1" in "$rig_result_prefix"*) return 0 ;; esac; return 1; }

# The one launcher script (packaging/formalshell-launcher.in) with the PATH
# line of the build that uses it.
rig_launcher() {
  local path_line=$1
  sed "s|@PATH@|$path_line|" "$rig_root/packaging/formalshell-launcher.in"
}

# The greeter's session script: the launcher's `greeter` verb.
rig_greeter_wrapper() {
  printf '#!/bin/sh\nexec "$(dirname "$(readlink -f "$0")")/formalshell" greeter "$@"\n'
}

# The tree, as every slot user and guest gets it. Files are compared by
# checksum and written with the time of the sync, never the sender's mtime:
# a slot's target can be newer than a source carried over from another
# checkout, and cargo would skip rebuilding it. .gitignore's patterns keep
# crates/target, result and artifacts on their own side; macOS's openrsync
# reads a `:- .gitignore` dir-merge for the sending side only and lets
# --delete remove the same paths on the receiver, which wiped crates/target
# before every build, so --exclude-from protects them there too.
# rig_sync_tree <ssh-dest>
rig_sync_tree() {
  rsync -rlpcz --delete \
    --exclude-from="$rig_root/.gitignore" \
    --filter=':- .gitignore' \
    --exclude '/.git' \
    --exclude '/.claude/' \
    --exclude '/.rig-target-checked' \
    -e "ssh ${rig_ssh_opts[*]}" \
    "$rig_root/" "$1:$rig_remote_dir/"
}

# Removes what a finished run left in a slot user's /tmp: its mktemp
# directories, and a shared result nobody removed. The slot is held by one run
# at a time (dev/rig_lock.py), so no run of this user is still using them; a
# result younger than three hours may be a list's live build, which its own
# command removes (rig_drop_shared).
# rig_slot_clean <ssh-dest>
rig_slot_clean() {
  # find cannot stat other users' FUSE mounts in /tmp, so its status says
  # nothing; the check is what is left.
  ssh "${rig_ssh_opts[@]}" "$1" 'pat() { find /tmp -maxdepth 1 -user "$(id -u)" \( -name "tmp.*" -o \( -name "formalshell-rig-result-*" -mmin +180 \) \) "$@" 2>/dev/null; }
    pat -exec rm -rf {} +
    [ -z "$(pat -print -quit)" ]' \
    || echo "rig: could not clean /tmp on $1" >&2
}

# Builds on a rig slot into a shared directory, for runs on other slots or in
# guests. Sets rig_shared and rig_builder; rig_drop_shared removes it.
# rig_build_shared <rig> build | rig_build_shared <rig> check <crate>
rig_build_shared() {
  local rig=$1 what=$2 crate=${3:-} cmd slot_file log t0=$SECONDS
  rig_shared=$(rig_result_name)
  rig_shared_rig=$rig
  rig_builder=
  case "$what" in
    build) cmd="dev/build.sh && dev/share-result.sh $rig_shared" ;;
    check) cmd="dev/check.sh build $crate $rig_shared" ;;
    *) echo "rig_build_shared: build or check, not '$what'" >&2; return 1 ;;
  esac
  slot_file=$(mktemp)
  log=$(mktemp)
  if ! env -u FS_RIG_SLOT FS_RIG="$rig" FS_SLOT_FILE="$slot_file" "$rig_root/dev/remote.sh" run "$cmd" > "$log" 2>&1; then
    read -r rig_builder _ < "$slot_file" || true
    tail -30 "$log" >&2
    rm -f "$slot_file" "$log"
    rig_drop_shared
    echo "$what failed on the $rig rig" >&2
    return 1
  fi
  read -r rig_builder _ < "$slot_file"
  rm -f "$slot_file" "$log"
  echo "built in $((SECONDS - t0))s on $rig slot $rig_builder"
}

rig_drop_shared() {
  [ -n "${rig_shared:-}" ] && [ -n "${rig_builder:-}" ] || return 0
  ssh "${rig_ssh_opts[@]}" "fstest$rig_builder@$(rig_host "$rig_shared_rig")" "rm -rf $rig_shared" \
    || echo "rig: could not remove $rig_shared on slot $rig_builder" >&2
  rig_shared=
}

# Runs a command with FS_VM_RESULT naming a fresh build, removed afterwards
# on every way out.
# rig_with_build build -- <cmd...> | rig_with_build check <crate> -- <cmd...>
rig_with_build() {
  local what=$1 crate= status=0
  shift
  if [ "$what" = check ]; then crate=$1; shift; fi
  shift
  rig_build_shared g815 "$what" "$crate" || return 1
  trap 'rig_drop_shared; exit 143' INT TERM HUP
  FS_VM_RESULT="$rig_shared" "$@" || status=$?
  trap - INT TERM HUP
  rig_drop_shared
  return "$status"
}

# Copies $FS_VM_RESULT from g815's /tmp into a VM rig guest without leaving
# g815, over the agent this mac forwards. /var/tmp, not the home, so the
# greeter account can read it. Its links name g815's fspec formalshell
# package; a guest saved at another rev of it has its own, so a link that
# does not resolve there is pointed at the same file in the guest's package.
# rig_guest_copy_result <vm> <ssh-port>
rig_guest_copy_result() {
  local vm=$1 port=$2
  ssh "${rig_ssh_opts[@]}" -A "$rig_vm_host" "rsync -a --delete -e 'ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -p $port' $(printf '%q' "$FS_VM_RESULT")/ $rig_guest_admin@127.0.0.1:$rig_vm_result/"
  ssh "${rig_ssh_opts[@]}" "$vm" "set -e; pkg=\$(readlink -f /opt/fspec/current/bin/formalshell); pkg=\${pkg%/bin/formalshell}
    find $rig_vm_result -type l ! -exec test -e {} \\; -print | while read -r link; do
      to=\$(readlink \"\$link\"); ln -sfn \"\$pkg/\${to#*/formalshell/}\" \"\$link\"
    done"
}

# Installs the copied result over the guest's package: the greeter's session
# script and the desktop's config run /opt/fspec/current/bin/*. With `keep`,
# the package's own binaries are saved first, for dev/seams.sh to put back.
# rig_guest_link_result <vm> [keep]
rig_guest_link_result() {
  local vm=$1 keep=${2:-}
  rig_greeter_wrapper | ssh "${rig_ssh_opts[@]}" "$vm" "cat > $rig_vm_result/bin/formalshell-greeter && chmod 755 $rig_vm_result/bin/formalshell-greeter"
  ssh "${rig_ssh_opts[@]}" "$vm" "set -e
    if [ -n '$keep' ]; then test -e $rig_vm_result.orig || { mkdir -p $rig_vm_result.orig; for b in $rig_greeter_bins; do sudo cp -a /opt/fspec/current/bin/\$b $rig_vm_result.orig/ 2>/dev/null || true; done; }; fi
    for b in $rig_greeter_bins; do sudo ln -sfn $rig_vm_result/bin/\$b /opt/fspec/current/bin/\$b; done
    sync"
}

# A remote file or directory a run listed, into <artifacts> under its own
# basename; a .tar is a directory of frames.
# rig_pull_one <ssh-dest> <remote-path> <artifacts>
rig_pull_one() {
  local dest=$1 remote=$2 artifacts=$3 local_path
  mkdir -p "$artifacts"
  case "$remote" in
    *.tar)
      local_path="$artifacts/$(basename "$remote" .tar)"
      mkdir -p "$local_path"
      ssh "${rig_ssh_opts[@]}" "$dest" "cat $(printf '%q' "$remote")" | tar -xf - -C "$local_path" \
        && echo "pulled frames: $local_path"
      ;;
    *)
      local_path="$artifacts/$(basename "$remote")"
      rsync -a -e "ssh ${rig_ssh_opts[*]}" "$dest:$remote" "$local_path" \
        && echo "pulled: $local_path"
      ;;
  esac
}

# What a smoke run's output (a file) names, pulled into <artifacts>: the
# SMOKE_OK screenshot, the rest of its output as a JSON sidecar, every
# SMOKE_<NAME> artifact, and the committed output of the legs in the flags.
# Returns the run's own status, or 1 when a passing run printed no SMOKE_OK.
# rig_pull_smoke <ssh-dest> <label> <artifacts> <status> <output-file> [flags...]
rig_pull_smoke() {
  local dest=$1 label=$2 artifacts=$3 status=$4 out_file=$5 ts remote rest line dir glob flag
  shift 5
  if [ "$status" -ne 0 ]; then
    for remote in $(grep -oE '^SMOKE_[A-Za-z0-9_]+ [^[:space:]]+\.(png|log|tar)' "$out_file" | awk '{print $2}'); do
      rig_pull_one "$dest" "$remote" "$artifacts"
    done
    echo "$label: smoke run failed (exit $status)" >&2
    return "$status"
  fi
  remote=$(grep -E '^SMOKE_OK [^[:space:]]+' "$out_file" | tail -1 | awk '{print $2}')
  if [ -z "$remote" ]; then
    echo "$label: no SMOKE_OK line in smoke output" >&2
    return 1
  fi
  ts=$(date +%Y%m%d-%H%M%S)
  mkdir -p "$artifacts"
  rsync -a -e "ssh ${rig_ssh_opts[*]}" "$dest:$remote" "$artifacts/smoke-$ts.png"
  echo "pulled screenshot: $artifacts/smoke-$ts.png"
  rest=$(sed -E 's/SMOKE_OK [^[:space:]]+//' "$out_file" | sed '/^[[:space:]]*$/d')
  if [ -n "$(printf '%s' "$rest" | tr -d '[:space:]')" ]; then
    printf '%s\n' "$rest" > "$artifacts/smoke-$ts.json"
    echo "pulled dump/status output: $artifacts/smoke-$ts.json"
  fi
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    remote=$(printf '%s\n' "$line" | awk '{print $2}')
    case "$remote" in *.png|*.log|*.tar) rig_pull_one "$dest" "$remote" "$artifacts" ;; esac
  done <<< "$(grep -oE '^SMOKE_[A-Za-z0-9_]+ [^[:space:]]+' "$out_file" | grep -v '^SMOKE_OK ')"
  for flag in "$@"; do
    case "$flag" in --*) ;; *) continue ;; esac
    while IFS=$'\t' read -r dir glob; do
      [ -n "$dir" ] || continue
      mkdir -p "$rig_root/$dir"
      rsync -az -e "ssh ${rig_ssh_opts[*]}" --include "$glob" --exclude '*' \
        "$dest:$rig_remote_dir/$dir/" "$rig_root/$dir/"
      echo "pulled: $rig_root/$dir/$glob"
    done < <(printf '%s' "$FS_MANIFEST" | jq -r --arg flag "$flag" '.legs[] | select(.flag | split(" ")[0] == $flag) | .commits[] | [.dir, .glob] | @tsv')
  done
}

# Environment a leg declares (leg_<n>_env) and the run knobs, as a
# `NAME=value ` prefix for a remote command line.
# rig_forward_env <name...>
rig_forward_env() {
  local name forward=
  for name in "$@" $(printf '%s' "$FS_MANIFEST" | jq -r '[.legs[].env[]] | unique[]'); do
    [ -z "${!name:-}" ] || forward+="$name=$(printf '%q' "${!name}") "
  done
  printf '%s' "$forward"
}
