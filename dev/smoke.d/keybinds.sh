# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --keybinds: the launcher's parsed-binds route. The fixture is a real set of
# `hl.bind` calls written into this session's own hyprland.lua, and the route
# answers `hyprctl binds`, so what is asserted is that hyprland's own expanded
# bind table came back as rows carrying the fixture's chords and their
# descriptions. A Lua bind reports the anonymous `__lua` dispatcher, so the
# description is the whole action text a row has.
#
# Two passes on the query: the hyprctl Process is async, so the first pass
# can land before it has exited. The rows
# are inert notes by construction (a bind acts on whatever has focus, and at
# Enter that is the launcher), so there is no activation to drive.
#
# The fixture carries one commented-out bind, which must NOT come back as a
# live row, and a description holding "//" and braces, which must survive
# hyprland's config parser and the JSON round trip intact. An exact row count
# is what makes the commented-out assertion mean anything: the scaffold's
# base config declares no binds at all, so these are the whole table.
#
# The plain table, never `-j`: Hyprland 0.56.0's JSON encoder puts every value
# from `modmask` on under the PREVIOUS key's name (the fixture's modmask 65
# comes back as `"submap": "65"`, its key `slash` as `"keycode": slash`
# unquoted) and emits `allow_input_capture` with no value at all, so the reply
# is not JSON and JSON.parse throws on it. keybinds.js reads the text table
# for that reason, and this leg dumps the same table beside the route's rows.
leg_keybinds_flag="--keybinds"
leg_keybinds_order=240

# Hyprland's own bind table, dumped beside the shell's rows: it separates a
# compositor that never registered the fixture from a route that failed to
# read one that did.
keybinds_hyprctl_plain_path="$shot_dir/keybinds-hyprctl-plain.txt"
keybinds_query1_path="$shot_dir/keybinds-query-1.json"
keybinds_query2_path="$shot_dir/keybinds-query-2.json"
keybinds_menu_status_path="$shot_dir/keybinds-menu-status.json"
keybinds_menu_png="$shot_dir/keybinds-menu.png"

# This leg's own clock: the launcher covers the whole output, so under
# --wallpaper it starts after that leg's last frame.
keybinds_t0() {
  cover_t0 4
}

leg_keybinds_timing() {
  local t0
  t0=$(keybinds_t0)
  leg_timing $((t0 + 8)) $((t0 + 38))
}

leg_keybinds_drive() {
  local t0 script="$shot_dir/keybinds-drive.sh"
  t0=$(keybinds_t0)
  # The fixture itself. hyprland reads this config, so these are real binds in
  # a real bind table, not a file the shell is pointed at.
  echo 'hl.bind("SUPER + SHIFT + slash", hl.dsp.exec_cmd("hyprctl version"), { description = "exec hyprctl version" })'
  echo 'hl.bind("SUPER + T", hl.dsp.exec_cmd("ghostty"), { description = "Open a Terminal" })'
  echo 'hl.bind("SUPER + Q", hl.dsp.window.close(), { description = "killactive" })'
  echo 'hl.bind("SUPER + CTRL + 1", hl.dsp.focus({ workspace = 1 }), { description = "workspace 1" })'
  echo 'hl.bind("SUPER + N", hl.dsp.exec_cmd("notify-send"), { description = "exec notify-send {braces} // not-a-comment" })'
  echo '-- hl.bind("SUPER + Z", hl.dsp.exit())'
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
"$hyprctl_bin" binds > "$keybinds_hyprctl_plain_path" 2>&1
$ipc call debug query ":k " > /dev/null 2>&1
sleep 1
$ipc call debug query ":k " > "$keybinds_query1_path" 2>&1
$ipc call debug query ":k SUPER+T" > "$keybinds_query2_path" 2>&1
$ipc call menu summon keybinds > /dev/null 2>&1
sleep 2
$ipc call menu status > "$keybinds_menu_status_path" 2>&1
"$grim_bin" "$keybinds_menu_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_keybinds_assert() {
  local f row rows
  for f in "$keybinds_hyprctl_plain_path" \
    "$keybinds_query1_path" "$keybinds_query2_path" "$keybinds_menu_status_path"; do
    [ -s "$f" ] || fail "no keybinds artifact produced at $f"
  done
  # The compositor's own table first. Everything below is about what the route
  # made of it, and none of it means anything until the fixture is really in
  # there and really readable.
  cat "$keybinds_hyprctl_plain_path"
  if [ "$(grep -c '^bind[a-z]*$' "$keybinds_hyprctl_plain_path" | tr -d ' ')" != "5" ]; then
    fail "hyprland's own bind table does not hold the fixture's 5 binds: $(cat "$keybinds_hyprctl_plain_path")"
  fi
  cat "$keybinds_query1_path"; echo
  # Both are real answers elsewhere; here they would mean the route never got
  # hyprland's bind table, which is the one thing this leg exists to prove.
  for row in keybinds.nobinds keybinds.failed; do
    if grep -qF "\"id\":\"$row\"" "$keybinds_query1_path"; then
      fail "keybinds route answered $row: hyprland's bind table was not what it read"
    fi
  done
  # Case-insensitive on the chord: hyprland reports a bind's key back the way
  # the config spelled it, and the leg's claim is the bind, not its casing.
  if ! grep -qiF '"label":"SUPER+SHIFT+slash' "$keybinds_query1_path" \
    || ! grep -qF '"desc":"exec hyprctl version"' "$keybinds_query1_path"; then
    fail "keybinds route did not parse the fixture's first bind: $(cat "$keybinds_query1_path")"
  fi
  if ! grep -qF '"desc":"killactive"' "$keybinds_query1_path" \
    || ! grep -qF '"desc":"workspace 1"' "$keybinds_query1_path"; then
    fail "keybinds route lost a described bind: $(cat "$keybinds_query1_path")"
  fi
  # Survives hyprland's own config parser and the round trip out of it.
  if ! grep -qF '"desc":"exec notify-send {braces} // not-a-comment"' "$keybinds_query1_path"; then
    fail "the fixture's brace-and-slash argument did not survive intact: $(cat "$keybinds_query1_path")"
  fi
  if grep -qiF 'SUPER+Z' "$keybinds_query1_path"; then
    fail "a commented-out bind rendered as a live keybind row: $(cat "$keybinds_query1_path")"
  fi
  rows=$(grep -o '"kind":"note"' "$keybinds_query1_path" | wc -l | tr -d ' ' || true)
  if [ "$rows" != "5" ]; then
    fail "keybinds route returned $rows rows, expected the fixture's 5 live binds"
  fi
  cat "$keybinds_query2_path"; echo
  if ! grep -qiF '"label":"SUPER+T' "$keybinds_query2_path" \
    || ! grep -qF '"desc":"Open a Terminal"' "$keybinds_query2_path"; then
    fail "':k SUPER+T' did not rank the fixture's own chord with its action: $(cat "$keybinds_query2_path")"
  fi
  if ! grep -qF '"isOpen":true' "$keybinds_menu_status_path" \
    || ! grep -qF '"level":"keybinds"' "$keybinds_menu_status_path"; then
    fail "menu did not summon to the keybinds route: $(cat "$keybinds_menu_status_path")"
  fi
  [ -f "$keybinds_menu_png" ] || fail "no keybinds screenshot produced at $keybinds_menu_png"
  echo "SMOKE_KEYBINDS $keybinds_menu_png ($rows parsed rows)"
}
