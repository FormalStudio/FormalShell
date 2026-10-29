#!/usr/bin/env bash
# The rig: builds, checks, cargo and dev/smoke.sh on g815, natively on
# x86_64.
#
# Every run is one of the slot users on g815, fstest0 to fstest7
# (hosts/g815/rig.star of the owner's fspec spec, FormalSnake/dotfiles). A
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
# slot user's ~/formalshell (a worktree into ~/formalshell-wt/<name>, so two
# checkouts sharing a slot do not rebuild over each other), leaving
# crates/target alone so cargo stays incremental between runs. A command
# takes the free slot this checkout last ran on before any other, for a warm
# target. cargo runs in dev.star's environment
# (`fspec dev`), and dev/build.sh links the art and helpers no distro
# carries out of g815's installed formalshell package.
#
#   dev/remote.sh smoke --lock --depth
#   dev/remote.sh cargo test -p fs-menu
#   dev/remote.sh check fs-tray
#   dev/remote.sh smoke-affected [base] the runs dev/smoke-map.toml maps the
#                                      change against base to, the default
#   dev/remote.sh smoke-all [list]     every run in dev/smoke-all.list, for
#                                      milestones; one at a time per mac
# Both build once and spread their runs over the free slots.
set -euo pipefail
cd "$(dirname "$0")/.."
repo_root="$(pwd)"
self="$repo_root/dev/remote.sh"

usage() {
  echo "usage: $0 {sync|run <cmd...>|build|cargo <args...>|lint|check <name>|smoke [flags...]|smoke-affected [base]|smoke-all [list]|pull <remote-dir> <local-dir>|shell}" >&2
  exit 1
}
[ $# -gt 0 ] || usage

host="${FS_G815_HOST:-g815}"
# The count of slot users hosts/g815/rig.star declares.
slots="${FS_G815_SLOTS:-8}"

case "$repo_root" in
  */FormalShell) remote_dir=formalshell ;;
  *) remote_dir="formalshell-wt/$(basename "$repo_root")" ;;
esac

# Every run's own time on its slot, kept across runs so a list starts with
# its longest legs and the last one to finish is not the slowest started last.
times="${XDG_STATE_HOME:-$HOME/.local/state}/formalshell-rig/leg-times.tsv"

