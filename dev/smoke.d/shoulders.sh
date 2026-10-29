# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --shoulders proves the join (M54 D6): a card hanging off the bar and the
# bar's own inward line are one silhouette, not a card parked against a line.
# The bar opens a gap of the card's rect plus a fillet's radius at either end,
# and the shoulders component draws a concave quarter arc into each of those
# two ends, running from the card's side out to the line.
#
# Nothing consumes Shoulders until M54 Task 3, so the leg publishes the join
# itself over `debug join <edge> <x> <width>` and the shell's own preview
# surface hangs one card at that rect. The
# card's depth is that file's business; everything asserted here is along the
# bar.
#
# Only a band with a line has a gap to open: the default transparent band
# draws none (`debug dump`'s `bar[].line` is empty), so this leg needs
# `--bar-style solid` or `corners` riding it and refuses to run without one.
#
# Two claims, read two different ways:
#
# The gap is the shell's own number, off `debug dump`'s `bar[].line`, the two
# segment rects the strip actually laid out. Its two ends have to sit exactly
# `radiusXl` outside the card's rect, and `radiusXl` comes out of the same
# dump, so a change to the radius moves the expectation with the shape
# rather than breaking the leg.
#
# The seam is pixels, because no number can say whether the line and the arc
# meet: at the gap's own end column the bar's line row must carry the border
# ink both one column outside the gap (the line's last pixel) and at the gap's
# first column (the arc's first), so there is neither a gap pixel nor a second
# line. The two reference colours are sampled out of the same frame, the
# border off the line well clear of the join and the card fill out of the
# middle of the card, so nothing here restates a palette.
#
# The frame this reads is deliberately cut at the bar's line row and below it:
# the clock sits in the centre region right over the join and reprints itself
# between two grabs, and a crop that took in the bar's cells would differ
# between the open and closed frames for a reason that has nothing to do with
# the join.
leg_shoulders_flag="--shoulders"
leg_shoulders_order=74
leg_shoulders_needs="convert jq"

# Centred under the bar, and wide enough that both fillets are far from
# anything else on the strip.
shoulders_join_x=800
shoulders_join_width=320

shoulders_join_reply_path="$shot_dir/shoulders-join.txt"
shoulders_clear_reply_path="$shot_dir/shoulders-clear.txt"
shoulders_dump_closed_path="$shot_dir/shoulders-dump-closed.json"
shoulders_dump_open_path="$shot_dir/shoulders-dump-open.json"
shoulders_dump_cleared_path="$shot_dir/shoulders-dump-cleared.json"
shoulders_closed_path="$shot_dir/shoulders-closed.png"
shoulders_open_path="$shot_dir/shoulders-open.png"
shoulders_cleared_path="$shot_dir/shoulders-cleared.png"
shoulders_closed_crop_path="$shot_dir/shoulders-closed-crop.png"
shoulders_open_crop_path="$shot_dir/shoulders-open-crop.png"

# The join's own stretch of the output: one row of bar above the line, the
# line itself, and the card under it, wide enough to hold both fillets and a
# run of plain line either side. Cut out of the saved frames at assert time
# rather than grabbed with grim, since where the bar's line sits is the
# shell's own number and only the dump knows it.
shoulders_crop_width=480
shoulders_crop_height=110
shoulders_crop_x=730

leg_shoulders_validate() {
  local other
  case "$(leg_arg bar_style 2>/dev/null)" in
    solid|corners) ;;
    *)
      echo "usage: --shoulders needs --bar-style solid or corners: the transparent band has no line to open" >&2
      exit 1
      ;;
  esac
  for other in bar_layout bar_position config_reload frame fullscreen gallery lock \
    panel_emerge panel_handoff screensaver wallpaper; do
    if leg_on "$other"; then
      echo "usage: --shoulders measures the bar's own line and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_shoulders_timing() {
  leg_timing 14 45
}

