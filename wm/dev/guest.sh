#!/bin/bash
# The guest's half of dev/check.sh: a session of the last formalwm built by
# dev/rig.sh on the owner's settings from wm/example, with foot for the
# terminal, running the script named in $1.
set -euo pipefail
export PATH="/opt/fspec/current/bin:$PATH"
wm=$(cd "$(dirname "$0")/.." && pwd)
config=$HOME/fwm-config
rm -rf "$config" && mkdir -p "$config/formalwm"
cp "$wm/example/settings.json" "$config/formalwm/"
echo '{"terminal": "foot"}' > "$config/formalwm/host.json"
cat > "$config/formalwm/formalshell-start.sh" <<'START'
test -S "$XDG_RUNTIME_DIR/shojiwm-$WAYLAND_DISPLAY.sock" && up=up || up=missing
echo "display $WAYLAND_DISPLAY, socket $up" > "$XDG_CONFIG_HOME/started"
START
export XDG_CONFIG_HOME="$config" SHOJI_BIN="$HOME/fwm-target/release-fast/formalwm" SHOJI_RUN_TIMEOUT=${FWM_TIMEOUT:-240}
export FWM_SCRIPT="$wm/dev/${1:-session.sh}"
exec ~/shoji-run.sh -- bash -c 'echo "$SHOJI_RUN_DIR" > ~/fwm-last-run; exec bash "$FWM_SCRIPT"'
