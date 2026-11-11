#!/bin/bash
# Motion on the rig, read from the engine's frame log: each move run whole,
# then cut short by the next one, then with motion off. With FWM_TIMING set
# the core's own frame times for a tape scroll and an overview open.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
ws() { ipc state.get | jq -r '.focusedWorkspaceId'; }
focus() { ipc window.focus "{\"id\":\"$(id_of "$1")\"}" >/dev/null; sleep "${2:-1.2}"; }
# The largest step a field takes between two frames since `mark`.
stride() {
  jq -c --argjson from "$MARK" --arg app "$1" --arg field "$2" \
    'select(.t >= $from) | .windows[] | select(.app == $app) | .[$field]' "$XDG_CONFIG_HOME/frames" \
    | jq -sc '[range(1; length) as $i | (.[$i] - .[$i - 1]) | fabs] | {frames: (length + 1), largest_step: (max // 0 | round)}'
}
measured() { intervals; [ -z "${SHOJI_ANIMATION_TIMING_DEBUG:-}" ] || timing "$1" "$(loglen)"; }

step "four columns, the last in view"
open one two three four; sleep 0.6; tape

step "a tape scroll, whole: Super+H from three to two brings two into view"
key Super+h 1
mark; from=$(loglen); key Super+h 1.3
echo "two x: $(path two x)"; stride two x; measured "$from"

step "two scrolls, the second before the first has landed"
key Super+l 1.2; key Super+l 1.2
key Super+h 1
mark; key Super+h 0.05; key Super+h 1.4
echo "one x: $(path one x)"; stride one x

step "a workspace switch, whole"
mark; key Super+code:11 1.3; ws
echo "one y: $(path one y)"; stride one y
key Super+code:10 1.3

step "two switches, the second before the first has landed"
mark; key Super+code:11 0.05; key Super+code:12 1.5; ws
echo "one y: $(path one y)"; stride one y
key Super+code:10 1.3

step "the overview, opened whole and closed whole"
mark; from=$(loglen); ipc overview.set '{"open":true}' >/dev/null; sleep 1.3
echo "open: $(lens)"; measured "$from"
mark; ipc overview.set '{"open":false}' >/dev/null; sleep 1.3
echo "close: $(lens)"

step "the overview turned back halfway"
mark; ipc overview.set '{"open":true}' >/dev/null; sleep 0.12; ipc overview.set '{"open":false}' >/dev/null; sleep 1.3
echo "open then close: $(lens)"
echo "one w: $(path one w)"

step "zen on the column in the middle, whole, and back"
focus two
mark; key Super+z 1.4
echo "two x: $(path two x)"; echo "two w: $(path two w)"; echo "one x: $(path one x)"; echo "three x: $(path three x)"
mark; key Super+z 1.4
echo "two x: $(path two x)"; echo "one x: $(path one x)"

step "zen turned back during its own way in"
mark; key Super+z 0.05; key Super+z 1.5
echo "two x: $(path two x)"; echo "two w: $(path two w)"; echo "one x: $(path one x)"; echo "three x: $(path three x)"
stride two x; stride one x

step "a width change cut short by the next: Super+R twice"
mark; key Super+r 0.05; key Super+r 1.4
echo "two w: $(path two w)"; stride two w
key Super+r 1; key Super+r 1

