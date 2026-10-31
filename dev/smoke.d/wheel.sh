# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --wheel (M47 D3): the mouse wheel scrolls a launcher view, and the cursor
# stays where it is. One notch moves it exactly one step, and a scroll far
# past the end stops on it. The wallpaper grid is the surface that reported the
# bug, so it is the one driven here, over a directory of 40 fixtures, which
# is enough rows to overflow the card whatever the output height does to the
# cap.
#
# wlrctl, not an IPC "scroll" verb: what needs proving is the pointer path
# end to end (a real zwlr_virtual_pointer_v1 axis event, into the layer
# surface, past the cell under it, into the flickable), and a shell-side
# call would skip exactly the part that was broken. Same line the tooltip
# leg draws for hover, including its relative-only pointer trick: the
# pointer is slammed into the corner first, then moved once from a known
# origin.
#
# The reading, not the frame, is the claim. Two screenshots of a scrolled
# grid look alike whether the cursor followed the wheel or not, so the
# assertion is `menu status`'s scrollTop against `picker status`'s cursor,
# and `menu status`'s `hover`: the cell the pointer tints before the notch
# is the one it tints after, the content having moved under a pointer that
# did not.
#
# The other half of the contract runs on the same timeline once the grid is
# closed: a notch on the bar's audio cell has to keep adjusting the sink.
# That is the one path `Cell` still accepts the wheel on (`Cell.wheeled` with
# a consumer that re-accepts), so a fix that made lists scroll by making
# every cell transparent to the wheel would pass the half above and silently
# break this one. wpctl reads the sink back, since no frame can show that a
# volume moved for the right reason.
#
# Between the two, a touchpad over the same grid, through dev/vpointer.py's
# `finger` frames (axis_source finger) and `lift` (axis_stop), which wlrctl
# cannot send. From the end, 100px of finger travel up has to move
# scrollTop by exactly the finger gain (scroll.rs FINGER_GAIN, 1.12). Then
# 200px down with the fingers held: a status read mid-gesture has to show
# scrollTop past scrollMax, by less than the stretch's 40px ceiling, and
# 400ms after the lift exactly on it again. Then a wheel notch down while
# already at the end, read five times through its glide: never past it.
#
# Last the coast after a flick (scroll.rs `Touchpad`), status read back to
# back behind a millisecond stamp each. 30px of fingers at 70 px/s lifted
# at once coasts nothing. From the top, seven 60px finger frames 8ms apart
# and a lift at once: scrollTop keeps rising after the lift, never back,
# slower in each third of the run, and stops within 1.3s, its coast no
# longer than one viewHeight (the cap). The same flick with a finger
# touching 150ms after the lift stops there, short of where the free one
# went. The same flick 5 notches short of the end runs into it: overscroll
# goes above 0 and stays under the 40px stretch cap, and it settles
# exactly on scrollMax with nothing stretched, the frame then showing the
# last row whole. The 100px of travel up above lifts after a 100ms pause,
# so it coasts nothing either.
leg_wheel_flag="--wheel"
leg_wheel_order=125
leg_wheel_needs="wlrctl convert wpctl python3"

wheel_before_png="$shot_dir/wheel-before.png"
wheel_after_png="$shot_dir/wheel-after.png"
wheel_menu_before_path="$shot_dir/wheel-menu-before.json"
wheel_menu_after_path="$shot_dir/wheel-menu-after.json"
wheel_picker_before_path="$shot_dir/wheel-picker-before.json"
wheel_picker_after_path="$shot_dir/wheel-picker-after.json"
wheel_menu_end_path="$shot_dir/wheel-menu-end.json"
wheel_finger_up_path="$shot_dir/wheel-finger-up.json"
wheel_finger_mid_path="$shot_dir/wheel-finger-mid.json"
wheel_finger_settled_path="$shot_dir/wheel-finger-settled.json"
wheel_notch_end_path="$shot_dir/wheel-notch-end.jsonl"
wheel_finger_png="$shot_dir/wheel-finger-stretch.png"
wheel_finger_settled_png="$shot_dir/wheel-finger-settled.png"
wheel_slow_lift_path="$shot_dir/wheel-slow-lift.json"
wheel_slow_settled_path="$shot_dir/wheel-slow-settled.json"
wheel_coast_path="$shot_dir/wheel-coast.txt"
wheel_touch_path="$shot_dir/wheel-touch.json"
wheel_touch_settled_path="$shot_dir/wheel-touch-settled.json"
wheel_bounce_path="$shot_dir/wheel-bounce.txt"
wheel_coast_png="$shot_dir/wheel-coast-settled.png"
wheel_bounce_png="$shot_dir/wheel-bounce-settled.png"
wheel_vpointer="$PWD/dev/vpointer.py"
wheel_dispatch_path="$shot_dir/wheel-dispatch.txt"
wheel_bar_png="$shot_dir/wheel-bar.png"
wheel_volume_before_path="$shot_dir/wheel-volume-before.txt"
wheel_volume_after_path="$shot_dir/wheel-volume-after.txt"
wheel_dir="$iso_home/.local/share/formalshell/wheel-pictures"

