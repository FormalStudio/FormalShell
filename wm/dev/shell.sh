#!/bin/bash
# FormalShell on the engine (dev/check.sh with FWM_SHELL naming it): what
# layers it brings, where the tape sits under its bar, what it asked of the
# socket.
set -u
. "$(dirname "$0")/lib.sh"
step "the shell's layers"
sleep 8; ipc layers.list | jq -c '.[]'
ipc state.get | jq -c '{cursorHidden, chrome}'
step "two terminals under the bar"
for app in one two; do foot -a "$app" >/dev/null 2>&1 & sleep 1.5; done
ipc state.get | jq -c '[.windows[] | {app: .appId, rect: (.rect | [.x, .y, .width, .height])}]'; shot 40-shell
ipc state.get | jq -r '.windows[] | "\(.appId) \(.pid)"' | while read -r app pid; do echo "$app: pid $pid is $(cat "/proc/$pid/comm")"; done
step "the overview, and Super+1 to 3 by keycode"
key Super+o 1.5; shot 41-shell-overview; key Escape 1.2
key Super+code:11 1; ipc state.get | jq -c '.focusedWorkspaceId'
key Super+code:12 1; ipc state.get | jq -c '.focusedWorkspaceId'
key Super+code:10 1; ipc state.get | jq -c '.focusedWorkspaceId'
key Super+Shift+code:12 1; ipc state.get | jq -c '[.windows[] | {app: .appId, ws: .workspaceId}]'
step "a made output removed and made again under its name: each of the shell's layers once, none left on the other output"
per_output() { ipc layers.list | jq -c 'group_by(.output) | map({output: .[0].output, layers: (map(.namespace) | group_by(.) | map("\(.[0]) x\(length)"))}) | .[]'; }
ipc outputs.create '{"name":"FS-RIG","width":1280,"height":720}' >/dev/null; sleep 3; per_output
ipc outputs.remove '{"name":"FS-RIG"}' >/dev/null; ipc outputs.create '{"name":"FS-RIG","width":800,"height":600}' >/dev/null; sleep 3
echo "removed and made again at 800x600:"; per_output
step "outputs.set width and height on it: resized in place, its layers follow and none is made twice; refused on an output with a connector"
ipc outputs.set '{"name":"FS-RIG","width":1600,"height":900}' >/dev/null; sleep 3
ipc state.get | jq -c '.outputs[] | {name, width, height, scale, reserved}'
ipc layers.list | jq -c '.[] | select(.output == "FS-RIG") | {namespace, width, height}'; per_output
ipc outputs.set '{"name":"Virtual-4","width":800,"height":600}' 2>&1
ipc outputs.remove '{"name":"FS-RIG"}' >/dev/null; sleep 2; echo "removed:"; per_output
step "the logs"
tail -15 "$XDG_CONFIG_HOME/shell.log" | cut -c1-300
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cursor image\|cursor theme\|smithay::backend\|backend::tty\|XWayland" | cut -c1-300 | tail -12
