# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --sleep suspends the machine through logind with the session unlocked and
# reads the lock back after wake. `sudo systemctl suspend` is the real path:
# logind signals PrepareForSleep(true), waits on every delay inhibitor, then
# starts suspend.target. Only the kernel's freeze is stubbed (the VM rig
# carries why), and the stub stamps the millisecond logind got there, which
# has to come after the shell let its inhibitor go with the surface secure.
# The FormalShell delay inhibitor is read off `systemd-inhibit --list` before
# the suspend and again after wake, when it has to be held again.
#
# The whole thing runs in light mode over a dark wallpaper at a tenth of the
# motion speed, with a sampler grabbing quarter-size frames throughout: the
# lock has to be black while logind holds the machine asleep (the blank goes
# up before the inhibitor is let go), fade in after wake, blank again on
# `lock.blankAfterSeconds` and wake on a real Shift press. From the first
# black frame on, no frame's mean luminance may rise past the settled lock
# frame's: a light theme's background showing through any of those fades is
# the white flash e1504g showed on a lid open in a dark room. The stub's 2s
# is under the 3s a real resume needs before the lock holds its blank until
# input, so the wake here is the short sleep's own fade back in.
leg_sleep_flag="--sleep"
leg_sleep_order=112
leg_sleep_vm_only="it suspends the machine through logind with sudo"
leg_sleep_needs="jq wtype convert"

sleep_inhibit_before_path="$shot_dir/sleep-inhibitors-before.txt"
sleep_inhibit_after_path="$shot_dir/sleep-inhibitors-after.txt"
sleep_status_before_path="$shot_dir/sleep-lock-status-before.json"
sleep_status_asleep_path="$shot_dir/sleep-lock-status-asleep.json"
sleep_status_after_path="$shot_dir/sleep-lock-status-after.json"
sleep_suspend_rc_path="$shot_dir/sleep-suspend-rc.txt"
sleep_suspended_at_path="$shot_dir/sleep-suspended-at.txt"
sleep_asleep_path="$shot_dir/sleep-asleep.png"
sleep_locked_path="$shot_dir/sleep-locked.png"
sleep_ref_path="$shot_dir/sleep-ref.ppm"
sleep_burst_dir="$shot_dir/sleep-burst"
sleep_wallpaper="$shot_dir/sleep-dark.png"
sleep_unlocked_path="$shot_dir/sleep-unlocked.txt"

leg_sleep_fixture() {
  settings_fragment ', "lock": {"blankAfterSeconds": 12}'
  $convert_bin -size 1920x1080 xc:'#1e1e1e' "$sleep_wallpaper"
}

leg_sleep_timing() {
  leg_timing 46 80
}

