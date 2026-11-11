# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --console drives the quake console over three `console toggle` calls
# off one timeline: open, parked, and back, with a `console status` dump
# beside each frame. The claim: the console parks on a special workspace
# rather than closing, and the same window id has to come back across all
# three toggles. A console that closed and
# respawned its terminal would produce three perfectly good frames and throw
# the session away, which is the whole feature.
leg_console_flag="--console"
leg_console_order=90
leg_console_claims="console"
leg_console_needs="foot jq"

console_status_open_path="$shot_dir/console-status-open.json"
console_status_parked_path="$shot_dir/console-status-parked.json"
console_status_return_path="$shot_dir/console-status-return.json"
console_open_path="$shot_dir/console-open.png"
console_parked_path="$shot_dir/console-parked.png"
console_return_path="$shot_dir/console-return.png"

leg_console_fixture() {
  # The quake console spawns a REAL terminal (the shell embeds no emulator),
  # so the fixture is foot on a known blue background announcing the app id
  # ConsoleService matches on. Absolute path: this argv is spawned by the
  # shell through the compositor, against the session's PATH.
  settings_pin '. * {"console": {"command": ["'"$foot_bin"'", "--app-id=formalshell-console", "--override=colors.background=1f6feb", "--override=colors.foreground=f4f4f4", "sh", "-c", "echo FORMALSHELL QUAKE CONSOLE; sleep 600"], "appId": "formalshell-console", "share": 0.5}}'
}

leg_console_timing() {
  leg_timing 60
}

leg_console_drive() {
  local script="$shot_dir/console-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call console toggle > /dev/null 2>&1
sleep 4
$ipc call console status > "$console_status_open_path" 2>&1
"$grim_bin" "$console_open_path" > /dev/null 2>&1
sleep 1
$ipc call console toggle > /dev/null 2>&1
sleep 3
$ipc call console status > "$console_status_parked_path" 2>&1
"$grim_bin" "$console_parked_path" > /dev/null 2>&1
sleep 1
$ipc call console toggle > /dev/null 2>&1
sleep 3
$ipc call console status > "$console_status_return_path" 2>&1
"$grim_bin" "$console_return_path" > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_console_assert() {
  local f console_open_id console_parked_id console_return_id
  for f in "$console_status_open_path" "$console_status_parked_path" "$console_status_return_path"; do
    if [ ! -s "$f" ]; then
      fail "no console status produced at $f"
    fi
    cat "$f"; echo
  done
  console_open_id=$("$jq_bin" -r '.windowId // empty' "$console_status_open_path" 2>/dev/null)
  console_parked_id=$("$jq_bin" -r '.windowId // empty' "$console_status_parked_path" 2>/dev/null)
  console_return_id=$("$jq_bin" -r '.windowId // empty' "$console_status_return_path" 2>/dev/null)
  if [ -z "$console_open_id" ]; then
    fail "console toggle opened no window. Got: $(cat "$console_status_open_path")"
  fi
  expect_jq "$console_status_open_path" '.visible == true' "console not reported visible after the first toggle"
  expect_jq "$console_status_parked_path" '.visible == false' "console still reported visible after the hide"
  if [ "$console_parked_id" != "$console_open_id" ]; then
    fail "the parked console is not the window that was opened (open=$console_open_id parked=$console_parked_id)"
  fi
  expect_jq "$console_status_return_path" '.visible == true' "console not reported visible again after the third toggle"
  if [ "$console_return_id" != "$console_open_id" ]; then
    fail "the console that came back is a different window (open=$console_open_id return=$console_return_id)"
  fi
  for f in "$console_open_path" "$console_parked_path" "$console_return_path"; do
    if [ ! -f "$f" ]; then
      fail "no console screenshot produced at $f"
    fi
  done
  echo "SMOKE_CONSOLE_OPEN $console_open_path"
  echo "SMOKE_CONSOLE_PARKED $console_parked_path"
  echo "SMOKE_CONSOLE_RETURN $console_return_path"
  if cmp -s "$console_open_path" "$console_parked_path"; then
    fail "console-open and console-parked screenshots are byte-identical: the special workspace never left the screen"
  fi
}
