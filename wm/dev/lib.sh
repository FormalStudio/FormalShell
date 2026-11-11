# What the rig sessions share: keys, the socket, screenshots, the pointer.
key() { "$SHOJI_BIN" --key "$1" >/dev/null; sleep "${2:-0.7}"; }
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
hold() { "$SHOJI_BIN" --key "$1" --hold "$2" >/dev/null; sleep 0.2; }
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
