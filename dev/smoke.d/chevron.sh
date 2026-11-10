# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --chevron points bar.layout at today's exact default right region reordered
# around one `chevron` name: the governed cells lead the region, then the
# chevron, then battery/audio/network sit outboard against the screen edge. A
# right-region chevron governs what PRECEDES it, so those cells are never on
# the strip at all, and everything outboard of the chevron keeps its x
# whether its card is up or not.
#
# The chevron opens the quick settings card
# (docs/superpowers/specs/2026-11-04-quick-settings.md §8), hanging off the
# strip under it. `bar chevron expand` stands in for a click on the cell (no
# synthetic pointer here), deliberately with no region argument, since a
# single-chevron layout is meant to infer it; `quicksettings status` then
# reads the card open from `chevron:right`, with an info tile for every
# governed cell it does not draw itself (bluetooth and indicators are its
# own toggles) and whose honest state is shown. The two PNGs are asserted to
# differ: a correct IPC contract can sit over a bar that renders
# nothing, with every dump passing.
#
# It also carries the ink claim: the band reads its paint off the
# wallpaper, so the leg sets a flat bright field first and the strip goes
# to dark ink over it. The card the chevron opens is not the band, and its
# tiles have to say so, which is read off one patch of the frame inside the
# Weather tile: a dark plate with light words on it, not the band's black
# ink drawn onto it.
#
# A real click on the Weather tile hands the card over to the weather
# panel (§8: the panel takes the card's place rather than hanging off it),
# so the chevron reads shut with `weather` open. The weather panel's header
# carries a back button: a real click on it (found off `debug dump`'s
# panel `buttons`) brings the card back from `chevron:right`, and after a
# second hand-over a real Backspace does the same. Then the card around its
# tiles: the group carries a `command` module reading a file, which answers
# nothing at first and so has no tile; given a label under the open card, a
# tile appears for it, and every tile stays inside the card's rect.
#
# Last, the card under a real pointer behaving like a panel (owner,
# 2026-10-09: the chevron's old second bar ignored a click outside, and a
# click on another cell opened that cell's panel beside it): the chevron
# clicked open, a click on the desktop shutting it, then open again and a
# click on the clock shutting it and opening the calendar in its place.
leg_chevron_flag="--chevron"
leg_chevron_order=180
leg_chevron_needs="wlrctl wtype convert jq"
leg_chevron_claims="bar.layout.right bar.modules panel=shared"

chevron_status_shut_path="$shot_dir/chevron-status-shut.json"
chevron_status_open_path="$shot_dir/chevron-status-open.json"
chevron_quick_open_path="$shot_dir/chevron-quick-open.json"
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
chevron_quick_grown_path="$shot_dir/chevron-quick-grown.json"
chevron_grown_path="$shot_dir/chevron-grown.png"
chevron_room_path="$shot_dir/chevron-room.json"
chevron_tray_path="$shot_dir/chevron-tray.json"
chevron_clicks_path="$shot_dir/chevron-clicks.txt"
chevron_outside_path="$shot_dir/chevron-outside.png"
chevron_handoff_path="$shot_dir/chevron-handoff.png"
chevron_child_dump_path="$shot_dir/chevron-child-dump.json"
chevron_back_path="$shot_dir/chevron-back.png"
chevron_back_state_path="$shot_dir/chevron-back-state.txt"
chevron_back_quick_path="$shot_dir/chevron-back-quick.json"
chevron_rehanded_path="$shot_dir/chevron-rehanded.txt"
chevron_key_back_state_path="$shot_dir/chevron-key-back-state.txt"
chevron_key_back_quick_path="$shot_dir/chevron-key-back-quick.json"

# The Weather tile's centre in the open card, off `quicksettings status`'s
# own rects, in the output's logical pixels.
chevron_weather_centre='.rows[] | select(.id == "weather") | .rect | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"'
# The handed-over panel's back button, off `debug dump`.
chevron_back_centre='.panel.buttons[] | select(.key == "back") | .rect | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"'

leg_chevron_fixture() {
  : > "$chevron_grow_text_path"
  write_script "$chevron_grow_cmd_path" <<EOF
#!/usr/bin/env bash
printf '{"text": "%s"}' "\$(cat "$chevron_grow_text_path")"
EOF
  settings_pin '. * {"bar": {"layout": {"right": ["bluetooth", "weather", "tray", "bell", "indicators", "custom:chevgrow", "chevron", "battery", "audio", "network"]}, "modules": [{"id": "chevgrow", "type": "command", "command": ["bash", "'"$chevron_grow_cmd_path"'"], "interval": 500}]}}'
  # The wallpaper the band's dark ink comes from, the flat bright field
  # --bar-adaptive reads `dark` off: mean 230, no spread, nothing busy.
  flat_wallpaper "$chevron_bright_wp" '#e6e6e6'
}

