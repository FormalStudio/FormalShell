#!/usr/bin/env bash
# The rig: builds, checks, cargo and dev/smoke.sh on g815, natively on
# x86_64.
#
# Every run is one of two slot users on g815, fstest0 and fstest1
# (`_rig()` in g815.star of the owner's fspec spec, FormalSnake/dotfiles). A
# slot user is not the owner: its own uid, runtime dir, session bus and
# systemd user manager, no access to the live session's Wayland socket, the
# cards or the input devices. Its manager runs a headless labwc on the Intel
# render node (formalshell-rig-parent.service), so dev/smoke.sh's
# session-mode pick lands on `nested` inside that, with its private session
# and system buses as on any host, and no window ever reaches a real output.
# Legs that declare leg_<n>_vm_only and the VM-backed service checks need a
# VM rig g815 does not have yet; they fail saying so.
#
# The working tree, not a commit, is what runs: `sync` rsyncs it into the
# slot user's ~/formalshell, leaving crates/target alone so cargo stays
# incremental between runs. cargo runs in dev.star's environment
# (`fspec dev`), and dev/build.sh links the art and helpers no distro
# carries out of g815's installed formalshell package.
#
#   dev/remote.sh smoke --lock --depth
#   dev/remote.sh cargo test -p fs-menu
#   dev/remote.sh check fs-tray
set -euo pipefail
cd "$(dirname "$0")/.."
repo_root="$(pwd)"
self="$repo_root/dev/remote.sh"

usage() {
  echo "usage: $0 {sync|run <cmd...>|build|cargo <args...>|lint|check <name>|smoke [flags...]|pull <remote-dir> <local-dir>|shell}" >&2
  exit 1
}
[ $# -gt 0 ] || usage

host="${FS_G815_HOST:-g815}"


# Slot locks live on the mac, which is the one place runs start from. The
# whole command holds its slot, so a sync and the run after it share it.
if [ -z "${FS_G815_SLOT:-}" ]; then
  exec python3 -c '
import fcntl, os, subprocess, sys, time
cmd = sys.argv[1:]
while True:
    for n in range(2):
        lock = open("/tmp/formalshell-g815-%d.lock" % n, "w")
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            lock.close()
            continue
        os.environ["FS_G815_SLOT"] = str(n)
        print("g815: slot %d" % n, file=sys.stderr)
        sys.exit(subprocess.call(cmd))
    time.sleep(3)
' "$self" "$@"
fi
case "$FS_G815_SLOT" in
  0|1) ;;
  *) echo "FS_G815_SLOT must be 0 or 1, got '$FS_G815_SLOT'" >&2; exit 1 ;;
esac
target="fstest$FS_G815_SLOT@$host"
ssh_opts=(-o BatchMode=yes -o ForwardAgent=no -o ConnectTimeout=10 -o LogLevel=ERROR)
if ! ssh "${ssh_opts[@]}" "$target" true 2>/dev/null; then
  echo "g815: $target does not answer ssh" >&2
  exit 1
fi

# The slot user's shell is bash, and pam_systemd has already given it
# XDG_RUNTIME_DIR and its own bus. WAYLAND_DISPLAY is the headless parent's,
# from the file its startup writes: a nested Hyprland imports its own socket
# into the slot's manager environment, so that is not a safe place to read.
# The rig's own directory goes first on PATH: it carries the Hyprland that
# loads a patched aquamarine (`_rig()` in g815.star has the why), which
# dev/smoke.sh finds there. The guard refuses to go on as anyone but a slot
# user.
remote_env='unset HYPRLAND_INSTANCE_SIGNATURE
     export PATH=/usr/local/libexec/formalshell-rig:$PATH
     export WAYLAND_DISPLAY=$(cat "$XDG_RUNTIME_DIR/formalshell-rig-display")'
remote_ssh() {
  ssh "${ssh_opts[@]}" "$target" \
    "case \$(id -un) in fstest[01]) ;; *) echo 'g815: not a slot user' >&2; exit 1 ;; esac
     $remote_env
     cd ~/formalshell && $*"
}

# remote_ssh with the command in a scope of its own (dev/scoped-run.sh), so
# an interrupted run takes its whole tree down with it.
remote_run() {
  local unit="fs-run-$$-$RANDOM" pid status=0
  remote_ssh "exec dev/scoped-run.sh $unit $(printf '%q' "$*")" <&0 &
  pid=$!
  trap 'kill "$pid" 2>/dev/null; ssh "${ssh_opts[@]}" "$target" "systemctl --user stop $unit.scope" >/dev/null 2>&1; exit 143' INT TERM HUP
  wait "$pid" || status=$?
  trap - INT TERM HUP
  return "$status"
}

cmd_sync() {
  ssh "${ssh_opts[@]}" "$target" 'mkdir -p ~/formalshell'
  # .gitignore's patterns keep crates/target, result and artifacts on their
  # own side. macOS's openrsync reads a `:- .gitignore` dir-merge for the
  # sending side only and lets --delete remove the same paths on the
  # receiver, which wiped crates/target before every build; --exclude-from
  # protects them there too.
  rsync -az --delete \
    --exclude-from="$repo_root/.gitignore" \
    --filter=':- .gitignore' \
    --exclude '/.git' \
    --exclude '/.claude/' \
    -e "ssh ${ssh_opts[*]}" \
    "$repo_root/" "$target:formalshell/"
}

