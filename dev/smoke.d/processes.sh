# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --processes: the process table, which is the bottom half of the launcher's
# monitor view, so this leg summons that route and types into its own search
# field. The fixtures are copies of bash (not coreutils `sleep`, which
# nixpkgs builds as one multi-call binary that exits at once under any other
# name) spinning on a builtin loop: findable by a whole 15-byte comm, the
# busiest thing on the machine so the CPU column has something real to show,
# and dead on TERM with no foreground child to wait out.
#
# The kill runs through `menu activate` rather than `monitor kill`, since
# that is the rig's Enter stand-in and so exercises the whole path a keypress
# takes. It is called TWICE on purpose: this route arms on the first press
# and fires on the second, and the `kill -0` written between the two is what
# proves the arming press did not already kill.
#
# Before the kill, `menu actions 0` opens the process row's actions column
# (End, Kill) beside the halved table; the first `menu activate` closes it
# and arms End as before.
leg_processes_flag="--processes"
leg_processes_order=150
leg_processes_needs="jq"

processes_victim_pid_path="$shot_dir/processes-victim.pid"
processes_restart_pid_path="$shot_dir/processes-restart.pid"
processes_menu_reply_path="$shot_dir/processes-menu-reply.txt"
processes_filter_reply_path="$shot_dir/processes-filter-reply.txt"
processes_filtered_path="$shot_dir/processes-filtered.json"
processes_after_path="$shot_dir/processes-after.json"
processes_alive_arm_path="$shot_dir/processes-alive-arm.txt"
processes_alive_fire_path="$shot_dir/processes-alive-fire.txt"
processes_restart_reply_path="$shot_dir/processes-restart-reply.txt"
processes_restart_pids_path="$shot_dir/processes-restart-pids.txt"
processes_full_png="$shot_dir/processes-full.png"
processes_view_png="$shot_dir/processes-view.png"
processes_confirm_png="$shot_dir/processes-confirm.png"
processes_killed_png="$shot_dir/processes-killed.png"
processes_actions_png="$shot_dir/processes-actions.png"
processes_actions_status_path="$shot_dir/processes-actions.json"

leg_processes_timing() {
  leg_timing 35 73
  # --monitor owns the same launcher route, and this leg waits out its whole
  # timeline before summoning.
  if leg_on monitor; then leg_timing 55 98; fi
}

leg_processes_drive() {
  local script="$shot_dir/processes-drive.sh" monitor_wait=""
  local processes_victim_bin="$shot_dir/smokevictim"
  local processes_restart_bin="$shot_dir/smokerestart"
  cp "$(readlink -f "$(command -v bash)")" "$processes_victim_bin"
  cp "$(readlink -f "$(command -v bash)")" "$processes_restart_bin"
  chmod +x "$processes_victim_bin" "$processes_restart_bin"
  if leg_on monitor; then
    monitor_wait="for _ in \$(seq 1 60); do [ -f \"$monitor_done_path\" ] && break; sleep 1; done"
  fi
  # The fixtures are bash copies this script started itself, so they are
  # killed by name before the session goes away rather than left behind.
  add_cleanup "pkill -f \"$shot_dir/smoke\" 2>/dev/null || true"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 5
"$processes_victim_bin" -c 'while :; do :; done' & echo \$! > "$processes_victim_pid_path"
"$processes_restart_bin" -c 'while :; do :; done' & echo \$! > "$processes_restart_pid_path"
$monitor_wait
$ipc call menu summon monitor > "$processes_menu_reply_path" 2>&1
# Two poll ticks with the route open: the service polls only while something
# is subscribed, and the first tick after a subscribe has nothing to
# difference, so a measured CPU column needs the second.
sleep 6
"$grim_bin" "$processes_full_png" > /dev/null 2>&1
$ipc call menu filter smokevictim > "$processes_filter_reply_path" 2>&1
sleep 3
"$grim_bin" "$processes_view_png" > /dev/null 2>&1
$ipc call monitor processes smokevictim > "$processes_filtered_path" 2>&1
$ipc call menu actions 0 > /dev/null 2>&1
sleep 1.5
$ipc call menu status > "$processes_actions_status_path" 2>&1
"$grim_bin" "$processes_actions_png" > /dev/null 2>&1
$ipc call menu activate 0 > /dev/null 2>&1
sleep 2
"$grim_bin" "$processes_confirm_png" > /dev/null 2>&1
kill -0 \$(cat "$processes_victim_pid_path") 2>/dev/null; echo \$? > "$processes_alive_arm_path"
$ipc call menu activate 0 > /dev/null 2>&1
sleep 3
kill -0 \$(cat "$processes_victim_pid_path") 2>/dev/null; echo \$? > "$processes_alive_fire_path"
$ipc call monitor processes smokevictim > "$processes_after_path" 2>&1
"$grim_bin" "$processes_killed_png" > /dev/null 2>&1
$ipc call monitor restart \$(cat "$processes_restart_pid_path") > "$processes_restart_reply_path" 2>&1
sleep 6
pgrep -f smokerestart > "$processes_restart_pids_path"
EOF
  hypr_exec_once "bash $script"
}