leg_chevron_timing() {
  # chevron-drive.sh's own last step lands ~19s in; the run's own frame is
  # taken past that, so it shows the bar left with the group's card shut.
  # The wallpaper set and the retheme behind it push every step of that ~9s
  # later.
  leg_timing 106
}

leg_chevron_drive() {
  local script="$shot_dir/chevron-drive.sh"
  # The open shot sits two seconds behind the expand call, an order of
  # magnitude past the card's own entrance, which is what makes it a picture
  # of the end state rather than of a frame somewhere inside it.
  #
  # Then a real pointer click on the Weather tile, the half no IPC verb can
  # stand in for. wlrctl's pointer is relative-only, hence the slam into the
  # corner before the move, the same trick --wheel documents. A miss fails
  # the panel assert loudly rather than passing quietly.
  #
  # The bright wallpaper and the shell's own reading of the band over it.
  local paint_step="$ipc call wallpaper set \"$chevron_bright_wp\" > /dev/null 2>&1
until_wallpaper \"$chevron_bright_wp\"
until_settled
$ipc call bar paint > \"$chevron_paint_path\" 2>&1"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$paint_step
$ipc call bar chevron status > "$chevron_status_shut_path" 2>&1
sleep 1
"$grim_bin" "$chevron_shut_path" > /dev/null 2>&1
sleep 1
$ipc call tray status > "$chevron_tray_path" 2>&1
$ipc call bar chevron expand > "$chevron_expand_reply_path" 2>&1
sleep 3
$ipc call bar chevron status > "$chevron_status_open_path" 2>&1
$ipc call quicksettings status > "$chevron_quick_open_path" 2>&1
"$grim_bin" "$chevron_open_path" > /dev/null 2>&1
printf 'a label far wider than any cell' > "$chevron_grow_text_path"
sleep 3
$ipc call quicksettings status > "$chevron_quick_grown_path" 2>&1
"$grim_bin" "$chevron_grown_path" > /dev/null 2>&1
"$wlrctl_bin" pointer move -4000 -4000 > "$chevron_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer move \$("$jq_bin" -r '$chevron_weather_centre' "$chevron_quick_grown_path") >> "$chevron_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer click left >> "$chevron_dispatch_path" 2>&1
sleep 3
$ipc call panel state > "$chevron_panel_state_path" 2>&1
$ipc call bar chevron status > "$chevron_status_child_path" 2>&1
"$grim_bin" "$chevron_child_path" > /dev/null 2>&1
$ipc call debug dump > "$chevron_child_dump_path" 2>&1
"$wlrctl_bin" pointer move -4000 -4000 >> "$chevron_dispatch_path" 2>&1
"$wlrctl_bin" pointer move \$("$jq_bin" -r '$chevron_back_centre' "$chevron_child_dump_path") >> "$chevron_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer click left >> "$chevron_dispatch_path" 2>&1
sleep 3
$ipc call panel state > "$chevron_back_state_path" 2>&1
$ipc call quicksettings status > "$chevron_back_quick_path" 2>&1
"$grim_bin" "$chevron_back_path" > /dev/null 2>&1
"$wlrctl_bin" pointer move -4000 -4000 >> "$chevron_dispatch_path" 2>&1
"$wlrctl_bin" pointer move \$("$jq_bin" -r '$chevron_weather_centre' "$chevron_back_quick_path") >> "$chevron_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer click left >> "$chevron_dispatch_path" 2>&1
sleep 3
$ipc call panel state > "$chevron_rehanded_path" 2>&1
"$wtype_bin" -k BackSpace
sleep 3
$ipc call panel state > "$chevron_key_back_state_path" 2>&1
$ipc call quicksettings status > "$chevron_key_back_quick_path" 2>&1
$ipc call panel close > /dev/null 2>&1
sleep 2
$ipc call bar chevron status > "$chevron_status_closed_again_path" 2>&1
$ipc call bar room > "$chevron_room_path" 2>&1
cell() { "$jq_bin" -r --arg n "\$1" '.[0].cells[] | select(.name == \$n) | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"' "$chevron_room_path" | head -n 1; }
to() { park "\$1" "\$2"; }
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

# The weather panel handed over from the card carries a back button, and
# both a click on it and Backspace bring the card back from the chevron it
# was opened off, not from IPC.
_chevron_assert_back() {
  local buttons
  buttons=$("$jq_bin" -c '[.panel.buttons[].key]' "$chevron_child_dump_path" 2>/dev/null)
  echo "chevron back: weather header buttons $buttons"
  if [ "$buttons" != '["back","refresh","close"]' ]; then
    fail "the weather panel handed over from quick settings carries $buttons in its header, not back, refresh and close: $(cat "$chevron_child_dump_path" 2>/dev/null)"
  fi
  if ! grep -q '^quicksettings$' "$chevron_back_state_path" 2>/dev/null; then
    fail "clicking the back button did not bring quick settings back. Got: $(cat "$chevron_back_state_path" 2>/dev/null), pointer: $(cat "$chevron_dispatch_path" 2>/dev/null)"
  fi
  if [ "$("$jq_bin" -r '.from' "$chevron_back_quick_path" 2>/dev/null)" != "chevron:right" ]; then
    fail "the card the back button brought back is not the chevron's: $(cat "$chevron_back_quick_path" 2>/dev/null)"
  fi
  [ -f "$chevron_back_path" ] || fail "no chevron-back screenshot produced"
  echo "SMOKE_CHEVRON_BACK $chevron_back_path"
  if ! grep -q '^weather$' "$chevron_rehanded_path" 2>/dev/null; then
    fail "the Weather tile in the card brought back did not hand over again. Got: $(cat "$chevron_rehanded_path" 2>/dev/null)"
  fi
  if ! grep -q '^quicksettings$' "$chevron_key_back_state_path" 2>/dev/null; then
    fail "Backspace in the handed-over weather panel did not bring quick settings back. Got: $(cat "$chevron_key_back_state_path" 2>/dev/null)"
  fi
  if [ "$("$jq_bin" -r '.from' "$chevron_key_back_quick_path" 2>/dev/null)" != "chevron:right" ]; then
    fail "the card Backspace brought back is not the chevron's: $(cat "$chevron_key_back_quick_path" 2>/dev/null)"
  fi
}

# The brightest red sample of one patch of a frame, 0..255. Every colour here
# is grey, so one channel is the whole answer.
_chevron_patch_max() {
  $convert_bin "$1" -crop "$2" +repage \
    -format '%[fx:int(maxima.r*255+0.5)]' info: 2>/dev/null
}

# The ink claim. Three parts: the band settled
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
  # One patch of the open frame, in logical pixels: 30px either side of the
  # Weather tile's centre, the point the pointer above is already asserted
  # to land a panel from, and 10px either side of it across, over its title
  # and value. Two readings off it, since either alone passes for the wrong
  # reason: the mean says the patch is on the card rather than on the
  # wallpaper beside it, and the maximum says there are light words on it.
  local centre chevron_card_patch
  centre=$("$jq_bin" -r "$chevron_weather_centre" "$chevron_quick_open_path" 2>/dev/null)
  [ -n "$centre" ] || fail "the open card carries no Weather tile to read its ink at: $(cat "$chevron_quick_open_path")"
  # shellcheck disable=SC2086
  set -- $centre
  chevron_card_patch=$(logical_box 60 20 $(( $1 - 30 )) $(( $2 - 10 )))
  $convert_bin "$chevron_open_path" -crop "$chevron_card_patch" +repage \
    "$chevron_card_ink_path" > /dev/null 2>&1
  [ -f "$chevron_card_ink_path" ] && echo "SMOKE_CHEVRON_CARD_INK $chevron_card_ink_path"
  mean=$(patch_mean "$chevron_open_path" "$chevron_card_patch")
  max=$(_chevron_patch_max "$chevron_open_path" "$chevron_card_patch")
  [ -n "$mean" ] && [ -n "$max" ] \
    || fail "could not read the patch $chevron_card_patch out of $chevron_open_path"
  echo "chevron ink: the card patch $chevron_card_patch reads mean=$mean max=$max"
  if [ "$mean" -gt 471 ]; then
    fail "the patch $chevron_card_patch reads mean $mean, too bright to be the card: it is on the wallpaper beside it, so the ink below is untested"
  fi
  if [ "$max" -lt 150 ]; then
    fail "the card's brightest pixel over $chevron_card_patch is $max: the card is drawing the band's black ink on its own dark plate"
  fi
}

