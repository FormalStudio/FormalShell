# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --sidebar: `layout: sidebar` from settings.json alone
# (docs/superpowers/specs/2026-10-30-sidebar.md). The bar's own
# formalshell:bar layer is the column, read off `hyprctl -j layers`: 154
# wide on the left edge the whole output tall, its zone reserved. `sidebar
# status` names the cards it drew (time, spaces, notifications, the last
# one carrying the notification sent once its toast has gone) and the minimap's row holding the fixture window. A
# real click on the status row's network button opens the network panel; a
# real click on its collapse button folds the column to the 48 wide rail,
# state.json carrying `sidebarCollapsed`; `sidebar expand` opens it again.
# Then settings.json rewritten with `bar.position: right` moves it to the
# right edge. The session runs with Hyprland's `gaps_out` 10 and
# `border_size` 2, and the fixture window's visible left edge (its `at`
# less the border, off `hyprctl -j clients`) sits as far off the cards'
# right edge as the cards sit off the screen's, both read off the layer's
# geometry and the time card's hit rect. The open, panel and rail frames
# are pulled for reading.
# The clock card under a staged wallpaper (red left of x 52, green right of
# it, an orange band down x 38 to 44, its depth map white over the band and
# a ground strip) and `clockFormat` "h:mm AP", read off `debug dump`'s
# `sidebarClock`: the digits frosted and cut by the band's subject, the red
# and green reaching the digits' own two ends (a flat tint carries one hue),
# the band's orange over their middle row, the date box carrying its words,
# the time and period the bar clock's format writes and `date` agrees with,
# the first draw of the time under 50ms, and no bar commit in 3s at rest.
# Ridden by a budget leg (`--idle`, `--menu-budget`) it pins the layout
# alone, so the budget is read with the sidebar on and nothing driven; by
# `--tray` or `--tray-overflow` the same, those legs reading the tray off
# the status card's tray button.
leg_sidebar_flag="--sidebar"
leg_sidebar_order=237
leg_sidebar_needs="jq wlrctl notify-send convert"
leg_sidebar_fixture_window=keep

sidebar_status_open="$shot_dir/sidebar-status-open.json"
sidebar_status_rail="$shot_dir/sidebar-status-rail.json"
sidebar_status_expanded="$shot_dir/sidebar-status-expanded.json"
sidebar_layers_open="$shot_dir/sidebar-layers-open.json"
sidebar_layers_rail="$shot_dir/sidebar-layers-rail.json"
sidebar_layers_right="$shot_dir/sidebar-layers-right.json"
sidebar_monitors="$shot_dir/sidebar-monitors.json"
sidebar_clients="$shot_dir/sidebar-clients.json"
sidebar_panel_state="$shot_dir/sidebar-panel-state.txt"
sidebar_state_copy="$shot_dir/sidebar-state.json"
sidebar_open_png="$shot_dir/sidebar-open.png"
sidebar_panel_png="$shot_dir/sidebar-panel.png"
sidebar_rail_png="$shot_dir/sidebar-rail.png"
sidebar_clock_dir="$shot_dir/sidebar-clock-wallpapers"
sidebar_clock_dump="$shot_dir/sidebar-clock.json"
sidebar_clock_results="$shot_dir/sidebar-clock-results.txt"
sidebar_clock_png="$shot_dir/sidebar-clock.png"

sidebar_rider() {
  leg_on idle || leg_on menu_budget || leg_on tray || leg_on tray_overflow
}

# The clock card case stages its own wallpaper and clock format.
sidebar_clock_case() {
  ! sidebar_rider && ! leg_on showcase && ! leg_on wallpaper && ! leg_on depth && ! leg_on desktop_clock
}

leg_sidebar_validate() {
  if leg_on bar_position || leg_on frame; then
    echo "usage: --sidebar places the column itself and cannot combine with --bar-position or --frame" >&2
    exit 1
  fi
}

leg_sidebar_fixture() {
  settings_fragment ', "layout": "sidebar"'
  sidebar_clock_case || return 0
  local wall="$sidebar_clock_dir/split.png" state="$iso_home/.local/state/formalshell/state.json"
  mkdir -p "$sidebar_clock_dir/.depth" "$(dirname "$state")"
  $convert_bin -size 1920x1080 xc:'#101418' -fill '#e03020' -draw 'rectangle 0,0 51,200' -fill '#20c040' -draw 'rectangle 52,0 160,200' \
    -fill '#ff7a1a' -draw 'rectangle 38,0 44,1079' "$wall"
  $convert_bin -size 1920x1080 xc:black -fill white -draw 'rectangle 38,0 44,1079' -draw 'rectangle 0,900 1919,1079' "$sidebar_clock_dir/.depth/split.png.png"
  touch -r "$wall" "$sidebar_clock_dir/.depth/split.png.png"
  [ -s "$state" ] || echo '{}' > "$state"
  "$jq_bin" --arg w "$wall" '. + {"wallpaper": $w, "clockFormat": "h:mm AP"}' "$state" > "$state.new" && mv "$state.new" "$state"
}

