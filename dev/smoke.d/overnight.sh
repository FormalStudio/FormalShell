# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --overnight enables overnight over IPC, photographs the bar's moon glyph,
# disables it and reads state.json back for the restore record's removal.
#
# The VM has no backlight, no untriggered LED and no asusctl, so the honest
# answer on this rig is a restore record saying so: backlight -1, leds {},
# aura false. A real laptop fills them in; the leg asserts the record's shape
# rather than those values.
leg_overnight_flag="--overnight"
leg_overnight_order=215
leg_overnight_claims="overnight"

overnight_active_path="$shot_dir/overnight-active.png"
overnight_status1_path="$shot_dir/overnight-status-1.json"
overnight_status2_path="$shot_dir/overnight-status-2.json"
overnight_state_path="$shot_dir/overnight-state.json"

leg_overnight_timing() {
  leg_timing 16 40
}

leg_overnight_drive() {
  local script="$shot_dir/overnight-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3
$ipc call overnight enable > /dev/null 2>&1
# The LED listing and the asusctl probe are Processes; both have to land in
# the record before it is read.
sleep 3
$ipc call overnight status > "$overnight_status1_path" 2>&1
"$grim_bin" "$overnight_active_path" > /dev/null 2>&1
$ipc call overnight disable > /dev/null 2>&1
sleep 1
$ipc call overnight status > "$overnight_status2_path" 2>&1
cp "$iso_home/.local/state/formalshell/state.json" "$overnight_state_path" 2>/dev/null
EOF
  hypr_exec_once "bash $script"
}

leg_overnight_assert() {
  [ -f "$overnight_active_path" ] || fail "no overnight-active screenshot produced"
  echo "SMOKE_OVERNIGHT_ACTIVE $overnight_active_path"
  [ -s "$overnight_status1_path" ] || fail "no overnight status produced after enable"
  cat "$overnight_status1_path"; echo
  jq -e '.active == true
    and (.restore | type == "object")
    and (.restore.backlight | type == "number")
    and (.restore.leds | type == "object")
    and (.restore.ddc | type == "object")
    and (.restore.aura | type == "boolean")' "$overnight_status1_path" > /dev/null \
    || fail "overnight enable did not report active with a full restore record"
  [ -s "$overnight_status2_path" ] || fail "no overnight status produced after disable"
  cat "$overnight_status2_path"; echo
  jq -e '.active == false and .restore == null' "$overnight_status2_path" > /dev/null \
    || fail "overnight disable left a restore record behind"
  [ -s "$overnight_state_path" ] || fail "no state.json copied after disable"
  jq -e 'has("overnight") and .overnight == null' "$overnight_state_path" > /dev/null \
    || fail "state.json still carries an overnight record after disable"
}
