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

nightlight_active_path="$shot_dir/nightlight-active.png"
nightlight_status1_path="$shot_dir/nightlight-status-1.json"
nightlight_status2_path="$shot_dir/nightlight-status-2.json"
nightlight_status0_path="$shot_dir/nightlight-status-0.json"

leg_nightlight_fixture() {
  local longitude
  longitude=$(( ( -15 * 10#$(date -u +%H) + 540 ) % 360 - 180 ))
  settings_fragment ', "nightlight": {"schedule": "sun"}, "location": {"latitude": 0, "longitude": '"$longitude"'}'
}

# This leg's own clock. A gamma shift repaints every pixel on the output, so
# under --wallpaper it starts after that leg's last frame rather than warming
# the exact colours its dither assertions read back.
nightlight_t0() {
  cover_t0 3
}

leg_nightlight_timing() {
  local t0
  t0=$(nightlight_t0)
  leg_timing $((t0 + 25)) $((t0 + 54))
}

leg_nightlight_drive() {
  local t0 script="$shot_dir/nightlight-drive.sh"
  t0=$(nightlight_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
SECONDS=0
while [ "\$SECONDS" -lt 12 ]; do
  $ipc call nightlight status > "$nightlight_status0_path" 2>&1
  grep -qF '"active":true' "$nightlight_status0_path" && break
  sleep 1
done
$ipc call nightlight enable > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 8 ]; do
  $ipc call nightlight status > "$nightlight_status1_path" 2>&1
  grep -qF '"active":true' "$nightlight_status1_path" && break
  # A populated lastError is a settled answer too, so the poll stops there
  # rather than spinning out its whole budget on a session that already said
  # what went wrong.
  grep -qF '"lastError":""' "$nightlight_status1_path" || break
  sleep 1
done
"$grim_bin" "$nightlight_active_path" > /dev/null 2>&1
$ipc call nightlight disable > /dev/null 2>&1
sleep 1
$ipc call nightlight status > "$nightlight_status2_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_nightlight_assert() {
  cat "$nightlight_status0_path"; echo
  if ! grep -qF '"schedule":"sun"' "$nightlight_status0_path" \
      || ! grep -qF '"scheduleDark":true' "$nightlight_status0_path" \
      || ! grep -qF '"source":"location"' "$nightlight_status0_path"; then
    fail "the schedule did not read solar midnight off the fixture location: $(cat "$nightlight_status0_path")"
  fi
  if ! grep -qF '"active":true' "$nightlight_status0_path" && grep -qF '"lastError":""' "$nightlight_status0_path"; then
    fail "the schedule said dark but nothing started wlsunset: $(cat "$nightlight_status0_path")"
  fi
  if grep -qF '"active":true' "$nightlight_status1_path" && ! grep -qF '"active":true' "$nightlight_status0_path"; then
    fail "wlsunset holds on a manual enable but the schedule's never came up: $(cat "$nightlight_status0_path")"
  fi
  [ -f "$nightlight_active_path" ] || fail "no nightlight-active screenshot produced"
  echo "SMOKE_NIGHTLIGHT_ACTIVE $nightlight_active_path"
  if [ ! -s "$nightlight_status1_path" ]; then
    fail "no nightlight status produced after enable"
  fi
  cat "$nightlight_status1_path"; echo
  if grep -qF '"active":true' "$nightlight_status1_path"; then
    echo "nightlight: reached active:true, this session implements wlr-gamma-control-unstable-v1"
  elif grep -qF '"active":false' "$nightlight_status1_path" && ! grep -qF '"lastError":""' "$nightlight_status1_path"; then
    echo "nightlight: honest failure surface (active:false, lastError populated), this session likely lacks wlr-gamma-control-unstable-v1"
  else
    fail "nightlight enable produced neither active:true nor an honest lastError, got: $(cat "$nightlight_status1_path")"
  fi
  if [ ! -s "$nightlight_status2_path" ] || ! grep -qF '"active":false' "$nightlight_status2_path"; then
    fail "nightlight disable did not confirm active:false, got: $(cat "$nightlight_status2_path" 2>/dev/null)"
  fi
  cat "$nightlight_status2_path"; echo
}
