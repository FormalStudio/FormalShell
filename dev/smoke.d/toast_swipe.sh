# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --toast-swipe swipes a sticky critical toast the way two fingers on a
# touchpad do, through dev/vpointer.py's `hfinger` frames (horizontal axis,
# axis_source finger) and `hlift` (its axis_stop), with the pointer parked
# on the card. The stack hangs off the right edge (the default
# bottom-right), so the card leaves to the right, which is a negative axis
# value: the card follows a scroll area's content.
#
#   slow   ~22px of card travel at ~75 px/s, a frame and a status read with
#          the fingers still down, then a lift 200ms later: the card was
#          off its place to the right mid-gesture, springs back onto it,
#          and one popup is left.
#   long   ~67px, lifted 200ms after the fingers stopped (no speed left):
#          past sonner's 45px, so it goes and no popup is left.
#   flick  ~17px in three frames 8ms apart, lifted at once: short of 45px
#          but past sonner's 110 px/s, so it goes too.
#   wheel  a sideways wheel notch from wlrctl (no axis_source, a wheel to
#          Hyprland) over a fresh card: nothing moves and one popup is left.
#
# Each card's box is the difference from the frame before any toast, below
# the bar so the bell cell's unread mark stays out of it, cut at 20% so the
# fading card still reads. A new card maps a new surface under a still
# pointer, so the pointer is nudged off and back to send it an enter.
leg_toast_swipe_flag="--toast-swipe"
leg_toast_swipe_order=32
leg_toast_swipe_needs="notify-send convert wlrctl python3"

toast_swipe_vpointer="$PWD/dev/vpointer.py"
toast_swipe_body_top=60
toast_swipe_bare_path="$shot_dir/toast-swipe-bare.png"
toast_swipe_settled_path="$shot_dir/toast-swipe-settled.png"
toast_swipe_slow_mid_path="$shot_dir/toast-swipe-slow-mid.png"
toast_swipe_slow_back_path="$shot_dir/toast-swipe-slow-back.png"
toast_swipe_long_mid_path="$shot_dir/toast-swipe-long-mid.png"
toast_swipe_long_after_path="$shot_dir/toast-swipe-long-after.png"
toast_swipe_flick_after_path="$shot_dir/toast-swipe-flick-after.png"
toast_swipe_wheel_after_path="$shot_dir/toast-swipe-wheel-after.png"
toast_swipe_status_path="$shot_dir/toast-swipe-status.txt"
toast_swipe_dispatch_path="$shot_dir/toast-swipe-dispatch.txt"

leg_toast_swipe_validate() {
  local other
  for other in notify notify_emerge notify_close center reminder capture_edit deform polkit screensaver; do
    if leg_on "$other"; then
      echo "usage: --toast-swipe swipes one notification away and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_toast_swipe_timing() {
  leg_timing 30 70
}

leg_toast_swipe_drive() {
  local script="$shot_dir/toast-swipe-drive.sh"
  local body_h=$((1080 - toast_swipe_body_top))
  write_script "$script" <<EOF
#!/usr/bin/env bash
status() { echo "\$1 \$($ipc call notifications status 2>&1)" >> "$toast_swipe_status_path"; }
vp() { "$python3_bin" "$toast_swipe_vpointer" "\$@" >> "$toast_swipe_dispatch_path" 2>&1; }
nudge() {
  "$wlrctl_bin" pointer move 0 -300 >> "$toast_swipe_dispatch_path" 2>&1
  sleep 0.3
  "$wlrctl_bin" pointer move 0 300 >> "$toast_swipe_dispatch_path" 2>&1
  sleep 0.5
}
sleep 4
"$grim_bin" "$toast_swipe_bare_path" > /dev/null 2>&1
"$notify_send_bin" -u critical 'Swipe me' 'slow, then long'
sleep 3
"$grim_bin" "$toast_swipe_settled_path" > /dev/null 2>&1
rect=\$($convert_bin "$toast_swipe_bare_path" "$toast_swipe_settled_path" \\
  -crop "1920x${body_h}+0+${toast_swipe_body_top}" +repage \\
  -compose difference -composite -threshold 20% -format "%@" info: 2>/dev/null)
w=\${rect%%x*}
h=\$(echo "\$rect" | sed -n 's/^[0-9]*x\\([0-9]*\\)+.*/\\1/p')
x=\$(echo "\$rect" | sed -n 's/.*+\\([0-9]*\\)+[0-9]*\$/\\1/p')
y=\$(echo "\$rect" | sed -n 's/.*+\\([0-9]*\\)\$/\\1/p')
[ -n "\$w" ] && [ -n "\$h" ] && [ -n "\$x" ] && [ -n "\$y" ] || exit 0
"$wlrctl_bin" pointer move -4000 -4000 >> "$toast_swipe_dispatch_path" 2>&1
sleep 0.5
"$wlrctl_bin" pointer move \$((x + w / 2)) \$((y + $toast_swipe_body_top + h / 2)) >> "$toast_swipe_dispatch_path" 2>&1
sleep 1
status parked
vp \$(for _ in \$(seq 10); do printf 'hfinger -2 wait 30 '; done)
"$grim_bin" "$toast_swipe_slow_mid_path" > /dev/null 2>&1
status slow-mid
vp wait 200 hlift
sleep 1.5
"$grim_bin" "$toast_swipe_slow_back_path" > /dev/null 2>&1
status slow-after
vp \$(for _ in \$(seq 10); do printf 'hfinger -6 wait 30 '; done)
"$grim_bin" "$toast_swipe_long_mid_path" > /dev/null 2>&1
vp wait 200 hlift
sleep 1.5
"$grim_bin" "$toast_swipe_long_after_path" > /dev/null 2>&1
status long-after
"$notify_send_bin" -u critical 'Flick me' 'short and fast'
sleep 3
nudge
status flick-before
vp hfinger -5 wait 8 hfinger -5 wait 8 hfinger -5 hlift
sleep 1.5
"$grim_bin" "$toast_swipe_flick_after_path" > /dev/null 2>&1
status flick-after
"$notify_send_bin" -u critical 'Wheel me' 'a notch sideways'
sleep 3
nudge
status wheel-before
"$wlrctl_bin" pointer scroll 0 -15 >> "$toast_swipe_dispatch_path" 2>&1
sleep 1.5
"$grim_bin" "$toast_swipe_wheel_after_path" > /dev/null 2>&1
status wheel-after
echo "rect \$x \$y \$w \$h" >> "$toast_swipe_status_path"
EOF
  hypr_exec_once "bash $script"
}

