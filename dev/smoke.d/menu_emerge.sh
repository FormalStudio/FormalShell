# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --menu-emerge proves the launcher's own open (M57 D5): the card buds off the
# top line, the same metamorphosis a panel does off the bar's hairline, and is
# a plain card at its resting rect once it has let go. It replaced the unfold,
# where the card appeared at its search row's height and grew downward.
#
# At full speed the card is on the line for about as long as one screencopy
# takes, so the open runs at a tenth of its speed (`debug motionScale`, the
# same instrument --join uses) and is photographed frame by frame, back to
# back, with the wall clock stamped after each capture. Six claims, plus the
# number the whole change was made against:
#
#   out of the line   an early frame carries the card's own fill directly
#                     under the line at the card's centre column and nothing
#                     at the card's resting bottom rows. A card materialising
#                     in place covers its own bottom from the first frame and
#                     never touches the line at all.
#   the lit bar       on that same frame a bar pixel clear of the join is
#                     byte-equal to the closed output's, while the desktop
#                     below it is not: the scrim's share over the line's own
#                     band rides `1 - attach`, so the card buds off a lit bar.
#   the plain card    the rest frame has the card at its resting rect with a
#                     top border of its own and a row of desktop-through-scrim
#                     between the line and it, and the bar's band dimmed like
#                     the rest of the output.
#   the scrim         read off a `debug dump` at rest: every scrim pixel's own
#                     alpha at or under the launcher namespace's
#                     `ignore_alpha`, so the compositor never blurs behind it.
#   the dim           the rest frame's bar band and desktop each at 1 - alpha
#                     of their closed level: dimmed, not untouched, not black.
#                     Read on every theme, `--pantheon` included.
#   the settle       the first captured frame byte-equal to the rest frame,
#                     stamped against the summon that started it and divided
#                     by the motion scale. Printed before anything is
#                     asserted: it is the figure the owner's "don't make the
#                     animation any slower" is checked against, and it has to
#                     survive a run whose assertions fail.
#   the fade          the card and the scrim rise through the open and fall
#                     through the close, read off a second burst taken
#                     after `menu close`: a popover that travels at full
#                     opacity and is then cut (a subsurface multiplier
#                     Hyprland never applied) fails it.
#
# Probes are read by mean brightness, not by identity: the card is still being
# squashed for a beat after it lands (M54 D7), so a probe standing well inside
# it compares unequal to the settled frame for most of the open, and the scrim
# over the whole output makes --panel-emerge's "anything but the desktop" say
# nothing. Nothing the card draws is darker than its own fill, so a threshold
# halfway between that fill and the scrim under it separates "the card has
# reached this row" from "this is still the scrim" whatever is underneath.
#
# The card's own left edge, width and top are spelled out rather than measured:
# a `popupWidthMenu` card centred on the output with its top at 30% of it is
# what the tokens and the launcher's top fraction say, so a token change has to
# be a deliberate edit here too.
#
# Which row the line itself is on is the one number that is neither pinned nor
# measured: it comes out of a `debug dump`, so the same probes read a top
# bar's last row, a frame ring's top band on a run carrying `--frame` or
# `--bar-position <edge>`, and, on a top edge with neither, nothing at all. A
# card with no line to bud off has no claim to make here, so that last case
# prints as skipped and keeps its frames.
leg_menu_emerge_flag="--menu-emerge"
leg_menu_emerge_order=21
leg_menu_emerge_needs="convert jq"

menu_emerge_scale_path="$shot_dir/menu-emerge-scale.txt"
menu_emerge_summon_path="$shot_dir/menu-emerge-summon.txt"
menu_emerge_close_path="$shot_dir/menu-emerge-close.txt"
# Touched once the open and the close are photographed and the clock is
# back at full speed, for a leg sharing the session that drives a card of
# its own (--join) to start after.
menu_emerge_done_path="$shot_dir/menu-emerge-done"
menu_emerge_status_path="$shot_dir/menu-emerge-status.json"
menu_emerge_dump_path="$shot_dir/menu-emerge-dump.json"
menu_emerge_rest_dump_path="$shot_dir/menu-emerge-rest-dump.json"
menu_emerge_t0_path="$shot_dir/menu-emerge-t0.txt"
menu_emerge_stamp_path="$shot_dir/menu-emerge-stamps.txt"
menu_emerge_close_t0_path="$shot_dir/menu-emerge-close-t0.txt"
menu_emerge_close_stamp_path="$shot_dir/menu-emerge-close-stamps.txt"

