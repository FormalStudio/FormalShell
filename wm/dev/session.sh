#!/bin/bash
# One scripted session on the rig, run inside ~/shoji-run.sh by dev/check.sh:
# opens windows, drives the binds and the socket, takes screenshots and
# prints what the socket says after each step.
set -u
. "$(dirname "$0")/lib.sh"

step start
ipc state.get | jq -c '{outputs, workspaces: [.workspaces[] | {id, idx, name, output, active}], keyboard, cursorHidden, chrome}'
ipc binds.list | jq -c 'length, .[0], (map(select(.key | startswith("code:"))) | .[0])'

step "three terminals"
for app in one two three; do foot -a "$app" >/dev/null 2>&1 & sleep 1.2; done
windows; shot 01-three

step "focus left twice, then wrap right"
key Super+h; key Super+h; windows | jq -c '.focus'
key Super+h; windows | jq -c '.focus'
shot 02-focus

step "width presets, full, maximize"
key Super+r; windows | jq -c '[.windows[] | .rect]'
key Super+f; windows | jq -c '[.windows[] | .rect]'; shot 03-full
key Super+m; windows | jq -c '[.windows[] | .rect]'; shot 04-max
key Super+m; key Super+r

step "consume into a column, focus down"
key Super+comma; windows | jq -c '[.windows[] | {app, c, r, rect}]'; shot 05-stack
key Super+j; windows | jq -c '.focus'
key Super+Shift+comma

step "float and fullscreen"
key Super+v; windows | jq -c '[.windows[] | {app, fl, rect}]'; shot 06-float
key Super+v
key Super+Shift+f 1.2; windows | jq -c '[.windows[] | {app, fs, rect}]'; shot 07-fullscreen
key Super+Shift+f 1.2

step "workspace 3 by socket, move a window there, back by Super+Tab"
ipc workspace.focus '{"id":"3"}' >/dev/null; sleep 0.3; shot 08-switching; sleep 0.8
windows | jq -c '.ws'; shot 09-ws3
key Super+Tab 1; windows | jq -c '.ws'
first=$(ipc state.get | jq -r '.windows[0].id')
out=$(ipc state.get | jq -r '.focusedOutput')
ipc window.moveTo "{\"id\":\"$first\",\"idx\":3,\"output\":\"$out\"}" >/dev/null; sleep 1
windows | jq -c '[.windows[] | {app, ws}]'

step zen
key Super+z 1.2; ipc state.get | jq -c '[.workspaces[] | select(.active) | {id, zen: (.zen // "" | .[0:6])}]'
windows | jq -c '[.windows[] | {app, rect}]'; shot 10-zen
key Super+z 1.2

step "overview: open, walk, click a window"
key Super+o 1.5; windows | jq -c '.overview'; shot 11-overview
key Down 1.2; shot 12-overview-down; windows | jq -c '.ws'
key Up 1.2
wlrctl pointer move -5000 -5000; wlrctl pointer move 420 330; wlrctl pointer click left; sleep 1.5
windows | jq -c '{overview, focus, ws}'; shot 13-after-click

step "overlay: a console parked, shown, parked"
foot -a dev.formalshell.console >/dev/null 2>&1 & sleep 1.5
windows | jq -c '[.windows[] | select(.ov != null)]'
console=$(ipc state.get | jq -r '.windows[] | select(.overlay != null) | .id')
ipc window.place "{\"id\":\"$console\",\"x\":112,\"y\":40,\"width\":800,\"height\":420}" >/dev/null
ipc overlay.show "{\"id\":\"$console\"}" >/dev/null; sleep 1.2
windows | jq -c '{focus, console: [.windows[] | select(.ov != null) | {shown, rect}]}'; shot 14-console
ipc overlay.park "{\"id\":\"$console\",\"overlay\":\"formalshell-console\"}" >/dev/null; sleep 1.2
windows | jq -c '{focus, console: [.windows[] | select(.ov != null) | {shown}]}'; shot 15-parked

step "theme: gaps 16, border red, no motion"
ipc theme.set '{"colors":{"primary":"#ff0000"},"chrome":{"gapsIn":8,"gapsOut":16,"borderSize":4,"rounding":0}}' >/dev/null; sleep 1.2
ipc state.get | jq -c '.chrome'; windows | jq -c '[.windows[] | .rect]'; shot 16-theme

step "state.changed while idle and across one focus change"
( ipc state.get --listen 3 | grep -c state.changed ) & listener=$!
sleep 1; key Super+l 0.3; wait "$listener"

step "reload: a good file, then four the engine refuses: an action it has not, a key no keymap has, an env name and an env value that cannot be set"
( ipc state.get --listen 7 | jq -c 'select(.event != null and .event != "state.changed")' ) & listener=$!
sleep 0.5
printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1
cp "$XDG_CONFIG_HOME/formalwm/settings.json" "$XDG_CONFIG_HOME/formalwm/settings.json.keep"
for bad in '{"binds": [{"chord": "SUPER+Return", "action": "no-such-action"}]}' \
  '{"binds": [{"chord": "SUPER+Retrun", "action": "terminal"}]}' '{"env": {"A=B": "1"}}' '{"env": {"A": "x\u0000y"}}'; do
  echo "$bad" > "$XDG_CONFIG_HOME/formalwm/settings.json"
  printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1
done
mv "$XDG_CONFIG_HOME/formalwm/settings.json.keep" "$XDG_CONFIG_HOME/formalwm/settings.json"
wait "$listener"
windows | jq -c '[.windows[] | {app, ws, c}]'
key Super+Return 1.5; windows | jq -c '.windows | length'

step "close them all"
for _ in 1 2 3 4 5; do key Super+q 0.6; done
windows | jq -c '.windows | length'; shot 17-empty
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx" | cut -c1-300 | tail -15