# The left edge of everything that differs from the bare frame below the
# bar, or nothing when the two match there.
toast_swipe_left() {
  local h=$((1080 - toast_swipe_body_top)) rect
  rect=$($convert_bin "$toast_swipe_bare_path" "$1" -crop "1920x${h}+0+${toast_swipe_body_top}" +repage \
    -compose difference -composite -threshold 20% -format "%@" info: 2>/dev/null)
  echo "$rect" | sed -n 's/.*+\([0-9]*\)+[0-9]*$/\1/p'
}

toast_swipe_popups() {
  sed -n "s/^$1 .*\"popups\":\([0-9]*\).*/\1/p" "$toast_swipe_status_path" 2>/dev/null
}

leg_toast_swipe_assert() {
  local f
  for f in "$toast_swipe_settled_path" "$toast_swipe_slow_mid_path" "$toast_swipe_slow_back_path" \
    "$toast_swipe_long_mid_path" "$toast_swipe_long_after_path" "$toast_swipe_flick_after_path" "$toast_swipe_wheel_after_path"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done
  echo "SMOKE_TOAST_SWIPE_SETTLED $toast_swipe_settled_path"
  echo "SMOKE_TOAST_SWIPE_SLOW_MID $toast_swipe_slow_mid_path"
  echo "SMOKE_TOAST_SWIPE_SLOW_BACK $toast_swipe_slow_back_path"
  echo "SMOKE_TOAST_SWIPE_LONG_MID $toast_swipe_long_mid_path"
  echo "SMOKE_TOAST_SWIPE_LONG_AFTER $toast_swipe_long_after_path"
  echo "SMOKE_TOAST_SWIPE_FLICK_AFTER $toast_swipe_flick_after_path"
  echo "SMOKE_TOAST_SWIPE_WHEEL_AFTER $toast_swipe_wheel_after_path"
  echo "SMOKE_TOAST_SWIPE_STATUS $toast_swipe_status_path"
  # Printed before anything is asserted, so a failing run still says where
  # each frame put the card.
  local rest slow_mid slow_back long_mid wheel
  rest=$(toast_swipe_left "$toast_swipe_settled_path")
  slow_mid=$(toast_swipe_left "$toast_swipe_slow_mid_path")
  slow_back=$(toast_swipe_left "$toast_swipe_slow_back_path")
  long_mid=$(toast_swipe_left "$toast_swipe_long_mid_path")
  wheel=$(toast_swipe_left "$toast_swipe_wheel_after_path")
  echo "card left edge: rest=$rest slow-mid=$slow_mid slow-back=$slow_back long-mid=$long_mid wheel=$wheel"
  cat "$toast_swipe_status_path" 2>/dev/null || true
  [ -n "$rest" ] || fail "could not measure the settled card off the bare/settled pair"
  [ "$(toast_swipe_popups parked)" = 1 ] || fail "expected one popup before any swipe"
  [ -n "$slow_mid" ] && [ "$slow_mid" -ge $((rest + 12)) ] \
    || fail "the slow swipe did not carry the card right mid-gesture: rest $rest, mid ${slow_mid:-none}"
  [ "$(toast_swipe_popups slow-mid)" = 1 ] || fail "the card went before the fingers lifted"
  [ -n "$slow_back" ] && [ "$slow_back" -le $((rest + 2)) ] && [ "$slow_back" -ge $((rest - 2)) ] \
    || fail "the slow swipe did not spring back onto its place: rest $rest, after ${slow_back:-none}"
  [ "$(toast_swipe_popups slow-after)" = 1 ] || fail "a 22px slow swipe dismissed the card"
  [ -n "$long_mid" ] && [ "$long_mid" -ge $((rest + 40)) ] \
    || fail "the long swipe did not carry the card right: rest $rest, mid ${long_mid:-none}"
  [ "$(toast_swipe_popups long-after)" = 0 ] || fail "a 67px swipe left the card up"
  [ "$(toast_swipe_popups flick-before)" = 1 ] || fail "the flick's card never popped up"
  [ "$(toast_swipe_popups flick-after)" = 0 ] || fail "a fast 17px flick left the card up"
  [ "$(toast_swipe_popups wheel-before)" = 1 ] || fail "the wheel's card never popped up"
  [ "$(toast_swipe_popups wheel-after)" = 1 ] || fail "a sideways wheel notch dismissed the card"
  [ "$wheel" = "$rest" ] || fail "a sideways wheel notch moved the card: rest $rest, after ${wheel:-none}"
}
