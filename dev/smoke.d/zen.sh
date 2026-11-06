# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --zen: `zen enter` under `layout: sidebar` and the scrolling layout, three
# foot windows a b c in one column each on workspace 1 with b focused. On:
# b floating alone on a workspace of its own, centred on the output within
# 2px, the sidebar folded to its rail, and a, c still on workspace 1. Off:
# b tiled on workspace 1 again between a and c (by x), focused, the sidebar
# open. Then zen on again and b closed under it: zen off and the sidebar
# open. The on and off frames are pulled.
leg_zen_flag="--zen"
leg_zen_order=239
leg_zen_needs="jq foot"

zen_status=("$shot_dir"/zen-status-{before,on,off,closed}.json)
zen_clients=("$shot_dir"/zen-clients-{before,on,off}.json)
zen_sidebar=("$shot_dir"/zen-sidebar-{before,on,off,closed}.json)
zen_monitors="$shot_dir/zen-monitors.json"
zen_on_png="$shot_dir/zen-on.png"
zen_off_png="$shot_dir/zen-off.png"

leg_zen_validate() {
  if leg_on bar_position || leg_on frame; then
    echo "usage: --zen runs under layout sidebar and cannot combine with --bar-position or --frame" >&2
    exit 1
  fi
}

leg_zen_fixture() {
  leg_on sidebar || settings_fragment ', "layout": "sidebar"'
}

leg_zen_timing() {
  leg_timing 30 80 0 40
}

leg_zen_drive() {
  local script="$shot_dir/zen-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
for _ in \$(seq 1 60); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.25
done
"$hyprctl_bin" eval 'hl.config({ general = { layout = "scrolling" }, scrolling = { column_width = 0.5 } })' > /dev/null 2>&1
for n in a b c; do
  "$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-zen-\$n sh -c 'sleep 300']==])" > /dev/null 2>&1
  for _ in \$(seq 1 40); do
    "$hyprctl_bin" -j clients | "$jq_bin" -e --arg c "formalshell-zen-\$n" 'any(.[]; .class == \$c)' > /dev/null 2>&1 && break
    sleep 0.1
  done
  sleep 0.4
done
"$hyprctl_bin" dispatch 'hl.dsp.focus({ window = "class:formalshell-zen-b" })' > /dev/null 2>&1
sleep 1
"$hyprctl_bin" -j monitors > "$zen_monitors" 2>&1
$ipc call zen status > "${zen_status[0]}" 2>&1
$ipc call sidebar status > "${zen_sidebar[0]}" 2>&1
"$hyprctl_bin" -j clients > "${zen_clients[0]}" 2>&1
$ipc call zen enter > /dev/null 2>&1
sleep 2
$ipc call zen status > "${zen_status[1]}" 2>&1
$ipc call sidebar status > "${zen_sidebar[1]}" 2>&1
"$hyprctl_bin" -j clients > "${zen_clients[1]}" 2>&1
"$grim_bin" "$zen_on_png" > /dev/null 2>&1
$ipc call zen exit > /dev/null 2>&1
sleep 2
$ipc call zen status > "${zen_status[2]}" 2>&1
$ipc call sidebar status > "${zen_sidebar[2]}" 2>&1
"$hyprctl_bin" -j clients > "${zen_clients[2]}" 2>&1
"$hyprctl_bin" -j activewindow > "$shot_dir/zen-active.json" 2>&1
"$grim_bin" "$zen_off_png" > /dev/null 2>&1
$ipc call zen toggle > /dev/null 2>&1
sleep 2
"$hyprctl_bin" dispatch 'hl.dsp.window.close({ window = "class:formalshell-zen-b" })' > /dev/null 2>&1
sleep 2
$ipc call zen status > "${zen_status[3]}" 2>&1
$ipc call sidebar status > "${zen_sidebar[3]}" 2>&1
EOS
  hypr_exec_once "bash $script"
}

zen_client() {
  "$jq_bin" -c --arg c "formalshell-zen-$2" '.[] | select(.class == $c)' "$1"
}

