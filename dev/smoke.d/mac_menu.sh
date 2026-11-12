# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mac-menu: a Mac app's menu in the bar, its menu card and the launcher,
# against dev/mac-stub.py: a window tagged `atoll` whose process names a
# dbusmenu to the shell's registrar, as an app under atoll does
# (docs/superpowers/specs/2026-11-11-mac-apps.md).
# With the stub focused `appmenu status` has to list its four titles, and
# `appmenu items` a disabled item, a submenu, a section heading, a
# checkmark and the key equivalents as the stub sent them. Everything after
# is a real pointer or real keys, and the stub's own --log is the evidence:
# a click on File's title opens its card and a click on New's row reaches
# the stub as `Event(21, clicked)`; a click on Export's row opens the
# submenu in place and one on PDF under it reaches the stub; a click on
# Show Ruler comes back from the stub as a checked row; a click on Window
# lands the row the stub only adds on AboutToShow in the card that is
# already open; the pointer moved onto Edit's title with Window's card up
# swaps the card for Edit's with no click, and a click on that title shuts
# it. The stub has the keyboard throughout: a menu card takes none, so the
# app stays the active one while its menu is up.
# A foot window taking focus empties the bar of titles and the root query
# of menu rows; back on the stub the root query finds PDF, and `menu summon
# appmenu` with `html` typed and Return pressed reaches the stub while the
# launcher holds the keyboard.
# Ridden by --sidebar the titles are the column's menu card, found by its
# hits in `sidebar status`, and with the column folded to its rail a click
# on the rail's one menu button opens the whole menu in a card.
leg_mac_menu_flag="--mac-menu"
leg_mac_menu_order=172
leg_mac_menu_compositors="hyprland shoji"
leg_mac_menu_needs="python3 jq foot wlrctl wtype"
leg_mac_menu_claims="screen=shared launcher=shared"

mac_menu_prefix="$shot_dir/mac-menu"
mac_menu_log="$shot_dir/mac-menu-stub.log"

leg_mac_menu_timing() {
  leg_timing 90
}

