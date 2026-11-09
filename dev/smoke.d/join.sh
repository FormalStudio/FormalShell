# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --join photographs the join mid-flight. At full speed a card is on the line
# for about as long as one screencopy takes, so every open here runs at a
# tenth of its speed (`debug motionScale`) and is sampled across the travel:
#
#   a  `panel open network` with no cell, which lands against the far end of
#      the bar, a `screenPadding` off the screen's own edge
#   b  quick settings, opened by the chevron and budding off the line under
#      it (docs/superpowers/specs/2026-11-04-quick-settings.md §8)
#   d  `panel toggle usage`, which hangs off the one cell in the left region
#      and so rests against the START of the line: the other wall, and the
#      only case here that is not at the line's far end
#
# It rides `--bar-position <edge>` and `--frame`, which is what puts every
# case above on a vertical hairline or against a frame ring's line without a
# copy of this leg per layout. Riding a position, this leg carries it into its
# own `bar` key (two top-level keys leave only the later one standing) and
# --bar-position stands its own drive down. Nothing in the drive knows which
# edge it is on.
#
# Every frame is saved for reading by eye; two claims are asserted on an
# attached frame of case a, and case b's card has to be there at rest:
#
#   the seam (M57 D1)   the line's own row inside the gap must be byte-equal
#                       to the bar's fill two rows above it: the bar paints
#                       that row, and a card whose fill starts on it too
#                       leaves a line across the whole gap wherever the
#                       surfaces are translucent.
#   the wall (M57 D2)   case a rests a screenPadding off the output and its
#                       fillet wants radiusXl. Where that is more than the
#                       room, the attached silhouette runs out to the
#                       screen's own edge instead, and the output's last
#                       column carries card fill from under the line down to
#                       the shape's far edge; where the fillet fits (the one
#                       table's radiusXl of 10 in a 12px room), that column
#                       stays bare desktop throughout. At rest it is bare
#                       desktop again either way.
#
# Every probe is pinned to the top bar's own numbers, so a run on another
# edge or with the frame on photographs the same cases over the whole output
# and prints each claim as skipped instead; those layouts are read by eye.
leg_join_flag="--join"
leg_join_order=182
leg_join_needs="wlrctl convert jq"

# The crop the cases are photographed through, cut against the output's
# right end in logical pixels (grim -g's space) by the drive; every probe
# below reads its frames in their own pixels, the crop's width read back off
# the bare desktop's.
join_region=""
join_crop_width=540
join_frames=10
join_reply_path="$shot_dir/join-replies.txt"
join_quick_path="$shot_dir/join-quick.json"
join_desktop_path="$shot_dir/join-desktop.png"
join_dump_path="$shot_dir/join-dump.json"

# Case a's card in the crop's own logical coordinates: `panel open network`
# with no cell is `Theme.space.n` (380) wide and rests one screenPadding (12)
# off the output's right edge, and the crop is 540 wide against that edge, so
# it ends on the output's own last column. The bar is 40 rows, so its inward
# line is the last of them. The assert scales all three to the frame's pixels.
join_card_x=148
join_card_width=380
join_line_row=39

# Whether the cases are read off the whole output rather than through the
# crop above: any layout but the pinned one, where the probes mean nothing and
# what is left is the frames.
join_whole_output() {
  leg_on bar_position || leg_on frame
}

leg_join_validate() {
  local other
  for other in bar_layout chevron chevron_quiet panel_handoff tray_overflow; do
    if leg_on "$other"; then
      echo "usage: --join carries its own bar.layout and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_join_fixture() {
  local position=""
  if leg_on bar_position; then
    position='"position": "'"$(leg_arg bar_position)"'", '
  fi
  # One cell in the left region, which on any edge is the START of the line:
  # case d hangs off it and rests against that end the way case a rests
  # against the far one.
  settings_fragment ', "bar": {'"$position"'"layout": {"left": ["usage"], "right": ["bluetooth", "weather", "tray", "bell", "indicators", "chevron", "battery", "audio", "network"]}}'
}

