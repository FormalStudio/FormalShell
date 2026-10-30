# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --login-lock: the boot's lock (docs/superpowers/specs/2026-10-30-locked-login.md)
# raised by the shell itself at start, with no `lock lock` anywhere. The
# login marker is written before the shell starts, naming a flat-coloured
# picture smaller than the output, and the shell runs at a hundredth of its
# speed (FORMALSHELL_SMOKE_MOTION_SCALE, since the first frames come before
# any IPC call can reach it), so the card's fade is long enough to catch.
#
# An early script, started beside the shell rather than after the arrival
# like a drive, polls `lock status` until the lock is secure and grabs the
# first frame: the picture at 1:1 in the output's centre on black, no card.
# The drive then waits out the fade for the card over the picture, Tabs
# onto the buttons (`buttonCursor`, `armed`), arms Other user, confirms a
# Reboot logind refuses (the nested run's system bus is a private, empty
# one, checked before the confirm is typed), and types the real password:
# unlocked, and the marker gone.
leg_login_lock_flag="--login-lock"
leg_login_lock_order=113
leg_login_lock_needs="wtype convert"

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

leg_login_lock_validate() {
  # The lock covers the session from its first frame: no other leg's drive
  # could reach anything under it.
  local v
  for v in $(compgen -v | grep -E '^leg_[a-z0-9_]+_on$'); do
    if [ "$v" != leg_login_lock_on ] && [ "${!v}" = true ]; then
      echo "usage: --login-lock locks the session from its start and cannot combine with --$(echo "${v#leg_}" | sed 's/_on$//; s/_/-/g')" >&2
      exit 1
    fi
  done
}

leg_login_lock_fixture() {
  $convert_bin -size 640x360 xc:"$login_lock_colour" "png:$login_lock_picture"
  mkdir -p "$(dirname "$login_lock_marker")"
  printf '%s\n' "$login_lock_picture" > "$login_lock_marker"
  add_cleanup "rm -f '$login_lock_marker'"
  $convert_bin -size 256x256 xc:'#1FB86A' "png:$iso_home/.face"
  shell_env="${shell_env:+$shell_env }FORMALSHELL_SMOKE_MOTION_SCALE=10000"
}

leg_login_lock_timing() {
  leg_timing 60 100
}

leg_login_lock_drive() {
  local early="$shot_dir/login-lock-early.sh" script="$shot_dir/login-lock-drive.sh"
  write_script "$early" <<EOF
#!/usr/bin/env bash
for _ in \$(seq 1 200); do
  $ipc call lock status 2>/dev/null | grep -q '"secure":true' && break
  sleep 0.05
done
# The nested output starts at the backend's window size and takes its mode a
# moment later, Hyprland stretching the lock's first buffer until the shell
# commits one at the new size: the grab waits for the mode to hold.
last=
for _ in \$(seq 1 40); do
  now=\$("$hyprctl_bin" monitors -j 2>/dev/null | tr -d ' \\n' | grep -o '"width":[0-9]*,"height":[0-9]*' | head -n 1)
  [ -n "\$now" ] && [ "\$now" = "\$last" ] && break
  last=\$now
  sleep 0.15
done
sleep 0.2
"$grim_bin" "$login_lock_early_png" > /dev/null 2>&1
$ipc call lock status > "$login_lock_early_json" 2>&1
touch "$login_lock_early_done"
EOF
  # Beside the shell's own start, not behind the arrival a drive waits on.
  printf 'hl.on("hyprland.start", function() hl.exec_cmd([==[bash %s]==]) end)\n' "$early"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
for _ in \$(seq 1 200); do [ -e "$login_lock_early_done" ] && break; sleep 0.1; done
# The effects clock at a hundredth of its speed is 20s from the first frame.
sleep 24
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
"$wtype_bin" -s 300 "formalshell-test" -k Return
sleep 4
call lock status > "$login_lock_after_json" 2>&1
if [ -e "$login_lock_marker" ]; then echo present; else echo gone; fi > "$login_lock_marker_after"
EOF
  hypr_exec_once "bash $script"
}

# Pixels of the fixture's colour in a frame.
login_lock_pixels() {
  $convert_bin "$1" -fuzz 15% -fill black +opaque "$login_lock_colour" -fill white -opaque "$login_lock_colour" -format '%[fx:int(mean*w*h)]' info: 2>/dev/null
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
  grep -q '"locked":true' "$login_lock_early_json" && grep -q '"secure":true' "$login_lock_early_json" \
    || fail "the login lock was not up and secure with no lock call. Got: $(cat "$login_lock_early_json")"
  grep -q '"boot":true' "$login_lock_early_json" || fail "lock status did not report the boot lock"
  grep -q '"buttons":\["poweroff","reboot","otheruser"\]' "$login_lock_early_json" \
    || fail "the boot lock does not offer Shutdown, Reboot and Other user. Got: $(cat "$login_lock_early_json")"

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
  [ "${card:-0}" -le $((area / 2)) ] || fail "the later frame shows no card over the picture ($card of $area)"
  [ "${card:-0}" -ge 1 ] || fail "the later frame lost the picture under the card"

  cat "$login_lock_tab_json"; echo
  grep -q '"buttonCursor":"poweroff"' "$login_lock_tab_json" || fail "Tab did not reach the Shutdown button. Got: $(cat "$login_lock_tab_json")"
  cat "$login_lock_armed_json"; echo
  grep -q '"armed":"otheruser"' "$login_lock_armed_json" || fail "Return did not arm Other user. Got: $(cat "$login_lock_armed_json")"
  grep -q '"locked":true' "$login_lock_armed_json" || fail "arming Other user left the lock"
  if [ -f "$login_lock_refused_json" ]; then
    cat "$login_lock_refused_json"; echo
    grep -q '"authError":"Reboot refused"' "$login_lock_refused_json" \
      || fail "a Reboot logind cannot take did not say so under the field. Got: $(cat "$login_lock_refused_json")"
  else
    echo "login lock: the refused Reboot skipped, the system bus is not this run's private one"
  fi

  cat "$login_lock_after_json"; echo
  grep -q '"locked":false' "$login_lock_after_json" || fail "the real password did not unlock the login lock. Got: $(cat "$login_lock_after_json")"
  grep -q '"boot":false' "$login_lock_after_json" || fail "the boot lock outlived its unlock"
  grep -q '^gone$' "$login_lock_marker_after" || fail "the login marker is still there after the unlock"
}