leg_mac_menu_drive() {
  local script="$shot_dir/mac-menu-drive.sh"
  local stub="$PWD/dev/mac-stub.py"
  local sidebar=0
  leg_on sidebar && sidebar=1
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$mac_menu_prefix"
log="$mac_menu_log"
jq="$jq_bin"
"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$python3_bin $stub --name Stub --log \$log]==])" > /dev/null 2>&1
until_window org.atoll.stub 15
until_jq 15 "\$m-status.json" '.titles | length == 4' call appmenu status
call appmenu items File > "\$m-file.json" 2>&1
call appmenu items Edit > "\$m-edit.json" 2>&1
until_settled
shoot "\$m-bar.png"

# Where a title sits, as "x y" in logical pixels.
if [ "$sidebar" = 1 ]; then
  call sidebar status > "\$m-sidebar.json" 2>&1
  read -r ox oy <<< "\$("$wmctl_bin" -j layers | "\$jq" -r "\$layers_jq"'bar_origin')"
  title() { "\$jq" -r --argjson ox "\$ox" --argjson oy "\$oy" --arg on "appmenu:\$1" '.hits[] | select(.on == \$on) | "\(\$ox + .x + (.w / 2 | floor)) \(\$oy + .y + (.h / 2 | floor))"' "\$m-sidebar.json" | head -n 1; }
else
  call bar room > "\$m-room.json" 2>&1
  cy=\$("\$jq" -r '.[0].cells[] | select(.name == "appMenu") | (.y + .height / 2 | floor)' "\$m-room.json" | head -n 1)
  title() { "\$jq" -r --argjson id "\$1" --argjson cy "\$cy" '.titles[] | select(.id == \$id) | "\(.x + .width / 2 | floor) \(\$cy)"' "\$m-status.json"; }
fi
# A row of the open card, off the status in \$2.
row() { "\$jq" -r --argjson id "\$1" '.rows[] | select(.id == \$id) | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"' "\$2"; }
# open <id> <path> <claim>: a real click on the title, then the status once
# the card is up with its rows laid out.
open() {
  park \$(title "\$1"); click
  up "\$@"
}
# up <id> <path> [claim]: that menu's card up and at rest, its rows where
# they stay.
up() {
  until_jq 10 "\$2" ".open == \$1 and (\${3:-.rows | length > 0})" call appmenu status
  until_settled
  call appmenu status > "\$2" 2>&1
}
shut() { until_jq 10 "\$m-shut.json" '.open == null' call appmenu status; }

open 2 "\$m-open-file.json"
shoot "\$m-file.png"
park \$(row 21 "\$m-open-file.json"); click
shut

open 2 "\$m-open-file-again.json"
park \$(row 24 "\$m-open-file-again.json"); click
up 2 "\$m-open-export.json" 'any(.rows[]; .id == 241)'
shoot "\$m-export.png"
park \$(row 241 "\$m-open-export.json"); click
shut

open 3 "\$m-open-edit.json"
park \$(row 34 "\$m-open-edit.json"); click
shut
until_jq 10 "\$m-ruler.json" 'any(.[]; .id == 34 and .checked)' call appmenu items Edit

open 4 "\$m-open-window.json" 'any(.rows[]; .id == 41)'
shoot "\$m-window.png"
# Along the bar with a menu up: no click, the pointer alone.
park \$(title 3)
up 3 "\$m-scrub.json"
shoot "\$m-edit.png"
click
shut

if [ "$sidebar" = 1 ]; then
  # Folded to the rail the titles are one button, and its card the whole
  # menu, each title a submenu of it.
  origin() { read -r ox oy <<< "\$("$wmctl_bin" -j layers | "\$jq" -r "\$layers_jq"'bar_origin')"; }
  call sidebar collapse > /dev/null 2>&1
  until_jq 10 "\$m-sidebar.json" '.collapsed and any(.hits[]; .on == "appmenu:0")' call sidebar status
  until_settled
  call sidebar status > "\$m-sidebar.json" 2>&1
  origin
  park \$(title 0); click
  up 0 "\$m-open-rail.json" 'any(.rows[]; .id == 2)'
  shoot "\$m-rail.png"
  click
  shut
  call sidebar expand > /dev/null 2>&1
  until_jq 10 "\$m-sidebar.json" '(.collapsed | not) and any(.hits[]; .on == "appmenu:2")' call sidebar status
  until_settled
  call sidebar status > "\$m-sidebar.json" 2>&1
  origin
fi

cp "\$log" "\$m-log-menus.txt"
spawn_foot formalshell-mac-menu
until_window formalshell-mac-menu
until_jq 10 "\$m-foot.json" '.titles == []' call appmenu status
call debug query "pdf" > "\$m-query-foot.json" 2>&1
until_settled
shoot "\$m-foot.png"
"$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:org.atoll.stub' })" > /dev/null 2>&1
until_jq 10 "\$m-back.json" '.titles | length == 4' call appmenu status
call debug query "pdf" > "\$m-query-root.json" 2>&1

call menu summon appmenu > /dev/null 2>&1
until_settled
"$wtype_bin" html
row_index "\$m-launcher.json" "appmenu.File/Export/HTML" > "\$m-launcher-index.txt"
call appmenu status > "\$m-held.json" 2>&1
until_settled
shoot "\$m-launcher.png"
"$wtype_bin" -k Return
for _ in \$(seq 1 20); do grep -q '^menu: Event(242, clicked)\$' "\$log" 2>/dev/null && break; sleep 0.25; done
call menu close > /dev/null 2>&1
EOF
  write_script "$shot_dir/mac-menu-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -f "$stub --name Stub --log $mac_menu_log" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/mac-menu-kill.sh"
  wm_exec_once "bash $script"
}

