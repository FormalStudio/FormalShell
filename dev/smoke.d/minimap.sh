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
# Then the pointer, a held button off dev/vpointer.py over hit rects
# `sidebar status` reports: the carried window dragged off workspace 3's
# row onto workspace 2's beside mm-c (a mid-drag frame and status taken
# with the button still down: lifted, its target row 1), the two then
# trading places by a drag along that row, mm-c dragged to the empty row
# making workspace 3 again, and a drag let go past the column's edge
# moving nothing, the map holding no order after it. Last the scrolling
# layout, three windows on workspace 4, one column each: the first icon
# dragged past the last moves its column to the tape's end (e f d by x,
# where a swap would give f e d), with a frame mid-drag and one after; then
# `sidebar focus` and Shift+Left carry that column back one (e d f).
# After them, the map's rows photographed as the drags left them and
# again once a collapse and expand draws them whole: no pixel differs, so
# no drag left a trail behind its lifted icon. Then
# the launcher open under `sidebar focus` closing (`menu status`) with the
# cursor up, and `sidebar collapse` taking the cursor down. Then 26
# more workspaces: the map showing fewer rows than it has, every row hit
# above the foot's buttons, and a real wheel notch over it moving the
# first row shown. Each window is
# matched by its address (`sidebar status` ids carry no `0x`). The
# cursor's frame, the carried one, the mid-drag and the dropped ones are
# pulled for reading.
leg_minimap_flag="--minimap"
leg_minimap_order=238
leg_minimap_needs="jq foot wtype wlrctl python3"
minimap_vpointer="$PWD/dev/vpointer.py"

minimap_clients=("$shot_dir"/minimap-clients-{0,2,3,4,6,7,8,9,10,11,12}.json)
minimap_status=("$shot_dir"/minimap-status-{0,1,2,3,4,5,6,7,8,9,10}.json)
minimap_lifted_png="$shot_dir/minimap-lifted.png"
minimap_dropped_png="$shot_dir/minimap-dropped.png"
minimap_column_lifted_png="$shot_dir/minimap-column-lifted.png"
minimap_column_png="$shot_dir/minimap-column.png"
minimap_focus_png="$shot_dir/minimap-focus.png"
minimap_carried_png="$shot_dir/minimap-carried.png"
minimap_trail_png="$shot_dir/minimap-trail.png"
minimap_redrawn_png="$shot_dir/minimap-redrawn.png"
minimap_trail_status="$shot_dir/minimap-trail.json"

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
  leg_timing 30 110
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
# A hit's centre on the output, off the latest status: the layer sits at
# the output's origin on the left edge.
centre() {
  $ipc call sidebar status 2>/dev/null | "$jq_bin" -r --arg on "\$1" '.hits[] | select(.on == \$on) | "\(.x + (.w / 2 | floor)) \(.y + (.h / 2 | floor))"' | head -1
}
# Hold the button from one point to another in steps, then hold \$5 ms
# there before letting go.
drag() {
  "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
  "$wlrctl_bin" pointer move "\$1" "\$2" > /dev/null 2>&1
  sleep 0.3
  local dx=\$(( (\$3 - \$1) / 8 )) dy=\$(( (\$4 - \$2) / 8 )) steps=()
  for _ in 1 2 3 4 5 6 7 8; do steps+=(move "\$dx" "\$dy" wait 30); done
  "$python3_bin" "$minimap_vpointer" down wait 50 "\${steps[@]}" move \$(( \$3 - \$1 - dx * 8 )) \$(( \$4 - \$2 - dy * 8 )) wait "\$5" up > /dev/null 2>&1
}
carried=\$("$jq_bin" -r '.cursor // empty | .window' "${minimap_status[4]}")
[ -n "\$carried" ] || carried=\$("$jq_bin" -r '.rows[2].windows[0]' "${minimap_status[4]}")
read -r ax ay <<< "\$(centre "window:\$carried")"
read -r bx by <<< "\$(centre row:1)"
drag "\$ax" "\$ay" \$(( bx + 40 )) "\$by" 1500 &
sleep 1.2
$ipc call sidebar status > "${minimap_status[6]}" 2>&1
"$grim_bin" "$minimap_lifted_png" > /dev/null 2>&1
wait
sleep 1.5
$ipc call sidebar status > "${minimap_status[7]}" 2>&1
"$hyprctl_bin" -j clients > "${minimap_clients[4]}" 2>&1
"$grim_bin" "$minimap_dropped_png" > /dev/null 2>&1
first=\$("$jq_bin" -r '.rows[1].windows[0]' "${minimap_status[7]}")
second=\$("$jq_bin" -r '.rows[1].windows[1]' "${minimap_status[7]}")
read -r ax ay <<< "\$(centre "window:\$first")"
read -r bx by <<< "\$(centre "window:\$second")"
drag "\$ax" "\$ay" \$(( bx + 4 )) "\$by" 200
sleep 1.5
$ipc call sidebar status > "${minimap_status[8]}" 2>&1
"$hyprctl_bin" -j clients > "${minimap_clients[5]}" 2>&1
read -r ax ay <<< "\$(centre "window:\$first")"
read -r bx by <<< "\$(centre new)"
drag "\$ax" "\$ay" "\$bx" "\$by" 200
sleep 1.5
$ipc call sidebar status > "${minimap_status[9]}" 2>&1
"$hyprctl_bin" -j clients > "${minimap_clients[6]}" 2>&1
read -r ax ay <<< "\$(centre "window:\$second")"
drag "\$ax" "\$ay" 600 "\$ay" 200
sleep 1.5
$ipc call sidebar status > "${minimap_status[10]}" 2>&1
"$hyprctl_bin" -j clients > "${minimap_clients[7]}" 2>&1
"$hyprctl_bin" eval 'hl.config({ general = { layout = "scrolling" } })' > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.focus({ workspace = 4 })" > /dev/null 2>&1
for app in d e f; do
  "$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-mm-\$app sh -c 'sleep 300']==])" > /dev/null 2>&1
  sleep 1
