# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mac-app: a real Mac app under atoll, used through the shell
# (docs/superpowers/specs/2026-11-11-mac-apps.md). Where --mac-menu and
# --mac-keys drive dev/mac-stub.py, this one runs an atoll build on a real
# bundle, so it needs both on the slot and is not in dev/smoke-all.list:
# FS_ATOLL is the atoll binary (~/atoll/target/debug/atoll, where atoll's
# scripts/g815 builds it), FS_MAC_APP the bundle (CotEditor out of atoll's
# fixtures) and FS_MAC_APP_ID its bundle id, which is its windows' class.
# atoll runs with ATOLL_LOG=atoll::menu=debug, which prints a line per
# action a menu sends, `key equivalent "Copy" copy:` for a key and `chosen
# "Copy" copy:` for an item the shell ran; that log, the marks the session's
# own SUPER binds leave and the frames are the evidence. Every key is
# wtype's and every click wlrctl's.
# With the app in front the strip has to carry its real titles and the
# session the `mac` submap. Text is typed into its document; SUPER + A,
# SUPER + C, Right and SUPER + V twice have to reach it as Select All, Copy
# and Paste, with the session's own SUPER + C and SUPER + V binds leaving no
# mark; CTRL + C has to send no Copy. A click on Edit's title opens its
# card, one on Select opens the submenu in place, one on Select All under
# it reaches the app. `menu summon appmenu` with `delete` typed and Return
# runs Edit's Delete while the launcher holds the keyboard. SUPER + L moves
# focus to a foot window and the submap is the default one; SUPER + H comes
# back. The pointer parked on the bottom edge brings the dock out with the
# app in it as a Mac app, a right click on its icon opens its menu with
# its window and Quit, and a click on the window's row shuts it. SUPER + Q
# reaches the app as Quit with the session's SUPER + Q bind leaving no
# mark.
leg_mac_app_flag="--mac-app"
leg_mac_app_order=174
leg_mac_app_needs="jq foot wlrctl wtype"
leg_mac_app_claims="screen=shared launcher=shared hyprland.binds"
leg_mac_app_env="FS_ATOLL FS_MAC_APP FS_MAC_APP_ID"

mac_app_prefix="$shot_dir/mac-app"
mac_app_log="$shot_dir/mac-app-atoll.log"
mac_app_marks="$shot_dir/mac-app-marks.txt"
mac_app_atoll="${FS_ATOLL:-$HOME/atoll/target/debug/atoll}"
mac_app_bundle="${FS_MAC_APP:-$HOME/atoll/fixtures/out/coteditor/CotEditor.app}"
mac_app_id="${FS_MAC_APP_ID:-com.coteditor.CotEditor}"

leg_mac_app_timing() {
  leg_timing 150
}

