#!/bin/bash
# What the host file decides, on the rig: started on dev/host-default.json
# (FWM_HOST), then reloaded onto the g815's own file, whose monitors are
# not here, so the rig's output falls to the rule for an unlisted display.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
wm=$(cd "$(dirname "$0")/.." && pwd)
mode() { ipc outputs.get | jq -c '.[] | {name, width, height, refresh, scale, best: (.modes | max_by(.refresh, .width * .height) | "\(.width)x\(.height)@\(.refresh)")}'; }
step "the output's default workspace is the one in view at the start; the mode is the pinned one"
ipc state.get | jq -c '{ws: .focusedWorkspaceId, keyboard, pinned: [.workspaces[] | "\(.id):\(.output)"] | join(" ")}'
mode
foot -a one >/dev/null 2>&1 & sleep 1.2
layer wallpaper background fill 0 0 --color ffffffff; sleep 0.6
grim "$run/95-opaque.png"; echo "inside the terminal, opaque: $(px mean "$run/95-opaque.png" 500 500 40 40)"
step "reloaded onto the g815's file: highrr for a display it does not list, 0.96 windows, its environment"
cp "$wm/example/hosts/g815/host.json" "$XDG_CONFIG_HOME/formalwm/host.json"
( ipc state.get --listen 3 | jq -c 'select(.event != null and .event != "state.changed")' ) & listener=$!
sleep 0.4; printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; wait "$listener"
sleep 1.5; mode
ipc state.get | jq -c '{outputs, windows: [.windows[] | {app: .appId, rect}]}'
grim "$run/96-translucent.png"
echo "inside the terminal, 0.96 over white: $(px mean "$run/96-translucent.png" 500 500 40 40)"
ipc spawn "{\"argv\":[\"sh\",\"-c\",\"echo \$LIBVA_DRIVER_NAME \$NVD_BACKEND \$XCURSOR_THEME \$QT_QPA_PLATFORMTHEME > $run/env\"]}" >/dev/null; sleep 0.8; cat "$run/env"
step "a reload takes the keyboard's layout too, and the output back to its pinned mode"
echo '{"terminal": "foot", "input": {"keyboard": {"layout": "us"}}, "outputs": [{"name": "Virtual-4", "mode": "1024x768@60", "scale": 1}]}' > "$XDG_CONFIG_HOME/formalwm/host.json"
printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1.5
ipc state.get | jq -c '{keyboard, windows: [.windows[] | {app: .appId, rect}]}'; mode
step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme\|smithay::backend\|backend::tty\|XWayland\|xwayland" | cut -c1-300 | tail -12