# Runs a list of runs over the free slots off one build. The list goes
# longest recorded first; a run with no record goes first of all.
run_list() {
  local label=$1 list=$2 out shared builder started n=0 running=0 failed flags slug
  jobs="${FS_SMOKE_JOBS:-$slots}"
  out="$repo_root/artifacts/$label-$(date +%Y%m%d-%H%M%S)"
  shared="/tmp/formalshell-rig-result-$(date +%s)-$$"
  mkdir -p "$out" "$(dirname "$times")"
  touch "$times"
  python3 -c '
import collections, statistics, sys
runs = [l.strip() for l in open(sys.argv[1]) if l.strip() and not l.lstrip().startswith("#")]
seen = collections.defaultdict(list)
for line in open(sys.argv[2]):
    parts = line.rstrip("\n").split("\t")
    if len(parts) == 3:
        seen[parts[2]].append(int(parts[1]))
est = {r: statistics.median(seen[r][-3:]) if seen[r] else float("inf") for r in runs}
for r in sorted(dict.fromkeys(runs), key=lambda r: -est[r]):
    print("%s\t%s" % ("?" if est[r] == float("inf") else int(est[r]), r))
' "$list" "$times" > "$out/order"
  started=$SECONDS
  echo "$label: building once, then $(wc -l < "$out/order" | tr -d ' ') runs over $jobs slots into $out"
  if ! env -u FS_G815_SLOT "$self" run "dev/build.sh && dev/share-result.sh $shared" > "$out/build.log" 2>&1; then
    tail -20 "$out/build.log" >&2
    echo "$label: build failed, $out/build.log" >&2
    return 1
  fi
  builder=$(sed -n 's/^g815: slot \([0-9]*\)$/\1/p' "$out/build.log" | head -1)
  echo "$label: built in $((SECONDS - started))s on slot $builder"
  : > "$out/results"
  while IFS=$'\t' read -r _ flags; do
    n=$((n + 1))
    slug=$(printf '%03d-%s' "$n" "$(printf '%s' "$flags" | tr -cs 'a-z0-9' '-' | sed 's/^-*//;s/-*$//')")
    (
      t0=$SECONDS
      status=0
      # shellcheck disable=SC2086 # one line is one run's flags
      env -u FS_G815_SLOT FS_RESULT="$shared" FS_ARTIFACTS="$out/$slug" "$self" smoke $flags > "$out/$slug.log" 2>&1 </dev/null || status=$?
      slot=$(sed -n 's/^g815: slot \([0-9]*\)$/\1/p' "$out/$slug.log" | head -1)
      waited=$(sed -n 's/^g815: waited \([0-9]*\)s for a slot$/\1/p' "$out/$slug.log" | head -1)
      took=$((SECONDS - t0 - ${waited:-0}))
      verdict=PASS; [ "$status" -eq 0 ] || verdict=FAIL
      printf '%s\t%s\t%s\n' "$(date +%s)" "$took" "$flags" >> "$times"
      printf '%s %4ss slot%s %s\n' "$verdict" "$took" "$slot" "$flags" | tee -a "$out/results"
    ) &
    running=$((running + 1))
    if [ "$running" -ge "$jobs" ]; then wait -n; running=$((running - 1)); fi
  done < "$out/order"
  wait
  ssh -o BatchMode=yes -o LogLevel=ERROR "fstest$builder@$host" "rm -rf $shared" || true
  failed=$((n - $(grep -c '^PASS' "$out/results" || true)))
  echo "$label: $((n - failed)) of $n passed in $((SECONDS - started))s; logs and artifacts in $out"
  grep '^FAIL' "$out/results" | sed "s|^|  |" || true
  [ "$failed" -eq 0 ]
}

# Neither takes a slot itself: the build and every run take their own.
case "${1:-}" in
  smoke-affected)
    shift
    picked=$(mktemp)
    python3 "$repo_root/dev/smoke-affected.py" "$@" --out "$picked"
    if [ ! -s "$picked" ]; then rm -f "$picked"; exit 0; fi
    status=0
    run_list smoke-affected "$picked" || status=$?
    rm -f "$picked"
    exit "$status"
    ;;
  smoke-all)
    # The whole suite is for milestones. One at a time across every agent
    # on this mac: a second queues here rather than halving both.
    if [ -z "${FS_SMOKE_ALL_HELD:-}" ]; then
      exec python3 -c '
import fcntl, os, subprocess, sys
lock = open("/tmp/formalshell-smoke-all.lock", "a+")
try:
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
except OSError:
    lock.seek(0)
    print("smoke-all: queued behind the run started by %s" % (lock.read().strip() or "another agent"), file=sys.stderr)
    fcntl.flock(lock, fcntl.LOCK_EX)
lock.seek(0); lock.truncate(); lock.write("%s (pid %d)\n" % (sys.argv[1], os.getpid())); lock.flush()
os.environ["FS_SMOKE_ALL_HELD"] = "1"
sys.exit(subprocess.call(sys.argv[2:]))
' "$repo_root" "$self" "$@"
    fi
    shift
    list="${1:-$repo_root/dev/smoke-all.list}"
    [ -r "$list" ] || { echo "smoke-all: no list at $list" >&2; exit 1; }
    run_list smoke-all "$list"
    exit
    ;;
esac

# Slot locks live on the mac, which is the one place runs start from. The
# whole command holds its slot, so a sync and the run after it share it.
# A slot's .last names the checkout that ran there last: the picker tries
# the free slots this checkout left warm, then unused ones, then any.
if [ -z "${FS_G815_SLOT:-}" ]; then
  exec python3 -c '