leg_mac_app_drive() {
  # dev/smoke.sh's fail() is not defined yet where drives are recorded.
  [ -x "$mac_app_atoll" ] || { echo "SMOKE_FAIL: --mac-app: no atoll at $mac_app_atoll (FS_ATOLL)" >&2; exit 1; }
  [ -d "$mac_app_bundle" ] || { echo "SMOKE_FAIL: --mac-app: no bundle at $mac_app_bundle (FS_MAC_APP)" >&2; exit 1; }
  awk '/^-- MAC APPS/ { on = 1 } /^-- Window rounding/ { on = 0 } on { print }' docs/examples/hyprland/formalshell.lua
  cat <<EOF
hl.config({ input = { resolve_binds_by_sym = true } })
hl.bind("SUPER + L", hl.dsp.focus({ window = "class:formalshell-mac-app" }))
hl.bind("SUPER + H", hl.dsp.focus({ window = "class:$mac_app_id" }))
hl.bind("SUPER + C", hl.dsp.exec_cmd("sh -c 'echo c >> $mac_app_marks'"))
hl.bind("SUPER + V", hl.dsp.exec_cmd("sh -c 'echo v >> $mac_app_marks'"))
hl.bind("SUPER + Q", hl.dsp.exec_cmd("sh -c 'echo q >> $mac_app_marks'"))
EOF
  local script="$shot_dir/mac-app-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$mac_app_prefix"
log="$mac_app_log"
marks="$mac_app_marks"
jq="$jq_bin"
app="$mac_app_id"
: > "\$marks"
active() { "$hyprctl_bin" -j activewindow 2>/dev/null | "\$jq" -r '.class // ""'; }
# front <class>: until that window is the active one, 10s at most.
front() { for _ in \$(seq 1 40); do [ "\$(active)" = "\$1" ] && return 0; sleep 0.25; done; return 1; }
# seen <fixed text> <count> <path> [tries]: how many of atoll's lines carry
# it, once <count> of them have landed or the tries are up (40 of them,
# 10s), and a moment more for one too many. A real app under load takes its
# first keys late.
seen() {
  for _ in \$(seq 1 "\${4:-40}"); do [ "\$(grep -cF "\$1" "\$log" 2>/dev/null)" -ge "\$2" ] && break; sleep 0.25; done
  sleep 0.4
  grep -cF "\$1" "\$log" > "\$3" 2>/dev/null || true
}
mark() { sleep 0.4; grep -c "^\$1\\\$" "\$marks" > "\$2" 2>/dev/null || true; }
key() { "$wtype_bin" -M logo "\$1" -m logo; }

spawn_foot formalshell-mac-app
until_window formalshell-mac-app
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[sh -c 'ATOLL_LOG=atoll::menu=debug exec $mac_app_atoll run $mac_app_bundle > \$log 2>&1']==])" > /dev/null 2>&1
until_window "\$app" 60
"$hyprctl_bin" dispatch "hl.dsp.focus({ window = 'class:\$app' })" > /dev/null 2>&1
front "\$app"
until_jq 30 "\$m-status.json" '.macKeys and (.titles | length >= 4)' call appmenu status
"$hyprctl_bin" -j clients > "\$m-clients.json" 2>&1
"$hyprctl_bin" submap 2>&1 | head -n 1 > "\$m-submap-mac.txt"
call appmenu items Edit > "\$m-edit.json" 2>&1
call appmenu items Edit/Select > "\$m-select.json" 2>&1
until_settled
shoot "\$m-bar.png"

"$wtype_bin" 'typed on linux'
sleep 0.5
key a; seen 'key equivalent "Select All" selectAll:' 1 "\$m-key-select-all.txt"
key c; seen 'key equivalent "Copy" copy:' 1 "\$m-key-copy.txt"
mark c "\$m-mark-c.txt"
"$wtype_bin" -k Right
key v; key v; seen 'key equivalent "Paste" paste:' 2 "\$m-key-paste.txt"
mark v "\$m-mark-v.txt"
until_settled
shoot "\$m-pasted.png"
"$wtype_bin" -M ctrl c -m ctrl
seen 'key equivalent "Copy" copy:' 2 "\$m-key-copy-after-ctrl.txt" 8

call bar room > "\$m-room.json" 2>&1
call appmenu status > "\$m-status-titles.json" 2>&1
cy=\$("\$jq" -r '.[0].cells[] | select(.name == "appMenu") | (.y + .height / 2 | floor)' "\$m-room.json" | head -n 1)
title() { "\$jq" -r --arg l "\$1" --argjson cy "\$cy" '.titles[] | select(.label == \$l) | "\(.x + .width / 2 | floor) \(\$cy)"' "\$m-status-titles.json"; }
row() { "\$jq" -r --argjson id "\$1" '.rows[] | select(.id == \$id) | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"' "\$2"; }
item() { "\$jq" -r --arg l "\$1" '.[] | select(.label == \$l) | .id' "\$2" | head -n 1; }
# up <path> <claim>: the card up and at rest with the claim holding.
up() {
  until_jq 10 "\$1" ".open != null and (\$2)" call appmenu status
  until_settled
  call appmenu status > "\$1" 2>&1
}
park \$(title Edit); click
up "\$m-open-edit.json" '.rows | length > 0'
shoot "\$m-edit.png"
sel=\$(item Select "\$m-edit.json")
all=\$(item "Select All" "\$m-select.json")
park \$(row "\$sel" "\$m-open-edit.json"); click
up "\$m-open-select.json" "any(.rows[]; .id == \$all)"
shoot "\$m-select.png"
park \$(row "\$all" "\$m-open-select.json"); click
seen 'chosen "Select All" selectAll:' 1 "\$m-chosen-select-all.txt"
until_jq 10 "\$m-shut.json" '.open == null' call appmenu status
until_settled
shoot "\$m-selected.png"

