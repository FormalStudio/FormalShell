#!/usr/bin/env bash
# The rig: builds, checks, cargo and dev/smoke.sh on g815, natively on
# x86_64, or on the mac rig while g815 is off.
#
# FS_RIG picks it: g815, or mac, the macbook's own Tumbleweed aarch64 guest
# (dev/mac-vm.sh, hosts/fstestmac of the spec). Unset, a g815 that does not
# answer ssh in five seconds sends the command to mac, booting the guest
# when it is stopped. The mac rig has four slot users of the same shape;
# each Hyprland there runs on a vkms card of its own rather than nesting,
# since the guest has no GPU. Budgets (the legs declaring leg_<n>_budget, and
# dev/perf-gate.py) hold on g815 alone and are refused there. Release
# tarballs and the hosts' packages still build on g815 only.
#
# Every run is one of the slot users on g815, fstest0 to fstest7
# (hosts/g815/rig.star of the owner's fspec spec, FormalSnake/dotfiles). A
# slot user is not the owner: its own uid, runtime dir, session bus and
# systemd user manager, no access to the live session's Wayland socket, the
# cards or the input devices. Its manager runs a headless labwc on the Intel
# render node (formalshell-rig-parent.service), so dev/smoke.sh's
# session-mode pick lands on `nested` inside that, with its private session
# and system buses as on any host, and no window ever reaches a real output.
# Legs that declare leg_<n>_vm_only and the VM-backed service checks go to
# the VM rig instead (dev/vm.sh), smoke-all's and smoke-affected's included.
#
# The working tree, not a commit, is what runs: `sync` rsyncs it into the
# slot user's ~/formalshell, whichever checkout it comes from, leaving
# crates/target alone so cargo stays incremental between runs. One copy and
# one target per slot keeps the rig's disk to eight targets; a copy per
# checkout per slot filled g815's 256G. A command takes the free slot this
# checkout last ran on before any other, so only what differs from the
# checkout before it rebuilds. cargo runs in dev.star's environment
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

ssh_opts=(-o BatchMode=yes -o ForwardAgent=no -o ConnectTimeout=10 -o LogLevel=ERROR)

# Resolved once, by the command an agent ran; the build and runs it starts
# inherit it.
if [ -z "${FS_RIG:-}" ]; then
  if ssh -o ConnectTimeout=5 "${ssh_opts[@]}" "fstest0@g815" true 2>/dev/null; then
    FS_RIG=g815
  else
    echo "g815 does not answer ssh; using the mac rig" >&2
    FS_RIG=mac
  fi
fi
export FS_RIG
case "$FS_RIG" in
  g815)
    host="g815"
    # The count of slot users hosts/g815/rig.star declares.
    slots="${FS_G815_SLOTS:-8}"
    ;;
  mac)
    host=fstestmac
    # hosts/fstestmac/host.star's SLOTS.
    slots=4
    ;;
  *) echo "FS_RIG must be g815 or mac, got '$FS_RIG'" >&2; exit 1 ;;
esac
rig=$FS_RIG
if [ "$rig" = g815 ]; then
  . "$repo_root/dev/perf-hold.sh"
  perf_gate_wait remote.sh
fi
if [ "$rig" = mac ] && [ -z "${FS_RIG_SLOT:-}" ]; then
  "$repo_root/dev/mac-vm.sh" start
fi

remote_dir=formalshell

# Every run's own time on its slot, kept across runs so a list starts with
# its longest legs and the last one to finish is not the slowest started last.
# The mac rig's runs keep their own, being slower.
times="${XDG_STATE_HOME:-$HOME/.local/state}/formalshell-rig/leg-times.tsv"
[ "$rig" = g815 ] || times="${times%.tsv}-$rig.tsv"

# Every leg's declarations (dev/smoke.sh --manifest), read once and handed to
# dev/vm.sh and the runs through FS_MANIFEST.
load_manifest() {
  [ -n "${FS_MANIFEST:-}" ] || FS_MANIFEST=$("$repo_root/dev/smoke.sh" --manifest | jq -c .)
  export FS_MANIFEST
}

