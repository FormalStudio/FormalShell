#!/bin/bash
# The second scripted session on the rig: layers, the pointer's drags, the
# rest of the socket, binds that repeat and release, and the way out.
set -u
. "$(dirname "$0")/lib.sh"
cols() { ipc state.get | jq -c '[.windows[] | {app: .appId, ws: .workspaceId, c: .column, fl: .floating, rect: (.rect | [.x, .y, .width, .height])}]'; }
out=$(ipc state.get | jq -r '.focusedOutput')

step "the start script ran once the socket was up"
sleep 1; cat "$XDG_CONFIG_HOME/started"

step "layers: a wallpaper, a bar reserving 36 px, an overlay surface"
layer wallpaper background fill 0 0 --color ff203040; sleep 0.4
layer formalshell-bar top top 0 36 --exclusive 36 --color cc101010; sleep 0.4
layer osd overlay none 200 80 --color ffaa3300; osd=$!; sleep 1
ipc layers.list | jq -c '.[]'; ipc state.get | jq -c '.outputs[] | {name, reserved}'
for app in one two three; do foot -a "$app" >/dev/null 2>&1 & sleep 1.2; done
cols; shot 20-layers
kill "$osd"; sleep 0.8; ipc layers.list | jq -c '[.[] | .namespace]'

step "cursor.pos, cursor.hide"
hand at 300 200; ipc cursor.pos
ipc cursor.hide '{"hidden":true}' >/dev/null; sleep 0.3; ipc state.get | jq -c '.cursorHidden'
ipc cursor.hide '{"hidden":false}' >/dev/null; sleep 0.3; ipc state.get | jq -c '.cursorHidden'

step "a click focuses the window under it"
focus_app; hand at $(centre two) press release; sleep 0.6; focus_app

step "Super+drag: a tile to the other side of its neighbour"
hold Super 2500
hand at $(centre three) press glide 300 400 300 wait 700 glide 60 400 300 wait 100 release & drag=$!
sleep 0.9; shot 21-dragging; wait "$drag"; sleep 1.2
cols; shot 22-dropped

step "Super+drag: a float follows the hand, Super+right-drag resizes it"
hand at $(centre three) press release; sleep 0.4; key Super+v 1; cols | jq -c '[.[] | select(.fl)]'
hold Super 1500
hand at $(centre three) press glide 700 500 400 wait 100 release; sleep 1.2; cols | jq -c '[.[] | select(.fl)]'
hold Super 1500
hand at 760 560 press right glide 840 600 400 wait 100 release right; sleep 1.2; cols | jq -c '[.[] | select(.fl)]'
shot 23-float-moved; key Super+v 1

step "a tile held in the corner opens the overview; held over the next workspace it goes there, and lands there"
hold Super 5000
t0=$(date +%s%3N); opened=never
hand at $(centre two) press glide 2 2 500 wait 1300 glide 512 720 600 wait 700 release & drag=$!
for _ in $(seq 16); do
  if [ "$(ipc state.get | jq -c '.overview')" = true ]; then opened="$(($(date +%s%3N) - t0)) ms"; break; fi
  sleep 0.1
done
echo "the overview opened $opened after the hand set off for the corner"; shot 24-corner; wait "$drag"; sleep 1.5
ipc state.get | jq -c '{overview, ws: .focusedWorkspaceId}'; cols | jq -c '[.[] | {app, ws, c}]'; shot 25-sprung

step "the same, let go at once over the next workspace: it lands there and the overview stays"
ipc workspace.focus '{"id":"1"}' >/dev/null; sleep 1
hold Super 5000
hand at $(centre three) press glide 2 2 500 wait 1300 glide 512 720 120 release; sleep 1.5
ipc state.get | jq -c '{overview, ws: .focusedWorkspaceId}'; cols | jq -c '[.[] | {app, ws, c}]'; shot 26-dropped-in-overview

step "overview: a window dropped between two workspaces gets a new one"
hand at $(lensed one) press glide 512 591 500 wait 300 & drag=$!
sleep 0.6; shot 27-overview-drag; wait "$drag"; hand release; sleep 1.5
ipc state.get | jq -c '[.workspaces[] | {id, idx}] | .[0:4]'; cols | jq -c '[.[] | {app, ws, c}]'; shot 28-new-workspace

step "overview: Down walks to it; its window dropped on the next workspace's panel joins that tape"
key Down 1.2; ipc state.get | jq -c '.focusedWorkspaceId'
hand at $(lensed one) press glide 600 720 500 wait 100 release; sleep 1.5
cols | jq -c '[.[] | {app, ws, c}]'

step "overview: the wheel walks, Escape closes"
wlrctl pointer scroll 120 0; sleep 1; ipc state.get | jq -c '.focusedWorkspaceId'
wlrctl pointer scroll 0 120; sleep 1; ipc state.get | jq -c '.focusedWorkspaceId'
key Escape 1.2; ipc state.get | jq -c '{overview, ws: .focusedWorkspaceId}'; shot 29-closed