# The card from the chevron, and its info tiles: the governed cells the card
# does not draw itself, in `bar.layout` order, Tray only while `tray status`
# lists an item (the host's), the command module only once it has a label,
# and every row inside the card's rect.
_chevron_assert_card() {
  local want got outside items
  [ -s "$chevron_quick_open_path" ] || fail "no quicksettings status (open) produced"
  if ! "$jq_bin" -e '.open == true and .from == "chevron:right"' "$chevron_quick_open_path" > /dev/null 2>&1; then
    fail "bar chevron expand did not open quick settings from the right chevron: $(cat "$chevron_quick_open_path")"
  fi
  got=$("$jq_bin" -c '[.rows[] | select(.kind == "info") | .id]' "$chevron_quick_open_path" 2>/dev/null)
  items=$("$jq_bin" -r '.items | length' "$chevron_tray_path" 2>/dev/null)
  want='["weather","bell"]'
  [ "${items:-0}" -gt 0 ] && want='["weather","tray","bell"]'
  echo "chevron card: info tiles $got"
  [ "$got" = "$want" ] || fail "the card's info tiles are $got, not the governed cells it does not draw ($want): $(cat "$chevron_quick_open_path")"
  [ -s "$chevron_quick_grown_path" ] || fail "no quicksettings status (grown) produced"
  echo "SMOKE_CHEVRON_GROWN $chevron_grown_path"
  got=$("$jq_bin" -c '[.rows[] | select(.kind == "info") | .id]' "$chevron_quick_grown_path" 2>/dev/null)
  want="${want%]},\"custom:chevgrow\"]"
  [ "$got" = "$want" ] || fail "the command module given a label did not join the open card as a tile ($got, wanted $want): $(cat "$chevron_quick_grown_path")"
  for json in "$chevron_quick_open_path" "$chevron_quick_grown_path"; do
    outside=$("$jq_bin" -r '.card as $c | [.rows[] | .rect | select(. != null)
      | select(.x < $c.x or .y < $c.y or .x + .width > $c.x + $c.width or .y + .height > $c.y + $c.height)] | length' "$json" 2>/dev/null)
    [ "$outside" = "0" ] || fail "$outside rows lie outside the card in $json: $(cat "$json")"
  done
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
  _chevron_click_expect clicked-open 'open=true panel=quicksettings' "a real click on the chevron opens quick settings"
  _chevron_click_expect outside 'open=false panel=' "a click on the desktop shuts it"
  _chevron_click_expect reopened 'open=true panel=' "the chevron opens it again"
  _chevron_click_expect handoff 'open=false panel=calendar' "a click on the clock shuts it and opens the calendar"
}