menu_emerge_closed_path="$shot_dir/menu-emerge-closed.png"
menu_emerge_rest_path="$shot_dir/menu-emerge-rest.png"

# Back to back rather than armed at fractions of the clock: grim's own start-up
# costs about as much as the open does at full speed (panel_handoff.sh's
# finding), and ten of them armed at once come back out of order (the unfold
# leg's finding). One sequential loop at a tenth speed lands a frame every fifth
# of a second of a travel that runs for several, which is both a dense enough
# ladder and a stamp per frame that means what it says. The ladder has to
# outlast the spatial curve's own overshoot and its return, not just its
# nominal length: the card reads several pixels past its resting edge for most
# of a second after it has stopped travelling.
menu_emerge_frames=45
# The close, photographed the same way straight after the rest frame.
menu_emerge_close_frames=30
menu_emerge_frame_gap=0.09

# The motion scale the open runs at, as a percent, and the divisor the settle
# figure is reported in real milliseconds through.
menu_emerge_scale=1000

# The card's own geometry (DESIGN.md §1 Space): `popupWidthMenu` 560
# centred on the output, top at 30% of its height, both rounded as the
# launcher rounds them. Neither term moves with the bar's edge or the frame.
# x and y are cut in the assert, off the output's own size.
menu_emerge_card_w=560
# Read off the dump in the assert; the top bar's own last row is what it comes
# to on a default run.
menu_emerge_line_row=39

# A pixel of the line's own band clear of the join: the card is centred and
# its fillets reach a `radiusXl` past either end, so the output's
# own corner is nowhere near it and carries nothing that ticks. Four rows, so
# the probe stays inside a frame ring's band as well as a bar's.
menu_emerge_bar_box="8x4+2+2"

# Ridden by `--bar-style solid` or `corners` the band is black, which reads
# the same dimmed or not, so the two claims that the scrim reached the band
# are left to the desktop half.
menu_emerge_black_band() {
  leg_on bar_style && [ "$(leg_arg bar_style)" != transparent ]
}
# And a desktop one, well below the card, for the other half of the same
# claim: 40 rows off the output's bottom, cut in the assert.

leg_menu_emerge_validate() {
  local other
  for other in center clipboard clipssh clipssh_image emoji fullscreen \
    gallery gpu keybinds lock menu monitor notify picker plugins polkit processes \
    screensaver share toggles tray wallpaper; do
    if leg_on "$other"; then
      echo "usage: --menu-emerge measures the launcher's own frames and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_menu_emerge_timing() {
  leg_timing 48 100
}

leg_menu_emerge_drive() {
  local script="$shot_dir/menu-emerge-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
sleep 4
call debug motionScale $menu_emerge_scale > "$menu_emerge_scale_path" 2>&1
call debug dump > "$menu_emerge_dump_path" 2>&1
sleep 1
# The same output with the launcher closed: what the bar's band and the
# desktop under it read as before any scrim is drawn over either.
"$grim_bin" "$menu_emerge_closed_path" > /dev/null 2>&1
call menu summon "" > "$menu_emerge_summon_path" 2>&1
date +%s%N > "$menu_emerge_t0_path"
for i in \$(seq 1 $menu_emerge_frames); do
  "$grim_bin" "$shot_dir/menu-emerge-\$i.png" > /dev/null 2>&1
  echo "\$i \$(date +%s%N)" >> "$menu_emerge_stamp_path"
  sleep $menu_emerge_frame_gap
done
sleep 5
"$grim_bin" "$menu_emerge_rest_path" > /dev/null 2>&1
call menu status > "$menu_emerge_status_path" 2>&1
call debug dump > "$menu_emerge_rest_dump_path" 2>&1
call menu close > "$menu_emerge_close_path" 2>&1
date +%s%N > "$menu_emerge_close_t0_path"
for i in \$(seq 1 $menu_emerge_close_frames); do
  "$grim_bin" "$shot_dir/menu-emerge-close-\$i.png" > /dev/null 2>&1
  echo "\$i \$(date +%s%N)" >> "$menu_emerge_close_stamp_path"
  sleep $menu_emerge_frame_gap
done
call debug motionScale 100 > /dev/null 2>&1
sleep 1
touch "$menu_emerge_done_path"
EOF
  hypr_exec_once "bash $script"
}

