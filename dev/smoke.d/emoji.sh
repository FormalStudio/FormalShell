# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --emoji: what the route finds and what order it finds it in. menu.sh owns
# the grid's chrome (columns, headings, placeholder); this leg owns the two
# halves a frame cannot state.
#
# Keywords: Unicode's name for 😭 is "loudly crying face" and carries no
# "sob" anywhere, so a ':e sob' that lands on it proves the shipped
# emoji.json carries CLDR's annotations and that the search reads them.
#
# Usage order: ':e cry' is read before any copy (😢 leads, file order inside
# the name-prefix rank), the second cell is copied through the surface's own
# Enter path, and the same query is read back. 😿 leading it then is
# state.json's `emojiUses` reaching the ranking with menu.emoji.sortByUsage
# never written, which is the default this leg exists to hold. 😭 staying
# below both is the other half of the contract: a copy reorders a rank, it
# never promotes a row out of one.
#
# The clipboard read-back sits between the two: `menu activate` running the
# row's own `wl-copy` is what makes the recorded use a real use, and without
# it a reordered query would only prove the shell wrote its own state file.
#
# The copy is a pick too: state.json's `picks` holds `emoji:😿`, and with
# two other picks seeded the root at rest tiles it beside them.
#
# Both queries go through `debug query`, which ranks against the same dataset
# the surface does with no keyboard delivery needed, and both are narrow on
# purpose: the whole 3,944-entry set is a 300KB reply and the IPC socket
# drops it.
#
# Then the whole grid opened again and walked on real keys (Down, Page_Down,
# End): at each stop the bottom band of the body's viewport, a cell tall
# and clear of the grid's own inset, has to carry emoji ink (half a cell
# could land between two rows' glyphs). A grid
# whose lines came out shorter than their slots stacked every row up the
# card and left that band empty card fill. The viewport and the cursor's
# slot come off `menu status` (`body`, `viewCursor`), so nothing here
# assumes an output size or a theme's padding.
leg_emoji_flag="--emoji"
leg_emoji_order=25
leg_emoji_needs="wl-paste jq wtype convert"
emoji_stops="open Down Page_Down End"

# This leg's own clock: the launcher covers the whole output, so under
# --wallpaper it starts after that leg's last frame, the rule menu_t0 draws.
emoji_t0() {
  cover_t0 3
}

emoji_keyword_path="$shot_dir/emoji-keyword-query.json"
emoji_before_path="$shot_dir/emoji-rank-before.json"
emoji_after_path="$shot_dir/emoji-rank-after.json"
emoji_status_path="$shot_dir/emoji-status.json"
emoji_clipboard_path="$shot_dir/emoji-clipboard.txt"
emoji_state_path="$shot_dir/emoji-state.json"
emoji_png="$shot_dir/emoji-search.png"
emoji_recent_path="$shot_dir/emoji-recent.json"
emoji_recent_png="$shot_dir/emoji-recent.png"

leg_emoji_fixture() {
  local state="$iso_home/.local/state/formalshell/state.json" now
  now=$(($(date +%s) * 1000))
  mkdir -p "${state%/*}"
  # A leg riding along that seeds its own state keeps it.
  [ -e "$state" ] || printf '{"picks":[{"id":"clipboard","count":1,"lastMs":%s},{"id":"toggles","count":1,"lastMs":%s}]}\n' "$now" "$now" > "$state"
}

leg_emoji_timing() {
  local t0
  t0=$(emoji_t0)
  leg_timing $((t0 + 13)) $((t0 + 76))
}