step "a window opens from a point at its centre and fades in; closed, the tape closes up"
mark; foot -a five >/dev/null 2>&1 & sleep 1.5
echo "five w: $(path five w)"; echo "five o: $(jq -c --argjson from "$MARK" 'select(.t >= $from) | .windows[] | select(.app == "five") | .o * 100 | round' "$XDG_CONFIG_HOME/frames" | tr '\n' ' ')"
shot 89-before-closing
# The two fast clocks slowed to two seconds, so a screenshot can catch the
# way out: the shrink on spatialFast, the fade on effectsFast (its prompt
# goes grey in 90-closing; the window behind is the neighbour closing up).
ipc theme.set '{"colors":{},"chrome":{"motion":{"families":{"spatialFast":{"curve":[0.42,1.67,0.21,0.9],"ms":2000},"effectsFast":{"curve":[0.31,0.94,0.34,1],"ms":2000}}}}}' >/dev/null; sleep 0.5
read -r x y <<< "$(ipc state.get | jq -r '.windows[] | select(.appId == "five") | .rect | "\(.x) \(.y)"')"
mark; key Super+q 0.2; shot 90-closing; sleep 2.2
echo "three x: $(path three x)"
echo "five's top-left corner, where the shrink leaves the desk bare: $(px diff "$run/89-before-closing.png" "$run/90-closing.png" "$x" "$y" 12 300)"
ipc theme.set '{"colors":{},"chrome":{}}' >/dev/null; sleep 0.5

step "the console drops in from above with its dim, whole, then turned back"
win solid; sleep 1.2; focus solid; tape
foot -a dev.formalshell.console >/dev/null 2>&1 & sleep 1.5
console=$(ipc state.get | jq -r '.windows[] | select(.overlay != null) | .id')
ipc window.place "{\"id\":\"$console\",\"x\":112,\"y\":40,\"width\":800,\"height\":300}" >/dev/null; sleep 0.5
read -r x y <<< "$(centre solid)"; shot 91-undimmed
mark; ipc overlay.show "{\"id\":\"$console\"}" >/dev/null; sleep 1.3; shot 92-dimmed
echo "console y: $(path dev.formalshell.console y)"; stride dev.formalshell.console y
echo "the window under it, below the console: $(px mean "$run/91-undimmed.png" "$((x - 40))" 500 80 80) before, $(px mean "$run/92-dimmed.png" "$((x - 40))" 500 80 80) dimmed"
mark; ipc overlay.park "{\"id\":\"$console\",\"overlay\":\"formalshell-console\"}" >/dev/null; sleep 0.12
ipc overlay.show "{\"id\":\"$console\"}" >/dev/null; sleep 1.3
echo "parked then shown again: $(path dev.formalshell.console y)"; stride dev.formalshell.console y
ipc overlay.park "{\"id\":\"$console\",\"overlay\":\"formalshell-console\"}" >/dev/null; sleep 1.2

step "the border colour crosses over on a focus change"
focus one 0.1; shot 93-border-crossing; sleep 1; shot 94-border-landed
read -r x y <<< "$(ipc state.get | jq -r '.windows[] | select(.appId == "one") | .rect | "\(.x) \(.y)"')"
echo "one's top border: $(px mean "$run/93-border-crossing.png" "$((x + 100))" "$((y - 2))" 200 2) on the way, $(px mean "$run/94-border-landed.png" "$((x + 100))" "$((y - 2))" 200 2) landed"

step "motion off: every one of them in a single frame"
ipc theme.set '{"colors":{},"chrome":{"motion":{"enabled":false}}}' >/dev/null; sleep 0.8
mark; key Super+code:11 0.8; echo "switch, one y: $(path one y)"; key Super+code:10 0.8
mark; key Super+l 0.8; echo "scroll, one x: $(path one x)"
mark; ipc overview.set '{"open":true}' >/dev/null; sleep 0.8; echo "overview: $(lens)"
mark; ipc overview.set '{"open":false}' >/dev/null; sleep 0.8; echo "overview closed: $(lens)"
focus two 0.8
mark; key Super+z 0.8; echo "zen, two w: $(path two w), one x: $(path one x)"; key Super+z 0.8
mark; foot -a six >/dev/null 2>&1 & sleep 1.5; echo "opened, six w: $(path six w)"
mark; key Super+q 0.15; shot 95-closed-at-once; sleep 0.6; ipc state.get | jq -c '[.windows[] | .appId]'
mark; ipc overlay.show "{\"id\":\"$console\"}" >/dev/null; sleep 0.8; echo "console y: $(path dev.formalshell.console y)"

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme\|animation timing" | cut -c1-300 | tail -12