leg_mac_menu_assert() {
  local m="$mac_menu_prefix"
  [ -s "$m-status.json" ] || fail "--mac-menu: no appmenu status"
  cat "$m-status.json"; echo
  expect_jq "$m-status.json" '[.titles[].label] == ["Stub", "File", "Edit", "Window"] and .pid != null' \
    "--mac-menu: the focused stub's titles are not Stub, File, Edit, Window"
  if ! leg_on sidebar; then
    expect_jq "$m-status.json" 'all(.titles[]; .width > 0) and ([.titles[].x] | . == sort)' \
      "--mac-menu: the strip laid no box out for each title"
  fi
  expect_jq "$m-file.json" 'any(.[]; .label == "Save" and (.enabled | not) and .shortcut == ["Super", "S"]) and any(.[]; .label == "Export" and .submenu) and any(.[]; .separator)' \
    "--mac-menu: File does not carry its disabled Save, its Export submenu and a separator"
  expect_jq "$m-edit.json" 'any(.[]; .label == "Format" and .section) and any(.[]; .label == "Paste" and .shortcut == ["Shift", "Super", "V"]) and any(.[]; .id == 34 and (.checked | not))' \
    "--mac-menu: Edit does not carry its section, Paste's key equivalent and an unchecked Show Ruler"
  [ -s "$mac_menu_log" ] || fail "--mac-menu: the stub logged nothing"
  cat "$mac_menu_log"
  local want
  for want in 'menu: Event(21, clicked)' 'menu: Event(241, clicked)' 'menu: Event(34, clicked)' 'menu: AboutToShow(4)' 'menu: Event(242, clicked)'; do
    grep -qxF "$want" "$mac_menu_log" || fail "--mac-menu: the stub never saw '$want'"
  done
  expect_jq "$m-open-file.json" '[.rows[].id] | index(21) != null and index(25) != null' "--mac-menu: a click on File's title opened no card with its rows"
  expect_jq "$m-ruler.json" 'any(.[]; .id == 34 and .checked)' "--mac-menu: the stub's checkmark did not come back after a click on Show Ruler"
  expect_jq "$m-open-window.json" 'any(.rows[]; .id == 41)' "--mac-menu: the row the stub adds on AboutToShow did not land in the open card"
  expect_jq "$m-scrub.json" '.open == 3' "--mac-menu: the pointer on Edit's title with Window's card up did not swap the card"
  expect_jq "$m-shut.json" '.open == null' "--mac-menu: a click on the open menu's own title left the card up"
  if leg_on sidebar; then
    expect_jq "$m-open-rail.json" '.open == 0 and ([.rows[].id] | index(1) != null and index(4) != null)' \
      "--mac-menu: a click on the rail's menu button opened no card holding every title"
    [ -f "$m-rail.png" ] || fail "--mac-menu: no rail frame"
    echo "SMOKE_MAC_MENU_RAIL $m-rail.png"
  fi
  if grep -qx 'focus out' "$m-log-menus.txt" 2>/dev/null; then
    fail "--mac-menu: the stub lost the keyboard while its menus were up"
  fi
  expect_jq "$m-foot.json" '.titles == [] and .pid == null' "--mac-menu: a foot window in front still has a menu"
  if "$jq_bin" -e '[.. | strings] | any(startswith("appmenu."))' "$m-query-foot.json" > /dev/null 2>&1; then
    fail "--mac-menu: the root query lists menu rows with a foot window in front: $(cat "$m-query-foot.json")"
  fi
  expect_jq "$m-query-root.json" 'any(.[]; .id == "appmenu.File/Export/PDF")' "--mac-menu: the root query does not find PDF with the stub in front"
  [ -n "$(cat "$m-launcher-index.txt" 2>/dev/null)" ] || fail "--mac-menu: the menu route never listed HTML: $(cat "$m-launcher.json" 2>/dev/null)"
  expect_jq "$m-held.json" '.titles | length == 4' "--mac-menu: the menu went with the launcher holding the keyboard"
  local shot
  for shot in bar file export window edit foot launcher; do
    [ -f "$m-$shot.png" ] || fail "--mac-menu: no $shot frame"
    echo "SMOKE_MAC_MENU_${shot^^} $m-$shot.png"
  done
}
