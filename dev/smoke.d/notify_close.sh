# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --notify-close closes one sticky critical notification the way a hand does:
# a real pointer parked on the card's own close button, then a real click.
# `notifications status` has to count one popup with the pointer resting on
# the button and none after the click.
#
# What it guards: the card's hover has to keep reporting with the pointer on
# the close button. The bubble's button only exists while the card is
# hovered, so a hover that drops out over it fades the button away under the
# pointer and the click lands on the body instead, which focuses the sender
# and leaves a sticky card where it was.
#
# The button's place is measured, not hardcoded: the card's rect is the
# difference between a frame with no card and the settled one, cut at 20% so
# it is the `destructive` border's own box and a bubble's cast stays out of
# it, and the button is controlHeight square against panelPadding in from the
# top right corner of that.
#
# wlrctl for the reason tooltip.sh documents: only a real virtual-pointer
# client sends the surface a pointer enter.
leg_notify_close_flag="--notify-close"
leg_notify_close_order=32
leg_notify_close_needs="notify-send convert wlrctl"

notify_close_bare_path="$shot_dir/notify-close-bare.png"
notify_close_settled_path="$shot_dir/notify-close-settled.png"
notify_close_parked_path="$shot_dir/notify-close-parked.png"
notify_close_after_path="$shot_dir/notify-close-after.png"
notify_close_rect_path="$shot_dir/notify-close-rect.txt"
notify_close_dispatch_path="$shot_dir/notify-close-dispatch.txt"
notify_close_status_parked_path="$shot_dir/notify-close-status-parked.txt"
notify_close_status_after_path="$shot_dir/notify-close-status-after.txt"

# Below the bar, so the bell cell's own unread mark is outside the box.
notify_close_body_top=60
# panelPadding plus half a controlHeight (shell/Theme/tokens.js).
notify_close_button_inset=28

leg_notify_close_validate() {
  local other
  for other in notify notify_emerge center reminder capture_edit deform polkit screensaver; do
    if leg_on "$other"; then
      echo "usage: --notify-close clicks one notification's own button and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_notify_close_timing() {
  leg_timing 16 40
}

leg_notify_close_drive() {
  local script="$shot_dir/notify-close-drive.sh"
  local body_h=$((1080 - notify_close_body_top))
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
sleep 4
"$grim_bin" "$notify_close_bare_path" > /dev/null 2>&1
"$notify_send_bin" -u critical 'Command failed' 'exit status 1'
sleep 3
"$grim_bin" "$notify_close_settled_path" > /dev/null 2>&1
rect=\$($convert_bin "$notify_close_bare_path" "$notify_close_settled_path" \\
  -crop "1920x${body_h}+0+${notify_close_body_top}" +repage \\
  -compose difference -composite -threshold 20% -format "%@" info: 2>/dev/null)
echo "\$rect" > "$notify_close_rect_path"
w=\${rect%%x*}
x=\$(echo "\$rect" | sed -n 's/.*+\\([0-9]*\\)+[0-9]*\$/\\1/p')
y=\$(echo "\$rect" | sed -n 's/.*+\\([0-9]*\\)\$/\\1/p')
[ -n "\$w" ] && [ -n "\$x" ] && [ -n "\$y" ] || exit 0
bx=\$((x + w - $notify_close_button_inset))
by=\$((y + $notify_close_body_top + $notify_close_button_inset))
echo "button \$bx \$by" >> "$notify_close_rect_path"
"$wlrctl_bin" pointer move -4000 -4000 > "$notify_close_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer move \$bx \$by >> "$notify_close_dispatch_path" 2>&1
sleep 2
"$hyprctl_bin" cursorpos >> "$notify_close_dispatch_path" 2>&1
"$grim_bin" -c "$notify_close_parked_path" > /dev/null 2>&1
call notifications status > "$notify_close_status_parked_path" 2>&1
"$wlrctl_bin" pointer click left >> "$notify_close_dispatch_path" 2>&1
sleep 2
call notifications status > "$notify_close_status_after_path" 2>&1
"$grim_bin" -c "$notify_close_after_path" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_notify_close_assert() {
  local f
  for f in "$notify_close_settled_path" "$notify_close_parked_path" "$notify_close_after_path"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done
  echo "SMOKE_NOTIFY_CLOSE_SETTLED $notify_close_settled_path"
  echo "SMOKE_NOTIFY_CLOSE_PARKED $notify_close_parked_path"
  echo "SMOKE_NOTIFY_CLOSE_AFTER $notify_close_after_path"
  # Printed before anything is asserted: where the card was measured and
  # where the pointer ended up have to survive a run whose assertions fail.
  echo "card rect and button: $(tr '\n' ' ' < "$notify_close_rect_path" 2>/dev/null)"
  cat "$notify_close_dispatch_path" 2>/dev/null || true
  grep -q '^button ' "$notify_close_rect_path" 2>/dev/null \
    || fail "could not measure the notification off the bare/settled pair, got '$(head -n 1 "$notify_close_rect_path" 2>/dev/null)'"
  grep -q '"popups":1' "$notify_close_status_parked_path" 2>/dev/null \
    || fail "expected one popup with the pointer parked on its close button, got: $(cat "$notify_close_status_parked_path" 2>/dev/null)"
  grep -q '"popups":0' "$notify_close_status_after_path" 2>/dev/null \
    || fail "the click on the close button left the notification up, got: $(cat "$notify_close_status_after_path" 2>/dev/null)"
}
