# What the rig sessions share: keys, the socket, screenshots, the pointer.
# Keys come from the compositor's own --key, or with FWM_KEYS=wtype from
# wtype's virtual keyboard, which carries its own keymap: a `code:N` of the
# number row is sent as the digit the es layout has on that key.
typed() {
  local chord=$1 ms=${2:-} key= part mods=() args=() i
  IFS=+ read -ra parts <<< "$chord"
  for part in "${parts[@]}"; do
    case "$part" in
      Super) mods+=(logo) ;;
      Shift) mods+=(shift) ;;
      Alt) mods+=(alt) ;;
      Ctrl) mods+=(ctrl) ;;
      code:*) key=$(((${part#code:} - 9) % 10)) ;;
      *) key=$part ;;
    esac
  done
  for part in "${mods[@]}"; do args+=(-M "$part"); done
  if [ -n "$key" ] && [ -n "$ms" ]; then args+=(-P "$key" -s "$ms" -p "$key")
  elif [ -n "$key" ]; then args+=(-k "$key")
  else args+=(-s "$ms"); fi
  for ((i = ${#mods[@]} - 1; i >= 0; i--)); do args+=(-m "${mods[i]}"); done
  wtype "${args[@]}"
}
key() {
  if [ "${FWM_KEYS:-}" = wtype ]; then typed "$1"; else "$SHOJI_BIN" --key "$1" >/dev/null; fi
  sleep "${2:-0.7}"
}
ipc() { ~/shoji-ipc "$@"; }
shot() { grim "$SHOJI_RUN_DIR/$1.png"; }
windows() {
  ipc state.get | jq -c '{ws: .focusedWorkspaceId, focus: .focusedWindowId, overview,
    windows: [.windows[] | {id: .id[0:6], app: .appId, ws: .workspaceId, ov: .overlay, shown,
      c: .column, r: .row, fl: .floating, fs: .fullscreen, rect: (.rect | [.x, .y, .width, .height])}]}'
}
step() { echo "== $1"; }
hand() { "$(dirname "$SHOJI_BIN")/fwm-hand" "$@"; }
layer() { "$(dirname "$SHOJI_BIN")/fwm-layer" "$@" & }
id_of() { ipc state.get | jq -r --arg app "$1" '.windows[] | select(.appId == $app) | .id'; }
focus_app() { ipc state.get | jq -r '. as $s | [.windows[] | select(.id == $s.focusedWindowId) | .appId][0]'; }
# The middle of a window's layout rect, and where the open overview draws
# it when its workspace is the one the output shows (1024x768, zoom 0.5).
centre() { ipc state.get | jq -r --arg app "$1" '.windows[] | select(.appId == $app) | .rect | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"'; }
lensed() { ipc state.get | jq -r --arg app "$1" '.windows[] | select(.appId == $app) | .rect | "\(256 + (.x + .width / 2) / 2 | floor) \(192 + (.y + .height / 2) / 2 | floor)"'; }
hold() {
  if [ "${FWM_KEYS:-}" = wtype ]; then typed "$1" "$2" & else "$SHOJI_BIN" --key "$1" --hold "$2" >/dev/null; fi
  sleep 0.2
}
# The frames the engine presented since `mark`, from its frame log: for each
# app the biggest step between two frames, and where it ended.
mark() { MARK=$(date +%s%3N); }
steps() {
  jq -c --argjson from "$MARK" 'select(.t >= $from)' "$XDG_CONFIG_HOME/frames" | jq -sc '
    . as $f | [range(1; length) | . as $i | $f[$i].windows[] | . as $w
      | ($f[$i - 1].windows[] | select(.app == $w.app)) as $p
      | {app: $w.app, dx: ($w.x - $p.x | fabs), dy: ($w.y - $p.y | fabs), dt: ($f[$i].t - $f[$i - 1].t)}]
    | group_by(.app) | map({app: .[0].app, frames: length, dx: (map(.dx) | max | round), dy: (map(.dy) | max | round)})
    | {frames: ($f | length), ms: (($f[-1].t - $f[0].t) | round), overview: ([$f[].overview] | {min: (min * 100 | round), max: (max * 100 | round)}), steps: .}'
}
drawn() { tail -1 "$XDG_CONFIG_HOME/frames" | jq -c '[.windows[] | {app, x: (.x | round), y: (.y | round), w: (.w | round), h: (.h | round), o: (.o * 100 | round)}]'; }
px() { python3 "$(dirname "${BASH_SOURCE[0]}")/px.py" "$@"; }
win() { "$(dirname "$SHOJI_BIN")/fwm-drag" "$@" & }
# A workspace's windows where the layout has them, column by column.
tape() {
  ipc state.get | jq -c --arg ws "${1:-1}" '[.windows[] | select(.workspaceId == $ws)
    | {app: .appId, c: .column, r: .row, fl: .floating, x: .rect.x, y: .rect.y, w: .rect.width, h: .rect.height}] | sort_by(.c, .r) | .[]
    | "\(.app) c\(.c) r\(.r)\(if .fl then " float" else "" end) [\(.x),\(.y) \(.w)x\(.h)]"' | tr '\n' ' '; echo
}
open() { for app in "$@"; do foot -a "$app" >/dev/null 2>&1 & sleep 1.2; done; }
# Since `mark`: one field of one app frame by frame, the overview's progress
# frame by frame, and the time between frames.
path() {
  jq -c --argjson from "$MARK" --arg app "$1" --arg field "$2" \
    'select(.t >= $from) | .windows[] | select(.app == $app) | .[$field] | round' "$XDG_CONFIG_HOME/frames" | tr '\n' ' '; echo
}
lens() { jq -c --argjson from "$MARK" 'select(.t >= $from) | .overview * 100 | round' "$XDG_CONFIG_HOME/frames" | tr '\n' ' '; echo; }
intervals() {
  jq -c --argjson from "$MARK" 'select(.t >= $from) | .t' "$XDG_CONFIG_HOME/frames" | jq -sc '
    [range(1; length) as $i | .[$i] - .[$i - 1]] | sort
    | {frames: (length + 1), median_ms: .[length / 2 | floor], p95_ms: .[length * 0.95 | floor], worst_ms: .[-1]}'
}
# The core's own account of the frames it rendered between two lengths of
# its log (SHOJI_ANIMATION_TIMING_DEBUG), and the rate it presented at.
loglen() { wc -l < "$SHOJI_RUN_LOG"; }
timing() {
  sed -n "$1,$2p" "$SHOJI_RUN_LOG" | python3 -c '
import re, sys
render, cpu, rate = [], [], []
for line in sys.stdin:
    if "animation timing: tty frame" in line:
        render.append(float(re.search(r"render_elapsed_ms=([0-9.]+)", line).group(1)))
        cpu.append(float(re.search(r"total_cpu_elapsed_ms=([0-9.]+)", line).group(1)))
    elif "present rate" in line:
        rate.append(float(re.search(r"present_rate_hz=([0-9.]+)", line).group(1)))
def stat(values):
    values = sorted(values)
    return "none" if not values else "median %.1f, p95 %.1f, worst %.1f" % (values[len(values) // 2], values[int(len(values) * 0.95)], values[-1])
print("rendered frames %d; render ms: %s; cpu ms: %s; present rate Hz: %s" % (len(render), stat(render), stat(cpu), " ".join("%.0f" % r for r in rate) or "none"))'
}
