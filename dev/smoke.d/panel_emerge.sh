# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --panel-emerge proves the drawer open (M53 addendum 2026-09-09): a panel
# comes out from under the bar rather than fading in where it will end up. The
# card is displaced behind the bar's inner line by its own height while closed
# and travels out from under it, so its visible height grows from nothing to
# the whole card and no part of it is ever drawn inside the bar's own band.
#
# Ridden by --no-morph it reads the popover instead (M60 T2): a card that
# drops out of its cell has no line to unroll from, so the ladder is whole or
# empty on every frame and never a fraction of the card. The bar's band stays
# the claim it is either way, since a popover opens no gap in the line at all.
#
# Measured down the card's own centre column, and a probe is "covered" when it
# is NOT byte-identical to the same probe in the closed output: something the
# desktop does not draw is being drawn there, which is the card.
#
# Both halves of that are the deform's doing (M54 D7). The card arrives
# squashed and unwinds through its rest, so a probe cannot ask whether what it
# sees is what the settled card draws: for the tail of every open it is the
# same card a few percent narrower. What the deform does not move is the
# midpoint of the anchored edge, which is what its matrix is centred on, so
# the card's centre column stands still horizontally however hard it is
# squashed and a probe on it is inside the card whenever the card has reached
# that far down. The card fill is `card` at `surfaceOpacity` over a near-black
# desktop, so "not the desktop" is a byte difference at every depth, whether
# the pixel under the probe is fill or a row's own ink.
#
# The ladder sits deep in the card (60%, 75%, 88% and 97% of its height) on
# purpose. The travel is front-loaded even on `spatial`: over half of it is
# gone in the first fifth of the clock, and probes near the top would be
# covered before the first frame lands.
#
# The card rect is not hardcoded: it comes out of the difference between the
# frame with the panel open and the frame without it, cropped below the bar
# first so the cell's own panel mark is not in the box. Below the card's
# shoulders too (M54 D6): the two fillets reach a `radiusXl` outside the
# card's own sides for the first `radiusXl` under the bar's line, so a box
# that took them in would measure the silhouette rather than the card and
# put the gutter probes below on bare desktop.
#
# Ten samples, each armed BEFORE the ipc call rather than after it: grim's
# own start-up costs about as much as the travel does, so a capture asked for
# once the call has returned lands past the end of it every time (again
# panel_handoff.sh's finding). Arming first spends that start-up during the
# round trip instead.
leg_panel_emerge_flag="--panel-emerge"
leg_panel_emerge_order=79
leg_panel_emerge_needs="convert"

panel_emerge_open_path="$shot_dir/panel-emerge-open.txt"
panel_emerge_close_path="$shot_dir/panel-emerge-close.txt"
panel_emerge_reopen_path="$shot_dir/panel-emerge-reopen.txt"
panel_emerge_state_path="$shot_dir/panel-emerge-state.txt"

panel_emerge_rest_path="$shot_dir/panel-emerge-rest.png"
panel_emerge_bare_path="$shot_dir/panel-emerge-bare.png"
panel_emerge_settled_path="$shot_dir/panel-emerge-settled.png"
panel_emerge_mid_paths=(
  "$shot_dir/panel-emerge-mid-1.png"
  "$shot_dir/panel-emerge-mid-2.png"
  "$shot_dir/panel-emerge-mid-3.png"
  "$shot_dir/panel-emerge-mid-4.png"
  "$shot_dir/panel-emerge-mid-5.png"
  "$shot_dir/panel-emerge-mid-6.png"
  "$shot_dir/panel-emerge-mid-7.png"
  "$shot_dir/panel-emerge-mid-8.png"
  "$shot_dir/panel-emerge-mid-9.png"
  "$shot_dir/panel-emerge-mid-10.png"
)
# The emerge's own clock (the table's `motion.emerge`, Gala's 150ms menu map,
# run at `panel_emerge_scale` so it lasts 500ms, long enough for grim to land
# inside it) and the moment this rig
# has the window up, both in milliseconds; every sample is the map plus a
# fraction of that clock, so a change to the token moves the sampling with it
# rather than leaving the ladder pinned to a duration that has gone. The
# negative fractions are the samples that land before the window is up, which
# is where the ladder's first rungs come from, and the last one is well past
# the end so the ladder has to have landed by it. Built in the drive, since
# the legs are sourced before the flags are read.
panel_emerge_map_ms=200
panel_emerge_scale=333
panel_emerge_mid_fractions=(-36 -24 -18 -12 -6 0 12 24 48 150)
panel_emerge_mid_sleeps=()

