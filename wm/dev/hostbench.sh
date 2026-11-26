#!/bin/bash
# What a frame costs on this machine's own GPU: formalwm nested in the
# running session on a copy of the owner's settings, the installed shell
# inside it on a bus of its own, driven through a fixed set of moves. For
# each move: the GPU time the nested compositor used (i915's per-client
# busy time in /proc/<pid>/fdinfo, which needs sudo), the CPU time of the
# compositor and the shell, and the compositor's own frame times.
#   hostbench.sh [name]     results under ~/.cache/fwm-bench-runs/<name>
# FWM_BIN and FS_BIN name another compositor and shell; BENCH_WM_SETTINGS a
# jq filter laid over the copied formalwm settings, BENCH_SHELL_SETTINGS one
# over the shell's, BENCH_PHASES the moves to run, BENCH_PERF a perf
# record of both.
set -u
name=${1:-run}
wm_bin=${FWM_BIN:-/opt/fspec/current/bin/formalwm}
fs_bin=${FS_BIN:-/opt/fspec/current/bin/formalshell}
host_rt=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
host_display=$(ls "$host_rt" | grep -E '^wayland-[0-9]+$' | head -1)
[ -n "$host_display" ] || { echo "hostbench: no Wayland session to nest in" >&2; exit 1; }
out=$HOME/.cache/fwm-bench-runs/$name
rm -rf "$out"; mkdir -p "$out/log" "$out/config/formalwm" "$out/config/formalshell" "$out/state" "$out/cache"
rt=$host_rt/fwm-bench-$$
mkdir -m 700 "$rt"

