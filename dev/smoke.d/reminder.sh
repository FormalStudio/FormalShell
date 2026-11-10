# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --reminder sets one real countdown that fires inside the run. 12s rather
# than the 1s a hand-driven check would use: the pending state has to survive
# long enough to be photographed, dumped and read back out of state.json.
#
# DND goes on BEFORE the reminder is set. A fired reminder is authored at
# urgency 2 with the service's own `local` marker, which is exactly the pair
# bypassesDnd() lets through, so the toast landing in `popups` with dnd:true
# is the bypass proven rather than read off the source. Persistence is read
# off state.json itself, not through the service that wrote it.
leg_reminder_flag="--reminder"
leg_reminder_order=260
leg_reminder_claims="toasts=shared"

reminder_dnd_path="$shot_dir/reminder-dnd.txt"
reminder_set_reply_path="$shot_dir/reminder-set-reply.json"
reminder_status1_path="$shot_dir/reminder-status-1.json"
reminder_status2_path="$shot_dir/reminder-status-2.json"
reminder_state1_path="$shot_dir/reminder-state-1.json"
reminder_state2_path="$shot_dir/reminder-state-2.json"
reminder_notifications_path="$shot_dir/reminder-notifications.json"
reminder_pending_png="$shot_dir/reminder-pending.png"
reminder_fired_png="$shot_dir/reminder-fired.png"

leg_reminder_timing() {
  leg_timing 55
}

leg_reminder_drive() {
  local script="$shot_dir/reminder-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call notifications setDnd true > "$reminder_dnd_path" 2>&1
$ipc call reminder set 12s "SMOKE REMINDER FIXTURE" > "$reminder_set_reply_path" 2>&1
sleep 2
$ipc call reminder status > "$reminder_status1_path" 2>&1
cat "$iso_home/.local/state/formalshell/state.json" > "$reminder_state1_path" 2>&1
"$grim_bin" "$reminder_pending_png" > /dev/null 2>&1
# Past the deadline with room for the service's own 1s tick.
sleep 13
$ipc call reminder status > "$reminder_status2_path" 2>&1
$ipc call notifications status > "$reminder_notifications_path" 2>&1
cat "$iso_home/.local/state/formalshell/state.json" > "$reminder_state2_path" 2>&1
"$grim_bin" "$reminder_fired_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_reminder_assert() {
  local f
  for f in "$reminder_set_reply_path" "$reminder_status1_path" "$reminder_status2_path" \
    "$reminder_state1_path" "$reminder_state2_path" "$reminder_notifications_path"; do
    [ -s "$f" ] || fail "no reminder artifact produced at $f"
  done
  if ! grep -q '^on$' "$reminder_dnd_path"; then
    fail "notifications setDnd true did not report on, got: $(cat "$reminder_dnd_path" 2>/dev/null)"
  fi
  cat "$reminder_set_reply_path"; echo
  expect_jq "$reminder_set_reply_path" '.message == "SMOKE REMINDER FIXTURE" and (.id | startswith("rem-"))' \
    "reminder set did not answer with the stored entry"
  cat "$reminder_status1_path"; echo
  expect_jq "$reminder_status1_path" '.count == 1 and any(.reminders[]; .message == "SMOKE REMINDER FIXTURE")' \
    "reminder status did not report the pending entry"
  if ! grep -qF 'SMOKE REMINDER FIXTURE' "$reminder_state1_path"; then
    fail "state.json did not carry the pending reminder: $(cat "$reminder_state1_path")"
  fi
  [ -f "$reminder_pending_png" ] || fail "no pending-reminder screenshot produced at $reminder_pending_png"

  cat "$reminder_status2_path"; echo
  expect_jq "$reminder_status2_path" '.count == 0' "reminder status did not empty after the deadline"
  if grep -qF 'SMOKE REMINDER FIXTURE' "$reminder_state2_path"; then
    fail "a fired reminder is still in state.json: $(cat "$reminder_state2_path")"
  fi
  cat "$reminder_notifications_path"; echo
  expect_jq "$reminder_notifications_path" '.dnd == true' "DND was not on when the reminder fired"
  expect_jq "$reminder_notifications_path" '.popups > 0' "the fired reminder did not bypass DND into the popup tier"
  [ -f "$reminder_fired_png" ] || fail "no fired-reminder screenshot produced at $reminder_fired_png"
  echo "SMOKE_REMINDER_PENDING $reminder_pending_png"
  echo "SMOKE_REMINDER_FIRED $reminder_fired_png"
}