panel_emerge_popover() {
  leg_on no_morph
}

panel_emerge_build_sleeps() {
  local clock=$((150 * panel_emerge_scale / 100)) f at
  panel_emerge_mid_sleeps=()
  for f in "${panel_emerge_mid_fractions[@]}"; do
    at=$((panel_emerge_map_ms + f * clock / 100))
    panel_emerge_mid_sleeps+=("$(printf '%d.%03d' $((at / 1000)) $((at % 1000)))")
  done
}

# Where the card's own rect starts under the bar, past the strip (40), the
# `barMargin` the card hangs off it by and the fillets' own reach.
panel_emerge_body_top=60

# The bar's own band, over the stretch of it the card hangs under and clear of
# the right region's cells, whose panel mark and icons move on their own
# clocks, 380 in from the output's trailing edge. Nothing may ever paint here:
# the whole claim of the clip. Cut in the assert, off the output's width.
panel_emerge_bar_inset=380

leg_panel_emerge_validate() {
  local other
  for other in bar_layout bar_position center chevron config_reload frame fullscreen \
    gallery lock menu notify panel panel_anchor panel_at panel_handoff panel_keys \
    screensaver toggles tray_overflow wallpaper; do
    if leg_on "$other"; then
      echo "usage: --panel-emerge measures one panel's own frames and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_panel_emerge_timing() {
  leg_timing 14 46
}