leg_processes_assert() {
  local f victim_pid restart_pid restart_now
  victim_pid=$(cat "$processes_victim_pid_path" 2>/dev/null)
  if ! grep -q '^ok$' "$processes_menu_reply_path" 2>/dev/null; then
    fail "menu summon monitor did not answer ok, got: $(cat "$processes_menu_reply_path" 2>/dev/null)"
  fi
  if ! grep -q '^ok$' "$processes_filter_reply_path" 2>/dev/null; then
    fail "menu filter did not answer ok, got: $(cat "$processes_filter_reply_path" 2>/dev/null)"
  fi
  if [ ! -s "$processes_filtered_path" ]; then
    fail "no filtered process dump produced"
  fi
  cat "$processes_filtered_path"; echo
  # The filter's own claim: one row out of the whole table, and it is the
  # fixture, matched on a name the kernel's comm holds whole.
  if [ "$(grep -oF '"name":"smokevictim"' "$processes_filtered_path" | wc -l | tr -d ' ')" != "1" ] \
    || ! grep -qF "\"pid\":$victim_pid," "$processes_filtered_path"; then
    fail "the filter did not narrow to the fixture process (pid $victim_pid): $(cat "$processes_filtered_path")"
  fi
  for f in "$processes_full_png" "$processes_view_png" "$processes_confirm_png" "$processes_killed_png"; do
    if [ ! -f "$f" ]; then
      fail "no processes screenshot produced at $f"
    fi
  done
  # Arming is not killing: this route takes two presses, and the first has
  # to leave the process alive or the confirm is decoration.
  if [ "$(cat "$processes_alive_arm_path" 2>/dev/null)" != "0" ]; then
    fail "the arming press killed pid $victim_pid outright, the confirm did nothing"
  fi
  if [ "$(cat "$processes_alive_fire_path" 2>/dev/null)" = "0" ]; then
    fail "pid $victim_pid survived the confirming press"
  fi
  if grep -qF '"name":"smokevictim"' "$processes_after_path" 2>/dev/null; then
    fail "the killed fixture is still in the table: $(cat "$processes_after_path" 2>/dev/null)"
  fi
  # The kill's own exit status, which the synchronous IPC reply could not
  # carry: ProcessService.lastResult is where it lands, and the next dump
  # reports it.
  if ! grep -qF "\"lastAction\":{\"pid\":$victim_pid,\"action\":\"TERM\",\"ok\":true" "$processes_after_path"; then
    fail "no successful TERM recorded for pid $victim_pid: $(cat "$processes_after_path")"
  fi
  # The restart leg proves what no screenshot can: one smokerestart alive
  # afterwards, under a pid that is not the one it was started with.
  # `monitor restart` answers `ok:<pid>`, not a bare ok.
  if ! grep -q '^ok' "$processes_restart_reply_path" 2>/dev/null; then
    fail "monitor restart did not answer ok, got: $(cat "$processes_restart_reply_path" 2>/dev/null)"
  fi
  restart_pid=$(cat "$processes_restart_pid_path" 2>/dev/null)
  restart_now=$(cat "$processes_restart_pids_path" 2>/dev/null | tr '\n' ' ')
  if [ "$(cat "$processes_restart_pids_path" 2>/dev/null | wc -l | tr -d ' ')" != "1" ]; then
    fail "expected exactly one smokerestart alive after the restart, got: $restart_now"
  fi
  if [ "$(cat "$processes_restart_pids_path" 2>/dev/null | tr -d ' \n')" = "$restart_pid" ]; then
    fail "the restarted process kept its old pid ($restart_pid), nothing was re-run"
  fi
  [ -f "$processes_actions_png" ] || fail "no processes actions screenshot produced"
  "$jq_bin" -e '.view == "monitor" and .actionsOpen and .actions == ["end", "kill"]' "$processes_actions_status_path" > /dev/null \
    || fail "a process row's column is not End, Kill: $("$jq_bin" -c '{view, actionsOpen, actions}' "$processes_actions_status_path" 2>&1)"
  echo "SMOKE_PROCESSES_ACTIONS $processes_actions_png"
  echo "SMOKE_PROCESSES_FULL $processes_full_png"
  echo "SMOKE_PROCESSES_VIEW $processes_view_png"
  echo "SMOKE_PROCESSES_CONFIRM $processes_confirm_png"
  echo "SMOKE_PROCESSES_KILLED $processes_killed_png"
}
