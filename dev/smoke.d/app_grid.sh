# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --app-grid: the launcher's app results drawn as a grid of icons with the
# app's name under each one (`menu.appGrid: true`), and the row list it is
# with the key absent.
#
# The session's XDG_DATA_DIRS is the isolated HOME alone, so the installed-app
# list is exactly the seven entries staged below and every count here is a real
# number rather than whatever the VM happens to ship. One of them carries a
# solid green icon nothing else in the palette comes near, which is how the
# frame is read: at 64px the cell's icon covers thousands of pixels of that
# colour, where the same entry in a row list covers a couple of hundred. That
# is the whole claim a screenshot can make about a grid, and `menu status`'s
# `view` and `columns` make the rest.
#
# Real keystrokes rather than IPC for the cursor: Right has to move it by one
# cell and Down by a column count, and only a Qt key event through the
# compositor proves the grid answers the arrows at all (the same reason
# --panel-keys exists). `cursor` on `menu status` is what reads the answer.
#
# The key's default is the other half of the leg: rows (launcher spec §1
# "The app grid"). The shell re-reads settings.json on a write, so the key
# is deleted underneath the running shell, the launcher resummoned, and the
# level has to come back as one column of rows, each app's icon at the
# row's 22px; then it is put back, which leaves the grid up for the run's
# own frame. Ridden by
# --no-morph the grid is also read for sitting inside its own card: the
# popover emerge cuts nothing at the card's edge, so a view scrolled to a
# negative offset paints its cells up the output instead of being clipped
# away.
#
# The typing half (M72 T1), on real keys where it can be: after a re-rank
# and a real Backspace the cursor's row and the row the view holds its
# current item on have to be the same one (`cursorId` against `viewCursor`);
# a query no app matches keeps the grid as the level's view with no rows in
# it rather than swapping to the row list; Escape on a query clears it and
# leaves the launcher open; and Backspace held on an empty field for longer
# than the key repeat delay climbs one level, never every level and then
# out of the launcher.
#
# The icon a cell draws is the file sized for it: one entry's icon exists as
# a 48px file in a Scale=3 Scalable directory that claims 24 to 69 pixels
# (elementary-colloid's `apps@3x/16`, which the spec's size distance puts at
# 0 for a 64px cell) and as a 128px file, each its own colour. The cell has
# to carry the 128px file's colour and none of the 48px one's, cropped out
# to `app-grid-crisp.png` for reading.
leg_app_grid_flag="--app-grid"
leg_app_grid_order=26
leg_app_grid_needs="convert wtype jq"

# This leg's own clock: the launcher covers the whole output, so under
# --wallpaper it starts after that leg's last frame, the rule menu_t0 draws.
app_grid_t0() {
  if leg_on wallpaper; then echo 16; else echo 3; fi
}

app_grid_dir="$iso_home/.local/share/applications"
app_grid_icon_dir="$iso_home/.local/share/icons/hicolor"
app_grid_marker="$shot_dir/app-grid-launched.txt"
app_grid_settings="$iso_home/.config/formalshell/settings.json"
app_grid_settings_on="$shot_dir/app-grid-settings-on.json"
app_grid_settings_off="$shot_dir/app-grid-settings-off.json"

app_grid_png="$shot_dir/app-grid.png"
app_grid_mixed_png="$shot_dir/app-grid-mixed.png"
app_grid_rows_png="$shot_dir/app-grid-rows.png"
app_grid_open_path="$shot_dir/app-grid-status-open.json"
app_grid_right_path="$shot_dir/app-grid-status-right.json"
app_grid_down_path="$shot_dir/app-grid-status-down.json"
app_grid_narrow_path="$shot_dir/app-grid-status-narrow.json"
app_grid_mixed_path="$shot_dir/app-grid-status-mixed.json"
app_grid_off_path="$shot_dir/app-grid-status-off.json"
app_grid_probe_path="$shot_dir/app-grid-probe.json"
app_grid_mixed_query_path="$shot_dir/app-grid-mixed-query.json"
app_grid_typed_path="$shot_dir/app-grid-status-typed.json"
app_grid_backspace_path="$shot_dir/app-grid-status-backspace.json"
app_grid_empty_path="$shot_dir/app-grid-status-empty.json"
app_grid_escape_path="$shot_dir/app-grid-status-escape.json"
app_grid_hold_path="$shot_dir/app-grid-status-hold.json"

