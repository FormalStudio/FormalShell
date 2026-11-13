#!/bin/bash
# The panel edge round windows (dev/check.sh). With FWM_SHELL naming
# FormalShell: in dark and light, a window focused and one not beside the
# shell's audio card. EDGE_THEME names a program that turns the shell's
# theme.json on stdin into theme.set's params, for a shell older than the
# edge. With FWM_TIMING set, the core's frame times over twelve windows: tape
# scrolls with the stroke crossing over, and the overview.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
measured() { intervals; [ -z "${SHOJI_ANIMATION_TIMING_DEBUG:-}" ] || timing "$1" "$(loglen)"; }
open() { for app in "$@"; do foot -a "$app" >/dev/null 2>&1 & sleep 1.2; done; }

if [ -n "${FWM_SHELL:-}" ]; then
  sleep 8
  open left right
  for mode in dark light; do
    step "$mode"
    "$FWM_SHELL" theme mode "$mode" >/dev/null 2>&1; sleep 3
    if [ -n "${EDGE_THEME:-}" ]; then
      json=$(find "$HOME/.local/state/formalshell" -name theme.json 2>/dev/null | head -1)
      ipc theme.set "$("$EDGE_THEME" < "$json")" >/dev/null; sleep 1
    fi
    ipc state.get | jq -c '.chrome | {borderSize, activeBorder}'
    key Super+h 1.2; shot "edge-$mode-left-focused"
    "$FWM_SHELL" ipc --any-display call panel open audio >/dev/null 2>&1; sleep 2
    shot "edge-$mode-panel"
    key Escape 1; key Super+l 1.2; shot "edge-$mode-right-focused"
  done
  exit 0
fi

step "twelve windows"
[ -z "${EDGE_THEME:-}" ] || ipc theme.set "$(echo '{"mode": "dark"}' | "$EDGE_THEME")" >/dev/null
open a b c d e f g h i j k l; sleep 1
ipc state.get | jq -c '{windows: (.windows | length), border: .chrome.borderSize, active: .chrome.activeBorder}'
step "tape scrolls, the stroke crossing over on each"
mark; from=$(loglen)
for _ in 1 2 3; do for _ in 1 2 3 4; do key Super+h 0.6; done; for _ in 1 2 3 4; do key Super+l 0.6; done; done
measured "$from"
step "the overview, every window in it"
mark; from=$(loglen)
for _ in 1 2 3; do key Super+o 1.2; key Escape 1.2; done
measured "$from"
shot edge-many