strip() { python3 -c '
import re, sys
text = sys.stdin.read()
text = re.sub(r"^\s*//.*$", "", text, flags=re.M)
text = re.sub(r",(\s*[}\]])", r"\1", text)
print(text)'; }
strip < "$HOME/.config/formalwm/settings.json" | jq '.terminal = "foot" | .rules = [] | '"${BENCH_WM_SETTINGS:-.}" > "$out/config/formalwm/settings.json"
[ -f "$HOME/.config/formalwm/host.json" ] && strip < "$HOME/.config/formalwm/host.json" > "$out/config/formalwm/host.json"
jq 'del(.airplay, .localsend, .caffeinate, .greeter) | '"${BENCH_SHELL_SETTINGS:-.}" "$HOME/.config/formalshell/settings.json" > "$out/config/formalshell/settings.json"
cp -r "$HOME/.local/state/formalshell" "$out/state/" 2>/dev/null
cat > "$out/config/formalwm/formalshell-start.sh" <<START
exec "$fs_bin" > "$out/shell.log" 2>&1
START

unit=fwm-bench-$$
cleanup() {
  XDG_RUNTIME_DIR=$host_rt systemctl --user stop "$unit.scope" >/dev/null 2>&1
  rm -rf "$rt"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

XDG_RUNTIME_DIR=$host_rt systemd-run --user --scope --quiet --collect --unit="$unit" -p TimeoutStopSec=5 -p RuntimeMaxSec=600 -- \
  env -u HYPRLAND_INSTANCE_SIGNATURE -u SHOJIWM_SOCKET -u DISPLAY -u NOTIFY_SOCKET \
    XDG_RUNTIME_DIR="$rt" WAYLAND_DISPLAY="$host_rt/$host_display" \
    XDG_CONFIG_HOME="$out/config" XDG_STATE_HOME="$out/state" XDG_CACHE_HOME="$out/cache" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=$rt/bus" DBUS_SYSTEM_BUS_ADDRESS="unix:path=$rt/no-system-bus" \
    SHOJI_LOG_DIR="$out/log" SHOJI_LOG_ROTATE=off SHOJI_LOG_RATE_LIMIT=0 \
    SHOJI_ANIMATION_TIMING_DEBUG=1 SHOJI_ANIMATION_SPIKE_THRESHOLD_MS=0.001 SHOJI_PRESENT_RATE_DEBUG=1 \
    ${BENCH_ENV:-} \
  bash -c 'dbus-daemon --session --address="$DBUS_SESSION_BUS_ADDRESS" --nofork --nopidfile >/dev/null 2>&1 & exec "$@"' -- "$wm_bin" \
  > "$out/stdout.log" 2>&1 &

export XDG_RUNTIME_DIR=$rt XDG_CONFIG_HOME=$out/config XDG_STATE_HOME=$out/state XDG_CACHE_HOME=$out/cache
export DBUS_SESSION_BUS_ADDRESS=unix:path=$rt/bus DBUS_SYSTEM_BUS_ADDRESS=unix:path=$rt/no-system-bus
unset HYPRLAND_INSTANCE_SIGNATURE DISPLAY
display=""
for _ in $(seq 100); do
  display=$(ls "$rt" 2>/dev/null | grep -E '^wayland-[0-9]+$' | head -1)
  [ -n "$display" ] && [ -S "$rt/shojiwm-$display.sock" ] && break
  sleep 0.1
done
[ -n "$display" ] || { echo "hostbench: the nested compositor did not come up" >&2; tail -20 "$out/stdout.log" >&2; exit 1; }
export WAYLAND_DISPLAY=$display SHOJIWM_SOCKET=$rt/shojiwm.$display.sock
wm=$(pgrep -n -f "^$wm_bin\$")
log=$out/log/latest.log
sleep 6
sh=$(pgrep -n -x formalshell-rs)

gpu() { sudo cat /proc/"$1"/fdinfo/* 2>/dev/null | awk '/drm-client-id/{id=$2} /drm-engine-render/{if(!(id in s))s[id]=$2} END{t=0; for(i in s)t+=s[i]; print t+0}'; }
cpu() { awk '{print $14+$15}' "/proc/$1/stat" 2>/dev/null || echo 0; }
# Keys from wtype's virtual keyboard: the nested backend takes none over the
# control socket.
key() {
  local part key= mods=() args=() i
  IFS=+ read -ra parts <<< "$1"
  for part in "${parts[@]}"; do
    case "$part" in
      Super) mods+=(logo) ;;
      Shift) mods+=(shift) ;;
      code:*) key=$(((${part#code:} - 9) % 10)) ;;
      *) key=$part ;;
    esac
  done
  for part in "${mods[@]}"; do args+=(-M "$part"); done
  args+=(-k "$key")
  for ((i = ${#mods[@]} - 1; i >= 0; i--)); do args+=(-m "${mods[i]}"); done
  wtype "${args[@]}"; sleep "${2:-0.6}"
}
fs() { "$fs_bin" ipc call "$@" >/dev/null 2>&1; }
# One move: its name, then the commands that make it.
phase() {
  local label=$1; shift
  local g0 c0 s0 l0 t0 s1l
  g0=$(gpu "$wm"); c0=$(cpu "$wm"); s0=$(cpu "$sh"); l0=$(wc -l < "$log"); s1l=$(wc -l < "$out/shell.log"); t0=$(date +%s%N)
  "$@"
  local mhz; mhz=$(cat /sys/class/drm/card*/gt_cur_freq_mhz 2>/dev/null | head -1)
  local ms=$(( ($(date +%s%N) - t0) / 1000000 ))
  local g=$(( $(gpu "$wm") - g0 )) c=$(( $(cpu "$wm") - c0 )) s=$(( $(cpu "$sh") - s0 ))
  sed -n "$((l0 + 1)),\$p" "$log" | python3 -c '
import re, sys
label, ms, gpu_ns, wm_ticks, sh_ticks = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
render, cpu = [], []
for line in sys.stdin:
    if "animation timing:" in line and "frame" in line:
        r = re.search(r"render_elapsed_ms=([0-9.]+)", line); c = re.search(r"total_cpu_elapsed_ms=([0-9.]+)", line)
        if r: render.append(float(r.group(1)))
        if c: cpu.append(float(c.group(1)))
def stat(v):
    v = sorted(v)
    return "%.1f/%.1f/%.1f" % (v[len(v) // 2], v[int(len(v) * 0.95)], v[-1]) if v else "-"
n = len(render)
print("%-22s %5dms frames=%4d fps=%5.1f gpu=%5.1f%% gpu/frame=%5.2fms wm_cpu=%4.1f%% shell_cpu=%4.1f%% render p50/p95/max=%s cpu=%s" % (
    label, ms, n, n * 1000 / ms, gpu_ns / 1e4 / ms, (gpu_ns / 1e6 / n) if n else 0, wm_ticks * 1000 / ms, sh_ticks * 1000 / ms, stat(render), stat(cpu)))
' "$label" "$ms" "$g" "$c" "$s" | sed "s/\$/ gt=${mhz}MHz $(temp)C/"
  sed -n "$((s1l + 1)),\$p" "$out/shell.log" | grep -c "slow" | sed "s/^/    shell slow lines: /"
}

still() { sleep 3; }
scrolls() { for _ in 1 2 3; do key Super+h 0.5; key Super+l 0.5; done; }
workspaces() { for _ in 1 2 3; do key Super+code:11 0.5; key Super+code:10 0.5; done; }
overview() { for _ in 1 2 3; do key Super+o 0.8; key Super+o 0.8; done; }
launcher() { for _ in 1 2 3; do fs menu toggle; sleep 0.8; fs menu toggle; sleep 0.8; done; }
menu_rest() { sleep 3; }
menu_anim() { for _ in 1 2 3 4 5 6 7 8; do fs menu toggle; sleep 0.33; done; }
panel_rest() { sleep 3; }
typing() { fs menu toggle; sleep 0.6; wtype -d 60 "terminal files settings"; sleep 0.3; fs menu toggle; sleep 0.5; }
panel() { for p in quicksettings audio calendar; do fs panel open "$p"; sleep 0.9; fs panel close; sleep 0.7; done; }
spew() { wtype -d 5 "seq 1 60000"; wtype -k Return; sleep 4; }
opens() { for _ in 1 2 3; do foot -a spare >/dev/null 2>&1 & sleep 0.9; key Super+q 0.7; done; }
lock() { fs lock lock; sleep 1.5; wtype -d 40 "x"; sleep 0.5; }

# A session's start heats a fanless-quiet laptop into its throttle; the moves
# wait for it to cool so two runs compare.
temp() { awk '{print int($1 / 1000)}' /sys/class/thermal/thermal_zone*/temp 2>/dev/null | sort -n | tail -1; }
cool() { for _ in $(seq 60); do [ "$(temp)" -le "${BENCH_COOL:-62}" ] && break; sleep 2; done; }
echo "nested output: $(grep -m1 -oE 'size[=:][^,]*[0-9]+x[0-9]+|[0-9]{3,4}x[0-9]{3,4}' "$log" | head -1); shell pid $sh, wm pid $wm"
for app in one two three four; do foot -a "$app" >/dev/null 2>&1 & sleep 1; done
sleep 1; cool
# BENCH_PERF samples both processes through the moves, for `perf report`.
if [ -n "${BENCH_PERF:-}" ]; then
  sudo perf record -q -F 1999 --call-graph ${BENCH_CALLGRAPH:-lbr} -p "$sh" -o "$out/shell.perf" >/dev/null 2>&1 & 
  sudo perf record -q -F 1999 --call-graph ${BENCH_CALLGRAPH:-lbr} -p "$wm" -o "$out/wm.perf" >/dev/null 2>&1 &
  sleep 1
fi
echo "start at $(temp)C, gt floor $(cat /sys/class/drm/card*/gt_min_freq_mhz 2>/dev/null | head -1)MHz"
for move in ${BENCH_PHASES:-still scrolls workspaces overview opens launcher typing panel spew}; do
  case $move in
    menu_rest) fs menu toggle; sleep 1.2; phase "$move" "$move"; fs menu toggle; sleep 1 ;;
    panel_rest) fs panel open quicksettings; sleep 1.2; phase "$move" "$move"; fs panel close; sleep 1 ;;
    *) phase "$move" "$move" ;;
  esac
done
[ -z "${BENCH_PERF:-}" ] || { sudo pkill -INT -x perf; sleep 2; sudo chown "$(id -u)" "$out"/*.perf; }
[ -x /tmp/shoji-ipc ] && python3 /tmp/shoji-ipc layers.list --socket "$rt/shojiwm-$display.sock" > "$out/layers.json" 2>&1
grim "$out/desk.png" 2>/dev/null
grep "slow" "$out/shell.log" | sed -E 's/t=[0-9]+ms //' | sort -t= -k2 | tail -400 > "$out/shell-slow.log"
echo "results in $out"