call menu summon appmenu > /dev/null 2>&1
until_settled
"$wtype_bin" delete
idx=\$(row_index "\$m-launcher.json" "appmenu.Edit/Delete")
echo "\$idx" > "\$m-launcher-index.txt"
for _ in \$(seq 1 "\${idx:-0}"); do "$wtype_bin" -k Down; done
until_settled
shoot "\$m-launcher.png"
"$wtype_bin" -k Return
seen 'chosen "Delete" delete:' 1 "\$m-chosen-delete.txt"
call menu close > /dev/null 2>&1
until_settled
shoot "\$m-deleted.png"

front "\$app"
key l
front formalshell-mac-app; echo "\$?" > "\$m-left-app.txt"
until_jq 10 "\$m-outside.json" '.macKeys | not' call appmenu status
"$hyprctl_bin" submap 2>&1 | head -n 1 > "\$m-submap-default.txt"
key c
mark c "\$m-mark-c-native.txt"
key h
front "\$app"; echo "\$?" > "\$m-back-on-app.txt"
until_jq 10 "\$m-inside-again.json" '.macKeys' call appmenu status

read -r W H <<< "\$("$hyprctl_bin" -j monitors | "\$jq" -r '.[0] | "\(.width / .scale | round) \(.height / .scale | round)"')"
park \$((W / 2)) \$((H - 1))
until_jq 10 "\$m-dock.json" '.shown and all(.apps[]; .rect != null)' call dock status
until_settled
call dock status > "\$m-dock.json" 2>&1
shoot "\$m-dock.png"
park \$("\$jq" -r '.apps[] | select(.mac) | .rect | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"' "\$m-dock.json" | head -n 1); click right
until_jq 10 "\$m-dock-menu.json" '.menu != null and any(.menu.rows[]; .label == "Quit")' call dock status
until_settled
call dock status > "\$m-dock-menu.json" 2>&1
shoot "\$m-dock-menu.png"
park \$("\$jq" -r '.menu.rows[] | select(.id == -1000) | .rect | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"' "\$m-dock-menu.json"); click
until_jq 10 "\$m-dock-menu-shut.json" '.menu == null' call dock status
# Off the dock and onto the app's own window, which keeps the focus the
# pointer would otherwise take to whatever lies under it.
park \$("$hyprctl_bin" -j clients | "\$jq" -r --arg c "\$app" '.[] | select(.class == \$c) | "\(.at[0] + (.size[0] / 2 | floor)) \(.at[1] + (.size[1] / 2 | floor))"' | head -n 1)
until_jq 10 "\$m-dock-gone.json" '.shown | not' call dock status
front "\$app"
key q
seen 'key equivalent "Quit' 1 "\$m-key-quit.txt"
mark q "\$m-mark-q.txt"
sleep 2
"$hyprctl_bin" -j clients > "\$m-clients-after.json" 2>&1
shoot "\$m-quit.png"
EOF
  write_script "$shot_dir/mac-app-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -f "$mac_app_atoll run $mac_app_bundle" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/mac-app-kill.sh"
  hypr_exec_once "bash $script"
}

