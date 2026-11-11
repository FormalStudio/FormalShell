#!/bin/bash
# The lid output on the rig (FWM_HOST=host-lid.json names the rig's own
# output for it, FORMALWM_LID a file for the lid's state): shut, the output
# goes dark and its windows are kept; open, it comes back with them.
set -u
. "$(dirname "$0")/lib.sh"
lid() { echo "state:      $1" > "$FORMALWM_LID"; sleep 3; }
seen() {
  ipc outputs.get | jq -c '[.[] | {name, enabled, width, height}]'
  ipc state.get | jq -c '{outputs: [.outputs[] | {name, enabled}], windows: [.windows[] | {app: .appId, ws: .workspaceId}], ws: .focusedWorkspaceId}'
}
step "open: two windows on workspace 2"
open one two; ipc workspace.focus '{"id":"2"}' >/dev/null; sleep 1; open three; seen
step "the lid shuts"
lid closed; seen
step "the lid opens"
lid open; seen; shot 96-lid-open
step "shut over a reload: the output stays dark"
lid closed; printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 2; seen
lid open; seen
step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -8
