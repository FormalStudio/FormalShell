#!/bin/bash
# A locked start (FWM_LOCKED=1): nothing is drawn but a lock surface, from
# the first frame on. The start script takes screenshots from the moment it
# runs and starts a window beside the shell, fwm-drag's, whose blue no lock
# has. With FWM_SHELL the shell takes the lock and the password opens the
# desktop; without it the session stays dark.
set -u
. "$(dirname "$0")/lib.sh"
run=$SHOJI_RUN_DIR
burst=$XDG_CONFIG_HOME/burst
lock() { "$FWM_SHELL" ipc --any-display call lock status 2>/dev/null; }
now() { date +%s%3N; }
blue() { python3 -c "
import sys
from PIL import Image
colors = dict((c, n) for n, c in Image.open(sys.argv[1]).convert('RGB').getcolors(1 << 24))
print(len(colors), 'colours,', colors.get((42, 74, 106), 0), 'pixels of the window')" "$1"; }
# The burst in runs of frames that look alike: drawn in one colour, or not,
# and whether the window's blue is in them.
frames() { python3 - "$burst" "${1:-}" <<'PY'
import glob, os, sys
from PIL import Image
frames = sorted(glob.glob(sys.argv[1] + "/*.png"))
stamp = lambda f: int(os.path.basename(f)[:-4].split("-")[1])
first = stamp(frames[0])
runs = []
for f in frames:
    colors = dict((c, n) for n, c in Image.open(f).convert("RGB").getcolors(1 << 24))
    what = ("one colour %s" % (next(iter(colors)),) if len(colors) == 1 else "drawn") + \
        (", the window in it" if colors.get((42, 74, 106), 0) else "")
    if runs and runs[-1][0] == what:
        runs[-1][2] = stamp(f) - first; runs[-1][3] += 1
    else:
        runs.append([what, stamp(f) - first, stamp(f) - first, 1])
for what, since, until, count in runs:
    print(f"{since} to {until} ms: {count} frames, {what}")
if sys.argv[2]:
    print(f"the lock was secure {int(sys.argv[2]) - first} ms after the first frame")
PY
}

step "the start script ran while locked, and what it started is a window"
cat "$XDG_CONFIG_HOME/started"
for _ in $(seq 30); do [ -n "$(id_of autostart)" ] && break; sleep 0.2; done
ipc state.get | jq -c '[.windows[] | {app: .appId, rect: (.rect | [.x, .y, .width, .height])}]'

if [ -n "${FWM_SHELL:-}" ]; then
  step "the shell takes the lock"
  secure=
  for _ in $(seq 150); do lock | jq -e '.secure == true' >/dev/null 2>&1 && secure=$(now) && break; sleep 0.1; done
  lock | jq -c '{locked, secure, boot}'
  for _ in $(seq 60); do [ "$(ls "$burst" | wc -l)" -ge 70 ] && break; sleep 0.5; done
  step "every frame from the start script's first on"
  frames "$secure"
  shot 85-locked; echo "the lock at rest: $(blue "$run/85-locked.png")"
  step "the password opens the desktop"
  wtype -s 300 formalshell-test -k Return
  for _ in $(seq 100); do lock | jq -e '.locked == false' >/dev/null 2>&1 && break; sleep 0.2; done
  lock | jq -c '{locked}'; sleep 2.5; shot 86-unlocked; echo "the desktop: $(blue "$run/86-unlocked.png")"
  ipc layers.list | jq -c '[.[] | .namespace]'
else
  step "no shell: the session stays dark, the pointer too, past the ten seconds after which an unlocked start shows it"
  for _ in $(seq 60); do [ "$(ls "$burst" | wc -l)" -ge 70 ] && break; sleep 0.5; done
  frames
  started=$(stat -c %Y "$XDG_CONFIG_HOME/started")
  while [ $(($(date +%s) - started)) -lt 13 ]; do sleep 0.5; done
  shot 87-dark; grim -c "$run/88-dark-with-pointer.png"
  echo "$(($(date +%s) - started)) s after the start: $(blue "$run/87-dark.png"); with the pointer drawn in: $(blue "$run/88-dark-with-pointer.png")"
  ipc state.get | jq -c '{cursorHidden}'
  step "locked, a bind that is not marked for it does nothing"
  key Super+Return 1.5; ipc state.get | jq -c '[.windows[] | .appId]'
fi
cp "$burst"/*.png "$run/" 2>/dev/null

step "the log"
grep -E " (WARN|ERROR) |panicked" "$SHOJI_RUN_LOG" | grep -v "cliphist\|fcitx\|cursor image\|cursor theme\|smithay::backend\|backend::tty\|XWayland" | cut -c1-300 | tail -8
