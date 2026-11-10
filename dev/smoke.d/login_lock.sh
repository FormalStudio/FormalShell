# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --login-lock: the boot's lock (docs/superpowers/specs/2026-10-30-locked-login.md)
# raised by the shell itself at start, with no `lock lock` anywhere. The
# login marker is written before the shell starts, naming a flat-coloured
# picture smaller than the output, and the shell runs at a tenth of its
# speed (FORMALSHELL_SMOKE_MOTION_SCALE, since the first frames come before
# any IPC call can reach it), so the splash's dissolve into the backdrop
# (surfaces/dissolve.rs, 11s here) is long enough to photograph.
#
# An early script, started beside the shell rather than after the arrival
# like a drive, polls `lock status` until the lock is secure, then moves the
# output's scale under it (1.25, or 1 for a run already at 1.25), the way a
# host's monitor rule can land after the boot's lock is up, and grabs the
# first frame at the new scale: the picture at 1:1 in the output's centre on
# black, no card.
# The drive then grabs a burst through the dissolve: one frame from its
# middle third read on its 32 px grid (dev/dissolve-cells.py) carrying
# cells of the splash and of the lock, and no frame still showing the
# splash's colour carrying any of the card's (the avatar fixture's green).
# Once the card has entered, the splash's colour is gone. It then Tabs
# onto the buttons (`buttonCursor`, `armed`), arms Other user, confirms a
# Reboot logind refuses (the nested run's system bus is a private, empty
# one, checked before the confirm is typed), and types the real password:
# unlocked, the marker spent as the lock went up.
leg_login_lock_flag="--login-lock"
leg_login_lock_order=113
leg_login_lock_needs="wtype convert python3 pil"

login_lock_marker="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/formalshell/login-lock"
login_lock_picture="$shot_dir/login-lock-splash.png"
login_lock_colour='#E0457B'
login_lock_early_png="$shot_dir/login-lock-early.png"
login_lock_card_png="$shot_dir/login-lock-card.png"
login_lock_buttons_png="$shot_dir/login-lock-buttons.png"
login_lock_armed_png="$shot_dir/login-lock-armed.png"
login_lock_refused_png="$shot_dir/login-lock-refused.png"
login_lock_early_json="$shot_dir/login-lock-early.json"
login_lock_tab_json="$shot_dir/login-lock-tab.json"
login_lock_armed_json="$shot_dir/login-lock-armed.json"
login_lock_refused_json="$shot_dir/login-lock-refused.json"
login_lock_after_json="$shot_dir/login-lock-after.json"
login_lock_marker_after="$shot_dir/login-lock-marker-after.txt"
login_lock_early_done="$shot_dir/login-lock-early.done"
login_lock_avatar='#1FB86A'
login_lock_frames=50
login_lock_rescale=1.25
[ "${FS_SMOKE_SCALE:-1}" = 1.25 ] && login_lock_rescale=1

# The lock covers the session from its first frame: no other leg's drive
# could reach anything under it.
leg_login_lock_claims="session"

leg_login_lock_fixture() {
  $convert_bin -size 640x360 xc:"$login_lock_colour" "png:$login_lock_picture"
  mkdir -p "$(dirname "$login_lock_marker")"
  printf '%s\n' "$login_lock_picture" > "$login_lock_marker"
  $convert_bin -size 256x256 xc:"$login_lock_avatar" "png:$iso_home/.face"
  shell_env="${shell_env:+$shell_env }FORMALSHELL_SMOKE_MOTION_SCALE=1000"
}

# The marker sits in the runtime dir every later run's shell reads at its
# start, so it goes however this run ends.
leg_login_lock_cleanup() {
  rm -f "$login_lock_marker"
}

leg_login_lock_timing() {
  leg_timing 100
}

