#!/bin/bash
# The window rules and the pointer on the rig. fwm-drag stands in for the
# apps the rules name: it takes any app id, title and size, and `--fixed`
# makes it what a dialog is to the compositor.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
ws() { ipc state.get | jq -r '.focusedWorkspaceId'; }
row() {
  ipc state.get | jq -r --arg app "$1" --arg title "${2:-}" '.windows[] | select(.appId == $app and ($title == "" or .title == $title))
    | "\(.appId)|\(.title) ws \(.workspaceId) c\(.column)\(if .floating then " float" else "" end) [\(.rect.x),\(.rect.y) \(.rect.width)x\(.rect.height)]"'
}
hidden() { ipc state.get | jq -c '.cursorHidden'; }

step "the pointer is hidden until the shell arrives, ten seconds at the most"
hand at 400 300; sleep 0.3; echo "at the start: hidden $(hidden)"; grim -c "$run/60-cursor-hidden.png"
foot -a one >/dev/null 2>&1 & sleep 1.2
foot -a two >/dev/null 2>&1 & sleep 1.2

step "the pointer over another window does not focus it, and that window hears the pointer"
win hover > "$run/hover.log"; sleep 1.2; hand at $(centre two) press release; sleep 0.5; focus_app
hand at $(centre hover); sleep 0.5; focus_app; grep -c enter "$run/hover.log"
hand at $(centre hover) press release; sleep 0.5; focus_app
ipc window.close "{\"id\":\"$(id_of hover)\"}" >/dev/null; sleep 0.8

step "an app's window takes the view to its workspace"
for app in Helium code dev.zed.Zed Slack Beeper obsidian Claude kopuz dev.formalsnake.formalmusic chrome-music.youtube.com__-Default; do
  ipc workspace.focus '{"id":"2"}' >/dev/null; sleep 0.4
  win "$app" >/dev/null; sleep 1; echo "$(row "$app") -> view on $(ws)"
done
ipc workspace.focus '{"id":"2"}' >/dev/null; sleep 0.4
win "" --title Messages >/dev/null; sleep 1; echo "$(row "" Messages) -> view on $(ws)"
ipc workspace.focus '{"id":"2"}' >/dev/null; sleep 0.4
win heliumx >/dev/null; sleep 1; echo "$(row heliumx) -> view on $(ws): the whole id has to match"

step "Beeper maps floating and is tiled all the same; a dialog of no rule floats at its own size"
win Beeper --title beeper-fixed --fixed --size 500x400 >/dev/null; sleep 1; row Beeper beeper-fixed
ipc workspace.focus '{"id":"2"}' >/dev/null; sleep 0.4
win dialog --fixed --size 320x200 > "$run/dialog.log"; sleep 1.2; row dialog; sort "$run/dialog.log" | uniq -c | tr '\n' ' '; echo
shot 61-dialog

step "resize on the border: the dialog's right edge, then a tile's"
hand at 674 384 press glide 754 384 300 wait 100 release; sleep 0.8; row dialog
ipc window.close "{\"id\":\"$(id_of dialog)\"}" >/dev/null; sleep 0.8
ipc workspace.focus '{"id":"1"}' >/dev/null; sleep 1; hand at $(centre two) press release; sleep 0.5
row one; row two; row Helium
edge=$(ipc state.get | jq -r '.windows[] | select(.appId == "two") | .rect | "\(.x + .width + 1) \(.y + .height / 2 | floor)"')
hand at $edge press glide 600 384 300 wait 100 release; sleep 0.8; row one; row two; row Helium
ipc workspace.focus '{"id":"2"}' >/dev/null; sleep 1

step "Sushi floats centred at the size it chose"
win org.gnome.NautilusPreviewer --size 300x200 >/dev/null; sleep 1.2; row org.gnome.NautilusPreviewer
ipc window.close "{\"id\":\"$(id_of org.gnome.NautilusPreviewer)\"}" >/dev/null; sleep 0.8

step "a window with Atoll's tag floats at its own size; the same app without it tiles"
win com.apple.TextEdit --tag atoll --size 420x260 >/dev/null; sleep 1.2; row com.apple.TextEdit
ipc window.close "{\"id\":\"$(id_of com.apple.TextEdit)\"}" >/dev/null; sleep 0.8
win com.apple.TextEdit --size 420x260 >/dev/null; sleep 1.2; row com.apple.TextEdit
ipc window.close "{\"id\":\"$(id_of com.apple.TextEdit)\"}" >/dev/null; sleep 0.8

step "picture in picture: floating, 28%, bottom right, on every workspace"
win "" --title "Picture in picture" --size 800x600 >/dev/null; sleep 1.2; row "" "Picture in picture"
shot 62-pip; ipc workspace.focus '{"id":"6"}' >/dev/null; sleep 1.2
drawn | jq -c '[.[] | select(.app == "" and .w < 400)]'; shot 63-pip-on-6

step "a helium popup that maps floating: the same"
win helium --title popup --fixed --size 600x500 >/dev/null; sleep 1.2; row helium popup; ws

step "Chrome's notification, no id and no title: floating at its size, top right, pinned"
win "" --title "" --size 360x120 >/dev/null; sleep 1.2
ipc state.get | jq -r '.windows[] | select(.appId == "" and .title == "") | "ws \(.workspaceId) float \(.floating) [\(.rect.x),\(.rect.y) \(.rect.width)x\(.rect.height)]"'
ipc workspace.focus '{"id":"6"}' >/dev/null; sleep 1.2; shot 64-notification
drawn | jq -c '[.[] | select(.app == "" and .w < 400)]'

step "ten seconds in, the pointer shows"
hand at 400 300; sleep 0.3; echo "hidden $(hidden)"; grim -c "$run/65-cursor-shown.png"
echo "the pointer's picture against the first shot: $(px diff "$run/60-cursor-hidden.png" "$run/65-cursor-shown.png" 396 296 30 30)"

step "over a Steam game it hides after three still seconds, and only there"
ipc workspace.focus '{"id":"9"}' >/dev/null; sleep 0.6
win steam_app_1234 >/dev/null; sleep 1.2; row steam_app_1234; focus_app
hand at 500 400 glide 520 420 100; sleep 0.5; grim -c "$run/66-game-pointer.png"
sleep 3.5; grim -c "$run/67-game-pointer-idle.png"
echo "gone when it rests: $(px diff "$run/66-game-pointer.png" "$run/67-game-pointer-idle.png" 516 416 30 30)"
hand glide 540 440 100; sleep 0.3; grim -c "$run/68-game-pointer-moved.png"
echo "back when it moves: $(px diff "$run/67-game-pointer-idle.png" "$run/68-game-pointer-moved.png" 536 436 30 30)"
ipc workspace.focus '{"id":"2"}' >/dev/null; sleep 1; hand at $(centre two) press release glide 300 300 100; sleep 0.4
grim -c "$run/69-plain-pointer.png"; sleep 3.5; grim -c "$run/70-plain-pointer-later.png"
echo "over a terminal it stays: $(px diff "$run/69-plain-pointer.png" "$run/70-plain-pointer-later.png" 296 296 30 30)"

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme\|smithay::backend\|backend::tty\|XWayland\|xwayland" | cut -c1-300 | tail -12