# Both legs own picker.directory, and the picker leg asserts its own fixture
# count down to the number, so the two cannot share one settings file.
leg_wheel_validate() {
  if leg_on picker; then
    echo "usage: $0 --wheel and --picker both set picker.directory, run them separately" >&2
    exit 1
  fi
}

leg_wheel_fixture() {
  local i
  mkdir -p "$wheel_dir"
  # Small: 40 of them, and nothing here reads a pixel of the thumbnails.
  # The hue walks so a screenshot shows which rows are on screen.
  for i in $(seq 0 119); do
    $convert_bin -size 320x180 "xc:hsl($((i * 9)),70%,45%)" "$wheel_dir/img-$(printf '%02d' "$i").png"
  done
  settings_fragment ', "picker": {"directory": "'"$wheel_dir"'"}'
}

# The grid covers the whole output, so under --wallpaper this starts past
# that leg's own last frame, the same clock picker_t0 keeps.
wheel_t0() {
  cover_t0 4
}

leg_wheel_timing() {
  local t0
  t0=$(wheel_t0)
  leg_timing $((32 + t0)) $((73 + t0))
}

leg_wheel_drive() {
  local t0 script="$shot_dir/wheel-drive.sh"
  t0=$(wheel_t0)
  # 893x604 is the middle of a THUMBNAIL, second row, second column: the
  # card is centred and popupWidthMenu wide, its top edge sits at 30% of a
  # 1080-tall output, and the grid starts under the header band's rule, its
  # own inset below that. The middle of the grid is not good enough: 960
  # lands in the gutter between two cells, where the wheel reaches the
  # GridView without passing a cell at all, which is the one path that was
  # never broken.
  #
  # 1731x20 is the bar's audio cell, the leading cell of the right group,
  # vertically centred in the bar. It is the second half's target, once the
  # grid is closed and the bar is reachable again.
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
$ipc call picker summon > /dev/null 2>&1
sleep 2
"$wlrctl_bin" pointer move -4000 -4000 > "$wheel_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer move 893 604 >> "$wheel_dispatch_path" 2>&1
sleep 2
"$grim_bin" "$wheel_before_png" > /dev/null 2>&1
$ipc call menu status > "$wheel_menu_before_path" 2>&1
$ipc call picker status > "$wheel_picker_before_path" 2>&1
"$wlrctl_bin" pointer scroll 15 0 >> "$wheel_dispatch_path" 2>&1
sleep 3
"$grim_bin" "$wheel_after_png" > /dev/null 2>&1
$ipc call menu status > "$wheel_menu_after_path" 2>&1
$ipc call picker status > "$wheel_picker_after_path" 2>&1
"$wlrctl_bin" pointer scroll 100000 0 >> "$wheel_dispatch_path" 2>&1
sleep 2
$ipc call menu status > "$wheel_menu_end_path" 2>&1
"$python3_bin" "$wheel_vpointer" finger -50 wait 16 finger -50 wait 100 lift >> "$wheel_dispatch_path" 2>&1
sleep 1
$ipc call menu status > "$wheel_finger_up_path" 2>&1
"$python3_bin" "$wheel_vpointer" $(for _ in $(seq 20); do printf 'finger 10 wait 8 '; done) wait 100 >> "$wheel_dispatch_path" 2>&1
$ipc call menu status > "$wheel_finger_mid_path" 2>&1
"$grim_bin" "$wheel_finger_png" > /dev/null 2>&1
"$python3_bin" "$wheel_vpointer" lift >> "$wheel_dispatch_path" 2>&1
sleep 0.4
$ipc call menu status > "$wheel_finger_settled_path" 2>&1
"$grim_bin" "$wheel_finger_settled_png" > /dev/null 2>&1
: > "$wheel_notch_end_path"
"$wlrctl_bin" pointer scroll 15 0 >> "$wheel_dispatch_path" 2>&1
for _ in 1 2 3 4 5; do
  $ipc call menu status >> "$wheel_notch_end_path" 2>&1
  echo >> "$wheel_notch_end_path"
done
sleep 1
# One status a line behind its millisecond stamp, for \$2 ms.
sample() {
  local end=\$((\$(date +%s%3N) + \$2))
  : > "\$1"
  while [ "\$(date +%s%3N)" -lt "\$end" ]; do
    printf '%s ' "\$(date +%s%3N)" >> "\$1"
    $ipc call menu status 2>&1 | tr -d '\n' >> "\$1"
    echo >> "\$1"
  done
}
flick="\$(for _ in 1 2 3 4 5 6; do printf 'finger 60 wait 8 '; done)finger 60 lift"
"$python3_bin" "$wheel_vpointer" \$(for _ in \$(seq 30); do printf 'finger -1 wait 16 '; done) lift >> "$wheel_dispatch_path" 2>&1
$ipc call menu status > "$wheel_slow_lift_path" 2>&1
sleep 0.6
$ipc call menu status > "$wheel_slow_settled_path" 2>&1
"$wlrctl_bin" pointer scroll -100000 0 >> "$wheel_dispatch_path" 2>&1
sleep 1.5
"$python3_bin" "$wheel_vpointer" \$flick >> "$wheel_dispatch_path" 2>&1
echo "\$(date +%s%3N)" > "$wheel_coast_path.lift"
sample "$wheel_coast_path" 1800
"$grim_bin" "$wheel_coast_png" > /dev/null 2>&1
"$wlrctl_bin" pointer scroll -100000 0 >> "$wheel_dispatch_path" 2>&1
sleep 1.5
"$python3_bin" "$wheel_vpointer" \$flick wait 150 finger 1 wait 300 lift >> "$wheel_dispatch_path" 2>&1
$ipc call menu status > "$wheel_touch_path" 2>&1
sleep 0.5
$ipc call menu status > "$wheel_touch_settled_path" 2>&1
"$wlrctl_bin" pointer scroll 100000 0 >> "$wheel_dispatch_path" 2>&1
sleep 1.5
"$wlrctl_bin" pointer scroll -75 0 >> "$wheel_dispatch_path" 2>&1
sleep 1.5
"$python3_bin" "$wheel_vpointer" \$flick >> "$wheel_dispatch_path" 2>&1
echo "\$(date +%s%3N)" > "$wheel_bounce_path.lift"
sample "$wheel_bounce_path" 1800
"$grim_bin" "$wheel_bounce_png" > /dev/null 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 1
"$wpctl_bin" get-volume @DEFAULT_AUDIO_SINK@ 2>&1 | grep '^Volume:' > "$wheel_volume_before_path"
"$wlrctl_bin" pointer move -4000 -4000 >> "$wheel_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer move 1731 20 >> "$wheel_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer scroll 10 0 >> "$wheel_dispatch_path" 2>&1
sleep 2
"$grim_bin" -c "$wheel_bar_png" > /dev/null 2>&1
"$wpctl_bin" get-volume @DEFAULT_AUDIO_SINK@ 2>&1 | grep '^Volume:' > "$wheel_volume_after_path"
EOF
  hypr_exec_once "bash $script"
}

