#!/bin/bash
# The keyboard and the state on the rig: a window picked through
# `window.focus` has the keyboard whichever of the two opened first, and a
# press on the other moves both. The windows are fwm-drag's, which print
# the keys they are sent by their evdev codes: 30 is A.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR

declare -A seen=([older]=0 [newer]=0)
# The keys a window was sent since the last time, into $keys.
heard() {
  keys=$(tail -n +$((${seen[$1]} + 1)) "$run/$1.out" | grep '^key' | cut -d' ' -f2 | tr '\n' ' ')
  seen[$1]=$(wc -l < "$run/$1.out")
}
press() {
  key "$1" 0.6
  heard older; local older=${keys:-nothing}; heard newer
  printf '%-8s older heard: %-9s newer heard: %-9s the state says: %s\n' "$1" "$older" "${keys:-nothing}" "$(focus_app)"
}
pick() { ipc window.focus "{\"id\": \"$(id_of "$1")\"}" >/dev/null; sleep 0.6; }

step "two windows, neither used yet: the older one is picked and has the keyboard"
win older > "$run/older.out"; sleep 1.2
win newer > "$run/newer.out"; sleep 1.2
focus_app
pick older; press a

step "a press on the newer one: the keyboard and the state are on it"
hand at $(centre newer) wait 300 press release; sleep 0.8
press a

step "picked back and forth"
pick older; press a
pick newer; press a

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -12
