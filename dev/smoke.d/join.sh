# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --join photographs the join mid-flight. At full speed a card is on the line
# for about as long as one screencopy takes, so every open here runs at a
# tenth of its speed (`debug motionScale`) and is sampled across the travel:
#
#   a  `panel open network` with no cell, which lands against the far end of
#      the bar, a `screenPadding` off the screen's own edge
#   b  the chevron's second bar
#   c  a panel opened by a click on a cell inside that second bar
#   d  `panel toggle usage`, which hangs off the one cell in the left region
#      and so rests against the START of the line: the other wall, and the
#      only case here that is not at the line's far end
#
# It rides `--bar-position <edge>` and `--frame`, which is what puts every
# case above on a vertical hairline or against a frame ring's line without a
# copy of this leg per layout. Riding a position, this leg carries it into its
# own `bar` key (two top-level keys leave only the later one standing) and
# --bar-position stands its own drive down. Nothing in the drive knows which
# edge it is on: the strip's thickness and its position come out of `debug
# dump`, and the cell case c clicks is found by diffing the second bar out of
# the output past that thickness.
#
# Every frame is saved for reading by eye; four claims are asserted, the
# first two on an attached frame of case a and the last two on case c:
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
#   the bud (M57 D3)    case c's 380px card hangs off a second bar a third of
#                       its width. On the frames taken before it lets go of
#                       that bar's edge, nothing it paints under that edge may
#                       lie outside the bar's own columns, and at rest it is
#                       the full card again. Everything the claim is measured
#                       against, the second bar's rect and the card's resting
#                       rect, is read off the rig's own pixels: the strip is
#                       as wide as its cells measured themselves at, which is
#                       no number to pin.
#   the border (M57)    and on a frame where that same card is far enough out
#                       to read its sides and still widening, no row
#                       well inside it carries the card's own fill at its
#                       outermost painted column on either side. A clip taken
#                       along the line from the undeformed rect cuts the
#                       border off whichever side the deform, or the last of
#                       the attach clock, has carried past it.
#
# Every probe is pinned to the top bar's own numbers, so a run on another
# edge or with the frame on photographs the same cases over the whole output
# and prints each claim as skipped instead; those layouts are read by eye.
leg_join_flag="--join"
leg_join_order=182
leg_join_needs="wlrctl convert jq"

join_region="1380,0 540x600"
join_region_x=1380
join_crop_width=540
join_frames=10
join_reply_path="$shot_dir/join-replies.txt"
join_desktop_path="$shot_dir/join-desktop.png"
join_dump_path="$shot_dir/join-dump.json"
# Two whole-output frames, either side of the chevron's expand: what the
# second bar's own box is diffed out of, in the output's coordinates whatever
# region the cases themselves are cropped to.
join_bare_path="$shot_dir/join-bare.png"
join_owner_path="$shot_dir/join-owner.png"
join_click_path="$shot_dir/join-click.txt"