# How bright one box is on average. The card's own fill sits several times
# above the scrim it is drawn over, so this separates "the card has reached
# this row" from "this is still the scrim".
menu_emerge_level() {
  $convert_bin "$1" -crop "$2" +repage -format "%[fx:mean]" info: 2>/dev/null
}

# How far a frame differs from the closed one over a patch, 0 to 1.
menu_emerge_moved() {
  $convert_bin "$menu_emerge_closed_path" "$1" -compose difference -composite -crop "$2" +repage \
    -format "%[fx:mean]" info: 2>/dev/null
}

menu_emerge_covered() {
  awk -v v="$(menu_emerge_level "$1" "$2")" 'BEGIN { exit !(v > 0.06) }'
}

# One pixel of a frame as six hex digits, --join's own read-out.
menu_emerge_pixel() {
  local rgb
  rgb=$($convert_bin "$1" -format \
    "%[fx:int(255*p{$2,$3}.r+0.5)] %[fx:int(255*p{$2,$3}.g+0.5)] %[fx:int(255*p{$2,$3}.b+0.5)]" \
    info: 2>/dev/null)
  [ -n "$rgb" ] || fail "could not read the pixel at ($2,$3) out of $1"
  # shellcheck disable=SC2086  # three fields, one per channel
  printf '%02X%02X%02X' $rgb
}

# What fraction of one crop differs at all between two frames. The share, not
# the bounding box `--join` reads: ImageMagick answers a box of `0x0+W+H` for a
# difference that covers the whole crop as well as for one that covers none of
# it, and the two mean opposite things.
menu_emerge_diff() {
  $convert_bin "$1" "$2" -compose difference -composite -colorspace Gray \
    -crop "$3" +repage -threshold 0 -format '%[fx:mean]' info: 2>/dev/null
}

menu_emerge_same() {
  awk -v v="$(menu_emerge_diff "$1" "$2" "$3")" 'BEGIN { exit !(v == 0) }'
}

