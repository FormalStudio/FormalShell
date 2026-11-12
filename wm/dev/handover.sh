#!/bin/bash
# `binds.handOver` on the rig: a window that stands for a Mac app is handed
# the Super chords while it has the keyboard, with every bind back when it
# has not, and the hand-over ends with the connection that asked for it.
# The window is fwm-drag's, which prints the keys it is sent by their evdev
# codes: 125 Super, 42 Shift, 44 Z, 33 F, 35 H. With FWM_SHELL naming a
# shell, the lock too.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
[ -z "${FWM_SHELL:-}" ] || sleep 8

held() { ipc state.get | jq -c '.handOver | if . == null then "none" else {window: .window[0:6], reserved, active} end'; }
zen() { ipc state.get | jq -r '[.workspaces[] | select(.focused) | .zen][0] | if . == null then "off" else "on" end'; }
# The keys the window was sent since the last time, into $keys.
seen=0
heard() {
  keys=$(tail -n +$((seen + 1)) "$run/mac.out" | grep '^key' | cut -d' ' -f2 | tr '\n' ' ')
  seen=$(wc -l < "$run/mac.out")
}
press() { key "$1" 0.8; heard; printf '%-14s the window heard: %-11s zen %s, focus %s, %s\n' "$1" "${keys:-nothing}" "$(zen)" "$(focus_app)" "$(held)"; }
hold_over() { ~/shoji-ipc binds.handOver "$1" --listen "${2:-90}" > "$run/holder.out" 2>&1 & holder=$!; sleep 0.6; }

step "two windows, the Mac one with a tag: the tag is in its row"
win native > "$run/native.out"; sleep 1.2
win mac --tag atoll-mac > "$run/mac.out"; sleep 1.2
ipc state.get | jq -c '[.windows[] | {app: .appId, tag, focused}]'
mac=$(id_of mac)

step "nothing held: Super+Z is the zen bind and the window never sees the Z"
press Super+z; press Super+z

step "held over the Mac window, Super+H and Super+L reserved: its chords reach it and the binds on them do not run"
hold_over "{\"on\": true, \"reserved\": [\"SUPER + H\", \"SUPER + L\"], \"window\": \"$mac\"}"
held
press Super+z; press Super+Shift+f
ipc state.get | jq -c '[.windows[] | select(.appId == "mac") | {fullscreen}]'

step "a reserved chord is still the bind's: the focus moves, and over the other window every bind is back"
press Super+h; press Super+z; press Super+z

step "the keyboard back on the Mac window: its chords are its own again, with nothing asked again"
press Super+l; press Super+z

step "a bind off Super stays bound all along: the Copilot key's focus-column"
press XF86Tools; press Super+l

step "a layer that takes the keyboard: every bind is back while it has it"
layer launcher overlay none 300 200 --keyboard exclusive --color cc000000; layer_pid=$!; sleep 1
press Super+z; ipc zen.set '{"on": false}' >/dev/null; sleep 0.5
kill "$layer_pid"; sleep 1; held

step "the overview: every bind is back while it is open"
ipc overview.set '{"open": true}' >/dev/null; sleep 1; held
ipc overview.set '{"open": false}' >/dev/null; sleep 1; held

step "a reload keeps it: the binds are bound again without the window's chords"
printf 'reload\n' | socat - UNIX-CONNECT:"$SHOJIWM_SOCKET"; sleep 1
press Super+z

if [ -n "${FWM_SHELL:-}" ]; then
  step "locked: not active, and the chord reaches neither the window nor the bind; unlocked, it is the window's again"
  "$FWM_SHELL" ipc --any-display call lock lock; sleep 2
  press Super+z
  # The Z went to the lock screen's password field.
  wtype -k BackSpace -s 300 formalshell-test -k Return; sleep 3
  "$FWM_SHELL" ipc --any-display call lock status | jq -c '{locked}'
  press Super+z
fi

step "asked out with another window named, as the shell does on a focus change: it ends"
ipc binds.handOver "{\"on\": false, \"window\": \"$(id_of native)\"}" >/dev/null; sleep 0.5
press Super+z; press Super+z
kill "$holder" 2>/dev/null

step "the connection that asked closes: it ends, and the binds are on the keys again"
hold_over "{\"on\": true, \"reserved\": []}" 90
held
press Super+z
kill "$holder"; sleep 0.8
press Super+z; press Super+z

step "the window closes: it ends"
hold_over '{"on": true}' 30
held
ipc window.close "{\"id\": \"$mac\"}" >/dev/null; sleep 1.2
held; ipc state.get | jq -c '[.windows[] | .appId]'
kill "$holder" 2>/dev/null

step "a window that is none is refused"
ipc binds.handOver '{"on": true, "window": "nobody"}'

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -12
