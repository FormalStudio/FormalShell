#!/usr/bin/env bash
# Runs a command on a rig slot as a transient systemd user scope, so the
# whole tree it starts (nested Hyprland, its dbus-run-session, the shell,
# every client) lives in one cgroup that can be stopped as a unit. Without
# it the tree outlives an ssh drop: a command session has no tty to hang up,
# and `timeout` puts Hyprland in a process group of its own. Called by
# dev/remote.sh's remote_run; do not run it by hand on a host.
#
#   dev/scoped-run.sh <unit> <command string>
#
# Three ways the scope ends: the command exits, remote.sh stops it from the
# mac when its own side is signalled, or the watcher below sees the ssh
# session that started it gone (a SIGKILLed mac side). RuntimeMaxSec is the
# backstop.
#
# A slot is held by one run at a time (remote.sh's mac-side flock), so any
# scope or Hyprland still here at start is a leftover of a finished run.
set -uo pipefail
unit=$1
shift

for stale in $(systemctl --user list-units --plain --no-legend --all 'fs-run-*.scope' | awk '{print $1}'); do
  systemctl --user stop "$stale" 2>/dev/null || true
done
pkill -KILL -x Hyprland 2>/dev/null || true

# The sshd that owns this session: its exit is the signal the client is gone.
owner=$$
while [ "$owner" -gt 1 ]; do
  case "$(ps -o comm= -p "$owner")" in sshd*) break ;; esac
  owner=$(ps -o ppid= -p "$owner" | tr -d ' ')
done
if [ "$owner" -gt 1 ]; then
  setsid bash -c '
    while kill -0 "$1" 2>/dev/null; do sleep 2; done
    systemctl --user stop "$2" 2>/dev/null' _ "$owner" "$unit.scope" >/dev/null 2>&1 </dev/null &
  watcher=$!
fi

# --expand-environment=no: systemd-run otherwise expands $NAME and ${NAME}
# in the command line from the manager's environment before bash sees it,
# emptying every shell variable a remote.sh command carries.
systemd-run --user --scope --quiet --collect --unit="$unit" --expand-environment=no \
  -p RuntimeMaxSec=2h -p TimeoutStopSec=5 bash -c "$1"
status=$?
[ -n "${watcher:-}" ] && kill "$watcher" 2>/dev/null
exit "$status"
