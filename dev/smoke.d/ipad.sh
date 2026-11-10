# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --ipad: services/ipad.rs against dev/padbridge-stub.py, which owns
# padbridge's name on the session's private bus and logs every call it
# takes as a JSON line. In one session:
#
#  1. no stub: `ipad status` `available: false`, and quick settings' iPad
#     tile disabled with "padbridge is not running" (photographed);
#  2. the stub appearing (NameOwnerChanged): `ipad status` its name, link
#     and edge, the tile available, titled FormalPad and reading Connected;
#  3. the tile walked to on real arrows and Return reaching the stub as
#     exactly Switch, its PropertiesChanged turning the tile on, "On iPad";
#  4. the tile's arrow opened on a real Return, the edges with Right
#     checked (photographed), Return on Above reaching the stub as exactly
#     SetPosition top 0 1 and the check moving there, and the stub's Link
#     going down adding Reconnect (photographed);
#  5. the launcher's iPad route (`menu summon ipad`) ticking Above and the
#     whole edge, "first" typed on real keys and Return reaching the stub as
#     SetPosition top 0 0.5 and the tick moving to First half (photographed);
#  6. the stub SIGKILLed: `ipad status` unavailable and the tile disabled
#     again, the launcher's route gone from `menu summon`.
#
# Every move of a selection runs under the stale audit.
leg_ipad_flag="--ipad"
leg_ipad_order=263
leg_ipad_claims="ipad launcher=shared panel=shared"
leg_ipad_needs="python3 jq wtype grim"

ipad_dir="$shot_dir/ipad"
ipad_stub="$PWD/dev/padbridge-stub.py"
ipad_log="$ipad_dir/calls.jsonl"
ipad_fifo="$ipad_dir/control"
ipad_pid="$ipad_dir/stub.pid"

ipad_png() { printf '%s/ipad-%s.png' "$shot_dir" "$1"; }

leg_ipad_timing() {
  leg_timing 170
}

leg_ipad_fixture() {
  mkdir -p "$ipad_dir"
  : > "$ipad_log"
}