# Runs a list of runs over the free slots off one build. The list goes
# longest recorded first; a run with no record goes first of all.
run_list() {
  local label=$1 list=$2 out shared builder started n=0 running=0 failed flags slug
  jobs="${FS_SMOKE_JOBS:-$slots}"
  out="$repo_root/artifacts/$label-$(date +%Y%m%d-%H%M%S)"
  shared="/tmp/formalshell-rig-result-$(date +%s)-$$"
  mkdir -p "$out" "$(dirname "$times")"
  touch "$times"
  load_manifest
  printf '%s\n' "$FS_MANIFEST" > "$out/manifest.json"
  FS_SMOKE_SCALE="${FS_SMOKE_SCALE:-}" python3 -c '
import collections, json, os, statistics, sys
runs = [l.strip() for l in open(sys.argv[1]) if l.strip() and not l.lstrip().startswith("#")]
# Every line at 1.25 (the g815 panel) and again at 1 (its HDMI output, e1504g),
# but a line carrying a leg that declares leg_<n>_scale1 at 1 alone, and
# a run that asks for a scale (FS_SMOKE_SCALE) at that one.
legs = json.load(open(sys.argv[4]))["legs"]
scale1 = {l["flag"].split()[0] for l in legs if l["scale1"]}
asked = os.environ["FS_SMOKE_SCALE"]
# A leg declaring leg_<n>_budget measures numbers that hold on g815 alone,
# and the VM rig the vm_only legs go to is on g815 too.
if sys.argv[3] != "g815":
    g815 = {l["flag"].split()[0] for l in legs if l["budget"] or l["vm_only"]}
    held = [r for r in runs if g815 & set(r.split())]
    if held:
        print("needs g815 (a budget, or the VM rig), not run on %s: %s" % (sys.argv[3], ", ".join(held)), file=sys.stderr)
    runs = [r for r in runs if r not in held]
scaled = []
for r in dict.fromkeys(runs):
    one = scale1 & set(r.split())
    for scale in ["1"] if one else [asked] if asked else ["1.25", "1"]:
        scaled.append((scale, r))
# A run at 1 keeps the key its times were recorded under before scales.
key = lambda scale, r: r if scale == "1" else "%s @%s" % (r, scale)
seen = collections.defaultdict(list)
for line in open(sys.argv[2]):
    parts = line.rstrip("\n").split("\t")
    if len(parts) == 3:
        seen[parts[2]].append(int(parts[1]))
est = {k: statistics.median(seen[k][-3:]) if seen[k] else float("inf") for k in (key(*s) for s in scaled)}
for scale, r in sorted(scaled, key=lambda s: -est[key(*s)]):
    e = est[key(scale, r)]
    print("%s\t%s\t%s" % ("?" if e == float("inf") else int(e), scale, r))
' "$list" "$times" "$rig" "$out/manifest.json" > "$out/order"
  started=$SECONDS
  echo "$label: building once on $rig, then $(wc -l < "$out/order" | tr -d ' ') runs over $jobs slots into $out"
  if ! env -u FS_RIG_SLOT "$self" run "dev/build.sh && dev/share-result.sh $shared" > "$out/build.log" 2>&1; then
    tail -20 "$out/build.log" >&2
    echo "$label: build failed, $out/build.log" >&2
    return 1
  fi
  builder=$(sed -n 's/^[a-z0-9]*: slot \([0-9]*\)$/\1/p' "$out/build.log" | head -1)
  echo "$label: built in $((SECONDS - started))s on slot $builder"
  : > "$out/results"
  while IFS=$'\t' read -r _ scale flags; do
    n=$((n + 1))
    slug=$(printf '%03d-%s-x%s' "$n" "$(printf '%s' "$flags" | tr -cs 'a-z0-9' '-' | sed 's/^-*//;s/-*$//')" "${scale//./}")
    run="$flags"
    [ "$scale" = 1 ] || run="$flags @$scale"
    (
      t0=$SECONDS
      status=0
      # shellcheck disable=SC2086 # one line is one run's flags
      env -u FS_RIG_SLOT FS_RESULT="$shared" FS_ARTIFACTS="$out/$slug" FS_SMOKE_SCALE="$scale" "$self" smoke $flags > "$out/$slug.log" 2>&1 </dev/null || status=$?
      slot=$(sed -n 's/^[a-z0-9]*: slot \([a-z]*[0-9]*\)$/\1/p' "$out/$slug.log" | head -1)
      waited=$(sed -n 's/^[a-z0-9]*: waited \([0-9]*\)s for a slot$/\1/p' "$out/$slug.log" | head -1)
      took=$((SECONDS - t0 - ${waited:-0}))
      verdict=PASS; [ "$status" -eq 0 ] || verdict=FAIL
      printf '%s\t%s\t%s\n' "$(date +%s)" "$took" "$run" >> "$times"
      printf '%s %4ss slot%s %s\n' "$verdict" "$took" "$slot" "$run" | tee -a "$out/results"
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

# The VM rig's runs, on g815's fstestvm guests: dev/vm.sh takes a guest slot
# of its own.
case "${1:-}" in
  smoke) load_manifest; if [ "$rig" = g815 ] && "$repo_root/dev/vm.sh" wants "${@:2}"; then exec "$repo_root/dev/vm.sh" "$@"; fi ;;
  check) case "$rig:${2:-}" in g815:fs-auth|g815:fs-audio|g815:fs-bluez|g815:fs-network|g815:fs-upower) exec "$repo_root/dev/vm.sh" "$@" ;; esac ;;