leg_chevron_assert() {
  # The names the fixture puts before the chevron. Order matters:
  # `collapses` reports them in layout order, so one grep asserts the whole
  # boundary rather than six independent membership checks.
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
  if ! grep -q "\"collapses\":\[$chevron_hidden_names\],\"open\":false" "$chevron_status_shut_path"; then
    fail "bar chevron status does not govern the names placed before it, with its card shut. Got: $(cat "$chevron_status_shut_path")"
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
  # The same names, still governed, now with the card up. An empty
  # `collapses` here would mean the layout changed under the run rather than
  # the surface.
  if ! grep -q "\"collapses\":\[$chevron_hidden_names\],\"open\":true" "$chevron_status_open_path"; then
    fail "bar chevron expand did not open the card while keeping the same governed names. Got: $(cat "$chevron_status_open_path")"
  fi
  if [ ! -f "$chevron_open_path" ]; then
    fail "no chevron-open screenshot produced"
  fi
  if cmp -s "$chevron_shut_path" "$chevron_open_path"; then
    fail "chevron-shut and chevron-open screenshots are byte-identical: the card opened and rendered nothing"
  fi
  _chevron_assert_card
  # The tile's handoff: the weather panel open in the card's place, the
  # chevron's card no longer up.
  if ! grep -q '^weather$' "$chevron_panel_state_path" 2>/dev/null; then
    fail "clicking the Weather tile did not hand over to its panel. Got: $(cat "$chevron_panel_state_path" 2>/dev/null), pointer: $(cat "$chevron_dispatch_path" 2>/dev/null)"
  fi
  if ! grep -q '"open":false' "$chevron_status_child_path" 2>/dev/null; then
    fail "the weather panel hangs off a card still open rather than taking its place. Got: $(cat "$chevron_status_child_path" 2>/dev/null)"
  fi
  if [ ! -f "$chevron_child_path" ]; then
    fail "no chevron-child screenshot produced"
  fi
  echo "SMOKE_CHEVRON_CHILD $chevron_child_path"
  _chevron_assert_back
  if [ ! -s "$chevron_status_closed_again_path" ]; then
    fail "no bar chevron status (closed again) produced"
  fi
  if ! grep -q '"open":false' "$chevron_status_closed_again_path"; then
    fail "the chevron reads open with nothing up. Got: $(cat "$chevron_status_closed_again_path")"
  fi
  _chevron_assert_clicks
  _chevron_assert_ink
}
