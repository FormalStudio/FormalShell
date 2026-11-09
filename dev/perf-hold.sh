# shellcheck shell=bash
# dev/perf-gate.py holds g815 while it measures, its pid in the marker: any
# other run on g815 or its VM rig would land in the gate's numbers, so it
# waits here until the marker goes. A marker naming a pid that is gone is a
# gate that was killed, and holds nothing.
perf_gate_marker=$(python3 "$(dirname "${BASH_SOURCE[0]}")/rig_lock.py" marker)

perf_gate_held() {
  local pid
  [ -z "${FS_PERF_GATE_RUN:-}" ] && [ -e "$perf_gate_marker" ] || return 1
  pid=$(cat "$perf_gate_marker" 2>/dev/null) || return 1
  case "$pid" in ''|*[!0-9]*) return 0 ;; esac
  kill -0 "$pid" 2>/dev/null
}

perf_gate_wait() {
  perf_gate_held || return 0
  echo "$1: dev/perf-gate.py is measuring on g815 ($perf_gate_marker); waiting for it" >&2
  while perf_gate_held; do sleep 10; done
}
