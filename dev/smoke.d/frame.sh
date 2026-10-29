# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --frame pins `frame.thickness` (10) in the settings fixture, so the screen
# frame paints round the bar for the run's own frame, and rides whichever
# bar edge the run has (`--bar-position left --frame` is the frame wrapping
# a left bar). What the screenshot cannot say is asserted off `hyprctl -j
# layers`: the bar's own window grown to the size of the output (it paints
# the ring), and a formalshell:frame-zone on every edge, since
# those zones are what keep windows inside the cut-out.
#
# The last assert is on the wire instead, which is why this leg turns
# WAYLAND_DEBUG on: the framed bar has to reserve nothing at all, and the
# only place that shows is the exclusive zone it sent. A window that kept a
# strip's zone still arranges before the overlay zones reserve, so it lands
# on the whole output anyway and every pixel and every layer box above look
# right, until the same shell starts with the config already in place and
# the zone it froze on is 0. That was the bar drawing its ring one bar
# thickness in from the left edge and off the right one (e1504g,
# 2026-08-26); -1 is the only value that holds either way.
#
# The key is read under every table, so the same holds riding --pantheon.
# Ridden by --bar-style corners the ring's own fillets are the
# corners (the style adds no fillet of its own and no layer): on a top bar
# the pixel just inside each end of the band's inner edge, where the ring
# curves into the cut-out, is the solid band's black.
leg_frame_flag="--frame"
leg_frame_order=190
leg_frame_needs="jq convert"
leg_frame_wayland_debug=1

frame_layers_path="$shot_dir/frame-layers.json"
frame_monitors_path="$shot_dir/frame-monitors.json"
frame_dump_path="$shot_dir/frame-dump.json"
frame_shot_path="$shot_dir/frame.png"

leg_frame_fixture() {
  settings_fragment ', "frame": {"thickness": 10, "radius": 20}'
}

leg_frame_timing() {
  leg_timing 8 30
}

leg_frame_drive() {
  local script="$shot_dir/frame-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
sleep 5
"$hyprctl_bin" -j layers > "$frame_layers_path" 2>&1
"$hyprctl_bin" -j monitors > "$frame_monitors_path" 2>&1
$ipc call debug dump > "$frame_dump_path" 2>&1
"$grim_bin" "$frame_shot_path" > /dev/null 2>&1
EOS
  hypr_exec_once "bash $script"
}

leg_frame_assert() {
  local box mw mh zones edge surface zone state
  if [ ! -s "$frame_layers_path" ] || [ ! -s "$frame_monitors_path" ]; then
    fail "no layer or monitor dump produced for the frame check"
  fi
  box=$("$jq_bin" -r '[.[] | .levels[] | .[] | select(.namespace == "formalshell:bar")] | first | "\(.x) \(.y) \(.w) \(.h)"' "$frame_layers_path" 2>/dev/null)
  read -r mw mh <<< "$("$jq_bin" -r 'first | "\(.width / .scale | floor) \(.height / .scale | floor)"' "$frame_monitors_path")"
  zones=$("$jq_bin" -r '[.[] | .levels[] | .[] | select(.namespace == "formalshell:frame-zone")] | length' "$frame_layers_path")
  [ -s "$frame_dump_path" ] || fail "no debug dump produced for the frame check"
  state=$("$jq_bin" -c '.frame' "$frame_dump_path" 2>/dev/null)
  echo "bar window: $box on ${mw}x${mh}, frame zones: $zones, frame: $state"

  "$jq_bin" -e '.frame.enabled == true and .frame.thickness == 10' \
    "$frame_dump_path" > /dev/null 2>&1 \
    || fail "the framed look should report a 10px ring it drew, got: $state"
  if [ "$box" != "0 0 $mw $mh" ]; then
    fail "with the frame on the bar's window should span the whole output, got: $box"
  fi
  # One per edge, the bar's included: its window reserves nothing itself
  # while it is the whole output.
  if [ "$zones" -ne 4 ]; then
    fail "expected four frame exclusion zones (one per edge), got $zones"
  fi
  if leg_on bar_position; then
    edge=$(leg_arg bar_position)
  else
    edge=top
  fi
  echo "frame wraps a $edge bar"

  # The bar's own layer surface, by the namespace it was created under, and
  # the last zone it sent on it. The last one, not the first: it maps as an
  # unframed strip reserving its own thickness and only turns into the whole
  # output once settings.json lands. libwayland writes an object as
  # `interface#id` and a new one as `new id interface#id`; the rust shell's
  # wayland-backend writes both as `interface@id`.
  surface=$(grep 'get_layer_surface' "$shell_log_path" 2>/dev/null | grep '"formalshell:bar"' | tail -1 \
    | sed -n 's/.*(\(new id \)\{0,1\}zwlr_layer_surface_v1[#@]\([0-9][0-9]*\).*/\2/p')
  [ -n "$surface" ] || fail "no formalshell:bar layer surface in the shell's wire log"
  zone=$(grep -o "zwlr_layer_surface_v1[#@]$surface\.set_exclusive_zone([-0-9]*)" "$shell_log_path" \
    | tail -1 | sed -n 's/.*(\(-\{0,1\}[0-9][0-9]*\)).*/\1/p')
  echo "framed bar exclusive zone: $zone"
  if [ "$zone" != "-1" ]; then
    fail "the framed bar has to ignore the frame zones' reservations (-1), it last sent: ${zone:-none}"
  fi

  if leg_on bar_style && [ "$(leg_arg bar_style)" = corners ] && [ "$edge" = top ]; then
    local t x px
    [ -f "$frame_shot_path" ] || fail "no frame produced at $frame_shot_path"
    echo "SMOKE_FRAME_CORNERS $frame_shot_path"
    t=$("$jq_bin" -r '.theme.edgeInset.top' "$frame_dump_path")
    for x in 11 $((mw - 12)); do
      px=$($convert_bin "$frame_shot_path" -crop "1x1+$x+$((t + 1))" +repage -format '%[fx:int(r*255+0.5)]' info: 2>/dev/null)
      echo "ring fillet ($x,$((t + 1))): $px"
      [ -n "$px" ] && [ "$px" -le 12 ] || fail "no fillet where the band meets the ring at ($x,$((t + 1))): read ${px:-nothing}, not the solid band's black"
    done
  fi
}
