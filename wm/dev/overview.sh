#!/bin/bash
# The overview's clicks and keys that the other sessions leave out, the
# theme across a settings reload, and an output turned off and on over the
# socket.
set -u
. "$(dirname "$0")/lib.sh"
seen() { ipc state.get | jq -c '{overview, ws: .focusedWorkspaceId, focus: ([.windows[] | select(.focused) | .appId][0])}'; }

step "two columns on 1, one on 2, back on 1"
open one two; ipc workspace.focus '{"id":"2"}' >/dev/null; sleep 1; open three
ipc workspace.focus '{"id":"1"}' >/dev/null; sleep 1; seen

step "typing in the overview does nothing: it stays open, nothing moves"
ipc overview.set '{"open":true}' >/dev/null; sleep 1.3
key x 0.4; key a 0.4; key 1 0.4; seen; shot 20-overview

step "a window dragged past its neighbour in the overview changes columns"
tape
hand at $(lensed one) press glide 750 384 400 wait 150 release; sleep 1.3; tape; seen

step "a click on the bare part of the next workspace goes there and closes"
hand at 300 700 press wait 60 release; sleep 1.3; seen

step "a click beside the workspaces closes and changes nothing"
ipc overview.set '{"open":true}' >/dev/null; sleep 1.3
hand at 100 384 press wait 60 release; sleep 1.3; seen

step "SUPER+E opens it; Left and Right walk the columns of the workspace in view, Return closes"
ipc workspace.focus '{"id":"1"}' >/dev/null; sleep 1
key Super+e 1.3; seen
key Left 0.6; seen; key Right 0.6; seen; key Return 1.2; seen

step "the theme the shell sent stands across a settings reload"
ipc theme.set '{"colors":{"primary":"#ff0000"},"chrome":{"gapsIn":8,"gapsOut":16}}' >/dev/null; sleep 1
ipc state.get | jq -c '.chrome'
printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1.5
ipc state.get | jq -c '.chrome'; tape

step "outputs.set: off, then on, and everything is where it was"
out=$(ipc state.get | jq -r '.focusedOutput')
ipc outputs.set "{\"name\":\"$out\",\"enabled\":false}" >/dev/null; sleep 1.5
ipc outputs.get | jq -c '[.[] | {name, enabled, width, height, refresh}]'; ipc state.get | jq -c '{outputs: [.outputs[] | {name, enabled}], ws: .focusedWorkspaceId}'
ipc outputs.set "{\"name\":\"$out\",\"enabled\":true}" >/dev/null; sleep 2
ipc outputs.get | jq -c '[.[] | {name, enabled, width, height, refresh}]'; seen; tape; shot 21-output-back

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -8