# Case a's card in the crop's own coordinates: `panel open network` with no
# cell is `Theme.space.n` (380) wide and rests one screenPadding (12) off the
# 1920 output's right edge, and the crop starts at x = 1380, so it ends on
# the output's own last column. The bar is 40 rows, so its inward line is the
# last of them.
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
    # The output dev/smoke.sh pins for every session. A crop cut for the top
    # bar's right end holds nothing at all on a left bar, and with the probes
    # skipped anyway the whole output is what there is to read.
    join_region="0,0 1920x1080"
    join_region_x=0
    join_crop_width=1920
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
# Where the group's first cell is, in the output's own coordinates: the
# strip's edge and thickness out of the dump, the second bar whatever differs
# past that thickness between the two whole-output frames, and the cell a
# little in from the card's own start along the strip. Derived rather than
# pinned, so the click lands on a cell on any edge.
cell() {
  local edge inset crop ox=0 oy=0 box bw bh bx by
  edge=\$("$jq_bin" -r '.theme.barPosition' "$join_dump_path")
  inset=\$("$jq_bin" -r --arg e "\$edge" '.theme.edgeInset[\$e] | floor' "$join_dump_path")
  case \$edge in
    top) crop="1920x\$((1080 - inset))+0+\$inset"; oy=\$inset ;;
    bottom) crop="1920x\$((1080 - inset))+0+0" ;;
    left) crop="\$((1920 - inset))x1080+\$inset+0"; ox=\$inset ;;
    right) crop="\$((1920 - inset))x1080+0+0" ;;
  esac
  box=\$($convert_bin "$join_owner_path" "$join_bare_path" -compose difference -composite \
    -colorspace Gray -crop "\$crop" +repage -threshold 2% -format '%@' info: 2>/dev/null)
  echo "edge=\$edge inset=\$inset crop=\$crop box=\$box" >> "$join_click_path"
  IFS='x+' read -r bw bh bx by <<< "\$box"
  [ -n "\$by" ] || return 1
  if [ "\$bw" -lt "\$bh" ]; then
    echo "\$((ox + bx + bw / 2)) \$((oy + by + 24))"
  else
    echo "\$((ox + bx + 24)) \$((oy + by + bh / 2))"
  fi
}
sleep 5
$wait_menu
call debug motionScale 1000
# Not through call(), whose own redirect would swallow the dump into the
# reply log.
$ipc call debug dump > "$join_dump_path" 2>&1
"$grim_bin" -g "$join_region" "$join_desktop_path" > /dev/null 2>&1
"$grim_bin" "$join_bare_path" > /dev/null 2>&1
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
"$grim_bin" "$join_owner_path" > /dev/null 2>&1
at=\$(cell) || at=""
echo "click \$at" >> "$join_click_path"
"$wlrctl_bin" pointer move -4000 -4000 >> "$join_reply_path" 2>&1
sleep 0.5
if [ -n "\$at" ]; then
  "$wlrctl_bin" pointer move \$at >> "$join_reply_path" 2>&1
  sleep 0.2
  "$wlrctl_bin" pointer click left >> "$join_reply_path" 2>&1
fi
"$wlrctl_bin" pointer move -4000 -4000 >> "$join_reply_path" 2>&1
sample c
sleep 3
"$grim_bin" -g "$join_region" "$shot_dir/join-c-rest.png" > /dev/null 2>&1
call panel close
sample c-close
sleep 2
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
  rgb=$($convert_bin "$1" -format \
    "%[fx:int(255*p{$2,$3}.r+0.5)] %[fx:int(255*p{$2,$3}.g+0.5)] %[fx:int(255*p{$2,$3}.b+0.5)]" \
    info: 2>/dev/null)
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

# The card's own fill, as the ink column $2 of frame $1 carries on most of the
# rows between $3 and $4: one number read off the rig's own pixels, since a
# theme colour spelled out here would be a second copy of the palette. Four
# columns in from the card's own side is its padding, which is plain fill on
# every row but the few a rule or a hovered row runs the full width of.
join_fill() {
  join_column "$1" "$2" \
    | awk -v t="$3" -v b="$4" 'NR - 1 >= t && NR - 1 <= b { n[$1]++ }
        END { for (k in n) if (n[k] > m) { m = n[k]; v = k } print v }'
}