leg_menu_emerge_assert() {
  local f i path
  menu_emerge_card_x=$(((out_w - menu_emerge_card_w + 1) / 2))
  menu_emerge_card_y=$(((out_h * 3 + 5) / 10))
  menu_emerge_desk_box="8x8+2+$((out_h - 40))"
  for f in "$menu_emerge_summon_path" "$menu_emerge_close_path"; do
    if ! grep -q '^ok$' "$f" 2>/dev/null; then
      fail "a menu route did not answer ok at $f, got: $(cat "$f" 2>/dev/null)"
    fi
  done
  grep -q '^ok$' "$menu_emerge_scale_path" 2>/dev/null \
    || fail "debug motionScale did not answer ok, got: $(cat "$menu_emerge_scale_path" 2>/dev/null)"

  for f in "$menu_emerge_closed_path" "$menu_emerge_rest_path"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done
  echo "SMOKE_MENU_EMERGE_CLOSED $menu_emerge_closed_path"
  echo "SMOKE_MENU_EMERGE_REST $menu_emerge_rest_path"
  for i in $(seq 1 $menu_emerge_frames); do
    path="$shot_dir/menu-emerge-$i.png"
    [ -f "$path" ] || fail "no screenshot produced at $path"
    echo "SMOKE_MENU_EMERGE_$i $path"
  done

  # --- The settle, printed before anything is asserted ------------------
  #
  # The first frame that is byte-equal to the rest frame below the search row.
  # The field itself is left out of the comparison: its text cursor blinks, so
  # a crop carrying it is never equal to anything twice.
  local t0 settle_ns settle_ms stamp n d trace="" rest_box="${out_w}x$((out_h - 440))+0+400"
  t0=$(cat "$menu_emerge_t0_path" 2>/dev/null)
  [ -n "$t0" ] || fail "no summon stamp at $menu_emerge_t0_path"
  settle_ns=""
  for i in $(seq 1 $menu_emerge_frames); do
    d=$(menu_emerge_diff "$shot_dir/menu-emerge-$i.png" "$menu_emerge_rest_path" "$rest_box")
    trace="$trace $i:$d"
    [ -n "$settle_ns" ] && continue
    awk -v v="$d" 'BEGIN { exit !(v == 0) }' || continue
    stamp=$(awk -v n="$i" '$1 == n { print $2 }' "$menu_emerge_stamp_path" 2>/dev/null)
    [ -n "$stamp" ] || fail "frame $i has no stamp in $menu_emerge_stamp_path"
    settle_ns=$((stamp - t0))
    n=$i
  done
  echo "SMOKE_MENU_EMERGE_SETTLE_TRACE$trace"
  if [ -n "$settle_ns" ]; then
    settle_ms=$(( settle_ns / 1000000 * 100 / menu_emerge_scale ))
    echo "SMOKE_MENU_EMERGE_SETTLE_MS $settle_ms frame=$n scale=$menu_emerge_scale raw_ms=$((settle_ns / 1000000))"
  else
    echo "SMOKE_MENU_EMERGE_SETTLE_MS none no frame among $menu_emerge_frames matched the rest frame in $rest_box"
  fi

  # The root level names no route of its own, so it is the placeholder and
  # the row count that say the launcher is up and holding the root list.
  if [ ! -s "$menu_emerge_status_path" ] \
    || ! grep -q '"placeholder":"Type a command or search..."' "$menu_emerge_status_path" \
    || [ "$("$jq_bin" -r '.rows' "$menu_emerge_status_path" 2>/dev/null)" -lt 1 ]; then
    fail "the launcher did not settle back at the root level, got: $(cat "$menu_emerge_status_path" 2>/dev/null)"
  fi
  cat "$menu_emerge_status_path"; echo

  # --- The scrim under the blur mark ------------------------------------
  #
  # Hyprland's `ignore_alpha` reads a buffer's own alpha, never the
  # wp_alpha_modifier multiplier over it, and this rig runs with blur off, so
  # no frame here can show a scrim being blurred. The numbers can: every scrim
  # pixel of the launcher's namespace at or under the mark the shipped Lua
  # gives that namespace, and at rest each one faded fully in, the bar's band
  # included.
  local mark scrims
  mark=$(awk '/namespace = "formalshell:menu"/ { on = 1 } on && /ignore_alpha/ { gsub(/[^0-9.]/, "", $3); print $3; exit }' \
    docs/examples/hyprland/formalshell.lua)
  [ -n "$mark" ] || fail "no ignore_alpha for formalshell:menu in docs/examples/hyprland/formalshell.lua"
  scrims=$("$jq_bin" -c '[.modals[] | select(.namespace == "formalshell:menu") | .scrims[]]' "$menu_emerge_rest_dump_path" 2>/dev/null)
  [ -n "$scrims" ] && [ "$scrims" != "[]" ] || fail \
    "no formalshell:menu scrim in the dump taken at rest: $(head -c 400 "$menu_emerge_rest_dump_path" 2>/dev/null)"
  "$jq_bin" -e --argjson mark "$mark" 'all(.[]; .alpha > 0 and .alpha <= $mark)' <<< "$scrims" > /dev/null || fail \
    "a scrim pixel is over the namespace's ignore_alpha $mark, so Hyprland blurs the whole output behind it: $scrims"
  "$jq_bin" -e 'all(.[]; .fade > 0.999)' <<< "$scrims" > /dev/null || fail \
    "a scrim is not faded fully in at rest, the bar's band included: $scrims"
  echo "SMOKE_MENU_EMERGE_SCRIM ok under ignore_alpha $mark: $scrims"

  # --- The dim at rest --------------------------------------------------
  #
  # Black at the scrim's alpha over anything leaves it at 1 - alpha of what
  # it was, so the bar's band and the desktop each read that fraction of
  # their closed level in the rest frame: not untouched (a band that never
  # faded in, which a theme whose card never hangs off the line once left
  # the bar at) and not black.
  local tone want b_closed b_rest d_closed d_rest pair who box c r
  tone=$("$jq_bin" -r 'map(.alpha) | max' <<< "$scrims")
  want=$(awk -v a="$tone" 'BEGIN { print 1 - a }')
  b_closed=$(menu_emerge_level "$menu_emerge_closed_path" "$menu_emerge_bar_box")
  b_rest=$(menu_emerge_level "$menu_emerge_rest_path" "$menu_emerge_bar_box")
  d_closed=$(menu_emerge_level "$menu_emerge_closed_path" "$menu_emerge_desk_box")
  d_rest=$(menu_emerge_level "$menu_emerge_rest_path" "$menu_emerge_desk_box")
  local pairs=("bar $menu_emerge_bar_box $b_closed $b_rest" "desktop $menu_emerge_desk_box $d_closed $d_rest")
  if menu_emerge_black_band; then
    echo "SMOKE_MENU_EMERGE_DIM_BAR skipped: a black band reads the same dimmed"
    pairs=("${pairs[1]}")
  fi
  for pair in "${pairs[@]}"; do
    read -r who box c r <<< "$pair"
    awk -v c="$c" 'BEGIN { exit !(c > 0.03) }' || fail \
      "the $who at $box reads $c with the launcher closed, too dark to tell a dim from black"
    awk -v c="$c" -v r="$r" -v w="$want" 'BEGIN { q = r / c; exit !(q > w - 0.12 && q < w + 0.12) }' || fail \
      "the $who at $box reads $r at rest against $c closed, not $want of it: the scrim over it is missing or wrong"
  done
  echo "SMOKE_MENU_EMERGE_DIM ok bar $b_rest of $b_closed, desktop $d_rest of $d_closed, want $want"

  # --- Content on the card ----------------------------------------------
  #
  # Every frame of the open, how far the header's placeholder has come in
  # against how far the card's own fill has, each as a share of its change
  # at rest: the content may never lead the card under it. The fill is read
  # in the header's own left padding, beside the text, where nothing is
  # drawn on the card.
  local card cx cy ch fill_box text_box fill_rest text_rest ladder="" lead=""
  card=$("$jq_bin" -r '[.modals[] | select(.namespace == "formalshell:menu") | .card] | first | "\(.x) \(.y) \(.width) \(.height)"' \
    "$menu_emerge_rest_dump_path" 2>/dev/null)
  read -r cx cy cw ch <<< "$card"
  [ -n "$ch" ] && [ "$ch" != null ] || fail "no launcher card rect in the dump at rest: $card"
  fill_box="6x4+$((cx + 6))+$((cy + 24))"
  text_box="160x16+$((cx + 24))+$((cy + 20))"
  fill_rest=$(menu_emerge_moved "$menu_emerge_rest_path" "$fill_box")
  text_rest=$(menu_emerge_moved "$menu_emerge_rest_path" "$text_box")
  awk -v f="$fill_rest" -v t="$text_rest" 'BEGIN { exit !(f > 0.005 && t > 0.005) }' || fail \
    "the card fill ($fill_rest at $fill_box) or the header text ($text_rest at $text_box) does not differ from the closed frame at rest: the probes read nothing"
  for i in $(seq 1 $menu_emerge_frames); do
    path="$shot_dir/menu-emerge-$i.png"
    local f t
    f=$(menu_emerge_moved "$path" "$fill_box")
    t=$(menu_emerge_moved "$path" "$text_box")
    local pair
    pair=$(awk -v f="$f" -v t="$t" -v fr="$fill_rest" -v tr="$text_rest" 'BEGIN { printf "%.2f/%.2f", t / tr, f / fr }')
    ladder="$ladder $i:$pair"
    awk -v f="$f" -v t="$t" -v fr="$fill_rest" -v tr="$text_rest" 'BEGIN { exit !(t / tr > f / fr + 0.2) }' && lead="$lead $i"
  done
  echo "SMOKE_MENU_EMERGE_CONTENT text/fill$ladder"
  [ -z "$lead" ] || fail "the header's text came in ahead of the card fill under it in frames$lead"
  echo "SMOKE_MENU_EMERGE_CONTENT ok the fill is never behind the text"

  # --- The fade, open and close -----------------------------------------
  #
  # How much of the card each frame shows, read as its mean difference from
  # the closed frame over the card's resting box against the rest frame's,
  # and the scrim's share the same way off the desktop probe. Across the
  # open both rise through at least one frame strictly between 0.15 and
  # 0.85, and across the close both fall through one: a card that travels
  # at full opacity and is then cut has no such frame, and the pair of
  # neighbours where it goes from 0.9 or over to 0.1 or under is named.
  local card_box="$((cw - 16))x$((ch - 16))+$((cx + 8))+$((cy + 8))" card_rest scrim_rest
  card_rest=$(menu_emerge_moved "$menu_emerge_rest_path" "$card_box")
  scrim_rest=$(awk -v c="$d_closed" -v r="$d_rest" 'BEGIN { print c - r }')
  awk -v v="$card_rest" 'BEGIN { exit !(v > 0.01) }' || fail \
    "the rest frame's card box $card_box reads $card_rest off the closed frame: the probe reads nothing"
  local phase frames prefix series who_series v
  for phase in open close; do
    if [ "$phase" = open ]; then frames=$menu_emerge_frames prefix=menu-emerge; else frames=$menu_emerge_close_frames prefix=menu-emerge-close; fi
    local card_series="" scrim_series=""
    for i in $(seq 1 "$frames"); do
      path="$shot_dir/$prefix-$i.png"
      [ -f "$path" ] || fail "no screenshot produced at $path"
      [ "$phase" = close ] && echo "SMOKE_MENU_EMERGE_CLOSE_$i $path"
      v=$(menu_emerge_moved "$path" "$card_box")
      card_series="$card_series $(awk -v v="$v" -v r="$card_rest" 'BEGIN { printf "%.2f", v / r }')"
      v=$(menu_emerge_level "$path" "$menu_emerge_desk_box")
      scrim_series="$scrim_series $(awk -v c="$d_closed" -v v="$v" -v r="$scrim_rest" 'BEGIN { printf "%.2f", (c - v) / r }')"
    done
    for who in card scrim; do
      if [ "$who" = card ]; then who_series=$card_series; else who_series=$scrim_series; fi
      echo "SMOKE_MENU_EMERGE_FADE_${phase^^}_${who^^}$who_series"
      local verdict
      verdict=$(awk -v phase="$phase" -v s="$who_series" 'BEGIN {
        n = split(s, a, " "); mid = 0; cut = ""; back = ""; peak = 0; low = 2
        for (i = 1; i <= n; i++) {
          if (a[i] > 0.15 && a[i] < 0.85) mid = 1
          if (i > 1 && phase == "close" && a[i - 1] >= 0.9 && a[i] <= 0.1) cut = cut " " i - 1 "->" i
          if (i > 1 && phase == "open" && a[i - 1] <= 0.1 && a[i] >= 0.9) cut = cut " " i - 1 "->" i
          # Against the furthest reached so far: a fade that turns back.
          if (phase == "open" && a[i] < peak - 0.1 && peak < 0.95) back = back " " i
          if (phase == "close" && a[i] > low + 0.1 && low > 0.05) back = back " " i
          if (a[i] > peak) peak = a[i]
          if (a[i] < low) low = a[i]
        }
        if (!mid) { print "no frame strictly between 0.15 and 0.85"; exit }
        if (cut != "") { print "it jumps between neighbouring frames" cut; exit }
        if (back != "") { print "it turns back at frames" back; exit }
        print "ok"
      }')
      [ "$verdict" = ok ] || fail "the $who's $phase does not fade ($verdict):$who_series"
    done
  done
  echo "SMOKE_MENU_EMERGE_FADE ok card and scrim fade in across the open and out across the close"

  # --- Where the card rests ---------------------------------------------
  #
  # Its bottom is measured rather than stated: the row count is the rig's
  # own, and only the three numbers the tokens fix are spelled out above.
  local column_x=$((menu_emerge_card_x + menu_emerge_card_w / 2 - 2))
  local rest_bottom=0 row
  for ((row = menu_emerge_card_y + 40; row < out_h; row += 4)); do
    if menu_emerge_covered "$menu_emerge_rest_path" "5x4+${column_x}+${row}"; then
      rest_bottom=$row
    elif [ "$rest_bottom" -gt 0 ]; then
      break
    fi
  done
  [ "$rest_bottom" -gt $((menu_emerge_card_y + 200)) ] || fail \
    "the rest frame's card reaches only row $rest_bottom down its own centre column: it is not the card the tokens describe"
  echo "SMOKE_MENU_EMERGE_RESTING card ${menu_emerge_card_w}x$((rest_bottom - menu_emerge_card_y))+${menu_emerge_card_x}+${menu_emerge_card_y}"

  # Where the line the card buds off actually is: the last row of whatever
  # band the top edge carries, a bar's or a frame ring's, off the shell's own
  # numbers. Nothing there is nothing to bud off, and the claims below have
  # no subject.
  local inset
  inset=$("$jq_bin" -r '.theme.edgeInset.top | floor' "$menu_emerge_dump_path" 2>/dev/null)
  [ -n "$inset" ] && [ "$inset" != "null" ] || fail \
    "no top inset in the dump at $menu_emerge_dump_path: $(head -c 400 "$menu_emerge_dump_path" 2>/dev/null)"
  menu_emerge_line_row=$((inset - 1))
  echo "SMOKE_MENU_EMERGE_LINE row $menu_emerge_line_row (top inset $inset)"
  if [ "$inset" -le 0 ]; then
    for f in BUD LIT_BAR PLAIN; do
      echo "SMOKE_MENU_EMERGE_$f skipped (layout): this output's top edge carries no bar and no frame ring, so there is no line to bud off"
    done
    return 0
  fi
  # A popover theme's card drops in under the line rather than budding off
  # it, so the line has no bud, no lit band and no let-go to read; the dim
  # above already read the band at rest.
  if "$jq_bin" -e '[.modals[] | select(.namespace == "formalshell:menu") | .popover] | any' \
    "$menu_emerge_rest_dump_path" > /dev/null 2>&1; then
    for f in BUD LIT_BAR PLAIN; do
      echo "SMOKE_MENU_EMERGE_$f skipped (popover): this theme's card drops in as a popover and never buds off the line"
    done
    return 0
  fi

  # The two probes the travel is read with, and the guard that says they
  # measure something: neither may read as covered with the launcher closed.
  local neck_box="5x4+${column_x}+$((menu_emerge_line_row + 2))"
  local floor_box="5x4+${column_x}+$((rest_bottom - 12))"
  for f in "$neck_box" "$floor_box"; do
    if menu_emerge_covered "$menu_emerge_closed_path" "$f"; then
      fail "probe $f already reads as covered with the launcher closed, so it measures nothing"
    fi
  done
  menu_emerge_covered "$menu_emerge_rest_path" "$floor_box" || fail \
    "the settled card does not cover $floor_box, so the floor probe is not where the measurement thinks"

  # --- Out of the line --------------------------------------------------
  local attached="" last_attached="" ladder=""
  for i in $(seq 1 $menu_emerge_frames); do
    path="$shot_dir/menu-emerge-$i.png"
    if menu_emerge_covered "$path" "$neck_box"; then
      if menu_emerge_covered "$path" "$floor_box"; then
        ladder="$ladder $i:both"
      else
        ladder="$ladder $i:neck"
        [ -n "$attached" ] || attached="$path"
        last_attached="$path"
      fi
    else
      ladder="$ladder $i:-"
    fi
  done
  echo "SMOKE_MENU_EMERGE_LADDER$ladder"
  [ -n "$attached" ] || fail \
    "no frame among menu-emerge-1..$menu_emerge_frames carries the card's fill at $neck_box with nothing at $floor_box: the card never came out of the line"
  echo "SMOKE_MENU_EMERGE_BUD ok $attached"

  # --- The lit bar ------------------------------------------------------
  #
  # Read off the LAST frame the card is still on the line for, not the first:
  # the scrim rides the same pose the travel does, so on the first frame of an
  # open there is barely any of it on the output yet and "the desktop is
  # dimmed" says nothing. By the last attached frame the pose is well along
  # and the two halves of the claim are both worth making.
  local bar_open bar_closed desk_open desk_closed
  bar_open=$(menu_emerge_level "$last_attached" "$menu_emerge_bar_box")
  bar_closed=$(menu_emerge_level "$menu_emerge_closed_path" "$menu_emerge_bar_box")
  desk_open=$(menu_emerge_level "$last_attached" "$menu_emerge_desk_box")
  desk_closed=$(menu_emerge_level "$menu_emerge_closed_path" "$menu_emerge_desk_box")
  menu_emerge_same "$last_attached" "$menu_emerge_closed_path" "$menu_emerge_bar_box" || fail \
    "the bar's own band at $menu_emerge_bar_box reads $bar_open in $last_attached against $bar_closed closed: the scrim dims the line the card is still budding off"
  awk -v o="$desk_open" -v c="$desk_closed" 'BEGIN { exit !(o < c * 0.9) }' || fail \
    "the desktop at $menu_emerge_desk_box reads $desk_open in $last_attached against $desk_closed closed: the scrim is not on the output at all while the card is attached"
  echo "SMOKE_MENU_EMERGE_LIT_BAR ok bar $bar_open = $bar_closed, desktop $desk_open under $desk_closed in $last_attached"

  # --- The plain card ---------------------------------------------------
  #
  # A row of desktop-through-scrim between the line and the card says the
  # near edge let go of the line, and a border pixel differing from both the
  # scrim above it and the fill below says the card closed its own top back
  # up rather than staying open onto the gap.
  menu_emerge_covered "$menu_emerge_rest_path" "$neck_box" && fail \
    "the rest frame still carries the card's fill at $neck_box: the card never let go of the line"
  local above border fill
  above=$(menu_emerge_pixel "$menu_emerge_rest_path" "$column_x" $((menu_emerge_card_y - 4)))
  border=$(menu_emerge_pixel "$menu_emerge_rest_path" "$column_x" "$menu_emerge_card_y")
  fill=$(menu_emerge_pixel "$menu_emerge_rest_path" "$column_x" $((menu_emerge_card_y + 6)))
  if [ "$border" = "$above" ] || [ "$border" = "$fill" ]; then
    fail "the rest frame reads $above above, $border on and $fill under row $menu_emerge_card_y at column $column_x: the card has no top border of its own"
  fi
  menu_emerge_black_band || ! menu_emerge_same "$menu_emerge_rest_path" "$menu_emerge_closed_path" "$menu_emerge_bar_box" || fail \
    "the bar's own band at $menu_emerge_bar_box is unchanged in the rest frame: the scrim never reached it after the card let go"
  if menu_emerge_black_band; then
    echo "SMOKE_MENU_EMERGE_PLAIN ok border $border between $above and $fill, black band not read"
  else
    echo "SMOKE_MENU_EMERGE_PLAIN ok border $border between $above and $fill, bar band dimmed"
  fi
}