# The icon the frame is read by, and the one every other entry wears so the
# green count is one cell's worth and not five.
app_grid_mark="#2F9E44"
app_grid_other="#2A6FAE"
# The sized entry's 128px file and its 48px one.
app_grid_large="#7048E8"
app_grid_small="#E8590C"
app_grid_crisp_png="$shot_dir/app-grid-crisp.png"

# Everything above the card's own top row (the launcher's top fraction, 0.3 of a
# 1080 output, the same number --menu-emerge states). Nothing the launcher
# draws reaches it.
app_grid_above_card="1920x324+0+0"

app_grid_entry() {
  local file="$1" name="$2" icon="$3" exec_line="$4"
  {
    echo "[Desktop Entry]"
    echo "Type=Application"
    echo "Name=$name"
    echo "Exec=$exec_line"
    if [ -n "$icon" ]; then echo "Icon=$icon"; fi
  } > "$app_grid_dir/$file"
}

leg_app_grid_fixture() {
  settings_fragment ', "menu": {"appGrid": true}'

  mkdir -p "$app_grid_icon_dir/48x48/apps" "$app_grid_icon_dir/128x128/apps"
  local size
  for size in 48 128; do
    $convert_bin -size "${size}x${size}" "xc:$app_grid_mark" \
      "$app_grid_icon_dir/${size}x${size}/apps/formalshell-app-grid-mark.png"
    $convert_bin -size "${size}x${size}" "xc:$app_grid_other" \
      "$app_grid_icon_dir/${size}x${size}/apps/formalshell-app-grid-plain.png"
  done
  mkdir -p "$app_grid_icon_dir/16x16@3/apps"
  $convert_bin -size 48x48 "xc:$app_grid_small" "$app_grid_icon_dir/16x16@3/apps/formalshell-app-grid-sized.png"
  $convert_bin -size 128x128 "xc:$app_grid_large" "$app_grid_icon_dir/128x128/apps/formalshell-app-grid-sized.png"
  # QIconLoader enumerates no directories at all without an index.theme, so a
  # bare PNG in the right place still resolves to no icon.
  cat > "$app_grid_icon_dir/index.theme" <<'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=16x16@3/apps,48x48/apps,128x128/apps

[16x16@3/apps]
Size=16
Scale=3
MinSize=8
MaxSize=23
Context=Applications
Type=Scalable

[48x48/apps]
Size=48
Context=Applications
Type=Threshold

[128x128/apps]
Size=128
Context=Applications
Type=Threshold
EOF

  # Seven entries: the launch probe under a name nothing else here matches, four
  # ordinary ones so the grid wraps past its own column count, and one long
  # name with no icon at all, which is both the elision case and the honest
  # no-icon cell, and the sized one last.
  app_grid_entry "formalshell-probe.desktop" "Grid Launch Probe" \
    "formalshell-app-grid-mark" "touch $app_grid_marker"
  app_grid_entry "formalshell-files.desktop" "Files Fixture" \
    "formalshell-app-grid-plain" "true"
  app_grid_entry "formalshell-mail.desktop" "Mail Fixture" \
    "formalshell-app-grid-plain" "true"
  app_grid_entry "formalshell-music.desktop" "Music Fixture" \
    "formalshell-app-grid-plain" "true"
  app_grid_entry "formalshell-photos.desktop" "Photos Fixture" \
    "formalshell-app-grid-plain" "true"
  app_grid_entry "formalshell-long.desktop" "A Very Long Fixture Application Name" \
    "" "true"
  app_grid_entry "formalshell-sized.desktop" "Sized Fixture" \
    "formalshell-app-grid-sized" "true"
}