# Every row of frame $1, from row $2 down for $3 rows, whose outermost painted
# column on either side carries the card's own fill ($4) instead of its
# border: the silhouette running into the desktop with no edge on it, one
# "<side> <row> <column>" per line. What counts as painted is a channel sum
# more than 6 off the second bar's own resting frame, which has the pointer's
# hover on the strip in it already. A row carrying fewer than 40 painted
# columns is no card at all and is skipped. A dark theme's border over a
# dark desktop, antialiased across a fractional edge, lands inside that
# tolerance, so a fill column is bordered when the column past it is darker
# than the desktop, or when the column after that is still painted. The
# second case is a 0.75 black border on a fractional edge: the fill and the
# stroke each cover part of the outer pixels, and coverage compositing lets
# the fill outweigh the border there, so they read as the desktop or a step
# lighter. A clip that cuts the border, or a fill with none, leaves one
# partial column at most before the desktop.
join_borderless() {
  $convert_bin "$shot_dir/join-b-rest.png" -crop "${join_crop_width}x$3+0+$2" +repage \
    -depth 8 txt:- 2>/dev/null > "$shot_dir/join-base.txt"
  $convert_bin "$1" -crop "${join_crop_width}x$3+0+$2" +repage -depth 8 txt:- 2>/dev/null \
    | awk -v top="$2" -v fill="$4" '
      function delta(a, b,   x, y, i, s) {
        split(a, x, ","); split(b, y, ",");
        s = 0;
        for (i = 1; i <= 3; i++) s += (x[i] > y[i] ? x[i] - y[i] : y[i] - x[i]);
        return s
      }
      function darker(a, b,   x, y) {
        if (a == "" || b == "") return 0;
        split(a, x, ","); split(b, y, ",");
        return x[1] + x[2] + x[3] < y[1] + y[2] + y[3]
      }
      function edged(k, d, y,   k1, k2) {
        k1 = (k + d) "," y; k2 = (k + 2 * d) "," y;
        if (darker(cur[k1], base[k1])) return 1;
        return cur[k2] != "" && delta(cur[k2], base[k2]) > 0
      }
      FNR == 1 { next }
      { split($1, c, ","); sub(":", "", c[2]); gsub(/[()]/, "", $2);
        x = c[1] + 0; y = c[2] + 0 }
      NR == FNR { base[x "," y] = $2; next }
      { hex[x "," y] = toupper(substr($3, 2, 6)); cur[x "," y] = $2
        if (delta($2, base[x "," y]) > 6) {
          if (!(y in first)) first[y] = x;
          last[y] = x
        } }
      END {
        for (y in first) {
          a = first[y]; b = last[y];
          if (b - a < 40) continue;
          if (hex[a "," y] == fill && !edged(a, -1, y)) print "L", y + top, a;
          if (hex[b "," y] == fill && !edged(b, 1, y)) print "R", y + top, b
        }
      }' "$shot_dir/join-base.txt" -
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
  for name in a a-close b c c-close b-close d d-close; do
    for i in $(seq 1 $join_frames); do
      path="$shot_dir/join-$name-$i.png"
      [ -f "$path" ] || fail "no join frame $path"
      echo "SMOKE_JOIN_$(echo "$name" | tr 'a-z-' 'A-Z_')_$i $path"
    done
  done
  for name in a b c d; do
    path="$shot_dir/join-$name-rest.png"
    [ -f "$path" ] || fail "no join frame $path"
    echo "SMOKE_JOIN_$(echo "$name" | tr 'a-z' 'A-Z')_REST $path"
  done
  [ -f "$join_desktop_path" ] || fail "no bare desktop frame at $join_desktop_path"
  echo "SMOKE_JOIN_DESKTOP $join_desktop_path"
  for path in "$join_bare_path" "$join_owner_path"; do
    [ -f "$path" ] || fail "no whole-output frame at $path"
  done
  echo "SMOKE_JOIN_OWNER $join_owner_path"
  cat "$join_click_path" 2>/dev/null || true
  grep -qE '^click [0-9]+ [0-9]+$' "$join_click_path" 2>/dev/null || fail \
    "no cell was found inside the chevron's second bar, so case c clicked nothing: $(cat "$join_click_path" 2>/dev/null)"

  # The edge every case above was photographed on, off the shell's own
  # numbers: a position that landed late would otherwise photograph a top bar
  # and read as a pass on the frames alone.
  local want=top
  leg_on bar_position && want=$(leg_arg bar_position)
  grep -q "\"barPosition\":\"$want\"" "$join_dump_path" 2>/dev/null || fail \
    "the shell reports a bar on another edge than the $want this run asked for: $(head -c 400 "$join_dump_path" 2>/dev/null)"
  echo "SMOKE_JOIN_EDGE ok $want"

  if join_whole_output; then
    for name in SEAM WALL BUD BUD_BORDER; do
      echo "SMOKE_JOIN_$name skipped (layout): the probes are pinned to the top bar's own crop; the frames above carry this case"
    done
    return 0
  fi

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
  reach=$("$jq_bin" -r '.theme.radiusXl | round' "$join_dump_path" 2>/dev/null)
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

  # --- The bud (M57 D3) ------------------------------------------------
  #
  # The second bar itself, off the frame taken with it open and nothing
  # hanging off it, and the child's resting top under it. join-b-rest.png is
  # the reference the whole claim is read against: the bare desktop has no
  # second bar in it at all, and the rows the second bar covers carry the
  # pointer's own hover by the time case c is photographed, which is why the
  # band starts one row past its far edge.
  local box owner_w owner_h owner_x owner_y owner_bottom
  box=$(join_bbox "$shot_dir/join-b-rest.png" "$join_desktop_path" $((join_line_row + 1))) || fail \
    "nothing under the bar differs from the bare desktop in join-b-rest.png: the chevron's second bar never opened"
  read -r owner_w owner_h owner_x owner_y <<< "$box"
  owner_bottom=$((owner_y + owner_h - 1))

  local rest_w rest_h rest_x rest_y
  box=$(join_bbox "$shot_dir/join-c-rest.png" "$shot_dir/join-b-rest.png" $((owner_bottom + 1))) || fail \
    "nothing under the second bar differs from join-b-rest.png in join-c-rest.png: the child never opened"
  read -r rest_w rest_h rest_x rest_y <<< "$box"
  if [ "$rest_w" -lt $((join_card_width - 2)) ] || [ "$rest_w" -gt $((join_card_width + 4)) ]; then
    fail "the card in join-c-rest.png is ${rest_w}px wide against the $join_card_width it rests at: it never widened out of the bud"
  fi

  # Everything under the second bar's far edge, on the frames taken before the
  # card lets go of it. How far out the card is stands in for the pose: the
  # shape reaches `releaseAt` (0.85) of its resting depth on the tick the
  # attach clock starts, so a frame under four fifths of that depth is one the
  # card is still wholly on the edge for. Past it the card widens to its own
  # rect as it pulls away, which is what the clock is for, and those frames
  # say nothing about the bud.
  local band_top frames_attached bw bh bx by right
  band_top=$((owner_bottom + 1))
  frames_attached=0
  for i in $(seq 1 $join_frames); do
    path="$shot_dir/join-c-$i.png"
    box=$(join_bbox "$path" "$shot_dir/join-b-rest.png" "$band_top") || continue
    read -r bw bh bx by <<< "$box"
    [ $((bh * 100)) -lt $((rest_h * 80)) ] || continue
    frames_attached=$((frames_attached + 1))
    right=$((bx + bw - 1))
    if [ "$bx" -lt "$owner_x" ] || [ "$right" -ge $((owner_x + owner_w)) ]; then
      fail "$path paints columns $bx-$right under the second bar's own $owner_x-$((owner_x + owner_w - 1)) while the card is still on its edge: the bud is wider than the edge it buds from"
    fi
  done
  [ "$frames_attached" -gt 0 ] || fail \
    "no attached frame among join-c-1..$join_frames: nothing under row $band_top is shallower than four fifths of the card's own $rest_h rows"
  echo "SMOKE_JOIN_BUD ok $frames_attached attached frame(s) inside columns $owner_x-$((owner_x + owner_w - 1)), rest ${rest_w}x$rest_h at row $rest_y"

  # And the bud's own border while it widens. The card is far enough out of
  # the owner's edge to read its two sides and still narrower than its own
  # resting rect; on every row well inside it,
  # the outermost painted column on either side has to be the border's ink
  # rather than the fill's. A clip taken along the line from the undeformed
  # rect cuts exactly that column off whichever side the deform's stretch has
  # carried past the band, and the silhouette runs into the desktop with no
  # edge there for as long as that lasts. The card's own two ends are left out
  # of the scan by `inset`: inside a corner's radius the outermost pixel is
  # the arc's own antialiased tip, which is not the border's flat ink and
  # never was.
  local mid=0 inset=32 bare fill seen=""
  fill=$(join_fill "$shot_dir/join-c-rest.png" $((rest_x + 4)) \
    $((rest_y + inset)) $((rest_y + rest_h - inset)))
  [ -n "$fill" ] || fail "could not read the card's own fill out of join-c-rest.png"
  for i in $(seq 1 $join_frames); do
    path="$shot_dir/join-c-$i.png"
    box=$(join_bbox "$path" "$shot_dir/join-b-rest.png" "$band_top") || continue
    read -r bw bh bx by <<< "$box"
    seen="$seen ${bw}x$bh"
    [ $((bh * 100)) -ge $((rest_h * 60)) ] || continue
    [ "$bw" -lt $((rest_w - 6)) ] || continue
    [ "$bh" -gt $((inset * 3)) ] || continue
    bare=$(join_borderless "$path" $((by + inset)) $((bh - inset * 2)) "$fill")
    [ -z "$bare" ] || fail \
      "$path runs into the desktop with the card's own fill $fill at its outermost column, at side/row/column:$(echo "$bare" | tr '\n' ';') while the bud widens"
    mid=$((mid + 1))
  done
  [ "$mid" -gt 0 ] || fail \
    "no widening frame among join-c-1..$join_frames: no box under row $band_top is both three fifths of the card's own $rest_h rows deep and still narrower than its $rest_w columns (saw$seen)"
  echo "SMOKE_JOIN_BUD_BORDER ok $mid widening frame(s) bordered on every row of both sides against the fill $fill"
}