esac

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
    # The whole suite is for milestones. One at a time per rig across every
    # agent on this mac: a second queues here rather than halving both.
    if [ -z "${FS_SMOKE_ALL_HELD:-}" ]; then
      lock=/tmp/formalshell-smoke-all.lock
      [ "$rig" = g815 ] || lock=/tmp/formalshell-smoke-all-$rig.lock
      exec python3 -c '
import fcntl, os, subprocess, sys
lock = open(sys.argv[1], "a+")
try:
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
except OSError:
    lock.seek(0)
    print("smoke-all: queued behind the run started by %s" % (lock.read().strip() or "another agent"), file=sys.stderr)
    fcntl.flock(lock, fcntl.LOCK_EX)
lock.seek(0); lock.truncate(); lock.write("%s (pid %d)\n" % (sys.argv[2], os.getpid())); lock.flush()
os.environ["FS_SMOKE_ALL_HELD"] = "1"
sys.exit(subprocess.call(sys.argv[3:]))
' "$lock" "$repo_root" "$self" "$@"
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
if [ -z "${FS_RIG_SLOT:-}" ]; then
  exec python3 -c '
import fcntl, os, subprocess, sys, time
rig, slots, me, cmd = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4:]
def last(n):
    try:
        return open("/tmp/formalshell-%s-%d.last" % (rig, n)).read().strip()
    except OSError:
        return ""
began = time.time()
while True:
    order = sorted(range(slots), key=lambda n: (last(n) != me, last(n) != "", n))
    for n in order:
        lock = open("/tmp/formalshell-%s-%d.lock" % (rig, n), "w")
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            lock.close()
            continue
        with open("/tmp/formalshell-%s-%d.last" % (rig, n), "w") as f:
            f.write(me + "\n")
        os.environ["FS_RIG_SLOT"] = str(n)
        waited = int(time.time() - began)
        if waited:
            print("%s: waited %ds for a slot" % (rig, waited), file=sys.stderr)
        print("%s: slot %d" % (rig, n), file=sys.stderr)
        sys.exit(subprocess.call(cmd))
    time.sleep(3)
' "$rig" "$slots" "$repo_root" "$self" "$@"
fi
case "$FS_RIG_SLOT" in
  ''|*[!0-9]*) echo "FS_RIG_SLOT must be a slot number, got '$FS_RIG_SLOT'" >&2; exit 1 ;;
esac
[ "$FS_RIG_SLOT" -lt "$slots" ] || { echo "FS_RIG_SLOT $FS_RIG_SLOT is past $rig's $slots slots" >&2; exit 1; }
target="fstest$FS_RIG_SLOT@$host"
if ! ssh "${ssh_opts[@]}" "$target" true 2>/dev/null; then
  echo "$rig: $target does not answer ssh" >&2
  exit 1
fi

# The slot user's shell is bash, and pam_systemd has already given it
# XDG_RUNTIME_DIR and its own bus. WAYLAND_DISPLAY is the headless parent's,
# from the file its startup writes: a nested Hyprland imports its own socket
# into the slot's manager environment, so that is not a safe place to read.
# The rig's own directory goes first on PATH: it carries the Hyprland that
# loads a patched aquamarine (`_rig()` in g815.star has the why), which
# dev/smoke.sh finds there. The mac rig has neither: its sessions take a
# vkms card, which dev/smoke.sh picks off the slot user's name. The guard
# refuses to go on as anyone but a slot user.
case "$rig" in
  g815)
    remote_env='unset HYPRLAND_INSTANCE_SIGNATURE
     export PATH=/usr/local/libexec/formalshell-rig:$PATH
     export WAYLAND_DISPLAY=$(cat "$XDG_RUNTIME_DIR/formalshell-rig-display")'
    ;;
  mac) remote_env='unset HYPRLAND_INSTANCE_SIGNATURE WAYLAND_DISPLAY' ;;