leg_login_lock_drive() {
  local early="$shot_dir/login-lock-early.sh" script="$shot_dir/login-lock-drive.sh"
  write_script "$early" <<EOF
#!/usr/bin/env bash
for _ in \$(seq 1 200); do
  $ipc call lock status 2>/dev/null | "$jq_bin" -e '.secure == true' > /dev/null 2>&1 && break
  sleep 0.05
done
"$hyprctl_bin" eval 'hl.monitor({ output = "", mode = "${out_w}x${out_h}@60", position = "0x0", scale = $login_lock_rescale })' > /dev/null 2>&1
# The nested output starts at the backend's window size and takes its mode a
# moment later, Hyprland stretching the lock's first buffer until the shell
# commits one at the new size: the grab waits for the mode to hold.
last=
for _ in \$(seq 1 40); do
  now=\$("$hyprctl_bin" monitors -j 2>/dev/null | "$jq_bin" -r '.[0] | "\\(.width)x\\(.height)"' 2>/dev/null)
  [ -n "\$now" ] && [ "\$now" = "\$last" ] && break
  last=\$now
  sleep 0.15
done
sleep 0.2
"$grim_bin" "$login_lock_early_png" > /dev/null 2>&1
$ipc call lock status > "$login_lock_early_json" 2>&1
if [ -e "$login_lock_marker" ]; then echo present; else echo gone; fi > "$login_lock_marker_after"
touch "$login_lock_early_done"
EOF
  # Beside the shell's own start, not behind the arrival a drive waits on.
  printf 'hl.on("hyprland.start", function() hl.exec_cmd([==[bash %s]==]) end)\n' "$early"
  write_script "$script" <<EOF
#!/usr/bin/env bash
for _ in \$(seq 1 200); do [ -e "$login_lock_early_done" ] && break; sleep 0.1; done
# The dissolve is 11s at a tenth of its speed, from the backdrop or 2s after
# the first frame, and the card's entrance 2s after it.
burst "$shot_dir/login-lock-burst" $login_lock_frames 0.15
sleep 5
"$grim_bin" "$login_lock_card_png" > /dev/null 2>&1
call debug motionScale 100 > /dev/null 2>&1
"$wtype_bin" -s 300 -k Tab
sleep 1
call lock status > "$login_lock_tab_json" 2>&1
"$grim_bin" "$login_lock_buttons_png" > /dev/null 2>&1
"$wtype_bin" -s 300 -k Tab -k Tab
sleep 0.5
"$wtype_bin" -s 300 -k Return
sleep 1
call lock status > "$login_lock_armed_json" 2>&1
"$grim_bin" "$login_lock_armed_png" > /dev/null 2>&1
"$wtype_bin" -s 300 -k Escape
sleep 0.5
if [ "\${DBUS_SYSTEM_BUS_ADDRESS:-}" = "unix:path=$shot_dir/system-bus/socket" ]; then
  "$wtype_bin" -s 300 -k Tab -k Tab
  sleep 0.5
  "$wtype_bin" -s 300 -k Return
  sleep 0.5
  "$wtype_bin" -s 300 -k Return
  sleep 2
  call lock status > "$login_lock_refused_json" 2>&1
  "$grim_bin" "$login_lock_refused_png" > /dev/null 2>&1
  "$wtype_bin" -s 300 -k Escape
  sleep 0.5
fi
"$wtype_bin" -s 300 "$rig_password" -k Return
sleep 4
call lock status > "$login_lock_after_json" 2>&1
EOF
  hypr_exec_once "bash $script"
}

# Pixels of the fixture's colour (or the one given) in a frame.
login_lock_pixels() {
  colour_px "$1" "${2:-$login_lock_colour}" 15%
}

# dev/dissolve-cells.py's line: at least 90% of the 32 px cells one
# colour, and at least 20 cells of each picture among them.
login_lock_mixed() {
  local cells uniform from to
  cells=$(sed -n 's/.*cells=\([0-9]*\).*/\1/p' <<< "$1")
  uniform=$(sed -n 's/.*uniform=\([0-9]*\).*/\1/p' <<< "$1")
  from=$(sed -n 's/.*from=\([0-9]*\).*/\1/p' <<< "$1")
  to=$(sed -n 's/.* to=\([0-9]*\).*/\1/p' <<< "$1")
  [ "${cells:-0}" -gt 0 ] && [ "$((uniform * 10))" -ge "$((cells * 9))" ] && [ "${from:-0}" -ge 20 ] && [ "${to:-0}" -ge 20 ]
}

