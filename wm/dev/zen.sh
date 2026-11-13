#!/bin/bash
# Zen on the rig, focus move by focus move: each one takes zen to the window
# it lands on, the window before going back into its column.
set -u
. "$(dirname "$0")/lib.sh"
zen() { ipc state.get | jq -r '. as $s | [.workspaces[] | select(.zen != null) | . as $w | "\($w.id):" + ([$s.windows[] | select(.id == $w.zen) | .appId][0] // "?")] | join(" ")'; }
both() { echo "focus $(focus_app), zen $(zen)"; }
rect() { ipc state.get | jq -r --arg app "$1" '.windows[] | select(.appId == $app) | .rect | "\(.x) \(.y) \(.width) \(.height)"'; }
click_at() { wlrctl pointer move -5000 -5000; wlrctl pointer move "$1" "$2"; wlrctl pointer click left; sleep 0.8; }

step "three columns, SUPER+H to two, SUPER+Z"
open one two three; key Super+h; mark
key Super+z 0.2; shot zen-01-entering; sleep 1
both; tape; drawn; steps; shot zen-02-on-two

step "SUPER+H: zen goes to one, two slides back to its column off the right"
mark; key Super+h 0.2; shot zen-03-h-midway; sleep 1
both; tape; drawn; steps; path one x; path two x; shot zen-04-on-one

step "SUPER+L: back to two, one off the left"
mark; key Super+l 0.2; shot zen-05-l-midway; sleep 1
both; tape; steps; path one x; path two x; shot zen-06-on-two

step "SUPER+K and SUPER+J walk a stack: three taken into two's column first"
key Super+z 1.2; key Super+l; key Super+comma 1; key Super+z 1.2; both
mark; key Super+k 1.2; both; drawn; steps; path three y; shot zen-07-k-stack
key Super+j 1.2; both; key Super+z 1.2; key Super+Shift+comma 1; key Super+h; key Super+z 1.2; both

step "a float opened in zen has the keyboard over the zen window; a click on each"
foot -a org.gnome.NautilusPreviewer >/dev/null 2>&1 & sleep 1.5
both; shot zen-08-float
read -r x y w h < <(rect two); click_at $((x + 30)) $((y + 30)); both; shot zen-09-click-zen-window
read -r x y w h < <(rect org.gnome.NautilusPreviewer); click_at $((x + w / 2)) $((y + h / 2)); both; shot zen-10-click-float
key Super+q 1.2; both

step "the shell's switcher (window.focus) takes zen to one"
ipc window.focus "{\"id\":\"$(id_of one)\"}" >/dev/null; sleep 1.2; both; shot zen-11-switcher

step "a new window takes zen"
mark; open four; sleep 0.6; both; steps; shot zen-12-new-window

step "SUPER+Q on the zen window: zen goes to the window the focus goes to"
mark; key Super+q 0.25; shot zen-13-close-midway; sleep 1.2
both; tape; steps; shot zen-14-closed

step "SUPER+Z: the tape comes back"
key Super+z 1.4; both; tape; shot zen-15-off
