# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --menu: the launcher summoned at root, its fuzzy ranking against the live
# tree (`debug query`), a root query as one list in score order, the emoji route's grid, and the select round trip. The run's own
# smoke.png lands in select mode; menu-root.png is the root level, where
# the row icons, the group headings and the tree's own chrome are,
# menu-search.png is the same field with a query in it under no heading,
# and menu-emoji.png is the grid.
#
# The chrome that a frame cannot state is asserted off `menu status` instead
# (M48): the field's placeholder, the group headings in the order they
# appear, and the column count. That last one is what tells a grid from a
# list at all, since a frame cannot say whether eight glyphs on one line are
# eight cells of a grid or one wide row of a list. The root is the app grid
# (M72 T4): its apps under Applications, then the tree's own commands under
# their headings, so the leg stages one desktop entry for the isolated
# session to list, and the root has to report that order.
#
# Every status read here also checks the cursor against the view (M72 T1):
# `cursorId` is the row Enter acts on, and `viewCursor` is where the live
# view itself holds its current item and which row id its own model has
# there. A re-rank used to move the view's current item without moving the
# cursor, which put the fill on one row and Enter on another.
#
# The field is typed into on real keys: a word, Ctrl+A selecting all of it
# (menu-selected.png has the selection fill), a second word replacing it,
# then Shift+Home and Backspace emptying it, each read off `menu status`'s
# `query`, `caret` and `selection`.
leg_menu_flag="--menu"
leg_menu_order=20
leg_menu_needs="jq wtype"

# This leg's own clock. The launcher covers the whole output, so under
# --wallpaper it starts after that leg's last frame (t=14) rather than
# sampling the desktop through a scrim, the same rule lock_t0 draws.
menu_t0() {
  if leg_on wallpaper; then echo 16; else echo 3; fi
}

query_path="$shot_dir/query.json"
selection_path="$shot_dir/menu-selection.txt"
menu_root_path="$shot_dir/menu-root.png"
menu_search_path="$shot_dir/menu-search.png"
menu_emoji_path="$shot_dir/menu-emoji.png"
menu_status_root_path="$shot_dir/menu-status-root.json"
menu_status_search_path="$shot_dir/menu-status-search.json"
menu_status_emoji_path="$shot_dir/menu-status-emoji.json"
menu_down_path="$shot_dir/menu-down.png"
menu_status_down_path="$shot_dir/menu-status-down.json"
menu_selected_path="$shot_dir/menu-selected.png"

menu_app_dir="$iso_home/.local/share/applications"

leg_menu_fixture() {
  mkdir -p "$menu_app_dir"
  {
    echo "[Desktop Entry]"
    echo "Type=Application"
    echo "Name=Menu Fixture"
    echo "Exec=true"
  } > "$menu_app_dir/formalshell-menu-fixture.desktop"
  # A root grid of more than one row, the shape a real install has: the
  # Down check below is about a cursor moving between grid rows.
  local n
  for n in 01 02 03 04 05 06 07 08 09 10 11; do
    printf '[Desktop Entry]\nType=Application\nName=Fixture App %s\nExec=true\n' "$n" \
      > "$menu_app_dir/formalshell-menu-fixture-$n.desktop"
  done
}

leg_menu_timing() {
  # menu-finish.sh's own read-back lands 25s after menu_t0, so the session
  # has to outlive that, not just the screenshot.
  local t0
  t0=$(menu_t0)
  leg_timing $((26 + t0 - 3)) $((58 + t0 - 3)) 4
}

