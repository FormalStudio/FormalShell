#!/bin/bash
# Outputs with no connector, made and removed over the socket while the
# session runs.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
outs() { ipc state.get | jq -c '[.outputs[] | {name, x, y, width, height, scale, enabled}]'; }
stacks() { ipc state.get | jq -c '[.workspaces[] | select(.output != "Virtual-4" or .active) | {id, output, active}]'; }
where() { ipc state.get | jq -c '[.windows[] | {app: .appId, ws: .workspaceId, rect: (.rect | [.x, .y, .width, .height])}]'; }

step "one output, one window"
open one; outs

step "outputs.create: named by the engine, right of the others at y 0, a workspace of its own"
( ipc state.get --listen 3 | grep -c state.changed ) & listener=$!
sleep 0.5; made=$(ipc outputs.create '{"width":1280,"height":720}'); echo "$made"; wait "$listener"
first=$(jq -r '.name' <<< "$made")
outs; stacks
ipc outputs.get | jq -c '.[] | {name, enabled, x, y, width, height, refresh, scale, modes: (.modes | length)}'

step "a second one with a name and a scale, right of the first"
ipc outputs.create '{"name":"stream","width":1920,"height":1080,"scale":2}'; sleep 1.5; outs

step "what is refused"
ipc outputs.create '{"name":"stream","width":800,"height":600}' 2>&1
ipc outputs.create '{"width":0,"height":600}' 2>&1
ipc outputs.remove '{"name":"Virtual-4"}' 2>&1
ipc outputs.remove '{"name":"nope"}' 2>&1

step "a window sent to the first one is drawn there"
ipc window.moveTo "{\"id\":\"$(id_of one)\",\"idx\":1,\"output\":\"$first\"}" >/dev/null; sleep 1.5
where; grim -o "$first" "$run/50-made-output.png"
echo "the made output's picture: $(python3 -c "
from PIL import Image
im = Image.open('$run/50-made-output.png').convert('RGB'); print(im.size, 'the terminal at its middle:', im.getpixel((640, 360)), 'the desk at its edge:', im.getpixel((2, 360)))")"

step "a settings reload keeps them"
printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1.5; outs

step "outputs.set enabled on a made output: off, its workspace and window go to the first output; on, they go back"
ipc outputs.set "{\"name\":\"$first\",\"enabled\":false}" >/dev/null; sleep 1.5; where; stacks
ipc outputs.set "{\"name\":\"$first\",\"enabled\":true}" >/dev/null; sleep 2; where; stacks

step "outputs.set mirror: the core is given the source and outputs.get reports it; cleared, the output has its workspace and window again. A made output has no scanout, so its capture shows no mirrored picture"
mirrors() { ipc outputs.get | jq -c '[.[] | {name, mirrorOf}]'; }
middle() { python3 -c "
from PIL import Image
im = Image.open('$run/$1.png').convert('RGB'); w, h = im.size; print(im.size, im.getpixel((w // 2, h // 2)))"; }
open two; sleep 0.5
grim -o Virtual-4 "$run/51-source.png"; echo "the first output: $(middle 51-source)"
ipc outputs.set "{\"name\":\"$first\",\"mirror\":\"Virtual-4\"}" 2>&1; sleep 2; mirrors; where; stacks
grim -o "$first" "$run/52-mirror.png"; echo "the made output, mirroring: $(middle 52-mirror)"
ipc outputs.set "{\"name\":\"$first\",\"mirror\":\"\"}" 2>&1; sleep 2; mirrors; where
grim -o "$first" "$run/53-own-again.png"; echo "the made output, its own again: $(middle 53-own-again)"

step "the host pins workspaces 4 and 8 to a made output: a reload moves them there, and Slack, sent to 4 by its rule, opens on it"
sed -i 's/"outputs": \[/"outputs": [{"name": "stream", "workspaces": [4, 8], "defaultWorkspace": 4}, /' "$XDG_CONFIG_HOME/formalwm/host.json"
printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1.5
ipc state.get | jq -c '[.workspaces[] | select(.output == "stream") | {id, active}]'
open Slack; ipc state.get | jq -c '{focusedOutput, ws: .focusedWorkspaceId, slack: [.windows[] | select(.appId == "Slack") | {ws: .workspaceId, x: .rect.x}]}'

step "outputs.remove: its window goes to the output that is left"
ipc outputs.remove "{\"name\":\"$first\"}"; sleep 1.5; outs; where; stacks
ipc outputs.remove '{"name":"stream"}'; sleep 1.5; outs; where
ipc outputs.remove '{"name":"stream"}' 2>&1

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -8