esac
remote_ssh() {
  ssh "${ssh_opts[@]}" "$target" \
    "case \$(id -un) in fstest[0-9]*) ;; *) echo '$rig: not a slot user' >&2; exit 1 ;; esac
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
  # The copy is shared by every checkout that lands on this slot, which is
  # safe because the sync below rewrites every file whose content differs, at
  # the sync's own time: whatever the target was built from, a changed source
  # is newer than it. The slot lock holds the whole command, so no other
  # checkout's build runs between this sync and the command after it.
  # A target past 8G (dependency builds piling up across Cargo.lock and
  # feature changes) goes and is rebuilt cold, checked at most hourly. A
  # failed size check fails the sync.
  ssh "${ssh_opts[@]}" "$target" "set -e
    mkdir -p ~/$remote_dir && cd ~/$remote_dir
    if [ -z \"\$(find .rig-target-checked -mmin -60 2>/dev/null)\" ]; then
      touch .rig-target-checked
      if [ -d crates/target ]; then
        size=\$(du -sm crates/target | cut -f1)
        [ -n \"\$size\" ] || { echo 'sync: du could not size crates/target' >&2; exit 1; }
        [ \"\$size\" -le 8192 ] || rm -rf crates/target
      fi
    fi"
  # .gitignore's patterns keep crates/target, result and artifacts on their
  # own side. macOS's openrsync reads a `:- .gitignore` dir-merge for the
  # sending side only and lets --delete remove the same paths on the
  # receiver, which wiped crates/target before every build; --exclude-from
  # protects them there too.
  # Files are compared by checksum and written with the time of the sync,
  # never the sender's mtime: a slot's target can be newer than a source
  # carried over from another checkout, and cargo would skip rebuilding it.
  rsync -rlpcz --delete \
    --exclude-from="$repo_root/.gitignore" \
    --filter=':- .gitignore' \
    --exclude '/.git' \
    --exclude '/.claude/' \
    --exclude '/.rig-target-checked' \
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
  for name in FS_RESULT FS_CPU_QUOTA FS_CPU_QUOTA_PERIOD FS_SMOKE_SCALE FS_CARGO_PROFILE $(printf '%s' "$FS_MANIFEST" | jq -r '[.legs[].env[]] | unique[]'); do
    [ -z "${!name:-}" ] || forward+="$name=$(printf '%q' "${!name}") "
  done
  out=$(remote_run "${forward}./dev/smoke.sh $*" 2>&1) || status=$?
  echo "$out"

  local ts line remote
  ts=$(date +%Y%m%d-%H%M%S)
  if [ "$status" -ne 0 ]; then
    for remote in $(printf '%s\n' "$out" | grep -oE '^SMOKE_[A-Za-z0-9_]+ [^[:space:]]+\.(png|log|tar)' | awk '{print $2}'); do
      pull_one "$remote"
    done
    echo "$rig: smoke run failed (exit $status)" >&2
    exit "$status"
  fi

  remote=$(printf '%s\n' "$out" | grep -E '^SMOKE_OK [^[:space:]]+' | tail -1 | awk '{print $2}')
  if [ -z "$remote" ]; then
    echo "$rig: no SMOKE_OK line in smoke output" >&2
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
  done <<< "$(printf '%s\n' "$out" | grep -oE '^SMOKE_[A-Za-z0-9_]+ [^[:space:]]+' | grep -v '^SMOKE_OK ')"

  # Legs whose output is committed write it into the checkout's docs/.
  local dir glob flag
  for flag in "$@"; do
    case "$flag" in --*) ;; *) continue ;; esac
    while IFS=$'\t' read -r dir glob; do
      [ -n "$dir" ] || continue
      mkdir -p "$repo_root/$dir"
      rsync -az -e "ssh ${ssh_opts[*]}" --include "$glob" --exclude '*' \
        "$target:$remote_dir/$dir/" "$repo_root/$dir/"
      echo "pulled: $repo_root/$dir/$glob"
    done < <(printf '%s' "$FS_MANIFEST" | jq -r --arg flag "$flag" '.legs[] | select(.flag | split(" ")[0] == $flag) | .commits[] | [.dir, .glob] | @tsv')
  done
}

verb=$1
shift
case "$verb" in
  sync) cmd_sync ;;
  run) [ $# -gt 0 ] || usage; cmd_sync; remote_run "$@" ;;
  build) cmd_sync; remote_run dev/build.sh ;;
  # A test that shapes text (text.rs) reads the package's fonts the way the
  # launcher's FS_RS_FONT_DIRS hands them to the shell.
  cargo) cmd_sync >&2; remote_run 'export FS_RS_FONT_DIRS="${FS_RS_FONT_DIRS-$(dirname "$(dirname "$(readlink -f "$(command -v formalshell)")")")/share/formalshell/fonts}";'" cd crates && fspec dev -- cargo $*" ;;
  lint) cmd_sync; remote_run dev/lint.sh ;;
  check) [ $# -eq 1 ] || usage; cmd_sync; remote_run dev/check.sh "$1" ;;
  smoke) cmd_smoke "$@" ;;
  pull) [ $# -eq 2 ] || usage; mkdir -p "$2"; rsync -a -e "ssh ${ssh_opts[*]}" "$target:$remote_dir/$1/" "$2/" ;;
  shell) exec ssh -t "${ssh_opts[@]}" "$target" "cd ~/$remote_dir 2>/dev/null; $remote_env; exec bash" ;;
  *) usage ;;
esac
