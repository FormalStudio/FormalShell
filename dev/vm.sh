#!/usr/bin/env bash
# Driver for the runtime layer of the mac e2e rig
# (docs/superpowers/plans/2026-07-28-mac-e2e-rig.md): boots
# packages.aarch64-darwin.testvm (nix/testvm.nix) in the background, syncs
# the working tree into it, and runs commands/smoke tests inside with the
# nested session env wired up.
#
# The working tree — not a commit — is what gets tested: `sync` rsyncs it
# straight into the VM over the ssh port-forward.
#
# KEYS follows the same pattern nix-builder-vm.nix uses for its own
# authorized_keys share: `nix-store --add` before boot so the 9p mapped-xattr
# share reports the pubkey as root-owned, which is what lets sshd's
# AuthorizedKeysFile ownership check (StrictModes, left at its secure
# default) pass. Only the .pub is added — the store is world-readable and
# the private key never needs to leave dev/.testvm/keys.
set -euo pipefail
cd "$(dirname "$0")/.."
repo_root="$(pwd)"

# One VM per repository, not per checkout: a git worktree resolves its
# common dir to the main checkout's .git, so every worktree shares that
# checkout's dev/.testvm instead of building and booting a second VM.
testvm_dir="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")/dev/.testvm"

# FS_VM_SLOT picks one of the VMs dev/vm-lock.sh hands out. Slot 0 is the
# original layout (port 2222, disk, log and pid straight in dev/.testvm);
# slot N has its own VM, disk and pid under dev/.testvm/slotN and ssh on
# 2222+N. The keypair is shared. vm-lock.sh hands out slots 0 and 1 only, so
# a slot past those is free for a VM booted off a branch's own testvm.
slot="${FS_VM_SLOT:-0}"
case "$slot" in
  0) work_dir="$testvm_dir" ;;
  [1-9]) work_dir="$testvm_dir/slot$slot" ;;
  *) echo "FS_VM_SLOT must be a digit, got '$slot'" >&2; exit 1 ;;
esac
keys_dir="$testvm_dir/keys"
disk_image="$work_dir/formalshell-testvm.qcow2"
log_file="$work_dir/vm.log"
pid_file="$work_dir/vm.pid"
priv_key="$keys_dir/test_ed25519"

