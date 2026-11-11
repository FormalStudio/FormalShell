#!/bin/bash
# Runs a scripted session (dev/session.sh, or the script named) on the rig
# against the last formalwm built by dev/rig.sh and brings its screenshots
# and log back to wm/target/rig/. Goes through ShojiWM's shoji-rig, which
# queues behind FormalShell's own runs on the slot.
set -euo pipefail
guest=${FWM_GUEST:-fstest3@fstestmac}
here=$(cd "$(dirname "$0")/.." && pwd)
sdk=${FWM_SHOJIWM:-$HOME/Developer/ShojiWM-wt/rs-sdk}
wm='fwm/Developer/ecosystem/FormalShell-wt/shoji/wm'
rsync -a "$here/dev/" "$guest:$wm/dev/"
rsync -a "$here/example/" "$guest:$wm/example/"
status=0
"$sdk/dev/rig/shoji-rig" "bash $wm/dev/guest.sh ${1:-session.sh}" || status=$?
run=$(ssh "$guest" cat fwm-last-run)
mkdir -p "$here/target/rig"
rsync -a --delete --include '*.png' --include 'log/***' --exclude '*' "$guest:$run/" "$here/target/rig/"
echo "screenshots and log: $here/target/rig (exit $status)"
exit "$status"