import fcntl, os, subprocess, sys, time
slots, me, cmd = int(sys.argv[1]), sys.argv[2], sys.argv[3:]
def last(n):
    try:
        return open("/tmp/formalshell-g815-%d.last" % n).read().strip()
    except OSError:
        return ""
began = time.time()
while True:
    order = sorted(range(slots), key=lambda n: (last(n) != me, last(n) != "", n))
    for n in order:
        lock = open("/tmp/formalshell-g815-%d.lock" % n, "w")
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            lock.close()
            continue
        with open("/tmp/formalshell-g815-%d.last" % n, "w") as f:
            f.write(me + "\n")
        os.environ["FS_G815_SLOT"] = str(n)
        waited = int(time.time() - began)
        if waited:
            print("g815: waited %ds for a slot" % waited, file=sys.stderr)
        print("g815: slot %d" % n, file=sys.stderr)
        sys.exit(subprocess.call(cmd))
    time.sleep(3)
' "$slots" "$repo_root" "$self" "$@"
fi
case "$FS_G815_SLOT" in
  ''|*[!0-9]*) echo "FS_G815_SLOT must be a slot number, got '$FS_G815_SLOT'" >&2; exit 1 ;;
esac
[ "$FS_G815_SLOT" -lt "$slots" ] || { echo "FS_G815_SLOT $FS_G815_SLOT is past the $slots slots" >&2; exit 1; }
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
    "case \$(id -un) in fstest[0-9]*) ;; *) echo 'g815: not a slot user' >&2; exit 1 ;; esac
     $remote_env
     cd ~/$remote_dir && $*"
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
  # .rig-used marks when a checkout last ran here; a worktree's copy and its
  # target go once it has sat unused for three days.
  ssh "${ssh_opts[@]}" "$target" "mkdir -p ~/$remote_dir && touch ~/$remote_dir/.rig-used
    for d in ~/formalshell-wt/*/; do
      [ -n \"\$(find \"\$d.rig-used\" -mtime +3 2>/dev/null)\" ] && rm -rf \"\$d\"
    done; true"
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
    --exclude '/.rig-used' \
    -e "ssh ${ssh_opts[*]}" \
    "$repo_root/" "$target:$remote_dir/"
}

# Pulls a remote file or directory listed in the run's output into
# ./artifacts/ (FS_ARTIFACTS when set, smoke-all's per-run directory), under
# its own basename.
artifacts="${FS_ARTIFACTS:-$repo_root/artifacts}"
pull_one() {
  local remote=$1 local_path
  mkdir -p "$artifacts"
  case "$remote" in
    *.tar)
      local_path="$artifacts/$(basename "$remote" .tar)"
      mkdir -p "$local_path"
      ssh "${ssh_opts[@]}" "$target" "cat $(printf '%q' "$remote")" | tar -xf - -C "$local_path" \
        && echo "pulled frames: $local_path"
      ;;
    *)
      local_path="$artifacts/$(basename "$remote")"
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
  mkdir -p "$artifacts"
  rsync -a -e "ssh ${ssh_opts[*]}" "$target:$remote" "$artifacts/smoke-$ts.png"
  echo "pulled screenshot: $artifacts/smoke-$ts.png"
  local rest
  rest=$(printf '%s\n' "$out" | sed -E 's/SMOKE_OK [^[:space:]]+//' | sed '/^[[:space:]]*$/d')
  if [ -n "$(printf '%s' "$rest" | tr -d '[:space:]')" ]; then
    printf '%s\n' "$rest" > "$artifacts/smoke-$ts.json"
    echo "pulled dump/status output: $artifacts/smoke-$ts.json"
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
    "$target:$remote_dir/$dir/" "$repo_root/$dir/"
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
  pull) [ $# -eq 2 ] || usage; mkdir -p "$2"; rsync -a -e "ssh ${ssh_opts[*]}" "$target:$remote_dir/$1/" "$2/" ;;
  shell) exec ssh -t "${ssh_opts[@]}" "$target" "cd ~/$remote_dir 2>/dev/null; $remote_env; exec bash" ;;
  *) usage ;;
esac
