# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --minimap: the sidebar's Spaces map acted on
# (docs/superpowers/specs/2026-10-30-sidebar.md section 4), under
# `layout: sidebar`. Three foot windows, two on workspace 1 and one on
# workspace 2. `sidebar focus` hands the map the keyboard with its cursor on
# the focused window; every key after it is a real one off `wtype`. Left
# puts the cursor on the row's first window; Shift+Right carries it past
# its neighbour, the two trading places in `hyprctl clients` and in the
# row's order; Shift+Down carries it onto workspace 2, and Shift+Down again
# past the last row onto a new workspace 3; Escape gives the keyboard back.
# Each window is matched by its address (`sidebar status` ids carry no
# `0x`). The cursor's frame and the carried one are pulled for reading.
leg_minimap_flag="--minimap"
leg_minimap_order=238
leg_minimap_needs="jq foot wtype"

minimap_clients=("$shot_dir"/minimap-clients-{0,2,3,4}.json)
minimap_status=("$shot_dir"/minimap-status-{0,1,2,3,4,5}.json)
minimap_focus_png="$shot_dir/minimap-focus.png"
minimap_carried_png="$shot_dir/minimap-carried.png"

leg_minimap_validate() {
  if leg_on bar_position || leg_on frame; then
    echo "usage: --minimap runs under layout sidebar and cannot combine with --bar-position or --frame" >&2
    exit 1
  fi
}

leg_minimap_fixture() {
  leg_on sidebar || settings_fragment ', "layout": "sidebar"'
}

leg_minimap_timing() {
  leg_timing 30 70
}

leg_minimap_drive() {
  local script="$shot_dir/minimap-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
for _ in \$(seq 1 40); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.5
done
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-mm-a sh -c 'sleep 300']==])" > /dev/null 2>&1
sleep 1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-mm-b sh -c 'sleep 300']==])" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-mm-c sh -c 'sleep 300']==], { workspace = '2 silent' })" > /dev/null 2>&1
sleep 3
$ipc call sidebar focus > /dev/null 2>&1
sleep 0.5
$ipc call sidebar status > "${minimap_status[0]}" 2>&1
"$hyprctl_bin" -j clients > "${minimap_clients[0]}" 2>&1
"$wtype_bin" -k Left
sleep 0.5
$ipc call sidebar status > "${minimap_status[1]}" 2>&1
"$grim_bin" "$minimap_focus_png" > /dev/null 2>&1
"$wtype_bin" -M shift -k Right -m shift
sleep 1.5
$ipc call sidebar status > "${minimap_status[2]}" 2>&1
"$hyprctl_bin" -j clients > "${minimap_clients[1]}" 2>&1
"$wtype_bin" -M shift -k Down -m shift
sleep 1.5
$ipc call sidebar status > "${minimap_status[3]}" 2>&1
"$hyprctl_bin" -j clients > "${minimap_clients[2]}" 2>&1
"$wtype_bin" -M shift -k Down -m shift
sleep 1.5
$ipc call sidebar status > "${minimap_status[4]}" 2>&1
"$hyprctl_bin" -j clients > "${minimap_clients[3]}" 2>&1
"$grim_bin" "$minimap_carried_png" > /dev/null 2>&1
"$wtype_bin" -k Escape
sleep 0.5
$ipc call sidebar status > "${minimap_status[5]}" 2>&1
EOS
  hypr_exec_once "bash $script"
}

# A window by its sidebar id in a clients dump: "<x> <workspace id>".
minimap_where() {
  "$jq_bin" -r --arg id "$1" '.[] | select((.address | ltrimstr("0x") | sub("^0+"; "")) == $id) | "\(.at[0]) \(.at[1]) \(.workspace.id)"' "$2"
}

leg_minimap_assert() {
  local f
  for f in "${minimap_status[@]}" "${minimap_clients[@]}"; do
    [ -s "$f" ] || fail "minimap: $f missing or empty"
  done
  echo "SMOKE_MINIMAP_FOCUS $minimap_focus_png"
  echo "SMOKE_MINIMAP_CARRIED $minimap_carried_png"
  "$jq_bin" -e '.cursor.window != null and .cursor.row == 0 and (.rows[0].windows | length) == 2' "${minimap_status[0]}" > /dev/null \
    || fail "minimap: sidebar focus did not put the cursor on a window of a two-window row: $(cat "${minimap_status[0]}")"
  local carried other
  carried=$("$jq_bin" -r '.rows[0].windows[0]' "${minimap_status[1]}")
  other=$("$jq_bin" -r '.rows[0].windows[1]' "${minimap_status[1]}")
  "$jq_bin" -e --arg id "$carried" '.cursor.window == $id' "${minimap_status[1]}" > /dev/null \
    || fail "minimap: Left did not put the cursor on the row's first window: $(cat "${minimap_status[1]}")"
  local c_before o_before c_after o_after
  c_before=$(minimap_where "$carried" "${minimap_clients[0]}")
  o_before=$(minimap_where "$other" "${minimap_clients[0]}")
  c_after=$(minimap_where "$carried" "${minimap_clients[1]}")
  o_after=$(minimap_where "$other" "${minimap_clients[1]}")
  echo "minimap swap: carried $c_before -> $c_after, other $o_before -> $o_after"
  [ -n "$c_before" ] && [ "$c_after" = "$o_before" ] && [ "$o_after" = "$c_before" ] \
    || fail "minimap: Shift+Right did not swap the two windows of the row"
  "$jq_bin" -e --arg id "$carried" '.rows[0].windows[1] == $id' "${minimap_status[2]}" > /dev/null \
    || fail "minimap: the row's order did not follow the swap: $(cat "${minimap_status[2]}")"
  local ws
  ws=$(minimap_where "$carried" "${minimap_clients[2]}" | awk '{print $3}')
  [ "$ws" = "2" ] || fail "minimap: Shift+Down did not carry the window onto workspace 2 (on $ws)"
  ws=$(minimap_where "$carried" "${minimap_clients[3]}" | awk '{print $3}')
  [ "$ws" = "3" ] || fail "minimap: Shift+Down past the last row did not make workspace 3 (on $ws)"
  "$jq_bin" -e --arg id "$carried" '.cursor.window == $id and ([.rows[] | select(.windows | index($id) != null)] | length) == 1' "${minimap_status[4]}" > /dev/null \
    || fail "minimap: the cursor did not follow the carried window: $(cat "${minimap_status[4]}")"
  "$jq_bin" -e '.cursor == null' "${minimap_status[5]}" > /dev/null \
    || fail "minimap: Escape did not give the keyboard back: $(cat "${minimap_status[5]}")"
}