leg_panel_emerge_drive() {
  local script="$shot_dir/panel-emerge-drive.sh" i arm=""
  panel_emerge_build_sleeps
  for i in "${!panel_emerge_mid_paths[@]}"; do
    arm+="( sleep ${panel_emerge_mid_sleeps[$i]}; \"$grim_bin\" \"${panel_emerge_mid_paths[$i]}\" > /dev/null 2>&1 ) &"$'\n'
  done
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
# The card's resting place, and then the same output without it: between them
# they say where the card is and what the desktop under it looks like.
$ipc call panel open network > "$panel_emerge_open_path" 2>&1
sleep 2
"$grim_bin" "$panel_emerge_rest_path" > /dev/null 2>&1
$ipc call panel toggle network > "$panel_emerge_close_path" 2>&1
sleep 2
"$grim_bin" "$panel_emerge_bare_path" > /dev/null 2>&1
$ipc call debug motionScale $panel_emerge_scale > /dev/null 2>&1
sleep 1
$arm
$ipc call panel open network > "$panel_emerge_reopen_path" 2>&1
wait
$ipc call debug motionScale 100 > /dev/null 2>&1
sleep 2
"$grim_bin" "$panel_emerge_settled_path" > /dev/null 2>&1
$ipc call panel state > "$panel_emerge_state_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

# One box out of a saved frame, named after what it is looking at. -strip
# because ImageMagick writes the wall clock into a PNG's tIME chunk, which
# would leave two byte-identical pictures comparing unequal.
panel_emerge_crop() {
  local source="$1" box="$2" name="$3" out
  out="$shot_dir/panel-emerge-crop-$name.png"
  $convert_bin "$source" -crop "$box" +repage -strip "$out" > /dev/null 2>&1
  [ -s "$out" ] || fail "could not crop $box out of $source"
  echo "$out"
}

# How many of the four probes the card has reached in this frame.
panel_emerge_depth() {
  local frame="$1" name="$2" i depth=0 probe bare_probe
  for i in "${!panel_emerge_probe_boxes[@]}"; do
    probe=$(panel_emerge_crop "$frame" "${panel_emerge_probe_boxes[$i]}" "$name-$i")
    bare_probe="$shot_dir/panel-emerge-crop-bare-$i.png"
    if ! cmp -s "$probe" "$bare_probe"; then
      depth=$((depth + 1))
    fi
  done
  echo "$depth"
}

leg_panel_emerge_assert() {
  local f i
  for f in "$panel_emerge_open_path" "$panel_emerge_close_path" "$panel_emerge_reopen_path"; do
    if ! grep -q '^ok$' "$f" 2>/dev/null; then
      fail "a panel route did not answer ok at $f, got: $(cat "$f" 2>/dev/null)"
    fi
  done
  if ! grep -q '^network$' "$panel_emerge_state_path" 2>/dev/null; then
    fail "panel state after the emerge is not network, got: $(cat "$panel_emerge_state_path" 2>/dev/null)"
  fi
  for f in "$panel_emerge_rest_path" "$panel_emerge_bare_path" "$panel_emerge_settled_path" \
    "${panel_emerge_mid_paths[@]}"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done

  echo "SMOKE_PANEL_EMERGE_REST $panel_emerge_rest_path"
  echo "SMOKE_PANEL_EMERGE_BARE $panel_emerge_bare_path"
  echo "SMOKE_PANEL_EMERGE_SETTLED $panel_emerge_settled_path"
  for i in "${!panel_emerge_mid_paths[@]}"; do
    echo "SMOKE_PANEL_EMERGE_MID_$((i + 1)) ${panel_emerge_mid_paths[$i]}"
  done

  # The card's own rect, off the two frames that differ by exactly one card.
  # Cropped below the bar first: the cell that owns an open panel draws a mark
  # of its own, which is a difference and is not the card.
  local body_h=$((out_h - panel_emerge_body_top)) bare_body rest_body rect
  bare_body="$shot_dir/panel-emerge-body-bare.png"
  rest_body="$shot_dir/panel-emerge-body-rest.png"
  $convert_bin "$panel_emerge_bare_path" -crop "${out_w}x${body_h}+0+${panel_emerge_body_top}" +repage "$bare_body" > /dev/null 2>&1
  $convert_bin "$panel_emerge_rest_path" -crop "${out_w}x${body_h}+0+${panel_emerge_body_top}" +repage "$rest_body" > /dev/null 2>&1
  rect=$($convert_bin "$bare_body" "$rest_body" -compose difference -composite \
    -threshold 8% -format "%@" info: 2>/dev/null)
  local card_w card_h card_x card_y
  card_w=${rect%%x*}
  card_h=${rect#*x}; card_h=${card_h%%+*}
  card_x=$(echo "$rect" | sed -n 's/.*+\([0-9]*\)+[0-9]*$/\1/p')
  card_y=$(echo "$rect" | sed -n 's/.*+\([0-9]*\)$/\1/p')
  card_y=$((card_y + panel_emerge_body_top))
  if [ -z "$card_w" ] || [ "${card_w:-0}" -lt 200 ] || [ "${card_h:-0}" -lt 200 ]; then
    fail "could not measure the network card off the open/closed pair, got rect '$rect'"
  fi
  echo "SMOKE_PANEL_EMERGE_CARD ${card_w}x${card_h}+${card_x}+${card_y}"

  # The ladder, down the card's own centre column (see the header).
  local centre_x=$((card_x + card_w / 2 - 2)) depth_at
  panel_emerge_probe_boxes=()
  for depth_at in 60 75 88 97; do
    panel_emerge_probe_boxes+=("5x4+${centre_x}+$((card_y + card_h * depth_at / 100 - 4))")
  done

  # The closed output's own probes are the reference every other frame is read
  # against, so they are cropped first and by hand.
  for i in "${!panel_emerge_probe_boxes[@]}"; do
    panel_emerge_crop "$panel_emerge_bare_path" "${panel_emerge_probe_boxes[$i]}" "bare-$i" > /dev/null
  done
  # The classifier has to be able to tell the two apart at every rung, or a
  # depth of 4 would mean nothing.
  local settled_probe
  for i in "${!panel_emerge_probe_boxes[@]}"; do
    settled_probe=$(panel_emerge_crop "$panel_emerge_settled_path" "${panel_emerge_probe_boxes[$i]}" "settled-$i")
    if cmp -s "$settled_probe" "$shot_dir/panel-emerge-crop-bare-$i.png"; then
      fail "probe ${panel_emerge_probe_boxes[$i]} reads the same with the card there and gone, so it measures nothing"
    fi
  done

  # Nothing may ever paint in the bar's band: the card is cut at the bar's
  # inner line, so every frame of the travel has to leave it exactly as the
  # closed output does.
  local bare_bar frame_bar name panel_emerge_bar_box="180x36+$((out_w - panel_emerge_bar_inset))+2"
  bare_bar=$(panel_emerge_crop "$panel_emerge_bare_path" "$panel_emerge_bar_box" "bar-bare")
  for i in "${!panel_emerge_mid_paths[@]}"; do
    frame_bar=$(panel_emerge_crop "${panel_emerge_mid_paths[$i]}" "$panel_emerge_bar_box" "bar-mid-$((i + 1))")
    if ! cmp -s "$frame_bar" "$bare_bar"; then
      fail "sample $((i + 1)) painted inside the bar's own band: ${panel_emerge_mid_paths[$i]} differs from the closed output over $panel_emerge_bar_box"
    fi
  done
  frame_bar=$(panel_emerge_crop "$panel_emerge_settled_path" "$panel_emerge_bar_box" "bar-settled")
  if ! cmp -s "$frame_bar" "$bare_bar"; then
    fail "the settled card painted inside the bar's own band over $panel_emerge_bar_box"
  fi

  # The travel itself: how far down the card reaches in each sample, in order.
  local settled_depth first_depth last_depth depth previous=-1 ladder=""
  settled_depth=$(panel_emerge_depth "$panel_emerge_settled_path" "settled-check")
  if [ "$settled_depth" -ne 4 ]; then
    fail "the settled card does not cover its own ladder ($settled_depth of 4), so the probes are not where the measurement thinks"
  fi
  for i in "${!panel_emerge_mid_paths[@]}"; do
    depth=$(panel_emerge_depth "${panel_emerge_mid_paths[$i]}" "mid-$((i + 1))")
    ladder+=" $depth"
    if panel_emerge_popover; then
      if [ "$depth" -ne 0 ] && [ "$depth" -ne "$settled_depth" ]; then
        fail "sample $((i + 1)) drew part of the card (depths$ladder): a popover drops whole out of its cell, it does not unroll from the line"
      fi
    elif [ "$depth" -lt "$previous" ]; then
      fail "the card went back up between samples $i and $((i + 1)): depths$ladder"
    fi
    previous=$depth
    [ "$i" -eq 0 ] && first_depth=$depth
    last_depth=$depth
  done
  echo "SMOKE_PANEL_EMERGE_LADDER$ladder settled=$settled_depth"
  if ! panel_emerge_popover && [ "$first_depth" -ge "$settled_depth" ]; then
    fail "the first sample already showed the whole card (depths$ladder): the panel appeared rather than emerging"
  fi
  if [ "$last_depth" -ne "$settled_depth" ]; then
    fail "the last sample never reached the card's own height (depths$ladder): the emerge did not land"
  fi

  # And it landed on exactly the rect it rests at, the deform unwound. The box
  # straddles the card's own bottom edge (its padding, its border and the
  # desktop under it) rather than taking in the rows, which rescan.
  local rest_edge settled_edge edge_box
  edge_box="${card_w}x14+${card_x}+$((card_y + card_h - 7))"
  rest_edge=$(panel_emerge_crop "$panel_emerge_rest_path" "$edge_box" "edge-rest")
  settled_edge=$(panel_emerge_crop "$panel_emerge_settled_path" "$edge_box" "edge-settled")
  if ! cmp -s "$rest_edge" "$settled_edge"; then
    fail "the emerged card's own bottom edge is not where the resting one's is: $panel_emerge_settled_path differs from $panel_emerge_rest_path over $edge_box"
  fi
}
