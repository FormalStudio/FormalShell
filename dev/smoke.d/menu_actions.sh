# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --menu-actions: the launcher's in-process `@ipc:` actions reached through
# its own rows (`menu activate` on a row's index, the path Enter takes),
# each read back off the service it drives rather than off the frame. The
# toggle hub's Do Not Disturb, Caffeinate and Overnight rows each flip their
# service and leave the hub open with the row ticked. Then the reminder
# level: Set Reminder opens the launcher's own input step, "10m launcher
# check" typed on real keys sets one reminder, and Clear Reminders drops it.
#
# HDR is left out (the rig's EDID-less output has none to toggle, which
# --hdr proves), and so is the lock row (--lock owns a locked session).
leg_menu_actions_flag="--menu-actions"
leg_menu_actions_order=223
leg_menu_actions_needs="jq wtype"
leg_menu_actions_claims="launcher=shared"

leg_menu_actions_timing() {
  leg_timing 60
}

leg_menu_actions_drive() {
  local script="$shot_dir/menu-actions-drive.sh" p="$shot_dir/menu-actions"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call notifications dndState > "$p-dnd-before.txt" 2>&1
call caffeinate status > "$p-caffeinate-before.json" 2>&1
call overnight status > "$p-overnight-before.json" 2>&1
call menu summon toggles > /dev/null 2>&1
until_settled
for id in toggles.dnd toggles.caffeinate toggles.overnight; do
  idx=\$(row_index "$p-hub.json" "\$id")
  call menu activate "\$idx" > /dev/null 2>&1
  sleep 1
done
sleep 1
call menu status > "$p-hub-after.json" 2>&1
"$grim_bin" "$p-hub.png" > /dev/null 2>&1
call notifications dndState > "$p-dnd-after.txt" 2>&1
call caffeinate status > "$p-caffeinate-after.json" 2>&1
call overnight status > "$p-overnight-after.json" 2>&1
call menu close > /dev/null 2>&1
sleep 1

call menu summon reminder > /dev/null 2>&1
until_settled
idx=\$(row_index "$p-reminder.json" reminder.set)
call menu activate "\$idx" > /dev/null 2>&1
sleep 1.5
call menu status > "$p-reminder-input.json" 2>&1
"$wtype_bin" "10m launcher check"
sleep 0.5
"$wtype_bin" -k Return
sleep 1.5
call reminder status > "$p-reminder-set.json" 2>&1
call menu summon reminder > /dev/null 2>&1
until_settled
idx=\$(row_index "$p-reminder2.json" reminder.clear)
call menu activate "\$idx" > /dev/null 2>&1
sleep 1.5
call reminder status > "$p-reminder-cleared.json" 2>&1
call menu close > /dev/null 2>&1
call caffeinate disable > /dev/null 2>&1
call overnight disable > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_menu_actions_assert() {
  local p="$shot_dir/menu-actions" f
  for f in dnd-before.txt dnd-after.txt caffeinate-before.json caffeinate-after.json overnight-before.json \
    overnight-after.json hub-after.json reminder-input.json reminder-set.json reminder-cleared.json; do
    [ -s "$p-$f" ] || fail "menu actions: no $f"
  done
  echo "SMOKE_MENU_ACTIONS dnd $(cat "$p-dnd-before.txt") -> $(cat "$p-dnd-after.txt")"
  [ "$(cat "$p-dnd-before.txt")" != "$(cat "$p-dnd-after.txt")" ] \
    || fail "menu actions: the Do Not Disturb row left dnd at $(cat "$p-dnd-after.txt")"
  "$jq_bin" -e '.active == true' "$p-caffeinate-after.json" > /dev/null \
    || fail "menu actions: the Caffeinate row did not caffeinate: $(cat "$p-caffeinate-after.json")"
  "$jq_bin" -e '.active == true' "$p-overnight-after.json" > /dev/null \
    || fail "menu actions: the Overnight row did not enable overnight: $(cat "$p-overnight-after.json")"
  "$jq_bin" -e '.isOpen == true and .level == "toggles"' "$p-hub-after.json" > /dev/null \
    || fail "menu actions: the hub did not stay open on its level: $(cat "$p-hub-after.json")"
  for f in toggles.caffeinate toggles.overnight; do
    "$jq_bin" -e --arg id "$f" '.checked | index($id) != null' "$p-hub-after.json" > /dev/null \
      || fail "menu actions: $f carries no tick: $(cat "$p-hub-after.json")"
  done
  "$jq_bin" -e '.mode == "input"' "$p-reminder-input.json" > /dev/null \
    || fail "menu actions: Set Reminder did not open the input step: $(cat "$p-reminder-input.json")"
  expect_jq "$p-reminder-set.json" 'any(.. | objects; .message == "launcher check")' \
    "menu actions: the typed reminder was not set"
  expect_jq "$p-reminder-cleared.json" 'tostring | contains("launcher check") | not' \
    "menu actions: Clear Reminders left"
  echo "SMOKE_MENU_ACTIONS_HUB $p-hub.png"
}
