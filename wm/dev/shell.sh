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
step "the overview, and Super+1 to 3 by keycode"
key Super+o 1.5; shot 41-shell-overview; key Escape 1.2
key Super+code:11 1; ipc state.get | jq -c '.focusedWorkspaceId'
key Super+code:12 1; ipc state.get | jq -c '.focusedWorkspaceId'
key Super+code:10 1; ipc state.get | jq -c '.focusedWorkspaceId'
key Super+Shift+code:12 1; ipc state.get | jq -c '[.windows[] | {app: .appId, ws: .workspaceId}]'
step "the logs"
tail -15 "$XDG_CONFIG_HOME/shell.log" | cut -c1-300
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cursor image\|cursor theme\|smithay::backend\|backend::tty\|XWayland" | cut -c1-300 | tail -12
