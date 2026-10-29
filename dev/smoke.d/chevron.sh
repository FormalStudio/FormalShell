# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --chevron points bar.layout at today's exact default right region reordered
# around one `chevron` name: the five governed cells lead the region, then the
# chevron, then battery/audio/network sit outboard against the screen edge. A
# right-region chevron governs what PRECEDES it, so those five are never on
# the strip at all (M52): they are drawn in the second bar the cell opens, the
# way the tray lives behind its own dots, and everything outboard of the
# chevron keeps its x whether that bar is up or not.
#
# `bar chevron expand` stands in for a click on the cell (no synthetic pointer
# here), deliberately with no region argument, since a single-chevron layout
# is meant to infer it. The two PNGs are asserted to differ: M24 shipped a
# correct IPC contract over a bar that rendered nothing, and these two frames
# were byte-identical the whole time with every dump passing.
#
# It also carries the ink claim (M66): the band reads its paint off the
# wallpaper, so the leg sets a flat bright field first and the strip goes
# to dark ink over it. The card the chevron opens is not the
# band, and its cells have to say so, which is read off one patch of the
# frame inside the card: a dark plate with light words on it, not the band's
# black ink drawn onto it.
#
# Last, the second bar under a real pointer behaving like a panel (owner,
# 2026-10-09: it ignored a click outside, and a click on another cell opened
# that cell's panel beside it): the chevron clicked open, a click on the
# desktop shutting it, then open again and a click on the clock shutting it
# and opening the calendar in its place.
#
# Then the card's own size (owner, 2026-10-09: "the chevron panel doesnt
# resize"). The group carries one more cell, a `command` module reading a
# file, which answers nothing at first and so is no cell at all: the card
# opens exactly as it did without it, and the click above lands where it
# always did. With the weather panel still hanging off the card, the file
# gets a long label, so a cell appears inside the open card; then, panel
# shut, it gets a short one under `debug motionScale 1000`. Each settled
# `bar chevron status` has to show the card around its cells, the same
# padding before the first as after the last, and the samples taken through
# the shrink have to catch the card at a width between its two rests: it
# travels to its new size the way a panel card does, rather than keeping the
# size it opened at or jumping.
leg_chevron_flag="--chevron"
leg_chevron_order=180
leg_chevron_needs="wlrctl convert jq"

chevron_status_shut_path="$shot_dir/chevron-status-shut.json"
chevron_status_open_path="$shot_dir/chevron-status-open.json"
chevron_status_closed_again_path="$shot_dir/chevron-status-closed-again.json"
chevron_expand_reply_path="$shot_dir/chevron-expand-reply.txt"
chevron_shut_path="$shot_dir/chevron-shut.png"
chevron_open_path="$shot_dir/chevron-open.png"
chevron_child_path="$shot_dir/chevron-child.png"
chevron_status_child_path="$shot_dir/chevron-status-child.json"
chevron_panel_state_path="$shot_dir/chevron-panel-state.txt"
chevron_dispatch_path="$shot_dir/chevron-pointer.txt"
chevron_bright_wp="$shot_dir/chevron-bright.png"
chevron_paint_path="$shot_dir/chevron-paint.json"
chevron_card_ink_path="$shot_dir/chevron-card-ink.png"
chevron_grow_cmd_path="$shot_dir/chevron-grow.sh"
chevron_grow_text_path="$shot_dir/chevron-grow.txt"
chevron_status_grown_child_path="$shot_dir/chevron-status-grown-child.json"
chevron_grown_child_path="$shot_dir/chevron-grown-child.png"
chevron_status_grown_path="$shot_dir/chevron-status-grown.json"
chevron_grown_path="$shot_dir/chevron-grown.png"
chevron_shrink_samples_path="$shot_dir/chevron-shrink-samples.jsonl"
chevron_status_shrunk_path="$shot_dir/chevron-status-shrunk.json"
chevron_shrunk_path="$shot_dir/chevron-shrunk.png"
chevron_room_path="$shot_dir/chevron-room.json"
chevron_clicks_path="$shot_dir/chevron-clicks.txt"
chevron_outside_path="$shot_dir/chevron-outside.png"
chevron_handoff_path="$shot_dir/chevron-handoff.png"

# The weather cell's centre in the open card, off `bar chevron status`'s own
# cell rects: which cells the group shows (and so where weather lands) is the
# host's, a tray or an indicator being there or not.
chevron_weather_centre='.regions.right.card.cells[] | select(.name == "weather") | .rect | "\(.x + (.w / 2 | floor)) \(.y + (.h / 2 | floor))"'