wheel_field() {
  sed -n 's/.*"'"$2"'":\([0-9-]*\).*/\1/p' "$1"
}

leg_wheel_assert() {
  local f before_scroll after_scroll before_cursor after_cursor
  for f in "$wheel_menu_before_path" "$wheel_menu_after_path" \
    "$wheel_picker_before_path" "$wheel_picker_after_path"; do
    if [ ! -s "$f" ]; then
      fail "no status dump produced at $f"
    fi
    cat "$f"; echo
  done
  # Printed on the happy path too: the only evidence of where the pointer
  # ended up and that the axis event was sent at all.
  cat "$wheel_dispatch_path" 2>/dev/null || true
  # 120 fixtures and the Sky cell after them.
  if ! grep -q '"count":121' "$wheel_picker_before_path"; then
    fail "the grid did not list all 120 fixtures and the sky, so an unscrolled view proves nothing, got: $(cat "$wheel_picker_before_path")"
  fi
  before_scroll=$(wheel_field "$wheel_menu_before_path" scrollTop)
  after_scroll=$(wheel_field "$wheel_menu_after_path" scrollTop)
  before_cursor=$(wheel_field "$wheel_picker_before_path" cursor)
  after_cursor=$(wheel_field "$wheel_picker_after_path" cursor)
  if [ -z "$before_scroll" ] || [ -z "$after_scroll" ]; then
    fail "menu status carried no scrollTop"
  fi
  if [ "$before_scroll" != "0" ]; then
    fail "the grid was already scrolled to $before_scroll before the wheel moved"
  fi
  # Hyprland reads wlrctl's smooth-only scroll as a wheel's, 15 units to the
  # 120-unit notch, so 15 is exactly one notch: one row of cells, no more.
  local step
  step=$(wheel_field "$wheel_menu_after_path" wheelStep)
  if [ -z "$step" ] || [ "$after_scroll" != "$step" ]; then
    fail "one wheel notch moved scrollTop to $after_scroll, not one step of ${step:-nothing}"
  fi
  # A scroll far past the end stops on it.
  local end_scroll end_max
  end_scroll=$(wheel_field "$wheel_menu_end_path" scrollTop)
  end_max=$(wheel_field "$wheel_menu_end_path" scrollMax)
  if [ -z "$end_max" ] || [ "$end_max" -le 0 ] || [ "$end_scroll" != "$end_max" ]; then
    fail "a scroll far past the end left scrollTop at $end_scroll against a scrollMax of $end_max"
  fi
  echo "SMOKE_WHEEL_END scrollTop $end_scroll = scrollMax $end_max"
  # The touchpad half, from that end.
  local up_scroll mid_scroll mid_over settled_scroll settled_over want_up
  for f in "$wheel_finger_up_path" "$wheel_finger_mid_path" "$wheel_finger_settled_path"; do
    if [ ! -s "$f" ]; then
      fail "no status dump produced at $f"
    fi
  done
  up_scroll=$(wheel_field "$wheel_finger_up_path" scrollTop)
  want_up=$((end_max - 112))
  if [ "$up_scroll" != "$want_up" ]; then
    fail "100px of finger travel up from $end_max left scrollTop at $up_scroll, not $want_up (the 1.12 gain)"
  fi
  mid_scroll=$(wheel_field "$wheel_finger_mid_path" scrollTop)
  mid_over=$(wheel_field "$wheel_finger_mid_path" overscroll)
  if [ -z "$mid_scroll" ] || [ "$mid_scroll" -le "$end_max" ] || [ "$((mid_scroll - end_max))" -ge 40 ]; then
    fail "a finger held past the end read scrollTop $mid_scroll against scrollMax $end_max, not a stretch under 40px"
  fi
  settled_scroll=$(wheel_field "$wheel_finger_settled_path" scrollTop)
  settled_over=$(wheel_field "$wheel_finger_settled_path" overscroll)
  if [ "$settled_scroll" != "$end_max" ] || [ "$settled_over" != "0" ]; then
    fail "400ms after the lift scrollTop read $settled_scroll (overscroll $settled_over) against scrollMax $end_max"
  fi
  echo "SMOKE_WHEEL_FINGER up $up_scroll, held $mid_scroll (overscroll $mid_over), 400ms after lift $settled_scroll = scrollMax $end_max"
  local reads over
  reads=$(sed -n 's/.*"scrollTop":\([0-9-]*\).*/\1/p' "$wheel_notch_end_path" | tr '\n' ' ')
  if [ "$(echo "$reads" | wc -w)" -lt 5 ]; then
    fail "the notch at the end produced $(echo "$reads" | wc -w) status reads, not 5"
  fi
  for over in $reads; do
    if [ "$over" -gt "$end_max" ]; then
      fail "a wheel notch at the end read scrollTop $over past scrollMax $end_max"
    fi
  done
  echo "SMOKE_WHEEL_NOTCH_END scrollTop $reads<= scrollMax $end_max"
  wheel_assert_coast "$end_max"
  if [ -z "$before_cursor" ] || [ "$before_cursor" != "$after_cursor" ]; then
    fail "the wheel moved the cursor from $before_cursor to $after_cursor"
  fi
  local before_hover after_hover
  before_hover=$(wheel_field "$wheel_menu_before_path" hover)
  after_hover=$(wheel_field "$wheel_menu_after_path" hover)
  if [ -z "$before_hover" ] || [ "$before_hover" -lt 0 ] || [ "$before_hover" != "$after_hover" ]; then
    fail "the wheel moved the hover from cell ${before_hover:-none} to ${after_hover:-none}"
  fi
  echo "SMOKE_WHEEL scrollTop $before_scroll -> $after_scroll, cursor held at $after_cursor, hover held on cell $after_hover"
  for f in "$wheel_before_png" "$wheel_after_png" "$wheel_bar_png"; do
    if [ ! -f "$f" ]; then
      fail "no wheel screenshot produced at $f"
    fi
  done
  echo "SMOKE_WHEEL_BEFORE $wheel_before_png"
  echo "SMOKE_WHEEL_AFTER $wheel_after_png"
  echo "SMOKE_WHEEL_FINGER_STRETCH $wheel_finger_png"
  echo "SMOKE_WHEEL_FINGER_SETTLED $wheel_finger_settled_png"
  # The slider half: a notch on the bar's audio cell still moves the sink.
  local before_volume after_volume
  before_volume=$(cat "$wheel_volume_before_path" 2>/dev/null)
  after_volume=$(cat "$wheel_volume_after_path" 2>/dev/null)
  echo "bar audio cell volume: $before_volume -> $after_volume"
  case "$before_volume" in
    Volume:*) ;;
    *) fail "wpctl read no sink volume before the bar notch, got: $before_volume" ;;
  esac
  if [ "$before_volume" = "$after_volume" ]; then
    fail "a notch on the bar's audio cell left the sink at $after_volume, so Cell.wheeled no longer reaches its consumer"
  fi
  echo "SMOKE_WHEEL_BAR $wheel_bar_png"
}