step "socket: focusAt, spawn, swap, moveColumn, float, fullscreen, close"
ipc workspace.focusAt '{"idx":6}' >/dev/null; sleep 1; ipc state.get | jq -c '.focusedWorkspaceId'
ipc spawn '{"argv":["foot","-a","four"],"cwd":"/tmp"}' >/dev/null; sleep 1.2
ipc spawn '{"argv":["foot","-a","five"]}' >/dev/null; sleep 1.2
here() { cols | jq -c '[.[] | select(.app == "four" or .app == "five") | {app, c, fl}]'; }
here
ipc window.swap "{\"a\":\"$(id_of four)\",\"b\":\"$(id_of five)\"}" >/dev/null; sleep 0.6; here
ipc window.moveColumn "{\"id\":\"$(id_of five)\"}" >/dev/null; sleep 0.6; here
ipc window.float "{\"id\":\"$(id_of four)\",\"floating\":true}" >/dev/null; sleep 0.6; here
ipc window.fullscreen "{\"id\":\"$(id_of five)\"}" >/dev/null; sleep 1.2
ipc state.get | jq -c '[.windows[] | select(.appId == "five") | {fullscreen, rect}]'
ipc window.fullscreen "{\"id\":\"$(id_of five)\"}" >/dev/null; sleep 1.2
ipc window.close "{\"id\":\"$(id_of four)\"}" >/dev/null; sleep 1; here
ipc window.focus '{"id":"no-such-window"}' 2>&1

step "outputs.get, scale 2 and back, an output that is not there"
ipc outputs.get | jq -c '.[] | {name, enabled, x, y, width, height, refresh, scale, transform, modes: (.modes | length)}'
ipc outputs.set "{\"name\":\"$out\",\"scale\":2}" >/dev/null; sleep 1.5
ipc state.get | jq -c '.outputs'; cols | jq -c '[.[] | select(.app == "five") | .rect]'; shot 30-scale2
ipc outputs.set "{\"name\":\"$out\",\"scale\":1}" >/dev/null; sleep 1.5
ipc state.get | jq -c '.outputs'; cols | jq -c '[.[] | select(.app == "five") | .rect]'
ipc outputs.set '{"name":"nope","scale":2}' 2>&1

step "binds from a reloaded file: one that repeats, one that does not, one on release; bind.escape"
run=$SHOJI_RUN_DIR
cat > "$run/binds" <<BINDS
    {"chord": "SUPER+F9", "action": "spawn", "args": ["sh", "-c", "echo r >> $run/repeat"], "repeat": true},
    {"chord": "SUPER+F10", "action": "spawn", "args": ["sh", "-c", "echo o >> $run/once"]},
    {"chord": "ALT+F8", "action": "spawn", "args": ["sh", "-c", "echo next >> $run/alt"]},
    {"chord": "ALT+Alt_L", "action": "spawn", "args": ["sh", "-c", "sleep 0.2; echo commit >> $run/alt"], "release": true},
BINDS
sed -i "/\"binds\": \[/r $run/binds" "$XDG_CONFIG_HOME/formalwm/settings.json"
( ipc state.get --listen 2 | jq -c 'select(.event != null and .event != "state.changed")' ) & listener=$!
sleep 0.4; printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; wait "$listener"
cols | jq -c 'length'
hold Super+F9 800; sleep 1.2; echo "repeat: $(wc -l < "$run/repeat")"
hold Super+F10 800; sleep 1.2; echo "once: $(wc -l < "$run/once")"
key Alt+F8 1; tr '\n' ' ' < "$run/alt"; echo
ipc bind.escape "{\"name\":\"menu\",\"argv\":[\"sh\",\"-c\",\"echo esc >> $run/esc\"]}" >/dev/null
escapes() { ipc binds.list | jq -c '[.[] | select(.key == "Escape" and .mods == []) | {action, args: (.args | length)}]'; }
echo "binds.list while it is bound: $(escapes)"
key Escape 0.8; ipc bind.escape '{"name":"menu"}' >/dev/null; key Escape 0.8; echo "escape: $(wc -l < "$run/esc"), binds.list after: $(escapes)"

step "a window sent away without the view following gives the keyboard up, and a layer that took the keyboard in between shows no workspace"
heard() { echo "$1: $(ipc state.get | jq -c '. as $s | {ws: .focusedWorkspaceId, focus: ([.windows[] | select(.id == $s.focusedWindowId) | .appId][0])}') the terminal heard: $(tr '\n' ' ' < "$run/kb")"; }
empty=($(ipc state.get | jq -r '[.windows[].workspaceId] as $used | [.workspaces[] | select(.id as $id | $used | index($id) | not) | .idx] | .[0:2] | .[]'))
ipc workspace.focusAt "{\"idx\":${empty[0]}}" >/dev/null; sleep 1
foot -a kb sh -c "cat >> $run/kb" >/dev/null 2>&1 & sleep 1.5
key a 0.2; key Return 0.5; heard "in view"
ipc window.moveTo "{\"id\":\"$(id_of kb)\",\"idx\":${empty[1]},\"output\":\"$out\"}" >/dev/null; sleep 1
key b 0.2; key Return 0.5; heard "sent away"
layer launcher overlay none 300 200 --keyboard exclusive; sheet=$!; sleep 1
key c 0.2; key Return 0.5; kill "$sheet"; sleep 1
key d 0.2; key Return 0.5; heard "a layer came and went"
ipc workspace.focusAt "{\"idx\":${empty[1]}}" >/dev/null; sleep 1
key e 0.2; key Return 0.5; heard "its workspace in view"

step "the log, then session.exit"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|unknown key .code:" | cut -c1-300 | tail -12
ipc session.exit; sleep 2
test -S "$SHOJI_IPC_SOCKET" && echo "the socket is still there" || echo "the socket is gone"
echo "formalwm processes left: $(pgrep -u "$USER" -c formalwm)"