ssh_port=$((2222 + slot))
ssh_opts=(-p "$ssh_port" -i "$priv_key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o BatchMode=yes)
scp_opts=(-P "$ssh_port" -i "$priv_key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o BatchMode=yes)

is_running() {
  [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file")" 2>/dev/null
}

wait_for_ssh() {
  local tries=$1
  for ((i = 0; i < tries; i++)); do
    if ssh "${ssh_opts[@]}" test@localhost true 2>/dev/null; then
      return 0
    fi
    sleep 5
  done
  return 1
}

# Runs a command inside the VM with cwd at the synced repo and the nested
# smoke scripts' expected session env exported. WAYLAND_DISPLAY is read
# live from the systemd --user environment (same fallback dev/smoke.sh
# itself uses) rather than hardcoded, since it is the parent compositor's
# own -- not guaranteed to be wayland-1 forever.
vm_ssh() {
  ssh "${ssh_opts[@]}" test@localhost \
    "cd formalshell && export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=\$(systemctl --user show-environment | sed -n 's/^WAYLAND_DISPLAY=//p') && $*"
}

# vm_ssh with the command in a scope of its own (dev/scoped-run.sh), stopped
# whole if this side is signalled; the remote watcher covers a SIGKILL.
# ssh runs in the background so a trapped signal is acted on at once, with
# stdin handed over explicitly since a background job would lose it.
vm_run() {
  local unit="fs-run-$$-$RANDOM" pid status=0
  vm_ssh "exec dev/scoped-run.sh $unit $(printf '%q' "$*")" <&0 &
  pid=$!
  trap 'kill "$pid" 2>/dev/null; ssh "${ssh_opts[@]}" test@localhost "systemctl --user stop $unit.scope" >/dev/null 2>&1; exit 143' INT TERM HUP
  wait "$pid" || status=$?
  trap - INT TERM HUP
  return "$status"
}

# A qcow2 never gives guest-freed blocks back on this host, so past
# FS_VM_COMPACT_GB allocated (default 20) the image is rewritten with
# `qemu-img convert`, when the mac has room for the copy plus 15 GB spare.
# Runs with the VM stopped and the slot lock held. Prints nothing and leaves
# the image alone on any failure; the old file survives until the new one
# has booted (see cmd_start).
compact_disk() {
  [ -f "$disk_image" ] || return 0
  local used_kb free_kb limit_kb=$(( ${FS_VM_COMPACT_GB:-20} * 1024 * 1024 ))
  used_kb=$(du -k "$disk_image" | awk '{print $1}')
  [ "$used_kb" -gt "$limit_kb" ] || return 0
  free_kb=$(df -k "$work_dir" | awk 'NR==2 {print $4}')
  if [ "$free_kb" -lt $(( used_kb + 15 * 1024 * 1024 )) ]; then
    echo "testvm: $((used_kb / 1048576)) GB image, but only $((free_kb / 1048576)) GB free; not compacting" >&2
    return 0
  fi
  echo "testvm: compacting $disk_image ($((used_kb / 1048576)) GB)" >&2
  rm -f "$disk_image.new"
  if nix shell nixpkgs#qemu -c qemu-img convert -O qcow2 "$disk_image" "$disk_image.new" \
    && nix shell nixpkgs#qemu -c qemu-img check "$disk_image.new" >/dev/null; then
    mv "$disk_image" "$disk_image.old"
    mv "$disk_image.new" "$disk_image"
    echo "testvm: compacted to $(( $(du -k "$disk_image" | awk '{print $1}') / 1048576 )) GB" >&2
  else
    echo "testvm: compaction failed, keeping the old image" >&2
    rm -f "$disk_image.new"
  fi
}

cmd_start() {
  if is_running; then
    echo "testvm already running (pid $(cat "$pid_file"))"
    return 0
  fi
  mkdir -p "$keys_dir" "$work_dir"
  compact_disk
  if [ ! -f "$priv_key" ]; then
    ssh-keygen -t ed25519 -N "" -C "formalshell-testvm" -f "$priv_key" >/dev/null
    echo "generated ssh keypair: $priv_key"
  fi

  git -C "$repo_root" add -A >/dev/null 2>&1 || true  # flakes only see tracked files
  local vm_pkg
  vm_pkg=$(nix build --no-link --print-out-paths "$repo_root#testvm")
  echo "built $vm_pkg"

  # Copy just the pubkey into its own directory before adding to the
  # store — `nix-store --add` on $keys_dir would put the private key
  # (sshd never reads it; only the guest's authorized_keys .pub does)
  # into the world-readable store too.
  local pub_keys_dir="$testvm_dir/keys-pub"
  mkdir -p "$pub_keys_dir"
  cp "$priv_key.pub" "$pub_keys_dir/"
  local keys_store_path
  keys_store_path=$(nix-store --add "$pub_keys_dir")

  (
    cd "$work_dir"
    set -m
    QEMU_NET_OPTS="hostfwd=tcp:127.0.0.1:${ssh_port}-:22" \
      KEYS="$keys_store_path" NIX_DISK_IMAGE="$disk_image" \
      nohup "$vm_pkg/bin/run-formalshell-testvm-vm" >vm.log 2>&1 &
    echo $! >vm.pid
  )
  echo "booting testvm slot $slot (pid $(cat "$pid_file")), log: $log_file"

  if ! wait_for_ssh 60; then
    echo "testvm: ssh did not come up after 5 minutes; see $log_file" >&2
    if [ -f "$disk_image.old" ]; then
      cmd_stop
      mv -f "$disk_image.old" "$disk_image"
      echo "testvm: restored the uncompacted image" >&2
    fi
    exit 1
  fi
  rm -f "$disk_image.old"
  echo "testvm ssh is up on 127.0.0.1:${ssh_port}"
}

cmd_stop() {
  if ! is_running; then
    echo "testvm not running"
    rm -f "$pid_file"
    return 0
  fi
  local pid
  pid=$(cat "$pid_file")
  echo "stopping testvm (pid $pid)"
  ssh "${ssh_opts[@]}" test@localhost 'sudo poweroff' 2>/dev/null || true
  for ((i = 0; i < 30; i++)); do
    is_running || break
    sleep 1
  done
  if is_running; then
    echo "testvm did not power off gracefully, sending SIGTERM"
    kill -TERM "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
    for ((i = 0; i < 10; i++)); do
      is_running || break
      sleep 1
    done
    if is_running; then
      echo "testvm still up, sending SIGKILL"
      kill -KILL "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
    fi
  fi
  rm -f "$pid_file"
}

cmd_status() {
  if is_running; then
    echo "testvm running (pid $(cat "$pid_file"))"
    if ssh "${ssh_opts[@]}" test@localhost true 2>/dev/null; then
      echo "ssh reachable on 127.0.0.1:${ssh_port}"
    else
      echo "ssh not reachable"
    fi
  else
    echo "testvm not running"
  fi
}

cmd_sync() {
  # /.git stays out: in a worktree it is a gitdir file pointing at the mac,
  # which leaves the VM's copy dangling and every `git add -A` there
  # failing. The VM keeps its own repo instead (created below on first
  # sync); the flake only needs the files tracked, not the history.
  # /.claude holds every agent worktree with its own cargo target, tens of
  # gigabytes that filled the VM's 40G disk.
  # Every leg builds the shell inside the VM, and those store paths fill
  # the same disk in a night of parallel runs, so collect them first.
  # fstrim hands the freed blocks back to the qcow2 on the mac (the root
  # drive is attached with discard=unmap, nix/vm-discard.nix).
  vm_ssh 'used=$(df --output=pcent / | tail -1 | tr -dc 0-9); [ "$used" -lt 85 ] || { sudo nix-collect-garbage >/dev/null 2>&1; sudo fstrim -av >/dev/null 2>&1; df -h / | tail -1; }'
  rsync -az --delete \
    --exclude 'result*' \
    --exclude '/.git' \
    --exclude '/.claude/' \
    --exclude '/crates/target/' \
    --exclude 'artifacts/' \
    --exclude '/dev/.linux-builder/' \
    --exclude '/dev/.testvm/' \
    -e "ssh ${ssh_opts[*]}" \
    "$repo_root/" test@localhost:formalshell/
  vm_ssh 'cd ~/formalshell && { [ -d .git ] || git init -q; }'
}

cmd_run() {
  if [ $# -eq 0 ]; then
    echo "usage: $0 run <cmd...>" >&2
    exit 1
  fi
  vm_run "$@"
}

# sync, run the smoke rig with the given flags, then pull the SMOKE_OK
# screenshot plus any other stdout (the dump/status/query JSON the smoke
# script cats inline) back to ./artifacts/ on the mac.
# Builds the shell on the mac (through the linux-builder) and copies the
# closure into the VM's store, then prints the FS_RESULT assignment
# for the VM-side command. A closure the VM already holds copies as a no-op,
# and nothing compiles in the guest, whose freed blocks never shrink the
# qcow2 on the mac. FS_BUILD_IN_VM=1 skips this and lets smoke.sh build in
# the VM. Needs the VM up and the caller holding the slot lock.
prebuild_env() {
  [ -z "${FS_BUILD_IN_VM:-}" ] || return 0
  local attrs=(formalshell) paths=() out attr
  git -C "$repo_root" add -A >/dev/null 2>&1 || true  # flakes only see tracked files
  for attr in "${attrs[@]}"; do
    # One root per slot and package: the copy in use survives a mac GC, and
    # the build it replaces becomes garbage instead of piling up.
    out=$(nix build --out-link "$work_dir/gcroot-$attr" --print-out-paths "$repo_root#packages.aarch64-linux.$attr") || return 1
    paths+=("$out")
    printf 'FS_RESULT=%s ' "$out"
  done
  NIX_SSHOPTS="${ssh_opts[*]}" nix copy --no-check-sigs --to ssh-ng://test@localhost "${paths[@]}" >&2 || return 1
}

cmd_prebuild() {
  local env
  env=$(prebuild_env) || exit 1
  printf '%s\n' "$env"
}

cmd_smoke() {
  local script="./dev/smoke.sh"
  cmd_sync
  local out status=0 prebuilt
  prebuilt=$(prebuild_env) || { echo "testvm: building the shell on the mac failed" >&2; exit 1; }
  out=$(vm_run "${SMOKE_WALLPAPER_DITHER:+SMOKE_WALLPAPER_DITHER=$SMOKE_WALLPAPER_DITHER }${FS_CPU_QUOTA:+FS_CPU_QUOTA=$FS_CPU_QUOTA }${FS_CPU_QUOTA_PERIOD:+FS_CPU_QUOTA_PERIOD=$FS_CPU_QUOTA_PERIOD }${prebuilt}$script $*" 2>&1) || status=$?
  echo "$out"
  if [ "$status" -ne 0 ]; then
    # A failed run's own logs, so the failure can be read on the mac.
    mkdir -p "$repo_root/artifacts"
    local failed_log
    for failed_log in $(printf '%s\n' "$out" | grep -oE '^SMOKE_[A-Z0-9_]+ [^[:space:]]+\.log' | awk '{print $2}'); do
      scp "${scp_opts[@]}" "test@localhost:$failed_log" "$repo_root/artifacts/$(basename "$failed_log")" > /dev/null 2>&1 \
        && echo "pulled log: $repo_root/artifacts/$(basename "$failed_log")"
    done
    local failed_tar
    for failed_tar in $(printf '%s\n' "$out" | grep -oE '^SMOKE_[A-Z0-9_]+ [^[:space:]]+\.tar' | awk '{print $2}'); do
      mkdir -p "$repo_root/artifacts/$(basename "$failed_tar" .tar)"
      scp "${scp_opts[@]}" "test@localhost:$failed_tar" "$repo_root/artifacts/$(basename "$failed_tar")" > /dev/null 2>&1 \
        && tar -xf "$repo_root/artifacts/$(basename "$failed_tar")" -C "$repo_root/artifacts/$(basename "$failed_tar" .tar)" \
        && rm -f "$repo_root/artifacts/$(basename "$failed_tar")" \
        && echo "pulled frames: $repo_root/artifacts/$(basename "$failed_tar" .tar)"
    done
    echo "testvm: smoke run failed (exit $status)" >&2
    exit "$status"
  fi

  # `grep -oE`, not an anchored `^SMOKE_OK` sed: --menu's own last line is a
  # `cat` of selection.txt with no trailing newline, so the SMOKE_OK line
  # lands appended to that JSON instead of starting its own line.
  local remote_png
  remote_png=$(printf '%s\n' "$out" | grep -oE 'SMOKE_OK [^[:space:]]+' | tail -1 | awk '{print $2}')
  if [ -z "$remote_png" ]; then
    echo "testvm: no SMOKE_OK line in smoke output" >&2
    exit 1
  fi

  mkdir -p "$repo_root/artifacts"
  local ts local_png
  ts=$(date +%Y%m%d-%H%M%S)
  local_png="$repo_root/artifacts/smoke-${ts}.png"
  scp "${scp_opts[@]}" "test@localhost:$remote_png" "$local_png"
  echo "pulled screenshot: $local_png"

  # Strip the marker wherever it landed (its own line, or appended to the
  # last JSON line above) before saving whatever's left as the JSON sidecar.
  local rest
  rest=$(printf '%s\n' "$out" | sed -E 's/SMOKE_OK [^[:space:]]+//' | sed '/^[[:space:]]*$/d')
  if [ -n "$(printf '%s' "$rest" | tr -d '[:space:]')" ]; then
    local local_json="$repo_root/artifacts/smoke-${ts}.json"
    printf '%s\n' "$rest" > "$local_json"
    echo "pulled dump/status output: $local_json"
  fi

  # Named secondary screenshots (M14 Task 3's `SMOKE_WIFI_*` lines, same
  # convention --lock/--screensaver already print as SMOKE_LOCK_*/
  # SMOKE_SCREENSAVER_*): any "SMOKE_<NAME> <remote-path>.png" line beyond
  # the primary SMOKE_OK gets pulled too, under its own basename — a fixed
  # name, not timestamped, since these are the specific named artifacts a
  # task's own Verify step reads back (wifi-wrong.png, wifi-connected.png,
  # wifi-eap-connected.png, …), not the generic per-run screenshot.
  local extra_line remote_extra local_extra
  while IFS= read -r extra_line; do
    [ -z "$extra_line" ] && continue
    remote_extra=$(printf '%s\n' "$extra_line" | awk '{print $2}')
    case "$remote_extra" in
      *.png|*.log)
        local_extra="$repo_root/artifacts/$(basename "$remote_extra")"
        scp "${scp_opts[@]}" "test@localhost:$remote_extra" "$local_extra"
        echo "pulled screenshot: $local_extra"
        ;;
      *.tar)
        # A leg's whole frame directory (--use), unpacked beside the rest.
        local_extra="$repo_root/artifacts/$(basename "$remote_extra" .tar)"
        mkdir -p "$local_extra"
        scp "${scp_opts[@]}" "test@localhost:$remote_extra" "$local_extra.tar" \
          && tar -xf "$local_extra.tar" -C "$local_extra" && rm -f "$local_extra.tar"
        echo "pulled frames: $local_extra"
        ;;
    esac
  done <<< "$(printf '%s\n' "$out" | grep -oE '^SMOKE_[A-Z0-9_]+ [^[:space:]]+' | grep -v '^SMOKE_OK ')"

  # --screensaver-gif writes its committed output straight into the synced
  # repo's docs/media/ inside the VM, not artifacts/ — pull those back into
  # the real repo so the command actually reproduces the tracked GIFs
  # instead of requiring a manual scp. The next cmd_sync's `rsync --delete`
  # would otherwise be a no-op here (the pulled files now exist on the mac
  # side too), so this can't regress into deleting what it just pulled.
  case " $* " in
    *" --screensaver-gif "*)
      mkdir -p "$repo_root/docs/media"
      rsync -az -e "ssh ${ssh_opts[*]}" \
        --include 'screensaver-*.gif' --exclude '*' \
        test@localhost:formalshell/docs/media/ "$repo_root/docs/media/"
      echo "pulled screensaver gifs: $repo_root/docs/media/screensaver-*.gif"
      ;;
    *" --cartridge-media "*)
      mkdir -p "$repo_root/docs/media"
      rsync -az -e "ssh ${ssh_opts[*]}" \
        --include 'cartridge-*.gif' --exclude '*' \
        test@localhost:formalshell/docs/media/ "$repo_root/docs/media/"
      echo "pulled cartridge gifs: $repo_root/docs/media/cartridge-*.gif"
      ;;
  esac
}