leg_sidebar_timing() {
  leg_timing 30 90
}

leg_sidebar_drive() {
  sidebar_rider && return 0
  local script="$shot_dir/sidebar-drive.sh"
  local settings_path="$iso_home/.config/formalshell/settings.json"
  local state_path="$iso_home/.local/state/formalshell/state.json"
  write_script "$script" <<EOS
#!/usr/bin/env bash
for _ in \$(seq 1 40); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.5
done
"$notify_send_bin" -t 1500 "Sidebar" "One pending notification" > /dev/null 2>&1
sleep 5
$ipc call sidebar status > "$sidebar_status_open" 2>&1
"$hyprctl_bin" -j layers > "$sidebar_layers_open" 2>&1
"$hyprctl_bin" -j clients > "$sidebar_clients" 2>&1
"$hyprctl_bin" -j monitors > "$sidebar_monitors" 2>&1
"$grim_bin" "$sidebar_open_png" > /dev/null 2>&1
if [ "$(sidebar_clock_case && echo 1)" = 1 ]; then
  out="$sidebar_clock_results"
  echo "before=\$(date +%-I:%M) \$(date +%p)" > "\$out"
  $ipc call debug dump 2>/dev/null | "$jq_bin" -c '.sidebarClock' > "$sidebar_clock_dump"
  echo "after=\$(date +%-I:%M) \$(date +%p)" >> "\$out"
  commits() { $ipc call debug dump 2>/dev/null | "$jq_bin" '.sidebarClock.commits'; }
  # Clear of a minute boundary, so the minute's own redraw is not counted.
  s=\$((10#\$(date +%S)))
  [ "\$s" -lt 50 ] || sleep \$((61 - s))
  c0=\$(commits); sleep 3; c1=\$(commits)
  echo "rest=\$((c1 - c0))" >> "\$out"
fi
# A control's centre on the output: its rect in the window plus the
# layer's own origin.
at() {
  local origin
  origin=\$("$jq_bin" -r '[.[] | .levels[] | .[] | select(.namespace == "formalshell:bar")] | first | "\(.x) \(.y)"' "\$2")
  read -r ox oy <<< "\$origin"
  "$jq_bin" -r --arg on "\$1" --argjson ox "\$ox" --argjson oy "\$oy" \
    '.hits[] | select(.on == \$on) | "\(\$ox + .x + (.w / 2 | floor)) \(\$oy + .y + (.h / 2 | floor))"' "\$3" | head -1
}
click() {
  "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
  "$wlrctl_bin" pointer move \$1 \$2 > /dev/null 2>&1
  sleep 0.4
  "$wlrctl_bin" pointer click left > /dev/null 2>&1
}
read -r nx ny <<< "\$(at panel:network "$sidebar_layers_open" "$sidebar_status_open")"
click "\$nx" "\$ny"
sleep 2
$ipc call panel state > "$sidebar_panel_state" 2>&1
"$grim_bin" "$sidebar_panel_png" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
sleep 1
read -r cx cy <<< "\$(at collapse "$sidebar_layers_open" "$sidebar_status_open")"
click "\$cx" "\$cy"
"$wlrctl_bin" pointer move 600 0 > /dev/null 2>&1
sleep 2
$ipc call sidebar status > "$sidebar_status_rail" 2>&1
"$hyprctl_bin" -j layers > "$sidebar_layers_rail" 2>&1
cp "$state_path" "$sidebar_state_copy" 2>/dev/null
"$grim_bin" "$sidebar_rail_png" > /dev/null 2>&1
$ipc call sidebar expand > /dev/null 2>&1
sleep 2
$ipc call sidebar status > "$sidebar_status_expanded" 2>&1
"$jq_bin" -c '.bar = ((.bar // {}) + {"position": "right"})' "$settings_path" > "$settings_path.new" && mv "$settings_path.new" "$settings_path"
for _ in \$(seq 1 20); do
  $ipc call sidebar status 2>/dev/null | grep -q '"edge":"right"' && break
  sleep 0.5
done
sleep 1
"$hyprctl_bin" -j layers > "$sidebar_layers_right" 2>&1
EOS
  echo 'hl.config({ general = { gaps_out = 10, border_size = 2 } })'
  hypr_exec_once "bash $script"
}

sidebar_box() {
  "$jq_bin" -r '[.[] | .levels[] | .[] | select(.namespace == "formalshell:bar")] | first | "\(.x) \(.y) \(.w) \(.h)"' "$1" 2>/dev/null
}

leg_sidebar_assert() {
  sidebar_rider && return 0
  local f
  for f in "$sidebar_status_open" "$sidebar_layers_open" "$sidebar_monitors" "$sidebar_open_png" "$sidebar_rail_png"; do
    [ -s "$f" ] || fail "sidebar: $f missing or empty"
  done
  echo "SMOKE_SIDEBAR_OPEN $sidebar_open_png"
  echo "SMOKE_SIDEBAR_PANEL $sidebar_panel_png"
  echo "SMOKE_SIDEBAR_RAIL $sidebar_rail_png"
  local mw mh bx by bw bh
  read -r mw mh <<< "$("$jq_bin" -r 'first | "\(.width / .scale | floor) \(.height / .scale | floor)"' "$sidebar_monitors")"
  read -r bx by bw bh <<< "$(sidebar_box "$sidebar_layers_open")"
  echo "sidebar layer open: x=$bx y=$by w=$bw h=$bh on ${mw}x${mh}"
  [ "$bx" -eq 0 ] && [ "$bw" -eq 154 ] && [ "$bh" -eq "$mh" ] \
    || fail "sidebar: the open column is not 154 wide down the left edge: x=$bx y=$by w=$bw h=$bh"
  local cx cw wx outer inner
  read -r cx cw <<< "$("$jq_bin" -r '.hits[] | select(.on == "panel:calendar") | "\(.x) \(.w)"' "$sidebar_status_open" | head -1)"
  wx=$("$jq_bin" -r '[.[] | select(.workspace.id == 1 and .mapped and .hidden != true) | .at[0]] | min' "$sidebar_clients")
  outer=$((bx + cx))
  inner=$((wx - 2 - bx - cx - cw))
  echo "sidebar gaps: screen edge to card $outer, card to window $inner (window at $wx)"
  [ -n "$cw" ] && [ "$wx" != null ] && [ "$inner" -eq "$outer" ] \
    || fail "sidebar: the card sits $outer off the screen's edge but $inner off the window: $(cat "$sidebar_clients")"
  sidebar_clock_case && sidebar_clock_assert "$cx" "$cw"
  "$jq_bin" -e '.collapsed == false and .edge == "left" and .width == 154
      and (.cards | index("time") != null and index("spaces") != null and index("notifications") != null)
      and ([.rows[] | .windows | length] | add) >= 1' "$sidebar_status_open" > /dev/null \
    || fail "sidebar: status did not report the open column's cards and a window on the map: $(cat "$sidebar_status_open")"
  grep -q '^network$' "$sidebar_panel_state" 2>/dev/null \
    || fail "sidebar: the network button's click did not open the network panel: $(cat "$sidebar_panel_state" 2>/dev/null)"
  read -r bx by bw bh <<< "$(sidebar_box "$sidebar_layers_rail")"
  echo "sidebar layer rail: x=$bx y=$by w=$bw h=$bh"
  [ "$bw" -eq 48 ] || fail "sidebar: the collapse button did not fold the column to the 48 wide rail: w=$bw"
  "$jq_bin" -e '.collapsed == true and .width == 48' "$sidebar_status_rail" > /dev/null \
    || fail "sidebar: status after collapse: $(cat "$sidebar_status_rail" 2>/dev/null)"
  "$jq_bin" -e '.sidebarCollapsed == true' "$sidebar_state_copy" > /dev/null 2>&1 \
    || fail "sidebar: state.json does not carry sidebarCollapsed: $(cat "$sidebar_state_copy" 2>/dev/null)"
  "$jq_bin" -e '.collapsed == false and .width == 154' "$sidebar_status_expanded" > /dev/null 2>&1 \
    || fail "sidebar: sidebar expand did not open the column: $(cat "$sidebar_status_expanded" 2>/dev/null)"
  read -r bx by bw bh <<< "$(sidebar_box "$sidebar_layers_right")"
  echo "sidebar layer right: x=$bx y=$by w=$bw h=$bh"
  [ $((bx + bw)) -eq "$mw" ] && [ "$bw" -eq 154 ] \
    || fail "sidebar: bar.position right did not move the column to the right edge: x=$bx w=$bw on $mw"
}

_sidebar_clock() { "$jq_bin" -r "$1" "$sidebar_clock_dump"; }

# The mean of channel $2 (r, g or b) over the open frame's $1 crop.
_sidebar_clock_mean() {
  $convert_bin "$sidebar_open_png" -crop "$1" +repage -format "%[fx:mean.$2]" info:
}

# $1 and $2: the time card's x and width in the window.
sidebar_clock_assert() {
  [ -s "$sidebar_clock_dump" ] && [ -s "$sidebar_clock_results" ] || fail "sidebar: no clock card dump: $(cat "$sidebar_clock_results" 2>/dev/null)"
  echo "sidebar clock $(cat "$sidebar_clock_dump")"
  local ox oy ow oh x y w h q left right mid
  read -r ox oy ow oh < <(sidebar_box "$sidebar_layers_open")
  read -r x y w h < <(_sidebar_clock '.rect | @tsv')
  x=$((ox + x)) y=$((oy + y))
  $convert_bin "$sidebar_open_png" -crop "$(( $2 ))x60+$((ox + $1))+$((y - 15))" +repage -resize 400% "$sidebar_clock_png"
  echo "SMOKE_SIDEBAR_CLOCK $sidebar_clock_png"
  _sidebar_clock '.frosted and .depth and .cut and .face == "pixel"' | grep -qx true \
    || fail "sidebar: the clock card is not the frosted pixel face cut by the subject: $(cat "$sidebar_clock_dump")"
  [ "$w" -gt 20 ] && [ "$h" -gt 8 ] || fail "sidebar: the clock card's digits are ${w}x${h}"
  # Each end of the digits, a quarter of their width: the red half of the
  # wallpaper under the left one, the green under the right.
  q=$((w / 4))
  left=$(awk -v r="$(_sidebar_clock_mean "${q}x${h}+${x}+${y}" r)" -v g="$(_sidebar_clock_mean "${q}x${h}+${x}+${y}" g)" 'BEGIN { printf "%.4f", r - g }')
  right=$(awk -v r="$(_sidebar_clock_mean "${q}x${h}+$((x + w - q))+${y}" r)" -v g="$(_sidebar_clock_mean "${q}x${h}+$((x + w - q))+${y}" g)" 'BEGIN { printf "%.4f", r - g }')
  mid=$($convert_bin "$sidebar_open_png" -format '%[fx:int(255*p{41,'"$((y + h / 2))"'}.r)] %[fx:int(255*p{41,'"$((y + h / 2))"'}.g)] %[fx:int(255*p{41,'"$((y + h / 2))"'}.b)]' info:)
  echo "SMOKE_SIDEBAR_CLOCK_FROST rect=${x},${y},${w},${h} left_r-g=$left right_r-g=$right band=$mid"
  awk -v l="$left" -v r="$right" 'BEGIN { exit !(l - r > 0.03) }' \
    || fail "sidebar: the digits' ends read r-g $left and $right: one flat tint, not the wallpaper under them"
  read -r mr mg mb <<< "$mid"
  [ "$mr" -gt 180 ] && [ "$mg" -gt 70 ] && [ "$mg" -lt 170 ] && [ "$mb" -lt 90 ] \
    || fail "sidebar: the band's subject is not over the digits at x 41: rgb $mid"
  local b bx by sd
  b=$(( $2 * 35 / 136 ))
  bx=$((ox + $1 + $2 - $2 * 10 / 136 - b))
  sd=$($convert_bin "$sidebar_open_png" -crop "$((b - 4))x$((b - 4))+$((bx + 2))+$((y + h / 2 - b / 2 + 2))" +repage -colorspace Gray -format '%[fx:standard_deviation]' info:)
  echo "SMOKE_SIDEBAR_CLOCK_DATE box=${bx} sd=$sd"
  awk -v s="$sd" 'BEGIN { exit !(s > 0.03) }' || fail "sidebar: the date box carries no words (sd $sd)"
  local text period formatted before after
  text=$(_sidebar_clock '.text') period=$(_sidebar_clock '.period // ""') formatted=$(_sidebar_clock '.formatted')
  before=$(sed -n 's/^before=//p' "$sidebar_clock_results") after=$(sed -n 's/^after=//p' "$sidebar_clock_results")
  echo "SMOKE_SIDEBAR_CLOCK_TIME text=$text period=$period formatted=$formatted date=$before"
  [ "$formatted" = "$text $period" ] || fail "sidebar: the card reads '$text' '$period' where the bar clock's format writes '$formatted'"
  [ "$text $period" = "$before" ] || [ "$text $period" = "$after" ] || fail "sidebar: the card reads '$text $period' at $before"
  [ "$(_sidebar_clock '.minuteUs')" -lt 50000 ] || fail "sidebar: the clock card's draw took $(_sidebar_clock '.minuteUs')us"
  [ "$(sed -n 's/^rest=//p' "$sidebar_clock_results")" = 0 ] \
    || fail "sidebar: $(sed -n 's/^rest=//p' "$sidebar_clock_results") bar commits in 3s at rest"
}