leg_zen_assert() {
  local f b mon cx ws order
  for f in "${zen_status[@]}" "${zen_sidebar[@]}" "${zen_clients[@]}" "$zen_monitors"; do
    [ -s "$f" ] || fail "zen: nothing written to $f"
  done
  for f in "${zen_status[@]}"; do echo "zen status $(basename "$f"): $(cat "$f")"; done

  "$jq_bin" -e '.on == false' "${zen_status[0]}" > /dev/null || fail "zen: on before enter: $(cat "${zen_status[0]}")"
  "$jq_bin" -e '.collapsed == false' "${zen_sidebar[0]}" > /dev/null || fail "zen: the sidebar started folded"
  b=$(zen_client "${zen_clients[0]}" b)
  "$jq_bin" -e '.floating == false and .workspace.id == 1' <<< "$b" > /dev/null || fail "zen: b did not start tiled on workspace 1: $b"

  # On.
  "$jq_bin" -e '.on == true and .before != null' "${zen_status[1]}" > /dev/null \
    || fail "zen: not on, or no column to go back before: $(cat "${zen_status[1]}")"
  "$jq_bin" -e '.collapsed == true' "${zen_sidebar[1]}" > /dev/null || fail "zen: the sidebar did not fold to its rail"
  b=$(zen_client "${zen_clients[1]}" b)
  echo "zen on b: $b"
  "$jq_bin" -e '.floating == true and .workspace.id != 1' <<< "$b" > /dev/null || fail "zen: b is not floating off workspace 1: $b"
  ws=$("$jq_bin" -r '.workspace.id' <<< "$b")
  "$jq_bin" -e --argjson ws "$ws" '[.[] | select(.workspace.id == $ws)] | length == 1' "${zen_clients[1]}" > /dev/null \
    || fail "zen: b is not alone on workspace $ws"
  for n in a c; do
    zen_client "${zen_clients[1]}" "$n" | "$jq_bin" -e '.workspace.id == 1' > /dev/null || fail "zen: $n left workspace 1"
  done
  mon=$("$jq_bin" -c '.[0] | {x, y, w: (.width / .scale), h: (.height / .scale)}' "$zen_monitors")
  cx=$("$jq_bin" -n --argjson b "$b" --argjson m "$mon" \
    '[(($b.at[0] + $b.size[0] / 2) - ($m.x + $m.w / 2)), (($b.at[1] + $b.size[1] / 2) - ($m.y + $m.h / 2))] | map(fabs) | max')
  echo "zen: b off the output's centre by $cx px (monitor $mon)"
  "$jq_bin" -e -n --argjson d "$cx" '$d <= 2' > /dev/null || fail "zen: b is $cx px off the output's centre"

  # Off.
  "$jq_bin" -e '.on == false' "${zen_status[2]}" > /dev/null || fail "zen: still on after exit"
  "$jq_bin" -e '.collapsed == false' "${zen_sidebar[2]}" > /dev/null || fail "zen: the sidebar stayed folded after exit"
  b=$(zen_client "${zen_clients[2]}" b)
  "$jq_bin" -e '.floating == false and .workspace.id == 1' <<< "$b" > /dev/null || fail "zen: b is not tiled on workspace 1 again: $b"
  order=$("$jq_bin" -r '[.[] | select(.workspace.id == 1 and (.class | startswith("formalshell-zen-")))] | sort_by(.at[0]) | map(.class | ltrimstr("formalshell-zen-")) | join(" ")' "${zen_clients[2]}")
  echo "zen: tape after exit: $order"
  [ "$order" = "a b c" ] || fail "zen: b did not go back between a and c: $order"
  "$jq_bin" -e '.class == "formalshell-zen-b"' "$shot_dir/zen-active.json" > /dev/null || fail "zen: b is not focused after exit"

  # Closed under zen.
  "$jq_bin" -e '.on == false' "${zen_status[3]}" > /dev/null || fail "zen: still on after its window closed"
  "$jq_bin" -e '.collapsed == false' "${zen_sidebar[3]}" > /dev/null || fail "zen: the sidebar stayed folded after the zen window closed"

  echo "SMOKE_ZEN_ON $zen_on_png"
  echo "SMOKE_ZEN_OFF $zen_off_png"
}