# Pulls a remote file or directory listed in the run's output into
# ./artifacts/, under its own basename.
pull_one() {
  local remote=$1 local_path
  mkdir -p "$repo_root/artifacts"
  case "$remote" in
    *.tar)
      local_path="$repo_root/artifacts/$(basename "$remote" .tar)"
      mkdir -p "$local_path"
      ssh "${ssh_opts[@]}" "$target" "cat $(printf '%q' "$remote")" | tar -xf - -C "$local_path" \
        && echo "pulled frames: $local_path"
      ;;
    *)
      local_path="$repo_root/artifacts/$(basename "$remote")"
      rsync -a -e "ssh ${ssh_opts[*]}" "$target:$remote" "$local_path" \
        && echo "pulled: $local_path"
      ;;
  esac
}

cmd_smoke() {
  cmd_sync
  local out status=0 forward="" name
  for name in FS_RESULT SMOKE_WALLPAPER_DITHER FS_CPU_QUOTA FS_CPU_QUOTA_PERIOD FS_DEPTH_PICTURE FS_SMOKE_SCALE; do
    [ -z "${!name:-}" ] || forward+="$name=$(printf '%q' "${!name}") "
  done
  out=$(remote_run "${forward}./dev/smoke.sh $*" 2>&1) || status=$?
  echo "$out"

  local ts line remote
  ts=$(date +%Y%m%d-%H%M%S)
  if [ "$status" -ne 0 ]; then
    for remote in $(printf '%s\n' "$out" | grep -oE '^SMOKE_[A-Z0-9_]+ [^[:space:]]+\.(log|tar)' | awk '{print $2}'); do
      pull_one "$remote"
    done
    echo "g815: smoke run failed (exit $status)" >&2
    exit "$status"
  fi

  # grep -o, not an anchored match: --menu's last line is a JSON cat with no
  # trailing newline, so the SMOKE_OK marker can land appended to it.
  remote=$(printf '%s\n' "$out" | grep -oE 'SMOKE_OK [^[:space:]]+' | tail -1 | awk '{print $2}')
  if [ -z "$remote" ]; then
    echo "g815: no SMOKE_OK line in smoke output" >&2
    exit 1
  fi
  mkdir -p "$repo_root/artifacts"
  rsync -a -e "ssh ${ssh_opts[*]}" "$target:$remote" "$repo_root/artifacts/smoke-$ts.png"
  echo "pulled screenshot: $repo_root/artifacts/smoke-$ts.png"
  local rest
  rest=$(printf '%s\n' "$out" | sed -E 's/SMOKE_OK [^[:space:]]+//' | sed '/^[[:space:]]*$/d')
  if [ -n "$(printf '%s' "$rest" | tr -d '[:space:]')" ]; then
    printf '%s\n' "$rest" > "$repo_root/artifacts/smoke-$ts.json"
    echo "pulled dump/status output: $repo_root/artifacts/smoke-$ts.json"
  fi

  while IFS= read -r line; do
    [ -n "$line" ] || continue
    remote=$(printf '%s\n' "$line" | awk '{print $2}')
    case "$remote" in *.png|*.log|*.tar) pull_one "$remote" ;; esac
  done <<< "$(printf '%s\n' "$out" | grep -oE '^SMOKE_[A-Z0-9_]+ [^[:space:]]+' | grep -v '^SMOKE_OK ')"

  # Legs whose output is committed write it into the checkout's docs/.
  local glob dir
  case " $* " in
    *" --screensaver-gif "*) dir=docs/media glob='screensaver-*.gif' ;;
    *" --cartridge-media "*) dir=docs/media glob='cartridge-*.gif' ;;
    *" --shots "*) dir=docs/shots glob='*' ;;
    *) return 0 ;;
  esac
  mkdir -p "$repo_root/$dir"
  rsync -az -e "ssh ${ssh_opts[*]}" --include "$glob" --exclude '*' \
    "$target:formalshell/$dir/" "$repo_root/$dir/"
  echo "pulled: $repo_root/$dir/$glob"
}

verb=$1
shift
case "$verb" in
  sync) cmd_sync ;;
  run) [ $# -gt 0 ] || usage; cmd_sync; remote_run "$@" ;;
  build) cmd_sync; remote_run dev/build.sh ;;
  cargo) cmd_sync >&2; remote_run "cd crates && fspec dev -- cargo $*" ;;
  lint) cmd_sync; remote_run dev/lint.sh ;;
  check) [ $# -eq 1 ] || usage; cmd_sync; remote_run dev/check.sh "$1" ;;
  smoke) cmd_smoke "$@" ;;
  pull) [ $# -eq 2 ] || usage; mkdir -p "$2"; rsync -a -e "ssh ${ssh_opts[*]}" "$target:formalshell/$1/" "$2/" ;;
  shell) exec ssh -t "${ssh_opts[@]}" "$target" "cd ~/formalshell 2>/dev/null; $remote_env; exec bash" ;;
  *) usage ;;
esac
