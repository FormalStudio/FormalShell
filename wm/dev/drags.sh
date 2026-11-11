#!/bin/bash
# Data dragged between clients on the rig: the card and its badge, a drop
# taken and a drop refused, and what holding a drag still does at a
# window, a tape edge, the corner and a workspace in the overview.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
seen=0
said() { tail -n +$((seen + 1)) "$run/src.out" | tr '\n' ' '; echo; seen=$(wc -l < "$run/src.out"); }
region() { px diff "$run/$1.png" "$run/$2.png" "${@:3}"; }

step "a window that starts drags, a terminal that takes text, two more columns"
win src > "$run/src.out"; sleep 1.2
open term three four
ipc window.focus "{\"id\":\"$(id_of src)\"}" >/dev/null; sleep 1.2
tape; said; shot 70-before

step "the card rides the pointer; over its own window no drop is offered"
hand at $(centre src) press glide 400 300 300 wait 900 & drag=$!
sleep 0.8; shot 71-card-at-home; wait "$drag"
echo "the card's place against the empty desk: $(region 70-before 71-card-at-home 380 280 160 90)"

step "held over the terminal for half a second the terminal is focused; the drop lands in it and the keyboard follows"
focus_app
hand glide $(centre term) 400 wait 1000
focus_app; shot 72-card-over-terminal
hand release; sleep 1; said; key x 0.5; shot 73-dropped
echo "the card after the drop: $(region 70-before 73-dropped 380 280 160 90) at home, $(region 72-card-over-terminal 73-dropped 740 360 200 110) over the terminal"

step "a drop nobody takes: the card flies back to where the drag began"
ipc window.focus "{\"id\":\"$(id_of src)\"}" >/dev/null; sleep 1
hand at $(centre src) press glide 420 620 400 wait 400; shot 74-refused-held
hand release; shot 75-flying; sleep 1.2; shot 76-flown; said
echo "at the hand: $(region 74-refused-held 76-flown 400 600 180 100), on the way: $(region 75-flying 76-flown 230 360 370 340), left the hand: $(region 74-refused-held 75-flying 400 600 180 100)"

step "held at the tape's right edge the tape scrolls, at the left edge it scrolls back"
tape
hand at $(centre src) press glide 1018 400 300 wait 1200 & drag=$!
sleep 1.3; tape; shot 77-edge; wait "$drag"
hand glide 14 400 300 wait 1200; tape
hand glide $(centre src) 300 wait 100 release; sleep 0.8; said

step "let go at the edge without moving on: the drop is offered to the window the tape brought under the pointer, its own, which takes none"
hand at $(centre src) press glide 1018 400 300 wait 1200 glide 14 400 300 wait 1200 release; sleep 0.8; tape; said

step "held in the corner the overview opens; held over the next workspace there, the session goes to it"
hand at $(centre src) press glide 3 3 400 wait 900
ipc state.get | jq -c '{overview, ws: .focusedWorkspaceId}'; shot 78-corner
hand glide 512 720 500 wait 900
ipc state.get | jq -c '{overview, ws: .focusedWorkspaceId}'
hand release; sleep 1; said
ipc workspace.focus '{"id":"1"}' >/dev/null; sleep 1

step "a window in the hand at the tape's edge scrolls it too, and lands in the gap under the pointer"
ipc window.focus "{\"id\":\"$(id_of term)\"}" >/dev/null; sleep 1; tape
hold Super 4000
hand at $(centre term) press glide 1018 400 300 wait 900 glide 700 400 200 wait 150 release; sleep 1.3
tape

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -12