leg_login_lock_assert() {
  local f
  for f in "$login_lock_early_png" "$login_lock_card_png" "$login_lock_buttons_png" "$login_lock_armed_png"; do
    [ -f "$f" ] || fail "no screenshot at $f"
  done
  echo "SMOKE_LOGIN_LOCK_EARLY $login_lock_early_png"
  echo "SMOKE_LOGIN_LOCK_CARD $login_lock_card_png"
  echo "SMOKE_LOGIN_LOCK_BUTTONS $login_lock_buttons_png"
  echo "SMOKE_LOGIN_LOCK_ARMED $login_lock_armed_png"
  [ -f "$login_lock_refused_png" ] && echo "SMOKE_LOGIN_LOCK_REFUSED $login_lock_refused_png"

  echo "-- shell phases --"
  grep '^phase ' "$shell_log_path" | head -n 20
  grep -q 'phase t=.* login lock first commit' "$shell_log_path" || fail "the shell logged no first commit of the login lock"
  # The arrival's veil runs under the lock and commits on its configure;
  # the bar and the wallpaper wait for the lock's first frame.
  echo "-- first commits --"
  grep -E '^commit surface=[a-z]+ n=1 ' "$shell_log_path" | cut -d' ' -f1-4
  local first other
  first=$(grep -nE '^commit surface=lock n=1 ' "$shell_log_path" | head -n1 | cut -d: -f1)
  other=$(grep -nE '^commit surface=(bar|wallpaper)[a-z]* n=1 ' "$shell_log_path" | head -n1 | cut -d: -f1)
  [ -n "$first" ] || fail "no lock surface commit in the shell's log"
  if [ -n "$other" ] && [ "$first" -gt "$other" ]; then
    fail "a bar or wallpaper surface committed before the login lock's first frame"
  fi

  cat "$login_lock_early_json"; echo
  expect_jq "$login_lock_early_json" '.locked == true and .secure == true' "the login lock was not up and secure with no lock call" '{locked, secure}'
  expect_jq "$login_lock_early_json" '.boot == true' "lock status did not report the boot lock" '{boot}'
  expect_jq "$login_lock_early_json" '.buttons == ["poweroff", "reboot", "otheruser"]' "the boot lock does not offer Shutdown, Reboot and Other user" '{buttons}'

  # The early frame: the picture whole, at 1:1, centred on black.
  local size w h box early card area
  size=$($convert_bin "$login_lock_early_png" -format '%w %h' info:)
  read -r w h <<< "$size"
  box=$($convert_bin "$login_lock_early_png" -fuzz 15% -fill black +opaque "$login_lock_colour" -format '%@' info: 2>/dev/null)
  early=$(login_lock_pixels "$login_lock_early_png")
  card=$(login_lock_pixels "$login_lock_card_png")
  area=$((640 * 360))
  echo "login lock: output ${w}x${h}, picture box $box, picture pixels early $early card $card of $area"
  [ "$box" = "640x360+$(((w - 640) / 2))+$(((h - 360) / 2))" ] \
    || fail "the early frame's picture is not 640x360 centred at 1:1 (box $box)"
  [ "${early:-0}" -ge $((area * 95 / 100)) ] || fail "the early frame does not show the picture whole: a card over it already ($early of $area)"
  [ "${card:-0}" -le $((area / 100)) ] || fail "the card's frame still shows the splash ($card of $area): it never dissolved"
  local green
  green=$(login_lock_pixels "$login_lock_card_png" "$login_lock_avatar")
  [ "${green:-0}" -ge 1000 ] || fail "the card's frame shows no avatar: no card entered after the dissolve ($green)"

  echo "-- dissolve --"
  local frames max i pink cells mixed="" over=0 last_pink=0 first_card=0
  frames=$(grep -c '^dissolve frame ' "$shell_log_path")
  max=$(grep '^dissolve frame ' "$shell_log_path" | awk '{ if ($3 > m) m = $3 } END { print m + 0 }')
  echo "login lock: $frames dissolve frames, the slowest ${max}ms"
  [ "${frames:-0}" -ge 10 ] || fail "the shell logged $frames dissolve frames: the splash did not dissolve"
  for i in $(seq 1 $login_lock_frames); do
    f="$shot_dir/login-lock-burst-$i.png"
    [ -f "$f" ] || fail "no screenshot at $f"
    pink=$(login_lock_pixels "$f")
    green=$(login_lock_pixels "$f" "$login_lock_avatar")
    [ "${pink:-0}" -gt $((area / 100)) ] && last_pink=$i
    [ "${green:-0}" -gt 0 ] && [ "$first_card" -eq 0 ] && first_card=$i
    if [ "${pink:-0}" -gt $((area / 100)) ] && [ "${green:-0}" -gt 0 ]; then
      over=$((over + 1))
      echo "login lock: frame $i carries the splash ($pink) and the card ($green)"
    fi
    if [ -z "$mixed" ]; then
      cells=$("$python3_bin" "$script_dir/dissolve-cells.py" "$f" "$login_lock_early_png" "$login_lock_card_png")
      if login_lock_mixed "$cells"; then
        mixed=$i
        echo "login lock: frame $i mid-dissolve, $cells"
        echo "SMOKE_LOGIN_LOCK_MID $f"
      fi
    fi
  done
  echo "login lock: the splash last seen in frame $last_pink, the card first in frame $first_card of $login_lock_frames"
  [ "$over" -eq 0 ] || fail "$over frames show the card over the splash"
  [ -n "$mixed" ] || fail "no frame shows 32 px cells of both the splash and the lock: not the pixel dissolve"
  [ "$first_card" -eq 0 ] || [ "$first_card" -gt "$last_pink" ] || fail "the card entered before the splash was gone"

  cat "$login_lock_tab_json"; echo
  expect_jq "$login_lock_tab_json" '.buttonCursor == "poweroff"' "Tab did not reach the Shutdown button" '{buttonCursor}'
  cat "$login_lock_armed_json"; echo
  expect_jq "$login_lock_armed_json" '.armed == "otheruser"' "Return did not arm Other user" '{armed}'
  expect_jq "$login_lock_armed_json" '.locked == true' "arming Other user left the lock" '{locked}'
  if [ -f "$login_lock_refused_json" ]; then
    cat "$login_lock_refused_json"; echo
    expect_jq "$login_lock_refused_json" '.authError == "Reboot refused"' "a Reboot logind cannot take did not say so under the field" '{authError}'
  else
    echo "login lock: the refused Reboot skipped, the system bus is not this run's private one"
  fi

  cat "$login_lock_after_json"; echo
  expect_jq "$login_lock_after_json" '.locked == false' "the real password did not unlock the login lock" '{locked}'
  expect_jq "$login_lock_after_json" '.boot == false' "the boot lock outlived its unlock" '{boot}'
  grep -q '^gone$' "$login_lock_marker_after" || fail "the login marker was still there once the boot lock was up"
}