leg_emoji_drive() {
  local t0 script="$shot_dir/emoji-drive.sh"
  t0=$(emoji_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
$ipc call debug query ':e sob' > "$emoji_keyword_path" 2>&1
$ipc call debug query ':e cry' > "$emoji_before_path" 2>&1
$ipc call menu summon emoji > /dev/null 2>&1
sleep 1
$ipc call menu filter ':e cry' > /dev/null 2>&1
sleep 2
"$grim_bin" "$emoji_png" > /dev/null 2>&1
$ipc call menu status > "$emoji_status_path" 2>&1
$ipc call menu activate 1 > /dev/null 2>&1
sleep 3
"$wl_paste_bin" -n > "$emoji_clipboard_path" 2>&1
cat "$iso_home/.local/state/formalshell/state.json" > "$emoji_state_path" 2>&1
$ipc call debug query ':e cry' > "$emoji_after_path" 2>&1
$ipc call menu summon "" > /dev/null 2>&1
sleep 2
$ipc call menu status > "$emoji_recent_path" 2>&1
"$grim_bin" "$emoji_recent_png" > /dev/null 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 2
$ipc call menu summon emoji > /dev/null 2>&1
sleep 3
for stop in $emoji_stops; do
  case "\$stop" in
    open) ;;
    Down) for _ in 1 2 3 4 5 6 7 8 9; do "$wtype_bin" -k Down; sleep 0.1; done ;;
    *) "$wtype_bin" -k "\$stop" ;;
  esac
  sleep 2
  $ipc call menu status > "$shot_dir/emoji-grid-\$stop.json" 2>&1
  "$grim_bin" "$shot_dir/emoji-grid-\$stop.png" > /dev/null 2>&1
done
$ipc call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# The glyph of the row at `$2`, off `debug query`'s own JSON: the rows carry
# the char as their icon, and where one sits in that list is the whole claim
# every assertion below makes.
emoji_row_icon() {
  "$jq_bin" -r ".[$2].icon // \"\"" "$1" 2>/dev/null
}

leg_emoji_assert() {
  local f icon
  for f in "$emoji_keyword_path" "$emoji_before_path" "$emoji_after_path" \
    "$emoji_status_path" "$emoji_clipboard_path" "$emoji_state_path"; do
    [ -s "$f" ] || fail "no emoji artifact produced at $f"
  done
  [ -f "$emoji_png" ] || fail "no emoji screenshot produced at $emoji_png"

  cat "$emoji_keyword_path"; echo
  icon=$(emoji_row_icon "$emoji_keyword_path" 0)
  if [ "$icon" != "😭" ]; then
    fail "':e sob' led with '$icon', not the emoji whose only 'sob' is a CLDR keyword"
  fi
  if ! grep -qF '"label":"LOUDLY CRYING FACE"' "$emoji_keyword_path"; then
    fail "':e sob' did not name the row it matched: $(cat "$emoji_keyword_path")"
  fi

  cat "$emoji_before_path"; echo
  if [ "$(emoji_row_icon "$emoji_before_path" 0)" != "😢" ] \
    || [ "$(emoji_row_icon "$emoji_before_path" 1)" != "😿" ]; then
    fail "':e cry' did not open in file order on an empty ledger: $(cat "$emoji_before_path")"
  fi

  if ! grep -q '"level":"emoji"' "$emoji_status_path"; then
    fail "the route was not open when the row was activated: $(cat "$emoji_status_path")"
  fi
  if [ "$(cat "$emoji_clipboard_path")" != "😿" ]; then
    fail "Enter on the second cell put '$(cat "$emoji_clipboard_path")' on the clipboard, not that emoji"
  fi
  cat "$emoji_state_path"; echo
  # Through jq rather than a grep: JsonAdapter's own spacing is not this
  # leg's claim, and a \u-escaped glyph in the file would read the same.
  if [ "$("$jq_bin" -r '.emojiUses[0].id // ""' "$emoji_state_path")" != "emoji.😿" ] \
    || [ "$("$jq_bin" -r '.emojiUses[0].count // 0' "$emoji_state_path")" != "1" ]; then
    fail "the copy recorded no use in state.json: $(cat "$emoji_state_path")"
  fi

  cat "$emoji_after_path"; echo
  icon=$(emoji_row_icon "$emoji_after_path" 0)
  if [ "$icon" != "😿" ]; then
    fail "after one copy ':e cry' still led with '$icon': the usage ranking is not on by default"
  fi
  # 😭 matches "cry" on a word start, a rank below the two the copy shuffled.
  # Promoting it here would mean usage outranks the match itself.
  if [ "$(emoji_row_icon "$emoji_after_path" 1)" != "😢" ]; then
    fail "the copy did more than reorder its own rank: $(cat "$emoji_after_path")"
  fi
  echo "SMOKE_EMOJI $emoji_png (':e sob' → 😭, one copy leads its rank)"
  "$jq_bin" -e '[.picks[].id] | index("emoji:😿") != null' "$emoji_state_path" > /dev/null \
    || fail "the copy recorded no pick in state.json: $("$jq_bin" -c '.picks' "$emoji_state_path")"
  [ -s "$emoji_recent_path" ] && [ -f "$emoji_recent_png" ] || fail "no root status or frame after the copy"
  root_tiles_assert "$emoji_recent_path" '(.ids | sort) == (["clipboard", "toggles", "emoji.😿"] | sort)' \
    "the copied emoji is not a tile at the root beside the two seeded picks"
  echo "SMOKE_EMOJI_RECENT $emoji_recent_png (the copied emoji is a pick and lists under Recent)"
  emoji_grid_assert
}