leg_app_grid_timing() {
  local t0
  t0=$(app_grid_t0)
  leg_timing $((t0 + 37)) $((t0 + 72))
}

leg_app_grid_drive() {
  local t0 script="$shot_dir/app-grid-drive.sh"
  t0=$(app_grid_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
cp "$app_grid_settings" "$app_grid_settings_on"
$ipc call menu summon apps > /dev/null 2>&1
sleep 2
"$grim_bin" "$app_grid_png" > /dev/null 2>&1
$ipc call menu status > "$app_grid_open_path" 2>&1
"$wtype_bin" -k Right
sleep 1
$ipc call menu status > "$app_grid_right_path" 2>&1
"$wtype_bin" -k Down
sleep 1
$ipc call menu status > "$app_grid_down_path" 2>&1
# A token only the probe entry carries, so the narrowed grid is one cell and
# the cursor Enter lands on is unambiguous.
$ipc call menu filter probe > /dev/null 2>&1
sleep 1
$ipc call debug query probe > "$app_grid_probe_path" 2>&1
$ipc call menu status > "$app_grid_narrow_path" 2>&1
"$wtype_bin" -k Return
sleep 2
# A root query that ranks an app alongside routes: the grid takes the apps
# and the rest draw as rows under it.
$ipc call menu summon "" > /dev/null 2>&1
sleep 1
$ipc call menu filter app > /dev/null 2>&1
sleep 2
"$grim_bin" "$app_grid_mixed_png" > /dev/null 2>&1
$ipc call menu status > "$app_grid_mixed_path" 2>&1
$ipc call debug query app > "$app_grid_mixed_query_path" 2>&1
# Down onto the row under the grid queues its reveal for the next tick, and a
# new query inside that tick rebuilds the rows it was queued from. Verify reads
# the shell's own log for what a reveal evaluated after its row was gone says.
"$wtype_bin" -k Down
$ipc call menu filter ap > /dev/null 2>&1
"$wtype_bin" -k Down
sleep 1
$ipc call menu status > "$app_grid_typed_path" 2>&1
"$wtype_bin" -k BackSpace
sleep 1
$ipc call menu status > "$app_grid_backspace_path" 2>&1
$ipc call menu filter zzzz > /dev/null 2>&1
sleep 1
$ipc call menu status > "$app_grid_empty_path" 2>&1
"$wtype_bin" -k Escape
sleep 1
$ipc call menu status > "$app_grid_escape_path" 2>&1
$ipc call menu summon apps > /dev/null 2>&1
sleep 1
"$wtype_bin" -P BackSpace -s 1500 -p BackSpace
sleep 1
$ipc call menu status > "$app_grid_hold_path" 2>&1
$ipc call menu close > /dev/null 2>&1
# The key deleted underneath the running shell. Written back through the same
# inode rather than moved over, so the config watch sees a write and not a
# file it is no longer holding.
"$jq_bin" 'del(.menu.appGrid)' "$app_grid_settings" > "$app_grid_settings_off" 2>&1
cat "$app_grid_settings_off" > "$app_grid_settings"
sleep 2
$ipc call menu summon apps > /dev/null 2>&1
sleep 2
"$grim_bin" "$app_grid_rows_png" > /dev/null 2>&1
$ipc call menu status > "$app_grid_off_path" 2>&1
$ipc call menu close > /dev/null 2>&1
cat "$app_grid_settings_on" > "$app_grid_settings"
sleep 2
$ipc call menu summon apps > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# How many pixels of the probe entry's own colour one box of the frame
# carries, the whole frame with no box named. Two passes rather than one:
# everything off that colour goes black first, so a palette that is already
# near-white somewhere else cannot be counted as a match by the second.
app_grid_mark_pixels() {
  app_grid_colour_pixels "$1" "$app_grid_mark" "${2:-}"
}

app_grid_colour_pixels() {
  $convert_bin "$1" -crop "${3:-100%x100%+0+0}" +repage \
    -fuzz 12% -fill black +opaque "$2" \
    -fuzz 12% -fill white -opaque "$2" \
    -colorspace Gray -format '%[fx:int(mean*w*h+0.5)]' info: 2>/dev/null
}

# The box the sized entry's 128px colour covers, as a crop geometry.
app_grid_large_box() {
  $convert_bin "$1" -fuzz 12% -fill black +opaque "$app_grid_large" \
    -fuzz 12% -fill white -opaque "$app_grid_large" -format '%@' info: 2>/dev/null
}

app_grid_field() {
  "$jq_bin" -r ".$2 // empty" "$1" 2>/dev/null
}

app_grid_cursor_agrees() {
  local f="$1" what="$2" id view_id index view_index
  id=$(app_grid_field "$f" cursorId)
  view_id=$(app_grid_field "$f" viewCursor.id)
  index=$(app_grid_field "$f" cursor)
  view_index=$(app_grid_field "$f" viewCursor.index)
  if [ -z "$id" ] || [ "$id" != "$view_id" ] || [ "$index" != "$view_index" ]; then
    fail "$what: the cursor is on row $index ($id) but the view holds $view_index (${view_id:-nothing})"
  fi
  echo "SMOKE_APP_GRID_CURSOR $what: row $index ($id)"
}

leg_app_grid_assert() {
  local f
  for f in "$app_grid_open_path" "$app_grid_right_path" "$app_grid_down_path" \
    "$app_grid_narrow_path" "$app_grid_mixed_path" "$app_grid_off_path" \
    "$app_grid_probe_path" "$app_grid_mixed_query_path" "$app_grid_typed_path" \
    "$app_grid_backspace_path" "$app_grid_empty_path" "$app_grid_escape_path" \
    "$app_grid_hold_path"; do
    [ -s "$f" ] || fail "no app-grid artifact produced at $f"
  done
  for f in "$app_grid_png" "$app_grid_mixed_png" "$app_grid_rows_png"; do
    [ -f "$f" ] || fail "no app-grid screenshot produced at $f"
  done

  cat "$app_grid_open_path"; echo
  local columns rows cursor
  if [ "$(app_grid_field "$app_grid_open_path" view)" != "appGrid" ]; then
    fail "the apps route did not report the grid, got: $(cat "$app_grid_open_path")"
  fi
  columns=$(app_grid_field "$app_grid_open_path" columns)
  rows=$(app_grid_field "$app_grid_open_path" rows)
  if [ -z "$columns" ] || [ "$columns" -le 1 ]; then
    fail "the app grid rendered ${columns:-0} column(s), so it is still a row list"
  fi
  if [ -z "$rows" ] || [ "$rows" -lt 7 ]; then
    fail "the apps route listed ${rows:-0} entries, fewer than the seven staged: $(cat "$app_grid_open_path")"
  fi
  # A grid draws no group headings, so it must not report any either.
  if ! grep -q '"sections":\[\]' "$app_grid_open_path"; then
    fail "the app grid claimed group headings it cannot draw, got: $(cat "$app_grid_open_path")"
  fi
  echo "SMOKE_APP_GRID $app_grid_png ($rows apps in $columns columns)"

  # The icon at cell size. A row list draws the same entry at the body font's
  # own height, which is two orders of magnitude fewer pixels, so this number
  # is what says the frame is a grid of icons rather than a list carrying
  # them.
  local marked
  marked=$(app_grid_mark_pixels "$app_grid_png")
  if [ -z "$marked" ] || [ "$marked" -lt 2500 ]; then
    fail "the probe entry's icon covers ${marked:-0} pixels of the grid, too few to be a 64px cell icon"
  fi
  echo "SMOKE_APP_GRID_ICON ${marked}px of the probe entry's own colour"

  # The sized entry's cell wears the 128px file, resampled down, and not the
  # 48px one a size distance of 0 would have picked and blown up.
  local large small box
  large=$(app_grid_colour_pixels "$app_grid_png" "$app_grid_large")
  small=$(app_grid_colour_pixels "$app_grid_png" "$app_grid_small")
  if [ -z "$large" ] || [ "$large" -lt 2500 ] || [ -z "$small" ] || [ "$small" -gt 0 ]; then
    fail "the sized entry drew ${large:-0}px of its 128px file and ${small:-0}px of its 48px one"
  fi
  box=$(app_grid_large_box "$app_grid_png")
  local bw bh bx by
  IFS='x+' read -r bw bh bx by <<< "$box"
  $convert_bin "$app_grid_png" -crop "$((bw + 48))x$((bh + 64))+$((bx - 24))+$((by - 24))" +repage "$app_grid_crisp_png" 2>/dev/null
  echo "SMOKE_APP_GRID_CRISP $app_grid_crisp_png (${large}px of the 128px file over $box, none of the 48px one)"

  # The arrows: one cell right, then a whole row of cells down.
  cat "$app_grid_right_path"; echo
  cursor=$(app_grid_field "$app_grid_right_path" cursor)
  if [ "$cursor" != "1" ]; then
    fail "Right moved the grid cursor to ${cursor:-none}, not to the next cell"
  fi
  cat "$app_grid_down_path"; echo
  cursor=$(app_grid_field "$app_grid_down_path" cursor)
  if [ "$cursor" != "$((1 + columns))" ]; then
    fail "Down moved the grid cursor to ${cursor:-none}, not one row of $columns cells on from 1"
  fi

  # Seven cells over two rows already fit the card, so there is nothing for
  # either arrow to scroll and the view has to stay where it was. A reveal
  # queued for a footer row that a query rebuilt inside the same tick used
  # to write the card's own offset into the grid here instead.
  local scroll
  for f in "$app_grid_open_path" "$app_grid_right_path" "$app_grid_down_path"; do
    scroll=$(app_grid_field "$f" scrollTop)
    if [ "$scroll" != "0" ]; then
      fail "the grid sat at scrollTop ${scroll:-none} with nothing to scroll, from $f"
    fi
  done

  # A typed query narrows the grid to the one entry that carries the token,
  # and the level is still a grid rather than falling back to rows.
  cat "$app_grid_narrow_path"; echo
  if [ "$(app_grid_field "$app_grid_narrow_path" view)" != "appGrid" ] \
    || [ "$(app_grid_field "$app_grid_narrow_path" rows)" != "1" ]; then
    fail "'probe' did not narrow the grid to the one entry, got: $(cat "$app_grid_narrow_path")"
  fi
  cat "$app_grid_probe_path"; echo
  if [ "$("$jq_bin" -r '.[0].label // ""' "$app_grid_probe_path")" != "Grid Launch Probe" ]; then
    fail "'probe' did not rank the probe entry first: $(cat "$app_grid_probe_path")"
  fi
  # Enter on the cell runs the entry's own Exec through the same path a row
  # takes, and the entry's Exec is the only thing that can write this file.
  if [ ! -f "$app_grid_marker" ]; then
    fail "Enter on the cursor cell did not launch the probe entry (no $app_grid_marker)"
  fi
  echo "SMOKE_APP_GRID_LAUNCH $app_grid_marker"

  # Mixed: the apps take the grid, everything else the query ranked draws as
  # rows under it.
  cat "$app_grid_mixed_path"; echo
  if [ "$(app_grid_field "$app_grid_mixed_path" view)" != "appGrid" ]; then
    fail "a root query ranking apps did not draw the grid, got: $(cat "$app_grid_mixed_path")"
  fi
  cat "$app_grid_mixed_query_path"; echo
  local app_hits other_hits
  app_hits=$("$jq_bin" '[.[] | select(.kind == "app")] | length' "$app_grid_mixed_query_path" 2>/dev/null)
  other_hits=$("$jq_bin" '[.[] | select(.kind != "app")] | length' "$app_grid_mixed_query_path" 2>/dev/null)
  if [ -z "$app_hits" ] || [ "$app_hits" -lt 1 ] || [ -z "$other_hits" ] || [ "$other_hits" -lt 1 ]; then
    fail "'app' ranked ${app_hits:-0} app(s) and ${other_hits:-0} other row(s), so nothing here is mixed"
  fi
  echo "SMOKE_APP_GRID_MIXED $app_grid_mixed_png ($app_hits in the grid, $other_hits as rows)"

  app_grid_cursor_agrees "$app_grid_open_path" "apps route"
  app_grid_cursor_agrees "$app_grid_down_path" "after Right, Down"
  app_grid_cursor_agrees "$app_grid_mixed_path" "root query"
  cat "$app_grid_typed_path"; echo
  app_grid_cursor_agrees "$app_grid_typed_path" "re-ranked, then Down"
  cat "$app_grid_backspace_path"; echo
  app_grid_cursor_agrees "$app_grid_backspace_path" "a real Backspace"

  # No app matches: the level keeps its grid, with nothing in it.
  cat "$app_grid_empty_path"; echo
  if [ "$(app_grid_field "$app_grid_empty_path" view)" != "appGrid" ] \
    || [ "$(app_grid_field "$app_grid_empty_path" rows)" != "0" ]; then
    fail "a query no app matches swapped the view or kept rows, got: $(cat "$app_grid_empty_path")"
  fi
  # Escape on a query clears it: still open, still at the root, the root's
  # own rows back.
  cat "$app_grid_escape_path"; echo
  if [ "$(app_grid_field "$app_grid_escape_path" isOpen)" != "true" ] \
    || [ -n "$(app_grid_field "$app_grid_escape_path" level)" ] \
    || [ "$(app_grid_field "$app_grid_escape_path" rows)" -lt 1 ]; then
    fail "Escape on a query did not just clear it, got: $(cat "$app_grid_escape_path")"
  fi
  app_grid_cursor_agrees "$app_grid_escape_path" "query cleared"
  # A held Backspace on the apps route's empty field: one level up.
  cat "$app_grid_hold_path"; echo
  if [ "$(app_grid_field "$app_grid_hold_path" isOpen)" != "true" ] \
    || [ -n "$(app_grid_field "$app_grid_hold_path" level)" ]; then
    fail "a held Backspace did not stop at the root, got: $(cat "$app_grid_hold_path")"
  fi
  echo "SMOKE_APP_GRID_KEYS empty query kept the grid, Escape cleared, a held Backspace climbed one level"

  # The default: with the key gone the route is rows, and every entry the
  # grid had is still on it, the probe's icon at row size rather than cell
  # size.
  cat "$app_grid_off_path"; echo
  if [ "$(app_grid_field "$app_grid_off_path" rows)" != "$rows" ]; then
    fail "the default route lost entries the grid had, got: $(cat "$app_grid_off_path")"
  fi
  marked=$(app_grid_mark_pixels "$app_grid_rows_png")
  if [ "$(app_grid_field "$app_grid_off_path" view)" != "rows" ] \
    || [ "$(app_grid_field "$app_grid_off_path" columns)" != "1" ]; then
    fail "the apps route did not default to rows, got: $(cat "$app_grid_off_path")"
  fi
  if [ -z "$marked" ] || [ "$marked" -lt 100 ] || [ "$marked" -ge 2500 ]; then
    fail "the default rows drew the probe icon over ${marked:-0} pixels, not a 22px row icon"
  fi
  if leg_on no_morph; then
    # It drew inside its own card. The popover cuts nothing at the card's
    # edge, so a grid scrolled to a negative offset paints its cells up the
    # output rather than being clipped away: the band above the card's top
    # row is where they land, and the probe's own colour is what would be
    # in it.
    local above
    above=$(app_grid_mark_pixels "$app_grid_rows_png" "$app_grid_above_card")
    if [ -z "$above" ] || [ "$above" -gt 0 ]; then
      fail "${above:-0} pixels of the grid's own icon sit above the card's top row ($app_grid_above_card)"
    fi
  fi
  echo "SMOKE_APP_GRID_DEFAULT rows with no key set (${marked}px of the probe icon)"
}
