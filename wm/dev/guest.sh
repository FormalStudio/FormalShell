#!/bin/bash
# The guest's half of dev/check.sh: a session of the last formalwm built by
# dev/rig.sh on the owner's settings from wm/example, with foot for the
# terminal, running the script named in $1. FWM_SHELL names a shell for
# the session's start script to run.
set -euo pipefail
export PATH="/opt/fspec/current/bin:$PATH"
wm=$(cd "$(dirname "$0")/.." && pwd)
config=$HOME/fwm-config
rm -rf "$config" && mkdir -p "$config/formalwm"
cp "$wm/example/settings.json" "$config/formalwm/"
# The rig's output at the mode its sessions measure against; FWM_HOST
# names another host file under dev/.
if [ -n "${FWM_HOST:-}" ]; then
  cp "$wm/dev/$FWM_HOST" "$config/formalwm/host.json"
else
  echo '{"terminal": "foot", "outputs": [{"name": "Virtual-4", "mode": "1024x768@60", "scale": 1}]}' > "$config/formalwm/host.json"
fi
cat > "$config/formalwm/formalshell-start.sh" <<'START'
test -S "$XDG_RUNTIME_DIR/shojiwm-$WAYLAND_DISPLAY.sock" && up=up || up=missing
echo "display $WAYLAND_DISPLAY, socket $up" > "$XDG_CONFIG_HOME/started"
[ -z "${FWM_SHELL:-}" ] || exec "$FWM_SHELL" > "$XDG_CONFIG_HOME/shell.log" 2>&1
START
export FWM_SHELL=${FWM_SHELL:-} FWM_KEYS=${FWM_KEYS:-} FORMALWM_FRAMES="$config/frames"
# FWM_TIMING has the core log every frame's render and cpu time and the
# rate it presents at.
if [ -n "${FWM_TIMING:-}" ]; then
  export SHOJI_ANIMATION_TIMING_DEBUG=1 SHOJI_ANIMATION_SPIKE_THRESHOLD_MS=0.001 SHOJI_PRESENT_RATE_DEBUG=1
  export SHOJI_LOG_RATE_LIMIT=0
fi
export XDG_CONFIG_HOME="$config" SHOJI_BIN="$HOME/fwm-build/release-fast/formalwm" SHOJI_RUN_TIMEOUT=${FWM_TIMEOUT:-240}
export FWM_SCRIPT="$wm/dev/${1:-session.sh}"
exec ~/shoji-run.sh -- bash -c 'echo "$SHOJI_RUN_DIR" > ~/fwm-last-run; exec bash "$FWM_SCRIPT"'