leg_ipad_drive() {
  local script="$shot_dir/ipad-drive.sh" kill_script="$shot_dir/ipad-kill.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
d="$ipad_dir"
qs() { status quicksettings "\$d/qs-\$1.json"; }
shot() {
  sleep 0.6
  shoot "$shot_dir/ipad-\$1.png"
}
# The key toward the stop \$to off the boxes quicksettings status reports.
key_jq='
      ([.rows[] | {key: .id, value: .rect}, {key: (.id + ":icon"), value: .icon}, {key: (.id + ":menu"), value: .arrow}]
        + [(.menu.rows // [])[] | {key: .id, value: .rect}] | map(select(.value != null)) | from_entries) as \$box
      | \$box[\$at] as \$a | \$box[\$to] as \$b
      | if \$a == null or \$b == null then "Down"
        elif \$b.y >= \$a.y + \$a.height then "Down"
        elif \$b.y + \$b.height <= \$a.y then "Up"
        elif \$b.x > \$a.x then "Right" else "Left" end'
press() {
  "$wtype_bin" -k Return
  sleep "\${1:-1}"
}

call ipad status > "\$d/status-absent.json" 2>&1
call panel open quicksettings > /dev/null 2>&1
until_settled
qs absent
shot absent
call panel close > /dev/null 2>&1
sleep 1

"$python3_bin" "$ipad_stub" --log "$ipad_log" --control "$ipad_fifo" --ready "\$d/ready" > "\$d/stub.log" 2>&1 &
echo \$! > "$ipad_pid"
until_jq 10 "\$d/status-present.json" '.available' call ipad status

call panel open quicksettings > /dev/null 2>&1
until_settled
qs present
shot tile
$(stale_audit_start)
walk_to quicksettings .cursor ipad "\$d/walk-tile.txt" "\$key_jq"
press
until_jq 5 "\$d/qs-switched.json" '.rows[] | select(.id == "ipad") | .on' call quicksettings status
call ipad status > "\$d/status-switched.json" 2>&1
shot tile-on

walk_to quicksettings .cursor ipad:menu "\$d/walk-arrow.txt" "\$key_jq"
press 1.5
qs menu-open
shot menu
walk_to quicksettings .cursor m:ipad:top "\$d/walk-above.txt" "\$key_jq"
press
until_jq 5 "\$d/qs-above.json" '.menu.rows[] | select(.id == "m:ipad:top") | .checked' call quicksettings status
echo '{"Link": "down"}' > "$ipad_fifo"
until_jq 5 "\$d/qs-down.json" '.menu.rows | any(.id == "m:ipad:reconnect")' call quicksettings status
shot menu-down
$(stale_audit_stop ipad-quick)
"$wtype_bin" -k Escape
sleep 0.8
"$wtype_bin" -k Escape
sleep 1

call menu summon ipad > "\$d/summon.txt" 2>&1
until_settled
call menu status > "\$d/menu-route.json" 2>&1
shot launcher
$(stale_audit_start)
"$wtype_bin" first
sleep 1
call menu status > "\$d/menu-typed.json" 2>&1
press
until_jq 5 "\$d/menu-stretch.json" '.checked | index("ipad.stretch-first") != null' call menu status
$(stale_audit_stop ipad-launcher)
shot launcher-stretch
call ipad status > "\$d/status-stretch.json" 2>&1
"$wtype_bin" -k Escape
sleep 0.5
"$wtype_bin" -k Escape
sleep 1

kill -9 "\$(cat "$ipad_pid")"
until_jq 10 "\$d/status-gone.json" '.available == false' call ipad status
call panel open quicksettings > /dev/null 2>&1
until_settled
qs gone
shot gone
call panel close > /dev/null 2>&1
sleep 0.5
call menu summon ipad > "\$d/summon-gone.txt" 2>&1
until_settled
call menu status > "\$d/menu-gone.json" 2>&1
call menu close > /dev/null 2>&1
echo done > "\$d/drive-done"
EOF
  hypr_exec_once "bash $script"

  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
[ -f "$ipad_pid" ] && kill "\$(cat "$ipad_pid")" 2>/dev/null
rm -f "$ipad_fifo"
true
EOF
  add_cleanup "bash $kill_script"
}

ipad_jq() {
  local what="$1" file="$ipad_dir/$2" test="$3"
  "$jq_bin" -e "$test" "$file" > /dev/null 2>&1 || fail "$what (got: $(head -c 600 "$file" 2>/dev/null))"
}

ipad_calls() {
  "$jq_bin" -sc '[.[] | select(.call) | [.call, .edge, .start, .end]]' "$ipad_log" 2>/dev/null
}

leg_ipad_assert() {
  local name
  for name in absent tile tile-on menu menu-down launcher launcher-stretch gone; do
    [ -s "$(ipad_png "$name")" ] && echo "SMOKE_IPAD_$(tr 'a-z-' 'A-Z_' <<< "$name") $(ipad_png "$name")"
  done
  echo "SMOKE_IPAD_CALLS $ipad_log"
  [ -f "$ipad_dir/drive-done" ] || fail "the ipad drive never finished (stub log: $(tail -3 "$ipad_dir/stub.log" 2>/dev/null))"

  ipad_jq "ipad status read unavailable with no padbridge" status-absent.json '. == {available: false}'
  ipad_jq "the tile disabled saying padbridge is not running" qs-absent.json \
    '.rows[] | select(.id == "ipad") | .available == false and .reason == "padbridge is not running" and .on == false'
  ipad_jq "ipad status read the stub once it appeared" status-present.json \
    '.available and .name == "FormalPad" and .link == "up" and .edge == "right" and .start == 0 and .end == 1 and .active == false'
  ipad_jq "the tile live as a split, titled FormalPad and reading Connected" qs-present.json \
    '.rows[] | select(.id == "ipad") | .available and .kind == "split" and .on == false and .title == "FormalPad" and .subtitle == "Connected"'
  ipad_jq "Return on the tile switched input to the iPad" qs-switched.json \
    '.rows[] | select(.id == "ipad") | .on and .subtitle == "On iPad"'
  ipad_jq "ipad status read it active" status-switched.json '.active == true'
  ipad_jq "the menu listed the four edges with Right checked" qs-menu-open.json \
    '.menu.id == "ipad" and ([.menu.rows[] | select(.id | startswith("m:ipad:")) | .title] == ["Left", "Right", "Above", "Below"])
      and ([.menu.rows[] | select(.checked) | .id] == ["m:ipad:right"]) and (.menu.rows[-1].route == "ipad")'
  ipad_jq "Above checked once the stub moved" qs-above.json '[.menu.rows[] | select(.checked) | .id] == ["m:ipad:top"]'
  ipad_jq "the link down added Reconnect" qs-down.json '.menu.rows | any(.id == "m:ipad:reconnect" and .title == "Reconnect")'

  ipad_jq "menu summon ipad entered the route" menu-route.json \
    '.level == "ipad" and (.checked | index("ipad.edge-top") != null and index("ipad.stretch-whole") != null)
      and (.ids | index("ipad.switch") != null and index("ipad.reconnect") != null and index("ipad.stretch-middle") != null)'
  ipad_jq "typing first put the cursor on First half" menu-typed.json '.cursorId == "ipad.stretch-first"'
  ipad_jq "the tick moved to First half" menu-stretch.json '.checked | index("ipad.stretch-whole") == null'
  ipad_jq "ipad status read the new stretch" status-stretch.json '.edge == "top" and .start == 0 and .end == 0.5'

  local calls
  calls=$(ipad_calls)
  "$jq_bin" -e '. == [["Switch", null, null, null], ["SetPosition", "top", 0, 1], ["SetPosition", "top", 0, 0.5]]' <<< "$calls" > /dev/null \
    || fail "the stub took: $calls (wanted Switch, SetPosition top 0 1, SetPosition top 0 0.5)"

  ipad_jq "ipad status unavailable once the stub died" status-gone.json '. == {available: false}'
  ipad_jq "the tile disabled again" qs-gone.json '.rows[] | select(.id == "ipad") | .available == false'
  ipad_jq "the launcher route gone with padbridge" menu-gone.json '.level != "ipad"'

  stale_audit_assert ipad-quick
  stale_audit_assert ipad-launcher
  for name in absent tile tile-on menu menu-down launcher launcher-stretch gone; do
    [ -s "$(ipad_png "$name")" ] || fail "no $name frame"
  done
}