leg_shoulders_drive() {
  local script="$shot_dir/shoulders-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
# Closed: the line whole, and the desktop where the card is about to hang.
$ipc call debug dump > "$shoulders_dump_closed_path" 2>&1
"$grim_bin" "$shoulders_closed_path" > /dev/null 2>&1
$ipc call debug join top $shoulders_join_x $shoulders_join_width > "$shoulders_join_reply_path" 2>&1
# A full spatial clock (500ms) and a frame's grace past it: the two ends of
# the gap travel, and everything below reads them at rest.
sleep 0.6
$ipc call debug dump > "$shoulders_dump_open_path" 2>&1
"$grim_bin" "$shoulders_open_path" > /dev/null 2>&1
$ipc call debug joinClear > "$shoulders_clear_reply_path" 2>&1
sleep 0.6
$ipc call debug dump > "$shoulders_dump_cleared_path" 2>&1
"$grim_bin" "$shoulders_cleared_path" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# One field off a dump, rounded to the pixel: at rest every one of these is a
# whole number already, and rounding says so rather than comparing reals.
shoulders_field() {
  "$jq_bin" -r "$2 | if . == null then \"null\" else (. | round) end" "$1" 2>/dev/null
}

# One pixel's r,g,b out of a frame.
shoulders_pixel() {
  $convert_bin "$1" -format \
    "%[fx:int(255*p{$2,$3}.r+0.5)] %[fx:int(255*p{$2,$3}.g+0.5)] %[fx:int(255*p{$2,$3}.b+0.5)]" \
    info: 2>/dev/null
}

# The channel distance between two sampled pixels.
shoulders_distance() {
  local a b i delta total=0
  read -r -a a <<< "$1"
  read -r -a b <<< "$2"
  for i in 0 1 2; do
    delta=$(( ${a[$i]} - ${b[$i]} ))
    [ "$delta" -lt 0 ] && delta=$(( -delta ))
    total=$(( total + delta ))
  done
  echo "$total"
}

# Which of the three inks a probe is: the border, the card's fill, or the
# bare desktop. Nearest wins rather than a tolerance, because the three sit
# close together on a dark palette and a 1px stroke on a curve renderer
# leaves antialiasing at every edge; what a probe must never do is land
# nearer the wrong one of the three.
shoulders_ink() {
  local probe="$1" best="border" best_d name
  best_d=$(shoulders_distance "$probe" "$shoulders_border_ink")
  for name in card desktop; do
    local ref d
    eval "ref=\$shoulders_${name}_ink"
    d=$(shoulders_distance "$probe" "$ref")
    if [ "$d" -lt "$best_d" ]; then
      best_d=$d
      best=$name
    fi
  done
  echo "$best"
}

# One probe, named, asserted to be the ink it should be.
shoulders_probe() {
  local frame="$1" x="$2" y="$3" want="$4" name="$5" probe got
  probe=$(shoulders_pixel "$frame" "$x" "$y")
  got=$(shoulders_ink "$probe")
  echo "SMOKE_SHOULDERS_PROBE $name ($x,$y) $probe $got"
  if [ "$got" != "$want" ]; then
    fail "$name at ($x,$y) reads $got ($probe), not $want"
  fi
}

leg_shoulders_assert() {
  local f
  for f in "$shoulders_join_reply_path" "$shoulders_clear_reply_path"; do
    if ! grep -q '^ok$' "$f" 2>/dev/null; then
      fail "a debug join route did not answer ok at $f, got: $(cat "$f" 2>/dev/null)"
    fi
  done
  for f in "$shoulders_closed_path" "$shoulders_open_path" "$shoulders_cleared_path"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done

  echo "SMOKE_SHOULDERS_CLOSED_FRAME $shoulders_closed_path"
  echo "SMOKE_SHOULDERS_OPEN_FRAME $shoulders_open_path"
  echo "SMOKE_SHOULDERS_CLEARED_FRAME $shoulders_cleared_path"

  local radius line_row
  radius=$(shoulders_field "$shoulders_dump_open_path" '.theme.radiusXl')
  line_row=$(shoulders_field "$shoulders_dump_open_path" '.bar[0].line[0].y')
  if [ -z "$radius" ] || [ "$radius" = "null" ] || [ "$radius" -le 0 ]; then
    fail "the dump carries no theme.radiusXl, got '$radius' in $shoulders_dump_open_path"
  fi
  if [ -z "$line_row" ] || [ "$line_row" = "null" ]; then
    fail "the dump carries no bar line rect, got '$line_row' in $shoulders_dump_open_path"
  fi

  # Closed, twice: the two segments meet, so the line is one unbroken run of
  # the strip's own length. Where they meet is not asserted, only that nothing
  # is missing between them.
  local dump start_w end_x end_w
  for dump in "$shoulders_dump_closed_path" "$shoulders_dump_cleared_path"; do
    start_w=$(shoulders_field "$dump" '.bar[0].line[0].width')
    end_x=$(shoulders_field "$dump" '.bar[0].line[1].x')
    end_w=$(shoulders_field "$dump" '.bar[0].line[1].width')
    if [ "$end_x" != "$start_w" ]; then
      fail "the bar's line has a gap with no card joined to it: segments end at $start_w and resume at $end_x in $dump"
    fi
    if [ "$((start_w + end_w))" -le 0 ]; then
      fail "the bar drew no line at all in $dump"
    fi
  done

  # Open: the gap is the card's rect plus one fillet at either end, and its two
  # ends sit exactly `radiusXl` outside that rect.
  local gap_start gap_end gap expected_start expected_end expected_gap
  gap_start=$(shoulders_field "$shoulders_dump_open_path" '.bar[0].line[0].width')
  gap_end=$(shoulders_field "$shoulders_dump_open_path" '.bar[0].line[1].x')
  gap=$((gap_end - gap_start))
  expected_start=$((shoulders_join_x - radius))
  expected_end=$((shoulders_join_x + shoulders_join_width + radius))
  expected_gap=$((shoulders_join_width + radius * 2))
  echo "SMOKE_SHOULDERS_GAP ${gap_start}..${gap_end} width=$gap radius=$radius"
  if [ "$gap" -ne "$expected_gap" ]; then
    fail "the bar's gap is $gap, not the card's $shoulders_join_width plus two ${radius}px fillets ($expected_gap)"
  fi
  if [ "$gap_start" -ne "$expected_start" ] || [ "$gap_end" -ne "$expected_end" ]; then
    fail "the gap runs ${gap_start}..${gap_end}, not ${expected_start}..${expected_end}: its ends are not one fillet outside the card"
  fi

  # The join's own box out of both frames. -strip because ImageMagick writes
  # the wall clock into a PNG's tIME chunk, which would leave two
  # byte-identical pictures comparing unequal (panel_emerge.sh's finding).
  local box
  box="${shoulders_crop_width}x${shoulders_crop_height}+${shoulders_crop_x}+$((line_row - 1))"
  $convert_bin "$shoulders_closed_path" -crop "$box" +repage -strip "$shoulders_closed_crop_path" > /dev/null 2>&1
  $convert_bin "$shoulders_open_path" -crop "$box" +repage -strip "$shoulders_open_crop_path" > /dev/null 2>&1
  for f in "$shoulders_closed_crop_path" "$shoulders_open_crop_path"; do
    [ -s "$f" ] || fail "could not crop $box out of the saved frames"
  done
  echo "SMOKE_SHOULDERS_CLOSED $shoulders_closed_crop_path"
  echo "SMOKE_SHOULDERS_OPEN $shoulders_open_crop_path"

  # The two differ, which is the card being in exactly one of them: nothing
  # else in this box moves, the bar's cells are all above the line row.
  if cmp -s "$shoulders_closed_crop_path" "$shoulders_open_crop_path"; then
    fail "the box at $box is identical open and closed: no card was drawn in the gap"
  fi

  # The three inks, all sampled out of the frames under test rather than
  # restated from a palette: the border off the line a long way from the
  # join, the card fill out of the middle of the card, and the desktop out of
  # the same place with nothing joined.
  shoulders_border_ink=$(shoulders_pixel "$shoulders_open_path" 200 "$line_row")
  shoulders_card_ink=$(shoulders_pixel "$shoulders_open_path" \
    $((shoulders_join_x + shoulders_join_width / 2)) $((line_row + 60)))
  shoulders_desktop_ink=$(shoulders_pixel "$shoulders_closed_path" \
    $((shoulders_join_x + shoulders_join_width / 2)) $((line_row + 60)))
  echo "SMOKE_SHOULDERS_INK border=$shoulders_border_ink card=$shoulders_card_ink desktop=$shoulders_desktop_ink"
  local pair
  for pair in "$shoulders_border_ink:$shoulders_card_ink" \
    "$shoulders_border_ink:$shoulders_desktop_ink" \
    "$shoulders_card_ink:$shoulders_desktop_ink"; do
    if [ "$(shoulders_distance "${pair%%:*}" "${pair#*:}")" -lt 12 ]; then
      fail "two of the three reference inks are the same colour ($pair), so every probe below measures nothing"
    fi
  done

  # Each end of the gap: the line's last pixel and the arc's first, on the
  # same row, both border ink. A gap pixel or a doubled line shows up as one
  # of the two reading something else.
  shoulders_probe "$shoulders_open_path" $((gap_start - 1)) "$line_row" border "line-into-near-arc"
  shoulders_probe "$shoulders_open_path" "$gap_start" "$line_row" border "near-arc-end"
  shoulders_probe "$shoulders_open_path" $((gap_end - 1)) "$line_row" border "far-arc-end"
  shoulders_probe "$shoulders_open_path" "$gap_end" "$line_row" border "line-out-of-far-arc"

  # Inside the gap the line's own row is still the band's (`--join`'s seam
  # claim: a card filling it too would paint it twice), and one row inward
  # the card is the only thing drawing, so the desktop never shows through.
  shoulders_probe "$shoulders_open_path" "$shoulders_join_x" "$line_row" border "band-on-the-line"
  shoulders_probe "$shoulders_open_path" "$shoulders_join_x" $((line_row + 1)) card "card-one-row-inward"

  # And the fillet really is concave: just inside the arc's outer end, two
  # rows in, is desktop rather than card. A square shoulder would fill it.
  shoulders_probe "$shoulders_open_path" $((gap_start + 3)) $((line_row + 2)) desktop "inside-the-near-fillet"

  # With nothing joined, that same place on the line is the line itself.
  shoulders_probe "$shoulders_cleared_path" "$shoulders_join_x" "$line_row" border "line-closed-again"
}