leg_join_timing() {
  if leg_on menu_emerge; then
    leg_timing 150 230
  else
    leg_timing 110 180
  fi
}

leg_join_drive() {
  local script="$shot_dir/join-drive.sh" wait_menu=""
  if leg_on menu_emerge; then
    # Its launcher and scrim would otherwise sit over these cases, and its
    # motionScale fight this one's.
    wait_menu="for _ in \$(seq 1 120); do [ -e \"$menu_emerge_done_path\" ] && break; sleep 0.5; done"
  fi
  if join_whole_output; then
    # A crop cut for the top bar's right end holds nothing at all on a left
    # bar, and with the probes skipped anyway the whole output is what there
    # is to read.
    join_region="0,0 ${logical_w}x$logical_h"
  else
    join_region="$((logical_w - join_crop_width)),0 ${join_crop_width}x600"
  fi
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@" >> "$join_reply_path" 2>&1; }
sample() {
  local name=\$1 i
  for i in \$(seq 1 $join_frames); do
    sleep 0.3
    "$grim_bin" -g "$join_region" "$shot_dir/join-\$name-\$i.png" > /dev/null 2>&1
  done
}
sleep 5
$wait_menu
call debug motionScale 1000
# Not through call(), whose own redirect would swallow the dump into the
# reply log.
$ipc call debug dump > "$join_dump_path" 2>&1
"$grim_bin" -g "$join_region" "$join_desktop_path" > /dev/null 2>&1
call panel open network
sample a
sleep 3
"$grim_bin" -g "$join_region" "$shot_dir/join-a-rest.png" > /dev/null 2>&1
call panel close
sample a-close
sleep 3
call bar chevron expand
sample b
sleep 3
"$grim_bin" -g "$join_region" "$shot_dir/join-b-rest.png" > /dev/null 2>&1
$ipc call quicksettings status > "$join_quick_path" 2>&1
call bar chevron collapse
sample b-close
sleep 2
call panel toggle usage
sample d
sleep 3
"$grim_bin" -g "$join_region" "$shot_dir/join-d-rest.png" > /dev/null 2>&1
call panel close
sample d-close
call debug motionScale 100
EOF
  hypr_exec_once "bash $script"
}

# One pixel out of a frame, as six hex digits.
join_pixel() {
  local rgb
  rgb=$(pixel "$1" "$2" "$3")
  [ -n "$rgb" ] || fail "could not read the pixel at ($2,$3) out of $1"
  # shellcheck disable=SC2086  # three fields, one per channel
  printf '%02X%02X%02X' $rgb
}

# One whole column of a frame, as six hex digits per row, top to bottom: the
# wall runs the length of the card, and one read beats a probe per row.
join_column() {
  $convert_bin "$1" -crop "1x4096+$2+0" +repage -depth 8 txt:- 2>/dev/null \
    | awk 'NR > 1 { print toupper(substr($3, 2, 6)) }'
}

# One row of a frame from column $2 to the crop's own end, the same way.
join_row() {
  $convert_bin "$1" -crop "4096x1+$2+$3" +repage -depth 8 txt:- 2>/dev/null \
    | awk 'NR > 1 { print toupper(substr($3, 2, 6)) }'
}

