# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --tooltip rides on --panel <name>: after the panel's own frame is taken,
# it parks the pointer on the panel header's close button and proves the
# tooltip both maps and anchors there. `hyprctl -j layers` is dumped either
# side of the park, and the formalshell:tooltip namespace has to be absent
# before and present after: a tooltip that still suppressed itself under an
# open panel (the pre-M44 rule) would leave the second dump looking like the
# first. It anchors to a panel header's own close button, so it has nothing
# to park on without a panel open.
leg_tooltip_flag="--tooltip"
leg_tooltip_order=80
leg_tooltip_needs="wlrctl"
# The panel it rides on has already dropped the base fixture window.
leg_tooltip_fixture_window=keep

tooltip_dispatch_path="$shot_dir/tooltip-dispatch.txt"
tooltip_layers_before_path="$shot_dir/tooltip-layers-before.json"
tooltip_layers_after_path="$shot_dir/tooltip-layers-after.json"
tooltip_path="$shot_dir/panel-tooltip.png"

leg_tooltip_validate() {
  if ! leg_on panel; then
    echo "usage: $0 --panel <name> --tooltip" >&2
    exit 1
  fi
}

leg_tooltip_timing() {
  # The tooltip frame lands at 12, a beat past the card's 400ms delay.
  leg_timing 0 0 8
}

leg_tooltip_drive() {
  # 34 in from the right and 74 down, in logical pixels, is the panel
  # header's close button, and it is the same point whatever the panel's
  # width: the panel pins the frame's RIGHT edge at screen.width - barMargin, the Card insets by panelPadding, and the
  # button is controlHeight square against that inset edge, verticalCentred
  # in a header that starts panelPadding below a frame top of
  # barHeight + barMargin.
  #
  # wlrctl, not `hyprctl dispatch movecursor`: the dispatcher warps the
  # cursor (`hyprctl cursorpos` reads the target back, and `grim -c` draws
  # it on the button) without sending the surface under it a pointer enter,
  # so nothing hover-driven ever fires. wlrctl is a real
  # zwlr_virtual_pointer_v1 client, the same line the lock leg's wtype draws
  # for the keyboard. Its protocol is relative only, so the pointer is first
  # slammed into the top-left corner, which the compositor clamps, and the
  # target is then one move from a known origin.
  local script="$shot_dir/tooltip-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 9
"$hyprctl_bin" -j layers > "$tooltip_layers_before_path" 2>&1
"$wlrctl_bin" pointer move -4000 -4000 > "$tooltip_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer move $((logical_w - 34)) 74 >> "$tooltip_dispatch_path" 2>&1
sleep 2
"$hyprctl_bin" cursorpos >> "$tooltip_dispatch_path" 2>&1
"$hyprctl_bin" -j layers > "$tooltip_layers_after_path" 2>&1
"$grim_bin" -c "$tooltip_path" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_tooltip_assert() {
  local f
  for f in "$tooltip_layers_before_path" "$tooltip_layers_after_path"; do
    if [ ! -s "$f" ]; then
      fail "no layer dump produced at $f"
    fi
  done
  # The surface does not exist at all until a cell's own lazy Loader arms
  # it, so its absence here is what makes the second dump mean something.
  if grep -q 'formalshell:tooltip' "$tooltip_layers_before_path"; then
    fail "a tooltip layer surface was already mapped before the pointer parked on anything"
  fi
  if ! grep -q 'formalshell:tooltip' "$tooltip_layers_after_path"; then
    echo "--- pointer moves, then cursorpos ---" >&2
    cat "$tooltip_dispatch_path" >&2 2>/dev/null || true
    echo "--- layer namespaces after the park ---" >&2
    grep -o '"namespace": "[^"]*"' "$tooltip_layers_after_path" >&2 || true
    fail "no formalshell:tooltip layer surface after parking the pointer on the panel header's close button"
  fi
  # Printed on the happy path too: it is the only evidence of WHERE the
  # pointer ended up, which the frame alone cannot be trusted for.
  cat "$tooltip_dispatch_path" 2>/dev/null || true
  if [ ! -f "$tooltip_path" ]; then
    fail "no panel-tooltip screenshot produced"
  fi
  echo "SMOKE_TOOLTIP $tooltip_path"
}