# The share of a crop's pixels that are emoji ink: HSL saturation and
# lightness both past 35%, so a near-black pixel with a saturated hue drops
# out. A card over the wallpaper at the surface opacity stays under it.
emoji_ink() {
  "$convert_bin" "$1" -crop "$2" +repage -colorspace HSL -separate -delete 0 \
    -evaluate-sequence Min -threshold 35% -format '%[fx:mean]' info: 2>/dev/null
}

emoji_grid_assert() {
  local stop f png bx by bw bh top bottom ch scroll band mid ink inset=""
  for stop in $emoji_stops; do
    f="$shot_dir/emoji-grid-$stop.json"
    png="$shot_dir/emoji-grid-$stop.png"
    { [ -s "$f" ] && [ -f "$png" ]; } || fail "no emoji grid status or frame for the $stop stop"
    "$jq_bin" -e '.view == "emoji" and .body != null' "$f" > /dev/null \
      || fail "the emoji grid was not open at the $stop stop: $(cat "$f")"
    read -r bx by bw bh top bottom scroll < <("$jq_bin" -r '[.body.x, .body.y, .body.width, .body.height, .viewCursor.top, .viewCursor.bottom, .scrollTop] | @tsv' "$f")
    ch=$((bottom - top))
    if [ -z "$inset" ]; then
      # The first cell at scroll 0 sits the grid's own inset under the top.
      [ "$scroll" = 0 ] || fail "the grid did not open at the top: $(cat "$f")"
      inset=$top
    fi
    band=$ch
    ink=$(emoji_ink "$png" "${bw}x${band}+${bx}+$((by + bh - inset - band))")
    mid=$(emoji_ink "$png" "${bw}x${band}+${bx}+$((by + bh / 2))")
    echo "emoji grid $stop: scroll $scroll, cell ${ch}px, viewport ${bw}x${bh}+${bx}+${by}, bottom band ink $ink, middle band ink $mid"
    awk -v v="$ink" -v m="$mid" 'BEGIN { exit !(v > 0.01 && v > m * 0.25) }' \
      || fail "the bottom band of the emoji grid carries no emoji at the $stop stop (ink $ink, $mid mid-viewport): $png"
  done
  "$jq_bin" -e '.scrollTop > 0 and .cursor == .rows - 1 and .viewCursor.bottom <= .viewCursor.viewport' "$shot_dir/emoji-grid-End.json" > /dev/null \
    || fail "End did not scroll the grid to its foot: $(cat "$shot_dir/emoji-grid-End.json")"
  echo "SMOKE_EMOJI_GRID $shot_dir/emoji-grid-open.png (bottom row inked at: $emoji_stops)"
}