# `stamp scrollTop overscroll coasting` a line, off a sample file.
wheel_samples() {
  local line top over coast
  while IFS= read -r line; do
    top=$(sed -n 's/.*"scrollTop":\([0-9-]*\).*/\1/p' <<< "$line")
    over=$(sed -n 's/.*"overscroll":\([0-9-]*\).*/\1/p' <<< "$line")
    coast=$(sed -n 's/.*"coasting":\([a-z]*\).*/\1/p' <<< "$line")
    if [ -n "$top" ] && [ -n "$over" ] && [ -n "$coast" ]; then
      echo "${line%% *} $top $over $coast"
    fi
  done < "$1"
}

wheel_assert_coast() {
  local end_max=$1 f slow_a slow_b view lift report free_final touch_a touch_b
  for f in "$wheel_slow_lift_path" "$wheel_slow_settled_path" "$wheel_touch_path" "$wheel_touch_settled_path" \
    "$wheel_coast_path" "$wheel_bounce_path" "$wheel_coast_path.lift" "$wheel_bounce_path.lift"; do
    if [ ! -s "$f" ]; then
      fail "no coast reading produced at $f"
    fi
  done
  # 70 px/s of fingers lifted at once stays where it was lifted.
  slow_a=$(wheel_field "$wheel_slow_lift_path" scrollTop)
  slow_b=$(wheel_field "$wheel_slow_settled_path" scrollTop)
  if [ -z "$slow_a" ] || [ "$slow_a" != "$slow_b" ] || [ "$slow_a" -ge "$end_max" ] || grep -q '"coasting":true' "$wheel_slow_settled_path"; then
    fail "a slow drag lifted at scrollTop $slow_a read $slow_b 600ms later (scrollMax $end_max): it coasted"
  fi
  echo "SMOKE_WHEEL_SLOW_LIFT scrollTop $slow_a held at $slow_b"
  view=$(wheel_field "$wheel_slow_settled_path" viewHeight)
  # The flick from 0 lifts at 7 x 60 x 1.12 = 470.
  lift=$(cat "$wheel_coast_path.lift")
  if ! report=$(wheel_samples "$wheel_coast_path" | awk -v lift="$lift" -v view="$view" -v start=470 '
    { t[n] = $1 - lift; p[n] = $2; c[n] = $4; n++ }
    END {
      if (n < 10) { print "only " n " coast samples"; exit 1 }
      for (i = 1; i < n; i++) if (p[i] < p[i - 1]) { print "scrollTop went back from " p[i - 1] " to " p[i] " at " t[i] "ms"; exit 1 }
      final = p[n - 1]
      if (c[n - 1] != "false") { print "still coasting " t[n - 1] "ms after the lift"; exit 1 }
      settle = -1
      for (i = 0; i < n; i++) if (p[i] == final && c[i] == "false") { settle = t[i]; break }
      run = final - start
      if (run < 100) { print "the flick coasted " run "px, from 470 to " final; exit 1 }
      if (run > view + 1) { print "the flick coasted " run "px past its one viewHeight cap of " view; exit 1 }
      if (settle > 1300) { print "the coast settled " settle "ms after the lift"; exit 1 }
      # Speed over each third of the run, lift to settle.
      for (k = 0; k < 3; k++) {
        a = settle * k / 3; b = settle * (k + 1) / 3; lo = -1; hi = -1
        for (i = 0; i < n; i++) { if (t[i] >= a && lo < 0) lo = i; if (t[i] <= b) hi = i }
        v[k] = (hi > lo && lo >= 0) ? (p[hi] - p[lo]) * 1000 / (t[hi] - t[lo]) : 0
      }
      if (!(v[0] > v[1] && v[1] >= v[2])) { printf "the coast did not slow: %d, %d, %d px/s by thirds\n", v[0], v[1], v[2]; exit 1 }
      printf "coasted %dpx (cap %d) from 470 to %d, settled %dms after the lift, %d %d %d px/s by thirds, %d samples\n", run, view, final, settle, v[0], v[1], v[2], n
    }'); then
    fail "$report"
  fi
  echo "SMOKE_WHEEL_COAST $report"
  free_final=$(wheel_samples "$wheel_coast_path" | tail -1 | cut -d' ' -f2)
  touch_a=$(wheel_field "$wheel_touch_path" scrollTop)
  touch_b=$(wheel_field "$wheel_touch_settled_path" scrollTop)
  if [ -z "$touch_a" ] || [ "$touch_a" != "$touch_b" ] || [ "$touch_a" -le 470 ] || [ "$touch_a" -ge "$((free_final - 20))" ] \
    || grep -q '"coasting":true' "$wheel_touch_settled_path"; then
    fail "a finger 150ms into the coast left scrollTop $touch_a then $touch_b, against 470 at the lift and $free_final for a free coast"
  fi
  echo "SMOKE_WHEEL_COAST_TOUCH stopped at $touch_a, a free coast went to $free_final"
  lift=$(cat "$wheel_bounce_path.lift")
  if ! report=$(wheel_samples "$wheel_bounce_path" | awk -v lift="$lift" -v max="$end_max" '
    BEGIN { settle = -1; peak = 0 }
    {
      t = $1 - lift
      if ($2 - $3 > max) { print "scrollTop " $2 " less overscroll " $3 " past scrollMax " max; bad = 1; exit 1 }
      if ($3 > peak) peak = $3
      last = $2; over = $3; c = $4; n++
      if (last == max && over == 0 && c == "false") { if (settle < 0) settle = t } else settle = -1
    }
    END {
      if (bad) exit 1
      if (n < 10) { print "only " n " bounce samples"; exit 1 }
      if (peak <= 0 || peak >= 40) { print "the bounce peaked at " peak "px, not between 0 and 40"; exit 1 }
      if (settle < 0) { print "it ended at " last " overscroll " over " coasting " c " against scrollMax " max; exit 1 }
      if (settle > 1500) { print "the bounce settled " settle "ms after the lift"; exit 1 }
      printf "bounced %dpx past scrollMax %d, settled on it %dms after the lift, %d samples\n", peak, max, settle, n
    }'); then
    fail "$report"
  fi
  echo "SMOKE_WHEEL_COAST_BOUNCE $report"
  echo "SMOKE_WHEEL_COAST_SETTLED $wheel_coast_png"
  echo "SMOKE_WHEEL_BOUNCE_SETTLED $wheel_bounce_png"
}