leg_menu_drive() {
  # `ipc call`'s CLI11 parser splits any positional argument that starts
  # with "[" and ends with "]" into several comma-joined arguments (its
  # vector-literal shorthand), so the select list carries a leading space to
  # defeat that check. JSON.parse tolerates the whitespace.
  local t0 menu_script="$shot_dir/menu-drive.sh"
  t0=$(menu_t0)
  write_script "$menu_script" <<EOF
#!/usr/bin/env bash
# Every wtype is a new virtual keyboard whose first keys reach nobody until
# the shell binds it (lock.sh's drive has the detail), so each waits first.
sleep $t0
$ipc call menu summon "" > /dev/null 2>&1
sleep 2
$ipc call debug query 'e' > "$query_path" 2>&1
"$grim_bin" "$menu_root_path" > /dev/null 2>&1
$ipc call menu status > "$menu_status_root_path" 2>&1
"$wtype_bin" -s 300 -k Down
sleep 1
"$grim_bin" "$menu_down_path" > /dev/null 2>&1
$ipc call menu status > "$menu_status_down_path" 2>&1
for n in 2 3 4; do
  "$wtype_bin" -s 300 -k Down
  sleep 0.6
  $ipc call menu status > "$shot_dir/menu-status-down\$n.json" 2>&1
done
for n in 1 2 3 4; do
  "$wtype_bin" -s 300 -k Up
  sleep 0.6
  $ipc call menu status > "$shot_dir/menu-status-up\$n.json" 2>&1
done
sleep 1
$ipc call menu filter e > /dev/null 2>&1
sleep 1
"$grim_bin" "$menu_search_path" > /dev/null 2>&1
$ipc call menu status > "$menu_status_search_path" 2>&1
sleep 1
$ipc call menu summon "" > /dev/null 2>&1
sleep 1
"$wtype_bin" -s 300 fire
sleep 0.6
$ipc call menu status > "$shot_dir/menu-status-typed.json" 2>&1
"$wtype_bin" -s 300 -M ctrl -k a -m ctrl
sleep 0.6
"$grim_bin" "$menu_selected_path" > /dev/null 2>&1
$ipc call menu status > "$shot_dir/menu-status-selected.json" 2>&1
"$wtype_bin" -s 300 term
sleep 0.6
$ipc call menu status > "$shot_dir/menu-status-replaced.json" 2>&1
"$wtype_bin" -s 300 -M shift -k Home -m shift
"$wtype_bin" -s 300 -k BackSpace
sleep 0.6
$ipc call menu status > "$shot_dir/menu-status-emptied.json" 2>&1
$ipc call menu summon emoji > /dev/null 2>&1
sleep 1
$ipc call menu filter smiling > /dev/null 2>&1
sleep 2
"$grim_bin" "$menu_emoji_path" > /dev/null 2>&1
$ipc call menu status > "$menu_status_emoji_path" 2>&1
sleep 1
$ipc call menu select "Pick" ' ["a","b","c"]' tok1 > /dev/null 2>&1
touch "$shot_dir/menu-drive.done"
EOF

  # Runs after the screenshot, never before: closing the surface first would
  # leave nothing to photograph. The close cancels the still-pending select,
  # and the state file is what proves the {cancelled:true} write happened.
  local menu_finish_script="$shot_dir/menu-finish.sh"
  write_script "$menu_finish_script" <<EOF
#!/usr/bin/env bash
sleep $((t0 + 24))
# The mac rig's software compositor runs the drive past 24s; the select it
# ends on has to be pending before the close.
for _ in \$(seq 80); do [ -e "$shot_dir/menu-drive.done" ] && break; sleep 0.25; done
$ipc call menu close > /dev/null 2>&1
sleep 1
cat "$iso_home/.local/state/formalshell/menu-selection.txt" > "$selection_path" 2>&1
EOF
  hypr_exec_once "bash $menu_script"
  hypr_exec_once "bash $menu_finish_script"
}

menu_field() {
  sed -n 's/.*"'"$2"'":\([0-9-]*\).*/\1/p' "$1"
}

menu_cursor_agrees() {
  local f="$1" what="$2" id view_id index view_index
  id=$("$jq_bin" -r '.cursorId // ""' "$f" 2>/dev/null)
  view_id=$("$jq_bin" -r '.viewCursor.id // ""' "$f" 2>/dev/null)
  index=$("$jq_bin" -r '.cursor' "$f" 2>/dev/null)
  view_index=$("$jq_bin" -r '.viewCursor.index' "$f" 2>/dev/null)
  if [ -z "$id" ] || [ "$id" != "$view_id" ] || [ "$index" != "$view_index" ]; then
    fail "$what: the cursor is on row $index ($id) but the view holds $view_index (${view_id:-nothing})"
  fi
  echo "SMOKE_MENU_CURSOR $what: row $index ($id)"
}