# Copies a directory out of the VM's checkout (relative to ~/formalshell)
# into a local one.
cmd_pull() {
  mkdir -p "$2"
  scp -r "${scp_opts[@]}" "test@localhost:formalshell/$1/." "$2/"
}

cmd_shell() {
  exec ssh -t "${ssh_opts[@]}" test@localhost \
    "cd formalshell && export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=\$(systemctl --user show-environment | sed -n 's/^WAYLAND_DISPLAY=//p') && exec \$SHELL -l"
}

# A run that skipped dev/vm-lock.sh would share a slot with the lock's holder,
# and dev/scoped-run.sh stops every run scope it finds, so it would kill that
# holder's session. Anything that touches the VM's checkout or sessions goes
# through the lock.
case "${1:-}" in
  sync|run|smoke|pull|prebuild)
    if [ -z "${FS_VM_LOCK_HELD:-}" ]; then
      exec "$(dirname "$0")/vm-lock.sh" "$0" "$@"
    fi
    ;;
esac

case "${1:-}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  sync) cmd_sync ;;
  run) shift; cmd_run "$@" ;;
  smoke) shift; cmd_smoke "$@" ;;
  pull) shift; cmd_pull "$@" ;;
  prebuild) cmd_prebuild ;;
  shell) cmd_shell ;;
  *)
    echo "usage: $0 {start|stop|status|sync|run <cmd...>|smoke [flags...]|pull <vm-dir> <local-dir>|prebuild|shell}" >&2
    exit 1
    ;;
esac
