#!/bin/bash
# The tape against Hyprland's scrolling layout (0.56), bind by bind: columns
# opening and closing, the column binds with a float focused, the swap at
# the ends, expel, maximize and fullscreen inside the tape, windows that
# bound their own size, dialogs, and windows sent across outputs of other
# scales. The windows are fwm-drag's, which print each size asked of them.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
close() { ipc window.close "{\"id\": \"$(id_of "$1")\"}" >/dev/null; sleep 1; }
id_ws() { ipc state.get | jq -r --arg app "$1" '.windows[] | select(.appId == $app) | .workspaceId'; }
asked() { grep configure "$run/$1.out" | tail -1; }

step "three columns, the middle one closes, then the last"
win a > "$run/a.out"; sleep 1; win b > "$run/b.out"; sleep 1; win c > "$run/c.out"; sleep 1
ipc window.focus "{\"id\": \"$(id_of b)\"}" >/dev/null; sleep 0.6
tape; focus_app
close b; tape; focus_app
close c; tape; focus_app

step "SUPER+SHIFT+H on the first column and L on the last swap round the ends"
win b > "$run/b.out"; sleep 1; win c > "$run/c.out"; sleep 1
key Super+Shift+l; tape
key Super+h; key Super+h; focus_app
key Super+Shift+h; tape

step "SUPER+comma then SUPER+Shift+comma: the expelled column's width"
key Super+r; tape
key Super+l; key Super+comma; tape
key Super+comma; tape

step "a float has the focus: SUPER+H, L, R, F and comma"
win f > "$run/f.out"; sleep 1; key Super+v 1; focus_app; before=$(tape); echo "$before"
for chord in Super+h Super+l Super+r Super+f Super+comma Super+Shift+l; do
  key "$chord"; printf '%s: %s ' "$chord" "$(focus_app)"
done; echo
after=$(tape); [ "$before" = "$after" ] && echo "the tape is as it was" || echo "$after"

step "a window that opens while the float has the focus"
ipc window.focus "{\"id\": \"$(id_of f)\"}" >/dev/null; sleep 0.5
win d > "$run/d.out"; sleep 1; tape; focus_app

step "SUPER+M: the column in the tape, then L and H"
key Super+h; focus_app
key Super+m 1; tape; shot 60-maximized
key Super+l 1; focus_app; tape
key Super+h 1; focus_app; tape; shot 61-maximized-back
key Super+m 1; tape

step "SUPER+SHIFT+F: fullscreen, then L and H"
key Super+Shift+f 1; tape; shot 62-fullscreen
key Super+l 1; focus_app; tape
key Super+h 1; focus_app; tape
key Super+Shift+f 1; tape

step "a window that asks to be maximized before it maps, as T3 Code and Discord do"
win asks --maximized > "$run/asks.out"; sleep 1.5; tape; asked asks; shot 66-asks-maximized
key Super+Return 1.5; tape; focus_app
close asks; ipc state.get | jq -r '.windows[] | select(.appId == "foot") | .id' | while read -r id; do ipc window.close "{\"id\": \"$id\"}" >/dev/null; done; sleep 1

step "a window that will not go below 700x500 in a half column"
win big --min 700x500 > "$run/big.out"; sleep 1.5
tape; asked big; drawn; shot 63-min-size
step "a window that will not go above 300x200 in a half column"
win small --max 300x200 > "$run/small.out"; sleep 1.5
tape; asked small; drawn; shot 64-max-size
close big; close small

step "dialogs: one of a fixed size, one with a parent"
win fixed --fixed --size 320x200 > "$run/fixed.out"; sleep 1.5
win parent --child > "$run/parent.out"; sleep 1.5
ipc state.get | jq -c '[.windows[] | select(.appId | test("fixed|parent")) | {app: .appId, fl: .floating, ws: .workspaceId, rect: (.rect | [.x, .y, .width, .height])}]'
shot 65-dialogs
close fixed; close parent-child; close parent

step "outputs at 1.25 and 1.6 right of this one"
p125=$(ipc outputs.create '{"name":"p125","width":2560,"height":1600,"scale":1.25}'); echo "$p125"
p16=$(ipc outputs.create '{"name":"p16","width":2560,"height":1600,"scale":1.6}'); echo "$p16"
sleep 1.5
ipc state.get | jq -c '.outputs' | cut -c1-600

step "a tile and a float sent to the 1.6 output's workspace"
ipc window.focus "{\"id\": \"$(id_of a)\"}" >/dev/null; sleep 0.5
ipc window.place "{\"id\": \"$(id_of f)\", \"x\": 600, \"y\": 500, \"width\": 400, \"height\": 250}" >/dev/null; sleep 0.5
ipc state.get | jq -c '[.windows[] | select(.appId == "f") | .rect]'
ipc window.moveTo "{\"id\": \"$(id_of f)\", \"idx\": 1, \"output\": \"p16\"}" >/dev/null
ipc window.moveTo "{\"id\": \"$(id_of a)\", \"idx\": 1, \"output\": \"p16\"}" >/dev/null; sleep 1
ws16=$(id_ws a)
ipc state.get | jq -c '[.windows[] | select(.appId == "f" or .appId == "a") | {app: .appId, ws: .workspaceId, rect: (.rect | [.x, .y, .width, .height])}]'
tape "$ws16"; asked a

step "the same on the 1.25 output"
ipc window.moveTo "{\"id\": \"$(id_of a)\", \"idx\": 1, \"output\": \"p125\"}" >/dev/null
ipc window.moveTo "{\"id\": \"$(id_of b)\", \"idx\": 1, \"output\": \"p125\"}" >/dev/null; sleep 1
ws125=$(id_ws a)
tape "$ws125"; asked a; asked b

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -12