leg_mac_app_assert() {
  local m="$mac_app_prefix"
  count() { tr -d '[:space:]' < "$1" 2>/dev/null; }
  local shot
  for shot in bar pasted edit select selected launcher deleted dock dock-menu quit; do
    [ ! -f "$m-$shot.png" ] || echo "SMOKE_MAC_APP_$(echo "$shot" | tr 'a-z-' 'A-Z_') $m-$shot.png"
  done
  echo "SMOKE_MAC_APP_ATOLL $mac_app_log"
  [ -s "$mac_app_log" ] || fail "--mac-app: atoll logged nothing"
  grep -F 'atoll::menu' "$mac_app_log" | sed 's/\x1b\[[0-9;]*m//g'
  echo "marks: $(tr '\n' ' ' < "$mac_app_marks" 2>/dev/null)"
  expect_jq "$m-clients.json" "any(.[]; .class == \"$mac_app_id\" and .xdgTag == \"atoll\")" \
    "--mac-app: Hyprland lists no $mac_app_id window with the atoll tag" '[.[] | {class, xdgTag}]'
  "$jq_bin" -c '[.titles[].label]' "$m-status.json"
  expect_jq "$m-status.json" '.macKeys and ([.titles[].label] | index("File") != null and index("Edit") != null)' \
    "--mac-app: the app in front put no File and Edit on the strip, or left the session out of the mac submap"
  [ "$(cat "$m-submap-mac.txt" 2>/dev/null)" = "mac" ] || fail "--mac-app: Hyprland's submap with the app in front is '$(cat "$m-submap-mac.txt" 2>/dev/null)', not mac"
  expect_jq "$m-edit.json" 'any(.[]; .label == "Copy" and .shortcut == ["Super", "C"]) and any(.[]; .label == "Paste" and .shortcut == ["Super", "V"])' \
    "--mac-app: Edit does not carry Copy and Paste with their Super chords" '[.[] | {label, shortcut}]'
  [ "$(count "$m-key-select-all.txt")" = 1 ] || fail "--mac-app: SUPER + A did not reach the app as Select All (got $(count "$m-key-select-all.txt"))"
  [ "$(count "$m-key-copy.txt")" = 1 ] || fail "--mac-app: SUPER + C did not reach the app as Copy (got $(count "$m-key-copy.txt"))"
  [ "$(count "$m-key-paste.txt")" = 2 ] || fail "--mac-app: SUPER + V twice did not reach the app as two Pastes (got $(count "$m-key-paste.txt"))"
  [ "$(count "$m-mark-c.txt")" = 0 ] || fail "--mac-app: the session's SUPER + C fired over the Mac window"
  [ "$(count "$m-mark-v.txt")" = 0 ] || fail "--mac-app: the session's SUPER + V fired over the Mac window"
  [ "$(count "$m-key-copy-after-ctrl.txt")" = 1 ] || fail "--mac-app: CTRL + C sent the app a Copy"
  expect_jq "$m-open-edit.json" '.open != null and (.rows | length > 3)' "--mac-app: a click on Edit's title opened no card with its rows"
  [ "$(count "$m-chosen-select-all.txt")" = 1 ] || fail "--mac-app: a click on Select All under Edit's Select did not reach the app (got $(count "$m-chosen-select-all.txt"))"
  [ -n "$(cat "$m-launcher-index.txt" 2>/dev/null)" ] || fail "--mac-app: the menu route never listed Edit's Delete: $(cat "$m-launcher.json" 2>/dev/null)"
  [ "$(count "$m-chosen-delete.txt")" = 1 ] || fail "--mac-app: Delete run from the launcher did not reach the app (got $(count "$m-chosen-delete.txt"))"
  [ "$(count "$m-left-app.txt")" = 0 ] || fail "--mac-app: SUPER + L did not move focus off the Mac window"
  [ "$(cat "$m-submap-default.txt" 2>/dev/null)" = "default" ] || fail "--mac-app: Hyprland's submap with the foot window in front is '$(cat "$m-submap-default.txt")'"
  [ "$(count "$m-mark-c-native.txt")" = 1 ] || fail "--mac-app: SUPER + C did not fire over the foot window"
  [ "$(count "$m-back-on-app.txt")" = 0 ] || fail "--mac-app: SUPER + H did not move focus back onto the Mac window"
  expect_jq "$m-dock.json" '.exists and .shown and any(.apps[]; .mac and .running and .focused)' "--mac-app: the pointer on the bottom edge brought out no dock holding the app as a focused Mac app"
  expect_jq "$m-dock-menu.json" '[.menu.rows[].label] | index("Quit") != null and index("Keep in Dock") != null and length >= 3' \
    "--mac-app: a right click on the app's icon opened no menu with its window, Keep in Dock and Quit" '.menu'
  expect_jq "$m-dock-menu-shut.json" '.menu == null' "--mac-app: a click on the window's row left the dock's menu up"
  expect_jq "$m-dock-gone.json" '.shown | not' "--mac-app: the dock stayed out with the pointer gone from it"
  [ "$(count "$m-key-quit.txt")" = 1 ] || fail "--mac-app: SUPER + Q did not reach the app as Quit (got $(count "$m-key-quit.txt"))"
  [ "$(count "$m-mark-q.txt")" = 0 ] || fail "--mac-app: the session's SUPER + Q fired over the Mac window"
  for shot in bar pasted edit select selected launcher deleted dock dock-menu quit; do
    [ -f "$m-$shot.png" ] || fail "--mac-app: no $shot frame"
  done
}
