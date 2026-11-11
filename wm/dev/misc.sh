#!/bin/bash
# What the other sessions leave out (FWM_HOST=host-misc.json, windows at
# 0.8, vrr 2): variable refresh, the Caps Lock key as Escape, a window that
# asks for the focus, the pointer left where it is, the blur behind a window
# and its shadow.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR

step "vrr 2: asked of the core only while a window is fullscreen; the rig's connector cannot do it, which the core says each time it is asked"
vrr() { echo "$1: vrr $(ipc outputs.get | jq -c '.[0].vrr'), refused $(grep -c "not vrr_capable" "$SHOJI_RUN_LOG") times"; }
full() { key Super+Shift+f 1; vrr "$1 fullscreen"; key Super+Shift+f 1; vrr "$1 back"; key Super+q 1; }
open plain; vrr "a window"; full plain

step "vrr 3 after a reload: only for a fullscreen window that is a game or video, by a rule or by what the client declares"
sed -i 's/"vrr": 2/"vrr": 3/' "$XDG_CONFIG_HOME/formalwm/host.json"
printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1.5
open plain; full plain
win steam_app_1 > /dev/null; sleep 1.5; full "steam_app_1, a game by its rule"
win clip --content video > /dev/null; sleep 1.5; full "a client that declares video"
win album --content photo > /dev/null; sleep 1.5; full "a client that declares photo"

step "caps:escape: the Caps Lock key runs what Escape is bound to"
ipc bind.escape "{\"name\":\"misc\",\"argv\":[\"sh\",\"-c\",\"echo esc >> $run/esc\"]}" >/dev/null
"$SHOJI_BIN" --key code:66 >/dev/null; sleep 0.8; echo "escape ran $(wc -l < "$run/esc" 2>/dev/null || echo 0) time"
ipc bind.escape '{"name":"misc"}' >/dev/null

step "a window that asks for the focus gets it, and the view goes to its workspace"
open one; ipc workspace.focus '{"id":"2"}' >/dev/null; sleep 1; open two; focus_app
wlrctl toplevel focus app_id:one; sleep 1.2; focus_app; ipc state.get | jq -r '.focusedWorkspaceId'

step "a focus change leaves the pointer where it is"
open three four; hand at 700 500; ipc cursor.pos
key Super+h; key Super+h; focus_app; ipc cursor.pos

step "behind a window at 0.8 the desk is blurred; with the theme's blur off it shows through sharp"
foot -a text sh -c 'for i in $(seq 60); do echo "#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#=#="; done; sleep 300' >/dev/null 2>&1 & sleep 1.5
key Super+f 1.2
win glass --size 400x300 --fixed
for _ in $(seq 25); do [ -n "$(id_of glass)" ] && break; sleep 0.2; done; sleep 1.2; ipc state.get | jq -c '[.windows[] | select(.appId == "glass" or .appId == "text") | {app: .appId, fl: .floating, rect: (.rect | [.x, .y, .width, .height])}]'
read -r x y <<< "$(ipc state.get | jq -r '.windows[] | select(.appId == "glass") | .rect | "\(.x + 40) \(.y + 40)"')"
shot 97-blur
ipc theme.set '{"colors":{},"chrome":{"blur":false}}' >/dev/null; sleep 1; shot 98-no-blur
echo "sharpness of the text: $(px edge "$run/97-blur.png" 40 400 300 200) beside the window, $(px edge "$run/97-blur.png" "$x" "$y" 300 200) behind it blurred, $(px edge "$run/98-no-blur.png" "$x" "$y" 300 200) behind it with the blur off"

step "the theme's shadow round a floating window, white so it shows on the dark desk"
ipc theme.set '{"colors":{},"chrome":{"blur":false,"shadow":true,"shadowRange":16,"shadowPower":1,"shadowColor":"#ffffffff","shadowInactiveColor":"#ffffffff"}}' >/dev/null; sleep 1; shot 99-shadow
echo "just above the window: $(px mean "$run/98-no-blur.png" "$x" "$((y - 52))" 300 6) without, $(px mean "$run/99-shadow.png" "$x" "$((y - 52))" 300 6) with the shadow"

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -8
