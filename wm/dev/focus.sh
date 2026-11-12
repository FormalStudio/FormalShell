#!/bin/bash
# The keyboard and the state on the rig: a window picked through
# `window.focus` has the keyboard whichever of the two opened first, a
# press on the other moves both, the column binds, a window mapping and
# closing and a workspace switch move both together, and `bind.escape` runs
# its command with the Escape still the focused client's. The windows are
# fwm-drag's, which print the keys they are sent by their evdev codes: 30 A,
# 1 Escape. Every press checks that the one window that heard that key is
# the one `state.get` names, or none when it names none, and the session
# fails if they ever disagree. A bind's Super goes down in the window focused
# before the bind ran, so only the pressed key counts.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR

apps=(older newer)
declare -A seen=([older]=0 [newer]=0)
disagreed=0
# The keys a window was sent since the last time, into $keys.
heard() {
  keys=$(tail -n +$((${seen[$1]:-0} + 1)) "$run/$1.out" | grep '^key' | cut -d' ' -f2 | tr '\n' ' ')
  seen[$1]=$(wc -l < "$run/$1.out")
}
press() {
  key "$1" 0.6
  local app line= heard_by= said code
  case $1 in a) code=30 ;; Escape) code=1 ;; esac
  for app in "${apps[@]}"; do
    heard "$app"
    line+=$(printf '%s heard: %-9s ' "$app" "${keys:-nothing}")
    [[ " $keys" != *" $code "* ]] || heard_by+="$app "
  done
  said=$(focus_app)
  if [ "${heard_by% }" = "${said/#null/}" ]; then said+=", agreed"; else said+=", DISAGREED"; disagreed=1; fi
  printf '%-8s %sthe state says: %s\n' "$1" "$line" "$said"
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

step "SUPER+L and SUPER+H, the column binds, one way and back"
key Super+l; press a
key Super+h; press a

step "a window that maps takes the keyboard and the state"
win third > "$run/third.out"; apps+=(third); sleep 1.2
press a

step "the window closing gives both back to one window"
ipc window.close "{\"id\": \"$(id_of third)\"}" >/dev/null; sleep 1.2
press a

step "to an empty workspace and back"
key Super+code:11; press a
key Super+code:10; press a

step "bind.escape: the command runs and the Escape reaches the window"
ipc bind.escape "{\"name\": \"card\", \"argv\": [\"touch\", \"$run/escaped\"]}" >/dev/null; sleep 0.4
press Escape
[ -e "$run/escaped" ] && echo "the command ran" || echo "the command did not run"
ipc bind.escape '{"name": "card", "argv": null}' >/dev/null; sleep 0.4; rm -f "$run/escaped"
press Escape
[ -e "$run/escaped" ] && echo "unbound, the command ran all the same" || echo "unbound, nothing ran"

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme" | cut -c1-300 | tail -12
[ "$disagreed" = 0 ] || { echo "the keyboard and the state disagreed"; exit 1; }
