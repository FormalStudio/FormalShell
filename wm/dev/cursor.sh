#!/bin/bash
# The pointer: a window whose rule confines it, and the dynamic cursor, read
# from the engine's cursor log and from screenshots with the pointer drawn in.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
log=$XDG_CONFIG_HOME/cursor
settings="$XDG_CONFIG_HOME/formalwm/settings.json"
at() { ipc cursor.pos | jq -c '[.x, .y | round]'; }
rect() { ipc state.get | jq -c --arg app "$1" '[.windows[] | select(.appId == $app) | .rect | [.x, .y, .width, .height]][0]'; }
reload() { printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1.5; }
# What the compositor was told since `mark`: how often it was given a shape,
# the longest and the thinnest of them, their axes and turns, and how it ended.
shapes() { jq -c --argjson from "$MARK" 'select(.t >= $from) | .shape' "$log" | jq -sc '
  [.[] | select(. != null)] as $s | {shaped: ($s | length),
    along: ([$s[].along] | max // 1 | . * 100 | round / 100), across: ([$s[].across] | min // 1 | . * 100 | round / 100),
    axis: ([$s[].axis | round] | unique), rotate: ([$s[].rotate | round] | unique),
    last: (.[-1] | if . then "shaped" else "as it is" end)}'; }
# The box the pointer's picture fills on the bare desk.
box() { grim -c "$run/$1.png"; px box "$run/60-bare.png" "$run/$1.png"; }
# The hand put down and left a moment before it does the rest, so what is
# told since `mark` is the rest's and not the jump there.
from() { hand at "$1" "$2" wait 1500 "${@:3}" & moving=$!; sleep 1.3; mark; }
shake=$(for _ in $(seq 14); do printf 'glide 600 400 40 glide 400 400 40 '; done)

step "the cursor shows once the ten seconds it waits for a shell are up"
for _ in $(seq 60); do [ "$(ipc state.get | jq -r .cursorHidden)" = false ] && break; sleep 0.5; done
ipc state.get | jq -c '{cursorHidden}'

step "a window whose rule confines the pointer holds it while it has the keyboard: a mouse stops at its edges"
foot -a Minecraft-rig >/dev/null 2>&1 & sleep 1.5
echo "the window: $(rect Minecraft-rig), the keyboard: $(focus_app)"
hand at $(centre Minecraft-rig) wait 200 by 3000 0 wait 200; echo "pushed right: $(at)"
hand by 0 3000 wait 200; echo "pushed down: $(at)"
hand by -6000 -6000 wait 200; echo "pushed up and left: $(at)"
foot -a other >/dev/null 2>&1 & sleep 1.5
hand at $(centre Minecraft-rig) wait 200; echo "another window opened, the keyboard: $(focus_app)"
hand by 3000 0 wait 200; echo "pushed right: $(at)"
for app in Minecraft-rig other; do ipc window.close "{\"id\":\"$(id_of "$app")\"}" >/dev/null; done; sleep 1

step "the dynamic cursor at rest: the image as it is"
hand at 300 400 wait 1500; grim "$run/60-bare.png"; mark
echo "the pointer's picture: $(box 61-rest), told: $(shapes)"

step "at speed it is longer along its way and thinner across it, and upright"
glide() {
  from "$1" "$2" glide "$3" "$4" "$5"; sleep "$6"; picture=$(box "$8")
  wait "$moving"; sleep 1.4; echo "$7: the picture mid-way $picture, told: $(shapes)"
}
glide 100 400 900 400 500 0.45 "right at 1600 px/s" 62-right
glide 500 100 500 700 400 0.4 "down at 1500 px/s" 63-down
glide 200 150 700 650 400 0.4 "down and right at 1770 px/s" 64-diagonal
echo "a second after the last: $(box 65-rested)"

step "shaken, it grows and stays grown for two seconds after"
from 400 400 $shake; wait "$moving"
echo "just shaken: the picture $(box 66-shaken), told: $(shapes)"
sleep 4.5; echo "four and a half seconds on: the picture $(box 67-settled), told last: $(tail -1 "$log" | jq -c '.shape')"

step "shake turned off by a reload while the cursor is magnified: the image as it is, and its clock stops"
# With FWM_TIMING the core logs every frame it renders; on a still desk the
# only thing asking for frames is the cursor's clock.
drawn() { [ -z "${SHOJI_ANIMATION_TIMING_DEBUG:-}" ] || timing "$1" "$(loglen)" | cut -d';' -f1; }
from 400 400 $shake; wait "$moving"
line=$(loglen); sleep 1.5; echo "magnified and still for a second and a half: $(drawn "$line")"
sed -i 's/"mode": "stretch"/"mode": "stretch", "shake": { "enabled": false }/' "$settings"; reload
echo "after the reload: the picture $(box 68-shake-off), told last: $(tail -1 "$log" | jq -c '.shape')"
line=$(loglen); sleep 2; echo "the two seconds after: $(drawn "$line")"
sed -i 's/, "shake": { "enabled": false }//' "$settings"; reload

step "a hidden cursor is not shaped"
ipc cursor.hide '{"hidden":true}' >/dev/null; from 100 400 glide 900 400 500 wait 300; wait "$moving"; echo "hidden: $(shapes)"
ipc cursor.hide '{"hidden":false}' >/dev/null; sleep 0.5

step "cursor.dynamic in the settings: tilt leans into sideways travel, none with shake off does nothing, a limit of 0 is refused"
sed -i 's/"mode": "stretch"/"mode": "tilt"/' "$settings"; reload
from 100 400 glide 900 400 500 wait 1400; wait "$moving"; echo "tilt, right: $(shapes)"
from 900 400 glide 100 400 500 wait 1400; wait "$moving"; echo "tilt, left: $(shapes)"
sed -i 's/"mode": "tilt"/"mode": "none", "shake": { "enabled": false }/' "$settings"; reload
from 400 400 $shake wait 300; wait "$moving"; echo "none, shaken: $(shapes)"
( ipc state.get --listen 2 | jq -c 'select(.event != null and .event != "state.changed")' ) & listener=$!
sed -i 's/"mode": "none"/"mode": "stretch", "stretch": { "limit": 0 }/' "$settings"; sleep 0.4; reload; wait "$listener"

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme\|smithay::backend\|backend::tty\|XWayland" | cut -c1-300 | tail -8