done
sleep 2
"$hyprctl_bin" -j clients > "${minimap_clients[8]}" 2>&1
"$hyprctl_bin" -j workspaces > "$shot_dir/minimap-workspaces.json" 2>&1
row=\$($ipc call sidebar status | "$jq_bin" '[.rows[] | .windows | length] | index(3)')
d=\$($ipc call sidebar status | "$jq_bin" -r --argjson r "\$row" '.rows[\$r].windows[0]')
f=\$($ipc call sidebar status | "$jq_bin" -r --argjson r "\$row" '.rows[\$r].windows[2]')
read -r ax ay <<< "\$(centre "window:\$d")"
read -r bx by <<< "\$(centre "window:\$f")"
drag "\$ax" "\$ay" \$(( bx + 12 )) "\$by" 1200 &
sleep 1
"$grim_bin" "$minimap_column_lifted_png" > /dev/null 2>&1
wait
sleep 1.5
"$hyprctl_bin" -j clients > "${minimap_clients[9]}" 2>&1
"$grim_bin" "$minimap_column_png" > /dev/null 2>&1
$ipc call sidebar focus > /dev/null 2>&1
sleep 0.5
"$wtype_bin" -M shift -k Left -m shift
sleep 1.5
"$hyprctl_bin" -j clients > "${minimap_clients[10]}" 2>&1
"$wtype_bin" -k Escape
sleep 0.5
# Every drag has settled and the keyboard is back: the map as the drags
# left it, then drawn whole again by a collapse and expand, the pointer
# parked off the column for both so no hover differs. It comes after
# Shift+Left, since parking the pointer focuses the window under it.
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
"$wlrctl_bin" pointer move 600 300 > /dev/null 2>&1
sleep 1
$ipc call sidebar status > "$minimap_trail_status" 2>&1
"$grim_bin" "$minimap_trail_png" > /dev/null 2>&1
$ipc call sidebar collapse > /dev/null 2>&1
sleep 1
$ipc call sidebar expand > /dev/null 2>&1
sleep 1.5
"$grim_bin" "$minimap_redrawn_png" > /dev/null 2>&1
$ipc call menu toggle > /dev/null 2>&1
sleep 1
$ipc call sidebar focus > /dev/null 2>&1
sleep 0.5
$ipc call menu status > "$shot_dir/minimap-menu-status.json" 2>&1
$ipc call sidebar status > "$shot_dir/minimap-focus-over-menu.json" 2>&1
$ipc call sidebar collapse > /dev/null 2>&1
sleep 1
$ipc call sidebar status > "$shot_dir/minimap-collapsed.json" 2>&1
$ipc call sidebar expand > /dev/null 2>&1
for n in \$(seq 5 30); do
  "$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-mm-\$n sh -c 'sleep 300']==], { workspace = '\$n silent' })" > /dev/null 2>&1