# How far under the line the shape reaches at column $2 of frame $1: the last
# row that is not the bare desktop's own. The card comes out from under the
# line, so this is what says how much of the wall there is to read.
join_shape_depth() {
  local i last=$join_line_row
  local -a shot desk
  mapfile -t shot < <(join_column "$1" "$2")
  mapfile -t desk < <(join_column "$join_desktop_path" "$2")
  for ((i = join_line_row + 1; i < ${#shot[@]} && i < ${#desk[@]}; i++)); do
    [ "${shot[i]}" = "${desk[i]}" ] && break
    last=$i
  done
  echo "$last"
}

# The box of everything that differs between two frames, from row $3 down for
# $4 rows (to the bottom of the crop with no count), as "W H X Y" with Y back
# in the crop's own rows. Nothing at all, and a non-zero status, when the two
# are identical there.
join_bbox() {
  local box
  box=$($convert_bin "$1" "$2" -compose difference -composite -colorspace Gray \
    -crop "${join_crop_width}x${4:-4096}+0+$3" +repage -threshold 0 -format '%@' info: 2>/dev/null)
  case "$box" in
    ''|0x0*) return 1 ;;
  esac
  echo "$box" | awk -F'[x+]' -v y0="$3" '{ print $1, $2, $3, $4 + y0 }'
}

# The first frame of case a with the card attached: its fill both a few rows
# under the line and on the very first row under it, that first row the same
# ink as the one below it. A card that has begun to let go pulls its near
# edge off the line and brings its own border up there, so both of those
# rows change and the frame is passed over; a card still behind the line has
# not reached the deep probe yet.
join_attached_frame() {
  local col=$1 i path deep near below deep_desk near_desk
  deep_desk=$(join_pixel "$join_desktop_path" "$col" $((join_line_row + 5)))
  near_desk=$(join_pixel "$join_desktop_path" "$col" $((join_line_row + 1)))
  for i in $(seq 1 $join_frames); do
    path="$shot_dir/join-a-$i.png"
    [ -f "$path" ] || continue
    deep=$(join_pixel "$path" "$col" $((join_line_row + 5)))
    near=$(join_pixel "$path" "$col" $((join_line_row + 1)))
    below=$(join_pixel "$path" "$col" $((join_line_row + 2)))
    if [ "$deep" != "$deep_desk" ] && [ "$near" != "$near_desk" ] && [ "$near" = "$below" ]; then
      echo "$path"
      return 0
    fi
  done
  return 1
}

leg_join_assert() {
  local name i path
  for name in a a-close b b-close d d-close; do
    for i in $(seq 1 $join_frames); do
      path="$shot_dir/join-$name-$i.png"
      [ -f "$path" ] || fail "no join frame $path"
      echo "SMOKE_JOIN_$(echo "$name" | tr 'a-z-' 'A-Z_')_$i $path"
    done
  done
  for name in a b d; do
    path="$shot_dir/join-$name-rest.png"
    [ -f "$path" ] || fail "no join frame $path"
    echo "SMOKE_JOIN_$(echo "$name" | tr 'a-z' 'A-Z')_REST $path"
  done
  [ -f "$join_desktop_path" ] || fail "no bare desktop frame at $join_desktop_path"
  echo "SMOKE_JOIN_DESKTOP $join_desktop_path"
  "$jq_bin" -e '.open == true and .from == "chevron:right"' "$join_quick_path" > /dev/null 2>&1 || fail \
    "case b's chevron did not leave quick settings open: $(cat "$join_quick_path" 2>/dev/null)"

  # The edge every case above was photographed on, off the shell's own
  # numbers: a position that landed late would otherwise photograph a top bar
  # and read as a pass on the frames alone.
  local want=top
  leg_on bar_position && want=$(leg_arg bar_position)
  grep -q "\"barPosition\":\"$want\"" "$join_dump_path" 2>/dev/null || fail \
    "the shell reports a bar on another edge than the $want this run asked for: $(head -c 400 "$join_dump_path" 2>/dev/null)"
  echo "SMOKE_JOIN_EDGE ok $want"

  if join_whole_output; then
    for name in SEAM WALL B; do
      echo "SMOKE_JOIN_$name skipped (layout): the probes are pinned to the top bar's own crop; the frames above carry this case"
    done
    return 0
  fi

  # The crop and the card in the frame's own pixels.
  join_crop_width=$($convert_bin "$join_desktop_path" -format %w info: 2>/dev/null)
  join_card_x=$(logical_px "$join_card_x")
  join_card_width=$(logical_px "$join_card_width")
  join_line_row=$(( $(logical_px $((join_line_row + 1))) - 1 ))
  local col attached seam bar
  col=$((join_card_x + join_card_width / 2))
  attached=$(join_attached_frame "$col") || fail \
    "no attached frame among join-a-1..$join_frames: nothing sits on the line's row at column $col"
  seam=$(join_pixel "$attached" "$col" "$join_line_row")
  bar=$(join_pixel "$attached" "$col" $((join_line_row - 2)))
  if [ "$seam" != "$bar" ]; then
    fail "the line's row at ($col,$join_line_row) reads $seam against the bar's own $bar two rows up in $attached: the card's fill covers it a second time"
  fi
  echo "SMOKE_JOIN_SEAM ok $seam $attached"

  local edge ink far top bottom row
  edge=$((join_crop_width - 1))
  ink=$(join_pixel "$attached" "$col" $((join_line_row + 5)))
  far=$(join_shape_depth "$attached" "$col")
  top=$((join_line_row + 2))
  bottom=$((far - 3))
  [ "$bottom" -gt "$top" ] || fail \
    "the shape reaches only row $far at column $col of $attached: too shallow to read the wall against"
  local -a wall rest desk
  mapfile -t wall < <(join_column "$attached" "$edge")
  mapfile -t rest < <(join_column "$shot_dir/join-a-rest.png" "$edge")
  mapfile -t desk < <(join_column "$join_desktop_path" "$edge")
  local reach room
  reach=$(scaled_jq -r '.theme.radiusXl * $s | round' "$join_dump_path" 2>/dev/null)
  room=$((join_crop_width - join_card_x - join_card_width))
  if [ -n "$reach" ] && [ "$reach" -le "$room" ]; then
    for ((row = top; row <= bottom; row++)); do
      [ "${wall[row]}" = "${desk[row]}" ] || fail \
        "the output's last column reads ${wall[row]} at row $row of $attached against the bare desktop's ${desk[row]}: a ${reach}px fillet fits the ${room}px room, so nothing should wall"
    done
    echo "SMOKE_JOIN_WALL ok fits radiusXl $reach room $room rows $top-$bottom at column $edge $attached"
  else
    for ((row = top; row <= bottom; row++)); do
      [ "${wall[row]}" = "$ink" ] || fail \
        "the output's last column reads ${wall[row]} at row $row of $attached against the card's own $ink: the walled silhouette stops short of the screen's edge"
      [ "${rest[row]}" = "${desk[row]}" ] || fail \
        "the output's last column still reads ${rest[row]} at row $row of join-a-rest.png against the bare desktop's ${desk[row]}: the card has let go and the edge should be desktop again"
    done

    # And the run out to the wall is one unbroken fill: two whole rows read
    # across, from the card's own middle to that last column, may not carry the
    # bare desktop's ink anywhere. A column probe alone misses a seam between
    # the card's side and the run-out, which is exactly where the two meet.
    local -a scan bare
    local depth col_i bad
    for depth in $((join_line_row + 8)) $((join_line_row + (bottom - join_line_row) * 2 / 3)); do
      mapfile -t scan < <(join_row "$attached" "$col" "$depth")
      mapfile -t bare < <(join_row "$join_desktop_path" "$col" "$depth")
      bad=""
      for ((col_i = 0; col_i < ${#scan[@]}; col_i++)); do
        [ "${scan[col_i]}" = "${bare[col_i]}" ] && bad="$bad $((col + col_i))"
      done
      [ -z "$bad" ] || fail \
        "row $depth of $attached shows the bare desktop at column(s)$bad between the card's middle and the output's end: the attached silhouette is not one unbroken fill"
      echo "SMOKE_JOIN_WALL_ROW ok row $depth columns $col-$edge $attached"
    done
    echo "SMOKE_JOIN_WALL ok $ink rows $top-$bottom at column $edge $attached"
  fi

  # Case b's card at rest under the line, off its own pixels.
  join_bbox "$shot_dir/join-b-rest.png" "$join_desktop_path" $((join_line_row + 1)) > /dev/null || fail \
    "nothing under the bar differs from the bare desktop in join-b-rest.png: the chevron's card never opened"
  echo "SMOKE_JOIN_B ok quick settings at rest under the line"
}
