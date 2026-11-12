# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --nightlight enables the wlsunset-backed night light over IPC, photographs
# the warmed session, then disables it and proves the flip back. First, the
# schedule: the base fixture pins `nightlight.schedule` off so no other
# leg's frame is warmed by the hour it runs at, and this leg turns it back
# on over a location where it is solar midnight now (the equator, at the
# longitude the run's own UTC hour puts there), which has to switch the
# night light on with no IPC call at all.
#
# Honest bifurcation is the contract: active:true means the session
# implements wlr-gamma-control-unstable-v1 and wlsunset held; active:false
# WITH a populated lastError is the correctly-surfaced failure of a session
# that does not, and is just as real a pass. active:false with an EMPTY
# lastError is the one shape that never is: a silent no-op.
leg_nightlight_flag="--nightlight"
leg_nightlight_order=210
leg_nightlight_compositors="hyprland shoji"
leg_nightlight_needs="jq"
leg_nightlight_claims="nightlight location"

nightlight_active_path="$shot_dir/nightlight-active.png"
nightlight_status1_path="$shot_dir/nightlight-status-1.json"
nightlight_status2_path="$shot_dir/nightlight-status-2.json"
nightlight_status0_path="$shot_dir/nightlight-status-0.json"

leg_nightlight_fixture() {
  local longitude
  longitude=$(( ( -15 * 10#$(date -u +%H) + 540 ) % 360 - 180 ))
  settings_pin ".nightlight.schedule = \"sun\" | .location = {\"latitude\": 0, \"longitude\": $longitude}"
}

leg_nightlight_timing() {
  leg_timing 57
}

leg_nightlight_drive() {
  local script="$shot_dir/nightlight-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
until_jq 12 "$nightlight_status0_path" '.active == true' call nightlight status
$ipc call nightlight enable > /dev/null 2>&1
# A populated lastError is a settled answer too, so the poll stops there
# rather than spinning out its whole budget on a session that already said
# what went wrong.
until_jq 8 "$nightlight_status1_path" '.active == true or .lastError != ""' call nightlight status
"$grim_bin" "$nightlight_active_path" > /dev/null 2>&1
$ipc call nightlight disable > /dev/null 2>&1
sleep 1
$ipc call nightlight status > "$nightlight_status2_path" 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_nightlight_assert() {
  cat "$nightlight_status0_path"; echo
  expect_jq "$nightlight_status0_path" '.schedule == "sun" and .scheduleDark == true and .source == "location"' \
    "the schedule did not read solar midnight off the fixture location"
  expect_jq "$nightlight_status0_path" '.active == true or .lastError != ""' "the schedule said dark but nothing started wlsunset"
  if "$jq_bin" -e '.active == true' "$nightlight_status1_path" > /dev/null 2>&1; then
    expect_jq "$nightlight_status0_path" '.active == true' "wlsunset holds on a manual enable but the schedule's never came up"
  fi
  [ -f "$nightlight_active_path" ] || fail "no nightlight-active screenshot produced"
  echo "SMOKE_NIGHTLIGHT_ACTIVE $nightlight_active_path"
  if [ ! -s "$nightlight_status1_path" ]; then
    fail "no nightlight status produced after enable"
  fi
  cat "$nightlight_status1_path"; echo
  if "$jq_bin" -e '.active == true' "$nightlight_status1_path" > /dev/null 2>&1; then
    echo "nightlight: reached active:true, this session implements wlr-gamma-control-unstable-v1"
  elif "$jq_bin" -e '.active == false and .lastError != ""' "$nightlight_status1_path" > /dev/null 2>&1; then
    echo "nightlight: honest failure surface (active:false, lastError populated), this session likely lacks wlr-gamma-control-unstable-v1"
  else
    fail "nightlight enable produced neither active:true nor an honest lastError, got: $(cat "$nightlight_status1_path")"
  fi
  expect_jq "$nightlight_status2_path" '.active == false' "nightlight disable did not confirm active:false"
  cat "$nightlight_status2_path"; echo
}
