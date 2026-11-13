#!/bin/bash
# The tape on the rig, bind by bind: a lone column, the widths, the fit,
# stacks, zen, the nine workspaces and the wheel.
set -u
. "$(dirname "$0")/lib.sh"
ws() { ipc state.get | jq -r '.focusedWorkspaceId'; }
zen() { ipc state.get | jq -c '[.workspaces[] | select(.zen != null) | {id, zen: .zen[0:6]}]'; }

step "a lone column keeps its width, centred: 0.5, then SUPER+R, minus, plus, F, M and back"
open one; tape
key Super+r; tape
key Super+minus; tape
key Super+plus; tape
key Super+f; tape
key Super+m; tape; shot 50-maximized
key Super+m; key Super+r; key Super+r; tape

step "four half columns: the focused one is fitted by the least scroll, H and L wrap"
open two three four; focus_app; tape
key Super+h; focus_app; tape
key Super+h; focus_app; tape
key Super+h; focus_app; tape
key Super+h; focus_app; tape
key Super+l; focus_app; tape
shot 51-four-columns

step "XF86Launch5 and XF86Tools walk columns, SUPER+C fits the focused one"
key XF86Launch5; focus_app; key XF86Tools; focus_app
key Super+c; tape

step "SUPER+SHIFT+L and H swap the column with its neighbour"
key Super+Shift+l; focus_app; tape
key Super+Shift+h; focus_app; tape

step "SUPER+comma takes the window into the column before it; K and J walk the stack, SHIFT swaps"
key Super+l; focus_app; key Super+comma; tape; shot 52-stack
key Super+k; focus_app; key Super+j; focus_app
key Super+Shift+k; focus_app; tape
key Super+Shift+j; focus_app; tape

step "SUPER+comma on a stacked window puts it out before the column, SUPER+SHIFT+comma takes it into the next and out after it"
key Super+comma; focus_app; tape
key Super+Shift+comma; focus_app; tape
key Super+Shift+comma; focus_app; tape

step "zen: the window in the middle takes the zen rect, the columns left and right leave by their own side"
key Super+l; focus_app; before=$(tape); echo "$before"; mark
key Super+z 0.25; shot 53-zen-midway; sleep 1.2
zen; tape; drawn; steps; shot 54-zen
key Super+h; focus_app; zen; key Super+l; focus_app; zen
mark; key Super+z 1.4; zen; after=$(tape); steps
[ "$before" = "$after" ] && echo "the tape is as it was" || { echo "the tape changed:"; echo "$after"; }

step "zen, then the window closes: zen goes to the window the focus goes to"
key Super+z 1.2; zen; key Super+q 1.4; zen; tape; drawn
key Super+z 1.4; zen; tape

step "a float in zen goes back to its float rect"
key Super+v 1; float=$(tape); echo "$float"
key Super+z 1.2; zen; tape
key Super+z 1.2; zen; back=$(tape)
[ "$float" = "$back" ] && echo "the float is where it was" || { echo "the float moved:"; echo "$back"; }
key Super+v 1

step "zen.set by id, on and off"
id=$(id_of two)
ipc zen.set "{\"id\":\"$id\",\"on\":true}" >/dev/null; sleep 1.2; zen
ipc zen.set "{\"id\":\"$id\",\"on\":false}" >/dev/null; sleep 1.2; zen

step "SUPER+1 to 9 by keycode, SUPER+Tab back"
for code in 11 12 13 14 15 16 17 18 10; do key Super+code:$code 0.5; printf '%s ' "$(ws)"; done; echo
key Super+code:14 0.5; key Super+Tab 0.5; ws; key Super+Tab 0.5; ws

step "SUPER+SHIFT+4 sends the focused window to 4 and follows it"
key Super+code:10 0.6; focus_app
key Super+Shift+code:13 1; ws; focus_app; tape 4

step "SUPER and the wheel walk the workspaces that hold a window, round and round"
ipc window.moveTo "{\"id\":\"$(id_of four)\",\"idx\":7,\"output\":\"$(ipc state.get | jq -r .focusedOutput)\"}" >/dev/null
echo "mouse_up, the wheel turned away:"
for turn in 1 2 3 4; do hold Super 500; hand wheel -1; sleep 1; printf '%s ' "$(ws)"; done; echo
echo "mouse_down, the wheel turned toward the user:"
for turn in 1 2 3 4; do hold Super 500; hand wheel 1; sleep 1; printf '%s ' "$(ws)"; done; echo

step "SUPER+SHIFT+F is fullscreen and back, SUPER+V floats and tiles"
key Super+code:10 0.8
key Super+Shift+f 1.2; ipc state.get | jq -c '[.windows[] | select(.fullscreen) | {app: .appId, rect}]'; shot 55-fullscreen
key Super+Shift+f 1.2; ipc state.get | jq -c '[.windows[] | select(.fullscreen)] | length'

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -12
