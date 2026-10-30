# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --deform proves the velocity deform (M54 D7) unwinds: the last frame of an
# open is the same rect as a card that has been sitting there for a second.
# The table's curves never overshoot (formalshell.json's motion note), so a
# frame past the resting card is not asked for.
#
# One number off one burst, the card's own silhouette in the band under the
# bar's line, measured as the bounding box of everything that differs between
# a frame and the same output with the panel shut:
#
#   rest    the last frame of the burst is the resting box exactly, all four
#           numbers. The springs are still integrating well past the travel
#           they were driven by (stiffness 200, damping 16, so a couple of
#           tenths), which is why that sample sits three clocks out rather
#           than at the end of the travel.
#
# The resting box is read off the settled frame of the same open, two
# seconds past the burst, never off the first open: the network list is
# live, and a scan that adds a row between the two opens rests the card a
# row taller (e1504g, 11 networks then 12, 32px).
#
# The band starts one row under the bar's own line: a joined card opens a gap
# in that line and the cell that owns the panel draws a mark of its own, and
# both are differences that are not the card. Everything from there down is,
# including the two fillets, which is why the box is a box and not a height.
#
# The samples are armed BEFORE the ipc call: grim's own start-up costs about
# as much as the first half of the travel, so a capture asked for once the
# call has returned lands past it (panel_emerge.sh's finding).
leg_deform_flag="--deform"
leg_deform_order=77
leg_deform_needs="convert"

deform_open_path="$shot_dir/deform-open.txt"
deform_close_path="$shot_dir/deform-close.txt"
deform_reopen_path="$shot_dir/deform-reopen.txt"
deform_state_path="$shot_dir/deform-state.txt"

deform_rest_path="$shot_dir/deform-rest.png"
deform_bare_path="$shot_dir/deform-bare.png"
deform_settled_path="$shot_dir/deform-settled.png"
deform_mid_paths=(
  "$shot_dir/deform-mid-1.png"
  "$shot_dir/deform-mid-2.png"
  "$shot_dir/deform-mid-3.png"
  "$shot_dir/deform-mid-4.png"
  "$shot_dir/deform-mid-5.png"
  "$shot_dir/deform-mid-6.png"
  "$shot_dir/deform-mid-7.png"
  "$shot_dir/deform-mid-8.png"
  "$shot_dir/deform-mid-9.png"
)
# The travel's own clock (Theme.motion.spatial, M54 D2) and the moment this
# rig has the window up, in milliseconds; every sample is the map plus a
# fraction of that clock, so a change to the token moves the sampling with it.
# The last one is three clocks out, past the springs as well as the travel.
deform_clock_ms=500
deform_map_ms=200
deform_mid_fractions=(0 20 40 60 75 90 110 150 300)
deform_mid_sleeps=()
for deform_f in "${deform_mid_fractions[@]}"; do
  deform_at=$((deform_map_ms + deform_f * deform_clock_ms / 100))
  deform_mid_sleeps+=("$(printf '%d.%03d' $((deform_at / 1000)) $((deform_at % 1000)))")
done

# One row under the bar's line (the strip is 40 tall and the line is its last
# row), so nothing the bar draws is in the measurement.
deform_band_top=41

leg_deform_validate() {
  local other
  for other in bar_layout bar_position center chevron chevron_quiet config_reload frame \
    fullscreen gallery lock menu notify panel panel_anchor panel_at panel_emerge \
    panel_handoff panel_keys screensaver toggles tray_overflow wallpaper; do
    if leg_on "$other"; then
      echo "usage: --deform measures one panel's own frames and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_deform_timing() {
  leg_timing 14 46
}

