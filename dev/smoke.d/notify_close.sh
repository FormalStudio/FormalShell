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
# The button's place is read, not hardcoded: the card's rect off `debug
# dump`'s `toastCards`, in logical pixels as the pointer moves, and the
# button is controlHeight square against panelPadding in from its top right
# corner.
#
# wlrctl for the reason tooltip.sh documents: only a real virtual-pointer
# client sends the surface a pointer enter.
leg_notify_close_flag="--notify-close"
leg_notify_close_order=32
leg_notify_close_needs="notify-send wlrctl jq"
leg_notify_close_claims="toasts pointer screen"

notify_close_settled_path="$shot_dir/notify-close-settled.png"
notify_close_parked_path="$shot_dir/notify-close-parked.png"
notify_close_after_path="$shot_dir/notify-close-after.png"
notify_close_rect_path="$shot_dir/notify-close-rect.txt"
notify_close_dump_path="$shot_dir/notify-close-dump.json"
notify_close_dispatch_path="$shot_dir/notify-close-dispatch.txt"
notify_close_status_parked_path="$shot_dir/notify-close-status-parked.txt"
notify_close_status_after_path="$shot_dir/notify-close-status-after.txt"

# panelPadding plus half a controlHeight (shell/Theme/tokens.js).
notify_close_button_inset=28

leg_notify_close_timing() {
  leg_timing 40
}

leg_notify_close_drive() {
  local script="$shot_dir/notify-close-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$notify_send_bin" -u critical 'Command failed' 'exit status 1'
sleep 3
"$grim_bin" "$notify_close_settled_path" > /dev/null 2>&1
call debug dump > "$notify_close_dump_path" 2>&1
"$jq_bin" -r '.toastCards[0].rect | "\(.width)x\(.height)+\(.x)+\(.y)", "button \(.x + .width - $notify_close_button_inset) \(.y + $notify_close_button_inset)"' "$notify_close_dump_path" > "$notify_close_rect_path" 2>&1
read -r _ bx by < <(grep '^button ' "$notify_close_rect_path")
[ -n "\$bx" ] && [ -n "\$by" ] || exit 0
park > "$notify_close_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer move \$bx \$by >> "$notify_close_dispatch_path" 2>&1
sleep 2
"$hyprctl_bin" cursorpos >> "$notify_close_dispatch_path" 2>&1
"$grim_bin" -c "$notify_close_parked_path" > /dev/null 2>&1
call notifications status > "$notify_close_status_parked_path" 2>&1
click >> "$notify_close_dispatch_path" 2>&1
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
    || fail "no toast card in debug dump's toastCards: $(head -c 300 "$notify_close_dump_path" 2>/dev/null)"
  grep -q '"popups":1' "$notify_close_status_parked_path" 2>/dev/null \
    || fail "expected one popup with the pointer parked on its close button, got: $(cat "$notify_close_status_parked_path" 2>/dev/null)"
  grep -q '"popups":0' "$notify_close_status_after_path" 2>/dev/null \
    || fail "the click on the close button left the notification up, got: $(cat "$notify_close_status_after_path" 2>/dev/null)"
}
