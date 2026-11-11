#!/bin/bash
# The layer rules on the rig: which namespaces get the blur behind them,
# and from what opacity of the layer's own. A black sheet of one alpha
# lies over a terminal full of text; how sharp the text still is under it
# says whether the backdrop was blurred.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
box="372 294 280 180"
foot -a text sh -c 'for n in $(seq 400); do printf "%s" "the quick brown fox jumps over 0123456789 "; done; sleep 600' >/dev/null 2>&1 & sleep 1.5
key Super+f 1
grim "$run/80-text.png"; base=$(px edge "$run/80-text.png" $box)
echo "the text alone: sharpness $base"
sheet() {
  local namespace=$1 alpha=$2 name=$3
  layer "$namespace" overlay none 300 200 --color "${alpha}000000"; local pid=$!
  sleep 1; grim "$run/$name.png"; kill "$pid"; sleep 0.5
  local edge; edge=$(px edge "$run/$name.png" $box)
  python3 - "$base" "$edge" "$alpha" "$namespace" <<'PY'
import sys
base, edge, alpha, namespace = float(sys.argv[1]), float(sys.argv[2]), int(sys.argv[3], 16) / 255, sys.argv[4]
plain = base * (1 - alpha)
print(f"{namespace} at alpha {alpha:.2f}: sharpness {edge}, {plain:.2f} if nothing were blurred -> {'blurred' if edge < plain * 0.5 else 'not blurred'}")
PY
}
step "0.2 for the bar's kind: a clear gap is left alone, anything denser is frosted"
sheet formalshell:bar 1a 81-bar-010
sheet formalshell:bar 66 82-bar-040
sheet formalshell:osd 66 83-osd-040
step "0.6 for the modal kind: a scrim is left alone, a card is frosted"
sheet formalshell:menu 66 84-menu-040
sheet formalshell:menu b3 85-menu-070
sheet formalshell:notifications 66 86-notifications-040
sheet formalshell:notifications b3 87-notifications-070
step "the veil and what the settings do not name get no blur"
sheet formalshell:veil b3 88-veil-070
sheet other b3 89-other-070
step "theme.set with blur off: nothing is frosted"
ipc theme.set '{"colors":{},"chrome":{"blur":false}}' >/dev/null; sleep 0.8; ipc state.get | jq -c '.chrome.blur'
sheet formalshell:menu b3 90-menu-070-no-blur
step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme\|smithay::backend\|backend::tty\|XWayland\|xwayland" | cut -c1-300 | tail -12