leg_deform_drive() {
  local script="$shot_dir/deform-drive.sh" i arm=""
  for i in "${!deform_mid_paths[@]}"; do
    arm+="( sleep ${deform_mid_sleeps[$i]}; \"$grim_bin\" \"${deform_mid_paths[$i]}\" > /dev/null 2>&1 ) &"$'\n'
  done
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
# The card at rest, and the same output without it: between them they say
# where the card is and what the desktop under it looks like.
$ipc call panel open network > "$deform_open_path" 2>&1
sleep 2
"$grim_bin" "$deform_rest_path" > /dev/null 2>&1
$ipc call panel toggle network > "$deform_close_path" 2>&1
sleep 2
"$grim_bin" "$deform_bare_path" > /dev/null 2>&1
sleep 1
$arm
$ipc call panel open network > "$deform_reopen_path" 2>&1
wait
sleep 2
"$grim_bin" "$deform_settled_path" > /dev/null 2>&1
$ipc call panel state > "$deform_state_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

# The card's box in one frame: everything under the bar's line that differs
# from the closed output. Empty (0x0+0+0) when there is no card in it, which
# is what a sample taken before the window is up reads as.
deform_box() {
  local frame="$1" name="$2" body bare_body box
  body="$shot_dir/deform-body-$name.png"
  bare_body="$shot_dir/deform-body-bare.png"
  $convert_bin "$frame" -crop "${out_w}x$((out_h - deform_band_top))+0+${deform_band_top}" +repage "$body" > /dev/null 2>&1
  [ -s "$body" ] || fail "could not crop the band out of $frame"
  box=$($convert_bin "$bare_body" "$body" -compose difference -composite \
    -threshold 8% -format "%@" info: 2>/dev/null)
  case "$box" in
    [0-9]*x[0-9]*+[0-9]*+[0-9]*) echo "$box" ;;
    *) echo "0x0+0+0" ;;
  esac
}

deform_height() {
  local box="${1#*x}"
  echo "${box%%+*}"
}

leg_deform_assert() {
  local f i
  for f in "$deform_open_path" "$deform_close_path" "$deform_reopen_path"; do
    if ! grep -q '^ok$' "$f" 2>/dev/null; then
      fail "a panel route did not answer ok at $f, got: $(cat "$f" 2>/dev/null)"
    fi
  done
  if ! grep -q '^network$' "$deform_state_path" 2>/dev/null; then
    fail "panel state after the open is not network, got: $(cat "$deform_state_path" 2>/dev/null)"
  fi
  for f in "$deform_rest_path" "$deform_bare_path" "$deform_settled_path" "${deform_mid_paths[@]}"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done

  echo "SMOKE_DEFORM_REST $deform_rest_path"
  echo "SMOKE_DEFORM_BARE $deform_bare_path"
  echo "SMOKE_DEFORM_SETTLED $deform_settled_path"
  for i in "${!deform_mid_paths[@]}"; do
    echo "SMOKE_DEFORM_MID_$((i + 1)) ${deform_mid_paths[$i]}"
  done

  # The reference the samples are read against, cropped by hand first since
  # every deform_box call below subtracts it.
  $convert_bin "$deform_bare_path" -crop "${out_w}x$((out_h - deform_band_top))+0+${deform_band_top}" \
    +repage "$shot_dir/deform-body-bare.png" > /dev/null 2>&1
  [ -s "$shot_dir/deform-body-bare.png" ] || fail "could not crop the band out of $deform_bare_path"

  local rest_box rest_h
  rest_box=$(deform_box "$deform_settled_path" "settled")
  rest_h=$(deform_height "$rest_box")
  if [ "${rest_h:-0}" -lt 80 ]; then
    fail "no resting card found under the bar: the diff against the closed output is '$rest_box'"
  fi
  echo "SMOKE_DEFORM_REST_BOX $rest_box"

  local box height last_box="" ladder=""
  for i in "${!deform_mid_paths[@]}"; do
    box=$(deform_box "${deform_mid_paths[$i]}" "mid-$((i + 1))")
    height=$(deform_height "$box")
    ladder+=" $height"
    echo "deform: sample $((i + 1)) at +${deform_mid_sleeps[$i]}s box $box"
    last_box="$box"
  done
  echo "SMOKE_DEFORM_LADDER$ladder rest=$rest_h"

  if [ "$last_box" != "$rest_box" ]; then
    fail "the last sample is $last_box against the resting $rest_box: the deform had not unwound to identity three clocks past the travel"
  fi
}