leg_chevron_fixture() {
  : > "$chevron_grow_text_path"
  write_script "$chevron_grow_cmd_path" <<EOF
#!/usr/bin/env bash
printf '{"text": "%s"}' "\$(cat "$chevron_grow_text_path")"
EOF
  settings_fragment ', "bar": {"layout": {"right": ["bluetooth", "weather", "tray", "bell", "indicators", "custom:chevgrow", "chevron", "battery", "audio", "network"]}, "modules": [{"id": "chevgrow", "type": "command", "command": ["bash", "'"$chevron_grow_cmd_path"'"], "interval": 500}]}'
  # The wallpaper the band's dark ink comes from, the flat bright field
  # --bar-adaptive reads `dark` off: mean 230, no spread, nothing busy.
  $convert_bin -size 1920x1080 xc:'#e6e6e6' "$chevron_bright_wp"
}

leg_chevron_timing() {
  # chevron-drive.sh's own last step lands ~19s in; the run's own frame is
  # taken past that, so it shows the bar left with the group's card shut.
  # The wallpaper set and the retheme behind it push every step of that ~9s
  # later.
  leg_timing 62 92
}

leg_chevron_drive() {
  local script="$shot_dir/chevron-drive.sh"
  # The open shot sits two seconds behind the expand call, an order of
  # magnitude past the card's own entrance, which is what makes it a picture
  # of the end state rather than of a frame somewhere inside it.
  #
  # Then a real pointer click on a cell INSIDE that card, which is the half
  # no IPC verb can stand in for: `panel open weather` would open the same
  # panel from nowhere, and what is being proved is that a cell living in a
  # popout opens its own panel on top of that popout rather than in place of
  # it. wlrctl's pointer is relative-only, hence the slam into the corner
  # before the move, the same trick --wheel documents. The point is the
  # weather cell's centre as the open status reports it. A miss fails the
  # panel assert loudly rather than passing quietly.
  #
  # The bright wallpaper and the shell's own reading of the band over it.
  local paint_step="$ipc call wallpaper set \"$chevron_bright_wp\" > /dev/null 2>&1
sleep 8
$ipc call bar paint > \"$chevron_paint_path\" 2>&1"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 5
$paint_step
$ipc call bar chevron status > "$chevron_status_shut_path" 2>&1
sleep 1
"$grim_bin" "$chevron_shut_path" > /dev/null 2>&1
sleep 1
$ipc call bar chevron expand > "$chevron_expand_reply_path" 2>&1
sleep 2
$ipc call bar chevron status > "$chevron_status_open_path" 2>&1
sleep 1
"$grim_bin" "$chevron_open_path" > /dev/null 2>&1
sleep 1
"$wlrctl_bin" pointer move -4000 -4000 > "$chevron_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer move \$("$jq_bin" -r '$chevron_weather_centre' "$chevron_status_open_path") >> "$chevron_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer click left >> "$chevron_dispatch_path" 2>&1
sleep 3
$ipc call panel state > "$chevron_panel_state_path" 2>&1
$ipc call bar chevron status > "$chevron_status_child_path" 2>&1
"$grim_bin" "$chevron_child_path" > /dev/null 2>&1
printf 'a label far wider than any cell' > "$chevron_grow_text_path"
sleep 3
$ipc call bar chevron status > "$chevron_status_grown_child_path" 2>&1
"$grim_bin" "$chevron_grown_child_path" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
sleep 2
$ipc call bar chevron status > "$chevron_status_grown_path" 2>&1
"$grim_bin" "$chevron_grown_path" > /dev/null 2>&1
$ipc call debug motionScale 1000 > /dev/null 2>&1
printf 'ab' > "$chevron_grow_text_path"
for _ in \$(seq 40); do
  $ipc call bar chevron status >> "$chevron_shrink_samples_path" 2>&1
  sleep 0.1
done
$ipc call debug motionScale 100 > /dev/null 2>&1
sleep 2
$ipc call bar chevron status > "$chevron_status_shrunk_path" 2>&1
"$grim_bin" "$chevron_shrunk_path" > /dev/null 2>&1
$ipc call bar chevron collapse > /dev/null 2>&1
sleep 2
$ipc call bar chevron status > "$chevron_status_closed_again_path" 2>&1
$ipc call bar room > "$chevron_room_path" 2>&1
cell() { "$jq_bin" -r --arg n "\$1" '.[0].cells[] | select(.name == \$n) | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"' "$chevron_room_path" | head -n 1; }
to() { "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1; "$wlrctl_bin" pointer move "\$1" "\$2" > /dev/null 2>&1; }
click() { "$wlrctl_bin" pointer click left > /dev/null 2>&1; }
check() { echo "\$1 open=\$($ipc call bar chevron status 2>/dev/null | "$jq_bin" -c '.regions.right.open') panel=\$($ipc call panel state 2>/dev/null)" >> "$chevron_clicks_path"; }
to \$(cell chevron); click; sleep 1.5; check clicked-open
to 900 700; click; sleep 1.5; check outside
"$grim_bin" "$chevron_outside_path" > /dev/null 2>&1
to \$(cell chevron); click; sleep 1.5; check reopened
to \$(cell clock); click; sleep 1.5; check handoff
"$grim_bin" "$chevron_handoff_path" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# One statistic of one patch of a frame, 0..255. Read off the red channel
# rather than through a grayscale conversion, the reason --bar-adaptive
# gives: every colour here is grey and a colorspace conversion answers in
# numbers these expectations are not written in.
_chevron_patch() {
  local png="$1" patch="$2" stat="$3"
  $convert_bin "$png" -crop "$patch" +repage \
    -format "%[fx:int($stat.r*255+0.5)]" info: 2>/dev/null
}

# The ink claim (M66). Three parts: the band settled
# on the dark paint, so its cells really are carrying black ink; the patch
# inside the card is on the card, not on the bright wallpaper around it; and
# the words drawn on that card are light. The middle one is what makes the
# last one mean anything, since a patch that slid off the card would read the
# wallpaper's own 230 and pass on nothing.
_chevron_assert_ink() {
  local paint source screen mean max
  [ -s "$chevron_paint_path" ] || fail "no bar paint reply produced at $chevron_paint_path"
  paint=$("$jq_bin" -r '.[0].paint.paint // "none"' "$chevron_paint_path" 2>/dev/null)
  source=$("$jq_bin" -r '.[0].paint.source // ""' "$chevron_paint_path" 2>/dev/null)
  screen=$("$jq_bin" -r '.[0].screen // ""' "$chevron_paint_path" 2>/dev/null)
  echo "chevron ink: band paint=$paint read on '$source' (this output is '$screen')"
  if [ "$paint" != "dark" ]; then
    fail "the band over the bright wallpaper reports '$paint', not 'dark', so its cells are not carrying black ink: $(cat "$chevron_paint_path")"
  fi
  # One reading for the whole session, taken on the main display: this rig
  # has one output, so the band reports itself as its own source.
  if [ "$source" != "$screen" ] || [ -z "$source" ]; then
    fail "bar paint reports the reading from '$source' on an output called '$screen': $(cat "$chevron_paint_path")"
  fi

  [ -f "$chevron_open_path" ] || fail "no chevron-open screenshot to read the card's ink off"
  # One patch of the open frame: 30px either side of the weather cell's
  # centre, the point the pointer above is already asserted to land a panel
  # from, and 10px either side of it across, clear of the card's own lit top
  # edge. Two readings off it, since either alone passes for the wrong
  # reason: the mean says the patch is on the card rather than on the
  # wallpaper beside it, and the maximum says there are light words on it.
  local centre chevron_card_patch
  centre=$("$jq_bin" -r "$chevron_weather_centre" "$chevron_status_open_path" 2>/dev/null)
  [ -n "$centre" ] || fail "the open status carries no weather cell to read the card's ink at: $(cat "$chevron_status_open_path")"
  # shellcheck disable=SC2086
  set -- $centre
  chevron_card_patch="60x20+$(( $1 - 30 ))+$(( $2 - 10 ))"
  $convert_bin "$chevron_open_path" -crop "$chevron_card_patch" +repage \
    "$chevron_card_ink_path" > /dev/null 2>&1
  [ -f "$chevron_card_ink_path" ] && echo "SMOKE_CHEVRON_CARD_INK $chevron_card_ink_path"
  mean=$(_chevron_patch "$chevron_open_path" "$chevron_card_patch" mean)
  max=$(_chevron_patch "$chevron_open_path" "$chevron_card_patch" maxima)
  [ -n "$mean" ] && [ -n "$max" ] \
    || fail "could not read the patch $chevron_card_patch out of $chevron_open_path"
  echo "chevron ink: the card patch $chevron_card_patch reads mean=$mean max=$max"
  if [ "$mean" -gt 120 ]; then
    fail "the patch $chevron_card_patch reads mean $mean, too bright to be the card: it is on the wallpaper beside it, so the ink below is untested"
  fi
  if [ "$max" -lt 150 ]; then
    fail "the card's brightest pixel over $chevron_card_patch is $max: the second bar is drawing the band's black ink on its own dark plate"
  fi
}

# A settled status's card around its cells: every shown cell inside the
# card's resting rect, and as much card before the first as after the last.
# A card left at the size it opened at fails one or the other.
_chevron_assert_fit() {
  local json="$1" what="$2" fit
  [ -s "$json" ] || fail "no bar chevron status ($what) produced"
  fit=$("$jq_bin" -r '.regions.right.card as $c | ($c.cells // [] | map(.rect)) as $r
    | if ($c == null or ($r | length) == 0) then "none"
      else [($r[0].x - $c.rest.x), (($c.rest.x + $c.rest.w) - ($r[-1].x + $r[-1].w)),
            ([$r[] | select(.x < $c.rest.x or .x + .w > $c.rest.x + $c.rest.w)] | length),
            $c.rest.w] | map(tostring) | join(" ") end' "$json" 2>/dev/null)
  echo "chevron fit ($what): lead trail outside width = $fit"
  # shellcheck disable=SC2086
  set -- $fit
  [ "$#" -eq 4 ] || fail "bar chevron status ($what) carries no card with cells in it: $(cat "$json")"
  if [ "$3" -ne 0 ] || [ $(( $1 - $2 )) -gt 1 ] || [ $(( $2 - $1 )) -gt 1 ]; then
    fail "the second bar's card is not sized to its cells ($what): $1px before the first cell, $2px after the last, $3 cells outside it: $(cat "$json")"
  fi
}

_chevron_card_w() {
  "$jq_bin" -r '.regions.right.card.rest.w // 0' "$1" 2>/dev/null
}

_chevron_assert_resize() {
  local open_w grown_w shrunk_w lo hi between
  echo "SMOKE_CHEVRON_GROWN_CHILD $chevron_grown_child_path"
  echo "SMOKE_CHEVRON_GROWN $chevron_grown_path"
  echo "SMOKE_CHEVRON_SHRUNK $chevron_shrunk_path"
  _chevron_assert_fit "$chevron_status_open_path" "open"
  _chevron_assert_fit "$chevron_status_grown_child_path" "grown, weather panel open"
  _chevron_assert_fit "$chevron_status_grown_path" "grown"
  _chevron_assert_fit "$chevron_status_shrunk_path" "shrunk"
  if ! "$jq_bin" -e '.regions.right.card.cells | map(.name) | index("custom:chevgrow")' "$chevron_status_grown_path" > /dev/null 2>&1; then
    fail "the command cell given a label never showed in the open card: $(cat "$chevron_status_grown_path")"
  fi
  open_w=$(_chevron_card_w "$chevron_status_open_path")
  grown_w=$(_chevron_card_w "$chevron_status_grown_path")
  shrunk_w=$(_chevron_card_w "$chevron_status_shrunk_path")
  echo "chevron resize: card width open=$open_w grown=$grown_w shrunk=$shrunk_w"
  if [ "$grown_w" -le "$shrunk_w" ] || [ "$shrunk_w" -le "$open_w" ]; then
    fail "the card did not follow its cells: $open_w wide on open, $grown_w with the long label, $shrunk_w with the short one"
  fi
  lo=$(( shrunk_w + 4 ))
  hi=$(( grown_w - 4 ))
  between=$("$jq_bin" -r --argjson lo "$lo" --argjson hi "$hi" \
    'select(.regions.right.card != null) | .regions.right.card.live.w | select(. >= $lo and . <= $hi)' \
    "$chevron_shrink_samples_path" 2>/dev/null | head -1)
  if [ -z "$between" ]; then
    fail "no sample through the shrink caught the card between $grown_w and $shrunk_w wide, so it jumped rather than travelled: $("$jq_bin" -c '.regions.right.card.live.w' "$chevron_shrink_samples_path" 2>/dev/null | tr '\n' ' ')"
  fi
  echo "chevron resize: one sample caught the card $between wide on its way"
}

_chevron_click_expect() {
  local name="$1" want="$2" what="$3" line
  line=$(grep "^$name " "$chevron_clicks_path" 2>/dev/null | head -n 1)
  case "$line" in
    *"$want"*) echo "SMOKE_CHEVRON $what" ;;
    *) fail "$what: $name wanted '$want', got '${line:-nothing}'" ;;
  esac
}

_chevron_assert_clicks() {
  echo "SMOKE_CHEVRON_OUTSIDE $chevron_outside_path"
  echo "SMOKE_CHEVRON_HANDOFF $chevron_handoff_path"
  cat "$chevron_clicks_path" 2>/dev/null
  _chevron_click_expect clicked-open 'open=true panel=' "a real click on the chevron opens its second bar"
  _chevron_click_expect outside 'open=false panel=' "a click on the desktop shuts it"
  _chevron_click_expect reopened 'open=true panel=' "the chevron opens it again"
  _chevron_click_expect handoff 'open=false panel=calendar' "a click on the clock shuts it and opens the calendar"
}

leg_chevron_assert() {
  # The five names the fixture puts before the chevron. Order matters:
  # `collapses` reports them in layout order, so one grep asserts the whole
  # boundary rather than five independent membership checks.
  local chevron_hidden_names='"bluetooth","weather","tray","bell","indicators","custom:chevgrow"'
  if [ ! -s "$chevron_status_shut_path" ]; then
    fail "no bar chevron status (shut) produced"
  fi
  cat "$chevron_status_shut_path"; echo
  # Named before the claims below, so a run that fails one still hands the
  # frames back to look at.
  if [ -f "$chevron_shut_path" ]; then
    echo "SMOKE_CHEVRON_SHUT $chevron_shut_path"
  fi
  if [ -f "$chevron_open_path" ]; then
    echo "SMOKE_CHEVRON_OPEN $chevron_open_path"
  fi
  if ! grep -q '"chevronRegions":\["right"\]' "$chevron_status_shut_path"; then
    fail "bar chevron status did not resolve exactly one chevron, in the right region. Got: $(cat "$chevron_status_shut_path")"
  fi
  # The group and the surface it lives in, in one line: those exact five
  # names, in layout order, and no bar up yet.
  if ! grep -q "\"collapses\":\[$chevron_hidden_names\],\"open\":false" "$chevron_status_shut_path"; then
    fail "bar chevron status does not govern the five names placed before it, with its bar shut. Got: $(cat "$chevron_status_shut_path")"
  fi
  if [ ! -f "$chevron_shut_path" ]; then
    fail "no chevron-shut screenshot produced"
  fi
  if ! grep -q '^ok$' "$chevron_expand_reply_path" 2>/dev/null; then
    fail "bar chevron expand was refused. Got: $(cat "$chevron_expand_reply_path" 2>/dev/null)"
  fi
  if [ ! -s "$chevron_status_open_path" ]; then
    fail "no bar chevron status (open) produced"
  fi
  cat "$chevron_status_open_path"; echo
  # Same five names, still governed, now with their bar up. An empty
  # `collapses` here would mean the layout changed under the run rather than
  # the surface.
  if ! grep -q "\"collapses\":\[$chevron_hidden_names\],\"open\":true" "$chevron_status_open_path"; then
    fail "bar chevron expand did not open the group's own bar while keeping the same governed names. Got: $(cat "$chevron_status_open_path")"
  fi
  if [ ! -f "$chevron_open_path" ]; then
    fail "no chevron-open screenshot produced"
  fi
  if cmp -s "$chevron_shut_path" "$chevron_open_path"; then
    fail "chevron-shut and chevron-open screenshots are byte-identical: the second bar opened and rendered nothing"
  fi
  # The click's own claim, in three parts: the panel opened, the card it was
  # clicked in is still up, and the frame shows both. A popout that replaced
  # the card would leave `panel state` right and `open` false.
  if ! grep -q '^weather$' "$chevron_panel_state_path" 2>/dev/null; then
    fail "clicking the card's middle cell did not open its panel. Got: $(cat "$chevron_panel_state_path" 2>/dev/null), pointer: $(cat "$chevron_dispatch_path" 2>/dev/null)"
  fi
  if ! grep -q '"open":true' "$chevron_status_child_path" 2>/dev/null; then
    fail "opening a panel from the card closed the card. Got: $(cat "$chevron_status_child_path" 2>/dev/null)"
  fi
  if [ ! -f "$chevron_child_path" ]; then
    fail "no chevron-child screenshot produced"
  fi
  echo "SMOKE_CHEVRON_CHILD $chevron_child_path"
  if [ ! -s "$chevron_status_closed_again_path" ]; then
    fail "no bar chevron status (closed again) produced"
  fi
  if ! grep -q '"open":false' "$chevron_status_closed_again_path"; then
    fail "bar chevron collapse left the group's bar open. Got: $(cat "$chevron_status_closed_again_path")"
  fi
  _chevron_assert_resize
  _chevron_assert_clicks
  _chevron_assert_ink
}
