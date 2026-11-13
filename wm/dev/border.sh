#!/bin/bash
# Hyprland's resize_on_border: a left-button drag on a window's border band
# resizes it, a tile through its column's width. A tile by its right border
# keeps its left edge, by its left border keeps its right edge; a float.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
right() { ipc state.get | jq -r --arg app "$1" '.windows[] | select(.appId == $app) | .rect | .x + .width'; }
width() { ipc state.get | jq -r --arg app "$1" '.windows[] | select(.appId == $app) | .rect.width'; }
fails=0
expect() {
  if [ "$2" = "$3" ]; then echo "ok: $1"; else echo "FAIL: $1: $2, not $3"; fails=$((fails + 1)); fi
}

step "two tiles; a's right border dragged 100 px out"
win a > "$run/a.out"; sleep 1; win b > "$run/b.out"; sleep 1
ipc window.focus "{\"id\": \"$(id_of a)\"}" >/dev/null; sleep 0.8
tape; w=$(width a); r=$(right a)
hand at $((r + 1)) 400 press glide $((r + 101)) 400 400 wait 300 release; sleep 1
tape; shot 80-tile-border
expect "a's column is 100 px wider" "$(width a)" "$((w + 100))"

step "b's left border dragged 60 px left: its right edge stays"
# The press focuses b, which brings it into view first.
ipc window.focus "{\"id\": \"$(id_of b)\"}" >/dev/null; sleep 0.8; tape
w=$(width b); r=$(right b); x=$(ipc state.get | jq -r '.windows[] | select(.appId == "b") | .rect.x')
hand at $((x - 3)) 400 press glide $((x - 63)) 400 400 wait 300 release; sleep 1
tape; shot 81-tile-left-border
expect "b is 60 px wider" "$(width b)" "$((w + 60))"
expect "b's right edge stayed" "$(right b)" "$r"

step "a float's left border dragged 100 px left"
win f > "$run/f.out"; sleep 1; key Super+v 1
w=$(width f); x=$(ipc state.get | jq -r '.windows[] | select(.appId == "f") | .rect.x')
hand at $((x - 3)) 300 press glide $((x - 103)) 300 400 wait 300 release; sleep 1
tape; shot 82-float-border
expect "f is 100 px wider" "$(width f)" "$((w + 100))"

step "the log"
grep -iE "resize" "$SHOJI_RUN_LOG" | cut -c1-240 | tail -8
echo "$fails failed"
[ "$fails" = 0 ]
