# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --instance launches a SECOND real daemon against the same shell path once
# the primary is up, then polls until exactly one survives: the takeover the
# owner's own rebuild+respawn performs, reproduced through the same wrapped
# launcher a keybind or a terminal would use. The pid arithmetic is the claim
# (the survivor has to be the new process, never the old one holding its
# ground), and both instances' own "instance lock" log lines are what prove
# the handshake happened rather than one side merely dying.
#
# Neither instance needs a kill script: the loser quits itself and the
# survivor dies with the session at teardown.
leg_instance_flag="--instance"
leg_instance_order=210
# Nothing is summoned over the desktop, so this leg keeps the base run's
# focused fixture window in its frame.
leg_instance_fixture_window=keep

instance_status_path="$shot_dir/instance-status.json"
instance_second_log_path="$shot_dir/instance-second.log"

leg_instance_fixture() {
  # InstanceLock keys its socket on WAYLAND_DISPLAY, so the session's own
  # shell and the owner's live bar can never share a lock even though this rig
  # deliberately keeps the outer XDG_RUNTIME_DIR. Sockets left behind by an
  # abnormal exit still pile up in that shared directory, so clear the dead
  # ones first: `wayland_socket_live` is the same listener check the scaffold
  # uses on WAYLAND_DISPLAY, and a socket something is still listening on
  # belongs to a live shell and is left alone.
  local lock_dir stale
  lock_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/formalshell"
  for stale in "$lock_dir"/instance-*.sock; do
    [ -e "$stale" ] || continue
    wayland_socket_live "$stale" && continue
    rm -f "$stale" 2>/dev/null || true
  done
}

leg_instance_timing() {
  # instance-drive.sh's worst case (3s initial sleep plus a 15s poll ceiling)
  # lands ~18s in; the run's own frame is taken past that, showing the session
  # with the takeover already resolved to a single bar.
  leg_timing 21 45
}

leg_instance_drive() {
  local script="$shot_dir/instance-drive.sh"
  # The wrapped launcher, as every real second instance starts.
  # LIBGL_ALWAYS_SOFTWARE for the same reason dev/smoke.sh exports it around
  # the primary. The daemon is the wrapped binary itself, told apart from
  # formalshell-ipc and the scripts by what /proc says it executes.
  local find_daemons launch
  find_daemons='for pid in $(pgrep -f -- formalshell-rs); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in */formalshell-rs) echo "$pid" ;; esac; done'
  launch="$PWD/result/bin/formalshell"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3
find_daemon_pids() {
  $find_daemons
}
old_pid=\$(find_daemon_pids | head -n1)
export LIBGL_ALWAYS_SOFTWARE=1
"$launch" > "$instance_second_log_path" 2>&1 &
new_pid=\$!
waited=0
count=0
while [ "\$waited" -lt 15000 ]; do
  sleep 0.5
  waited=\$((waited + 500))
  count=\$(find_daemon_pids | wc -l | tr -d ' ')
  if [ "\$count" = "1" ]; then
    break
  fi
done
survivor=\$(find_daemon_pids | head -n1)
printf '{"oldPid":%s,"newPid":%s,"survivorPid":%s,"waitedMs":%s,"finalCount":%s}\n' \
  "\${old_pid:-null}" "\${new_pid:-null}" "\${survivor:-null}" "\$waited" "\${count:-0}" \
  > "$instance_status_path"
EOF
  hypr_exec_once "bash $script"
}

leg_instance_assert() {
  local instance_old instance_new instance_survivor instance_final_count
  if [ ! -s "$instance_status_path" ]; then
    fail "no instance-status.json produced, the second daemon may never have launched"
  fi
  cat "$instance_status_path"
  instance_old=$(grep -o '"oldPid":[A-Za-z0-9]*' "$instance_status_path" | cut -d: -f2)
  instance_new=$(grep -o '"newPid":[A-Za-z0-9]*' "$instance_status_path" | cut -d: -f2)
  instance_survivor=$(grep -o '"survivorPid":[A-Za-z0-9]*' "$instance_status_path" | cut -d: -f2)
  instance_final_count=$(grep -o '"finalCount":[0-9]*' "$instance_status_path" | cut -d: -f2)
  # Both sides' log lines are surfaced pass or fail: a failure needs to be
  # loud about what each instance logged, not just that the pids disagreed.
  # The primary writes into the scaffold's own shell log.
  echo "-- primary shell log (instance lock lines) --"
  grep "instance lock" "$shell_log_path" 2>/dev/null || echo "(none found)"
  echo "-- instance-second.log (instance lock lines) --"
  grep "instance lock" "$instance_second_log_path" 2>/dev/null || echo "(none found)"
  if [ "$instance_old" = "null" ] || [ -z "$instance_old" ]; then
    fail "no live primary instance was found before the second launch, the takeover path was never exercised"
  fi
  if [ "$instance_final_count" != "1" ]; then
    fail "expected exactly one surviving daemon after the takeover poll, got $instance_final_count"
  fi
  if [ -z "$instance_survivor" ] || [ "$instance_survivor" = "null" ]; then
    fail "takeover poll settled on a count of 1 but recorded no survivor pid"
  fi
  if [ "$instance_survivor" != "$instance_new" ]; then
    fail "survivor pid ($instance_survivor) is not the second instance's pid ($instance_new), the old instance won the takeover instead of quitting"
  fi
  if ! grep -q "instance lock: being replaced" "$shell_log_path" 2>/dev/null; then
    fail "primary instance log never logged being replaced"
  fi
  if ! grep -q "instance lock: live instance found, requesting takeover" "$instance_second_log_path" 2>/dev/null; then
    fail "second instance log never logged finding a live instance"
  fi
  if ! grep -q "instance lock: acquired at" "$instance_second_log_path" 2>/dev/null; then
    fail "second instance log never logged acquiring the lock"
  fi
  echo "SMOKE_INSTANCE old=$instance_old new=$instance_new survivor=$instance_survivor"
}
