# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --tooltip-travel rides on --panel <name>, like --tooltip, and proves the
# other half of the group (M53 D10): the card is one surface per output that
# changes hands rather than one per item. It parks on the panel header's
# close button, waits the 400ms show delay out, then moves one button left
# onto the rescan button.
#
# The layer dump lands a quarter of a second after that move, and a quarter
# second is the discriminator: a card that had gone back to paying its own
# delay at every item would still be 150ms from appearing, so a tooltip
# surface in that second dump is the grace window doing its job. The layer
# count either side is what says there is one card and not two.
#
# The frame comes later than the dump, not with it: the card travels between
# the two anchors on the fast spatial clock (M54 D2), which outlasts the
# dump's own quarter second, so a frame taken there would catch the card
# between the buttons and say nothing about where it settled.
#
# Its own leg rather than two more parks inside --tooltip: that leg owns the
# pointer for its own run, and a second drive script moving the same pointer
# under it would decide neither claim.
leg_tooltip_travel_flag="--tooltip-travel"
leg_tooltip_travel_order=81
leg_tooltip_travel_needs="wlrctl jq"
# The panel it rides on has already dropped the base fixture window.
leg_tooltip_travel_fixture_window=keep

tooltip_travel_dispatch_path="$shot_dir/tooltip-travel-dispatch.txt"
tooltip_travel_layers_first_path="$shot_dir/tooltip-travel-layers-first.json"
tooltip_travel_layers_second_path="$shot_dir/tooltip-travel-layers-second.json"
tooltip_travel_first_path="$shot_dir/panel-tooltip-first.png"
tooltip_travel_second_path="$shot_dir/panel-tooltip-travelled.png"

leg_tooltip_travel_validate() {
  if ! leg_on panel; then
    echo "usage: $0 --panel <name> --tooltip-travel" >&2
    exit 1
  fi
  if leg_on tooltip; then
    echo "usage: --tooltip and --tooltip-travel drive the same pointer, run them separately" >&2
    exit 1
  fi
}

leg_tooltip_travel_timing() {
  # One second more tail than --tooltip's own: the drive script waits the
  # card's travel out between the layer dump and the frame.
  leg_timing 0 0 9
}

leg_tooltip_travel_drive() {
  # 34 in from the right and 74 down is the panel header's close button and
  # 40 left of it the rescan button; --tooltip's own header carries where those come from and why
  # this is wlrctl rather than a cursor warp. The second move is relative to
  # the first, so it needs no second slam into the corner, and it leaves the
  # close button and enters the rescan button in one motion, which is what
  # puts the show inside the group's grace window.
  local script="$shot_dir/tooltip-travel-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 9
"$wlrctl_bin" pointer move -4000 -4000 > "$tooltip_travel_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer move $((logical_w - 34)) 74 >> "$tooltip_travel_dispatch_path" 2>&1
sleep 2
"$hyprctl_bin" cursorpos >> "$tooltip_travel_dispatch_path" 2>&1
"$hyprctl_bin" -j layers > "$tooltip_travel_layers_first_path" 2>&1
"$grim_bin" -c "$tooltip_travel_first_path" > /dev/null 2>&1
"$wlrctl_bin" pointer move -40 0 >> "$tooltip_travel_dispatch_path" 2>&1
sleep 0.25
"$hyprctl_bin" cursorpos >> "$tooltip_travel_dispatch_path" 2>&1
"$hyprctl_bin" -j layers > "$tooltip_travel_layers_second_path" 2>&1
sleep 1
"$grim_bin" -c "$tooltip_travel_second_path" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

tooltip_travel_count() {
  "$jq_bin" '[.[].levels[]?[]? | select(.namespace == "formalshell:tooltip")] | length' "$1" 2>/dev/null
}

leg_tooltip_travel_assert() {
  local f first second
  for f in "$tooltip_travel_layers_first_path" "$tooltip_travel_layers_second_path"; do
    if [ ! -s "$f" ]; then
      fail "no layer dump produced at $f"
    fi
  done
  # Printed on the happy path too: it is the only evidence of WHERE the
  # pointer ended up, which the frames alone cannot be trusted for.
  cat "$tooltip_travel_dispatch_path" 2>/dev/null || true
  first=$(tooltip_travel_count "$tooltip_travel_layers_first_path")
  second=$(tooltip_travel_count "$tooltip_travel_layers_second_path")
  if [ "$first" != "1" ]; then
    fail "parking on the close button mapped $first tooltip surfaces, not one"
  fi
  if [ "$second" != "1" ]; then
    fail "a quarter second after moving to the rescan button there were $second tooltip surfaces: the card either paid the show delay again or a second one was built"
  fi
  for f in "$tooltip_travel_first_path" "$tooltip_travel_second_path"; do
    if [ ! -f "$f" ]; then
      fail "no screenshot produced at $f"
    fi
  done
  if cmp -s "$tooltip_travel_first_path" "$tooltip_travel_second_path"; then
    fail "the two frames are byte-identical: the card never left the close button"
  fi
  echo "SMOKE_TOOLTIP_FIRST $tooltip_travel_first_path"
  echo "SMOKE_TOOLTIP_TRAVELLED $tooltip_travel_second_path"
}