done
sleep 3
$ipc call sidebar status > "$shot_dir/minimap-crowded.json" 2>&1
"$grim_bin" "$shot_dir/minimap-crowded.png" > /dev/null 2>&1
read -r rx ry <<< "\$(centre row:0)"
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
"$wlrctl_bin" pointer move "\$rx" "\$ry" > /dev/null 2>&1
sleep 0.3
"$wlrctl_bin" pointer scroll 15 0 > /dev/null 2>&1
sleep 0.8
$ipc call sidebar status > "$shot_dir/minimap-scrolled.json" 2>&1
"$grim_bin" "$shot_dir/minimap-scrolled.png" > /dev/null 2>&1
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
  echo "SMOKE_MINIMAP_LIFTED $minimap_lifted_png"
  echo "SMOKE_MINIMAP_DROPPED $minimap_dropped_png"
  "$jq_bin" -e --arg id "$carried" '.minimap.drag.window == $id and .minimap.drag.lifted and .minimap.drag.target[0] == 1' "${minimap_status[6]}" > /dev/null \
    || fail "minimap: mid-drag the icon was not lifted over row 1: $(cat "${minimap_status[6]}")"
  ws=$(minimap_where "$carried" "${minimap_clients[4]}" | awk '{print $3}')
  [ "$ws" = "2" ] || fail "minimap: the drag onto workspace 2's row did not move the window there (on $ws)"
  local first second f_before s_before f_after s_after
  first=$("$jq_bin" -r '.rows[1].windows[0]' "${minimap_status[7]}")
  second=$("$jq_bin" -r '.rows[1].windows[1]' "${minimap_status[7]}")
  f_before=$(minimap_where "$first" "${minimap_clients[4]}")
  s_before=$(minimap_where "$second" "${minimap_clients[4]}")
  f_after=$(minimap_where "$first" "${minimap_clients[5]}")
  s_after=$(minimap_where "$second" "${minimap_clients[5]}")
  echo "minimap drag swap: $first $f_before -> $f_after, $second $s_before -> $s_after"
  [ -n "$f_before" ] && [ "$f_after" = "$s_before" ] && [ "$s_after" = "$f_before" ] \
    || fail "minimap: the drag along the row did not swap its two windows"
  "$jq_bin" -e --arg a "$first" --arg b "$second" '.rows[1].windows == [$b, $a]' "${minimap_status[8]}" > /dev/null \
    || fail "minimap: the row's order did not follow the dragged swap: $(cat "${minimap_status[8]}")"
  ws=$(minimap_where "$first" "${minimap_clients[6]}" | awk '{print $3}')
  [ "$ws" = "3" ] || fail "minimap: the drop on the empty row did not make workspace 3 (on $ws)"
  [ "$(minimap_where "$second" "${minimap_clients[7]}")" = "$(minimap_where "$second" "${minimap_clients[6]}")" ] \
    || fail "minimap: a drag let go past the column moved its window"
  "$jq_bin" -e '.minimap.drag == null and .minimap.held == null' "${minimap_status[10]}" > /dev/null \
    || fail "minimap: the cancelled drag left a drag or a held order: $(cat "${minimap_status[10]}")"
  echo "SMOKE_MINIMAP_COLUMN_LIFTED $minimap_column_lifted_png"
  echo "SMOKE_MINIMAP_COLUMN $minimap_column_png"
  "$jq_bin" -e '[.[] | select(.id == 4 and .tiledLayout == "scrolling")] | length == 1' "$shot_dir/minimap-workspaces.json" > /dev/null \
    || fail "minimap: workspace 4 is not under the scrolling layout: $(cat "$shot_dir/minimap-workspaces.json")"
  local order
  for f in 8 9 10; do
    order=$("$jq_bin" -r '[.[] | select(.workspace.id == 4)] | sort_by(.at[0]) | map(.class | ltrimstr("formalshell-mm-")) | join(" ")' "${minimap_clients[$f]}")
    echo "minimap scrolling order $f: $order"
    case $f in
      8) [ "$order" = "d e f" ] || fail "minimap: the scrolling tape did not start as d e f: $order" ;;
      9) [ "$order" = "e f d" ] || fail "minimap: the drag past the last column did not move d's column to the end: $order" ;;
      10) [ "$order" = "e d f" ] || fail "minimap: Shift+Left did not carry d's column back one: $order" ;;
    esac
  done
  "$jq_bin" -e '.isOpen == false' "$shot_dir/minimap-menu-status.json" > /dev/null \
    || fail "minimap: sidebar focus left the launcher open: $(cat "$shot_dir/minimap-menu-status.json")"
  "$jq_bin" -e '.cursor != null' "$shot_dir/minimap-focus-over-menu.json" > /dev/null \
    || fail "minimap: sidebar focus over the launcher put no cursor up"
  "$jq_bin" -e '.collapsed and .cursor == null' "$shot_dir/minimap-collapsed.json" > /dev/null \
    || fail "minimap: collapsing kept the minimap cursor: $(cat "$shot_dir/minimap-collapsed.json")"
  echo "SMOKE_MINIMAP_TRAIL $minimap_trail_png"
  local trail
  trail=$("$python3_bin" - "$minimap_trail_png" "$minimap_redrawn_png" "$minimap_trail_status" <<'PY' 2>&1
import json, sys
from PIL import Image, ImageChops
hits = [h for h in json.load(open(sys.argv[3]))["hits"] if (h.get("on") or "").startswith(("row:", "window:")) or h.get("on") == "new"]
box = (min(h["x"] for h in hits), min(h["y"] for h in hits), max(h["x"] + h["w"] for h in hits), max(h["y"] + h["h"] for h in hits))
a, b = (Image.open(p).convert("RGB").crop(box) for p in sys.argv[1:3])
diff = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 8 else 0)
n = sum(1 for v in diff.getdata() if v)
print(f"map {box}: {n} pixels differ from a whole redraw")
sys.exit(1 if n else 0)
PY
  ) || fail "minimap: the drags left pixels a whole redraw does not draw: $trail"
  echo "$trail"
  echo "SMOKE_MINIMAP_CROWDED $shot_dir/minimap-crowded.png"
  echo "SMOKE_MINIMAP_SCROLLED $shot_dir/minimap-scrolled.png"
  "$jq_bin" -e '(.rows | length) + 1 > .shown.count and .shown.first == 0
      and ([.hits[] | select(.on | startswith("row:") or . == "new") | .y + .h] | max)
        <= ([.hits[] | select(.on == "summon:system") | .y] | min)' "$shot_dir/minimap-crowded.json" > /dev/null \
    || fail "minimap: the crowded map was not cut to the rows that fit above the foot: $(cat "$shot_dir/minimap-crowded.json")"
  "$jq_bin" -e '.shown.first > 0' "$shot_dir/minimap-scrolled.json" > /dev/null \
    || fail "minimap: a wheel notch over the map did not scroll its rows: $("$jq_bin" -c .shown "$shot_dir/minimap-scrolled.json")"
}