leg_sleep_drive() {
  local script="$shot_dir/sleep-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sudo -n rm -f /run/formalshell-suspended-at
mkdir -p "$sleep_burst_dir"
sleep 5
$ipc call wallpaper set "$sleep_wallpaper" > /dev/null 2>&1
$ipc call theme mode light > /dev/null 2>&1
sleep 8
systemd-inhibit --list --no-pager > "$sleep_inhibit_before_path" 2>&1
$ipc call lock status > "$sleep_status_before_path" 2>&1
$ipc call debug motionScale 1000 > /dev/null 2>&1
(
  while [ ! -e "$sleep_burst_dir/stop" ]; do
    "$grim_bin" -s 0.25 -t ppm "$sleep_burst_dir/\$(date +%s%3N).ppm" > /dev/null 2>&1
  done
) &
sudo -n systemctl suspend
echo \$? > "$sleep_suspend_rc_path"
# The stub holds suspend.target for 2s, and PrepareForSleep(false) follows it.
for _ in \$(seq 1 60); do
  [ -s /run/formalshell-suspended-at ] && break
  sleep 0.1
done
$ipc call lock status > "$sleep_status_asleep_path" 2>&1
"$grim_bin" "$sleep_asleep_path" > /dev/null 2>&1
sleep 7
cat /run/formalshell-suspended-at > "$sleep_suspended_at_path" 2>&1
$ipc call lock status > "$sleep_status_after_path" 2>&1
systemd-inhibit --list --no-pager > "$sleep_inhibit_after_path" 2>&1
"$grim_bin" "$sleep_locked_path" > /dev/null 2>&1
"$grim_bin" -s 0.25 -t ppm "$sleep_ref_path" > /dev/null 2>&1
# lock.blankAfterSeconds runs out with nothing typed, the blank fades in at
# a tenth speed, then a real Shift press wakes it.
sleep 11
"$wtype_bin" -k Shift_L
sleep 7
touch "$sleep_burst_dir/stop"
wait
$ipc call debug motionScale 100 > /dev/null 2>&1
"$wtype_bin" "formalshell-test"
"$wtype_bin" -k Return
sleep 3
$ipc call lock isLocked > "$sleep_unlocked_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

sleep_inhibitor_held() {
  grep -E "^FormalShell +[0-9]+ +$(id -un) .* sleep +Lock the session before sleep +delay" "$1" > /dev/null
}

# Mean luminance, 0..1, in thousandths.
sleep_luma() {
  $convert_bin "$1" -colorspace gray -format '%[fx:int(mean*1000)]' info: 2>/dev/null
}

leg_sleep_assert() {
  local f suspended released ref asleep luma frames=0 judged=0 dark=0 between=0 worst=0 worst_at="" covering=""
  for f in "$sleep_inhibit_before_path" "$sleep_status_before_path" "$sleep_suspend_rc_path" \
    "$sleep_status_asleep_path" "$sleep_suspended_at_path" "$sleep_status_after_path" "$sleep_inhibit_after_path"; do
    [ -s "$f" ] || fail "sleep: $f missing or empty"
    cat "$f"; echo
  done
  [ -s "$sleep_locked_path" ] && echo "SMOKE_SLEEP_LOCKED $sleep_locked_path"
  [ -s "$sleep_asleep_path" ] && echo "SMOKE_SLEEP_ASLEEP $sleep_asleep_path"
  sleep_inhibitor_held "$sleep_inhibit_before_path" \
    || fail "sleep: no FormalShell delay inhibitor on sleep before the suspend"
  jq -e '.locked == false and .beforeSleep.enabled == true and .beforeSleep.monitoring == true
    and .beforeSleep.inhibiting == true and .beforeSleep.last == null' "$sleep_status_before_path" > /dev/null \
    || fail "sleep: the shell was not unlocked and watching logind before the suspend"
  grep -q '^0$' "$sleep_suspend_rc_path" || fail "sleep: systemctl suspend exited $(cat "$sleep_suspend_rc_path")"
  jq -e '.locked == true and .secure == true and .blanked == true' "$sleep_status_asleep_path" > /dev/null \
    || fail "sleep: the lock was not up and blanked while logind held the machine asleep"
  jq -e '.locked == true and .secure == true and .blanked == false and .beforeSleep.last.result == "ok"
    and .beforeSleep.last.release == "secure"' "$sleep_status_after_path" > /dev/null \
    || fail "sleep: PrepareForSleep did not lock the session, release on secure and wake the lock after a short sleep"
  suspended=$(cat "$sleep_suspended_at_path")
  released=$(jq -r '.beforeSleep.last.releasedAt' "$sleep_status_after_path")
  [[ "$suspended" =~ ^[0-9]+$ ]] || fail "sleep: logind never reached the suspend stub"
  [ "$released" -le "$suspended" ] \
    || fail "sleep: suspend began at $suspended, before the inhibitor was let go at $released"
  echo "sleep: inhibitor released at $released, suspend began at $suspended ($((suspended - released))ms later)"
  sleep_inhibitor_held "$sleep_inhibit_after_path" \
    || fail "sleep: the FormalShell delay inhibitor was not taken again after wake"
  jq -e '.beforeSleep.inhibiting == true' "$sleep_status_after_path" > /dev/null \
    || fail "sleep: lock status reports no inhibitor after wake"

  asleep=$(sleep_luma "$sleep_asleep_path")
  ref=$(sleep_luma "$sleep_ref_path")
  [[ "$ref" =~ ^[0-9]+$ ]] || fail "sleep: no settled lock frame at $sleep_ref_path"
  echo "sleep: settled lock luma=$ref/1000, asleep luma=$asleep/1000"
  [ "${asleep:-1000}" -le 20 ] || fail "sleep: the frame while asleep was not black (luma $asleep/1000)"
  for f in "$sleep_burst_dir"/*.ppm; do
    [ -f "$f" ] || continue
    frames=$((frames + 1))
    luma=$(sleep_luma "$f")
    [[ "$luma" =~ ^[0-9]+$ ]] || continue
    if [ -z "$covering" ]; then
      [ "$luma" -le 20 ] || continue
      covering=$(basename "$f" .ppm)
    fi
    judged=$((judged + 1))
    [ "$luma" -le 20 ] && dark=$((dark + 1))
    if [ "$luma" -gt $((ref / 5)) ] && [ "$luma" -lt $((ref * 4 / 5)) ]; then between=$((between + 1)); fi
    if [ "$luma" -gt "$worst" ]; then worst=$luma; worst_at=$f; fi
  done
  echo "sleep: $frames burst frames, $judged from the first black one at $covering on, $dark black, $between part-faded, brightest $worst/1000 at $worst_at"
  echo "SMOKE_SLEEP_BURST $sleep_burst_dir"
  [ "$judged" -ge 20 ] || fail "sleep: only $judged burst frames after the lock went black"
  [ "$between" -ge 2 ] || fail "sleep: no part-faded frame was sampled ($between); the fades were not observed"
  [ "$worst" -le $((ref + 30)) ] \
    || fail "sleep: a frame at $worst_at reached luma $worst/1000, past the settled lock's $ref/1000: the screen flashed"
  grep -q '^false$' "$sleep_unlocked_path" 2>/dev/null \
    || fail "sleep: the real password did not unlock after the wake: $(cat "$sleep_unlocked_path" 2>/dev/null)"
}