menu_cursor_on_screen() {
  local f="$1" what="$2" top bottom viewport
  menu_cursor_agrees "$f" "$what"
  top=$("$jq_bin" -r '.viewCursor.top' "$f" 2>/dev/null)
  bottom=$("$jq_bin" -r '.viewCursor.bottom' "$f" 2>/dev/null)
  viewport=$("$jq_bin" -r '.viewCursor.viewport' "$f" 2>/dev/null)
  if [ -z "$top" ] || [ "$top" -lt 0 ] || [ "$bottom" -gt "$viewport" ]; then
    fail "$what: the cursor row spans $top..$bottom in a viewport of $viewport"
  fi
  echo "SMOKE_MENU_ON_SCREEN $what: $top..$bottom of $viewport"
}

leg_menu_assert() {
  if [ -s "$query_path" ]; then
    cat "$query_path"
  else
    fail "no menu query result produced"
  fi
  # Printed before the selection read-back, whose `cat` has no trailing
  # newline and would otherwise swallow this line's own start-of-line anchor.
  if [ ! -f "$menu_root_path" ]; then
    fail "no root-level menu screenshot produced"
  fi
  echo "SMOKE_MENU_ROOT $menu_root_path"
  # The root's own chrome: shadcn's placeholder, and the body's sections in
  # order: the staged app's grid under Applications first, then the tree's
  # own two command groups, in declaration order, in the rows under it.
  if [ ! -s "$menu_status_root_path" ]; then
    fail "no root menu status produced"
  fi
  cat "$menu_status_root_path"; echo
  if ! grep -q '"placeholder":"Type a command or search..."' "$menu_status_root_path"; then
    fail "the root field did not carry the command-palette placeholder, got: $(cat "$menu_status_root_path")"
  fi
  if ! "$jq_bin" -e '.view == "appGrid" and .columns > 1
      and .sections == ["Applications", "Suggestions", "Commands"]' "$menu_status_root_path" > /dev/null 2>&1; then
    fail "the root did not lay out as the Applications grid, then Suggestions, then Commands, got: $(cat "$menu_status_root_path")"
  fi
  menu_cursor_agrees "$menu_status_root_path" "root"
  # One real Down straight after opening: the row it lands on has to be
  # inside the viewport, whole, not scrolled past.
  echo "SMOKE_MENU_DOWN $menu_down_path"
  menu_cursor_agrees "$menu_status_down_path" "root after Down"
  local top bottom viewport
  top=$("$jq_bin" -r '.viewCursor.top' "$menu_status_down_path" 2>/dev/null)
  bottom=$("$jq_bin" -r '.viewCursor.bottom' "$menu_status_down_path" 2>/dev/null)
  viewport=$("$jq_bin" -r '.viewCursor.viewport' "$menu_status_down_path" 2>/dev/null)
  if [ -z "$top" ] || [ "$top" -lt 0 ] || [ "$bottom" -gt "$viewport" ]; then
    fail "root after Down: the cursor row spans $top..$bottom in a viewport of $viewport"
  fi
  echo "SMOKE_MENU_DOWN_RECT $top..$bottom of $viewport"
  # The second grid row fits the card, so that Down scrolls nothing: the
  # heading over the grid stays in sight.
  if [ "$(menu_field "$menu_status_down_path" scrollTop)" != "0" ]; then
    fail "root after Down: the view scrolled to $(menu_field "$menu_status_down_path" scrollTop) for a row that already fit"
  fi
  # Down through the grid's last row into the rows under it and back up to
  # the first cell: every stop whole inside the viewport.
  local f
  for f in down2 down3 down4 up1 up2 up3 up4; do
    menu_cursor_on_screen "$shot_dir/menu-status-$f.json" "root after $f"
  done
  # Up out of the rows lands on the grid's last cell, so four Ups end in
  # the grid's first row rather than on cell 0.
  local back cols
  back=$("$jq_bin" -r '.cursor' "$shot_dir/menu-status-up4.json")
  cols=$("$jq_bin" -r '.columns' "$shot_dir/menu-status-up4.json")
  if [ -z "$back" ] || [ "$back" -ge "$cols" ]; then
    fail "root: four Downs then four Ups left the cursor on $back, outside the grid's first row"
  fi
  # The emoji route: a grid, filtered, with the route's own prompt in the
  # field. More columns than one is the whole claim, and it is not readable
  # off the frame.
  if [ ! -f "$menu_emoji_path" ]; then
    fail "no emoji-grid screenshot produced"
  fi
  echo "SMOKE_MENU_EMOJI $menu_emoji_path"
  if [ ! -f "$menu_search_path" ]; then
    fail "no search-level menu screenshot produced"
  fi
  echo "SMOKE_MENU_SEARCH $menu_search_path"
  if [ ! -s "$menu_status_search_path" ]; then
    fail "no search-level menu status produced"
  fi
  cat "$menu_status_search_path"; echo
  # A root query is one list in score order under no heading: the view
  # draws no section and its rows lead in the order `debug query` ranks.
  "$jq_bin" -e '(.sections | length) == 0' "$menu_status_search_path" > /dev/null 2>&1 \
    || fail "a root query for 'e' drew headings, got: $(cat "$menu_status_search_path")"
  "$jq_bin" -e --slurpfile q "$query_path" '.ids[0:5] == ($q[0] | map(.id))[0:5]' "$menu_status_search_path" > /dev/null 2>&1 \
    || fail "a root query for 'e' is not in debug query's order: $(cat "$menu_status_search_path") vs $(cat "$query_path")"
  menu_cursor_agrees "$menu_status_search_path" "root query"
  # The field on real keys: typed, selected whole, replaced, emptied.
  "$jq_bin" -e '.query == "fire" and .caret == 4 and (.selection | not)' "$shot_dir/menu-status-typed.json" > /dev/null 2>&1 \
    || fail "typing 'fire' did not land in the query: $(cat "$shot_dir/menu-status-typed.json")"
  "$jq_bin" -e '.query == "fire" and .selection' "$shot_dir/menu-status-selected.json" > /dev/null 2>&1 \
    || fail "Ctrl+A did not select the query: $(cat "$shot_dir/menu-status-selected.json")"
  [ -f "$menu_selected_path" ] || fail "no menu-selected screenshot produced"
  echo "SMOKE_MENU_SELECTED $menu_selected_path"
  "$jq_bin" -e '.query == "term" and .caret == 4 and (.selection | not)' "$shot_dir/menu-status-replaced.json" > /dev/null 2>&1 \
    || fail "typing over the Ctrl+A selection did not replace it: $(cat "$shot_dir/menu-status-replaced.json")"
  "$jq_bin" -e '.query == "" and .caret == 0 and .isOpen' "$shot_dir/menu-status-emptied.json" > /dev/null 2>&1 \
    || fail "Shift+Home then Backspace did not empty the query: $(cat "$shot_dir/menu-status-emptied.json")"
  echo "SMOKE_MENU_FIELD fire, Ctrl+A, term, Shift+Home and Backspace"
  if [ ! -s "$menu_status_emoji_path" ]; then
    fail "no emoji menu status produced"
  fi
  cat "$menu_status_emoji_path"; echo
  if ! grep -q '"level":"emoji"' "$menu_status_emoji_path" \
    || ! grep -q '"placeholder":"Search emoji"' "$menu_status_emoji_path"; then
    fail "the emoji route did not report itself, got: $(cat "$menu_status_emoji_path")"
  fi
  # A grid draws no group headings, so it must not report any either.
  if ! grep -q '"sections":\[\]' "$menu_status_emoji_path"; then
    fail "the emoji grid claimed group headings it cannot draw, got: $(cat "$menu_status_emoji_path")"
  fi
  local emoji_columns emoji_rows
  emoji_columns=$(menu_field "$menu_status_emoji_path" columns)
  emoji_rows=$(menu_field "$menu_status_emoji_path" rows)
  if [ -z "$emoji_columns" ] || [ "$emoji_columns" -le 1 ]; then
    fail "the emoji route rendered $emoji_columns column(s), so it is still a row list"
  fi
  if [ -z "$emoji_rows" ] || [ "$emoji_rows" -le "$emoji_columns" ]; then
    fail "the filtered emoji grid holds $emoji_rows cell(s), too few to fill a row of $emoji_columns"
  fi
  echo "SMOKE_MENU_GRID $emoji_rows emoji in $emoji_columns columns"
  menu_cursor_agrees "$menu_status_emoji_path" "emoji grid"
  if [ -s "$selection_path" ] && grep -q '"cancelled":true' "$selection_path"; then
    cat "$selection_path"
  else
    [ -f "$selection_path" ] && cat "$selection_path" >&2
    fail "menu close in select mode did not write {cancelled:true}"
  fi
}
