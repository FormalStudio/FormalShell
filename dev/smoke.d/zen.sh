# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --zen: `zen enter` under `layout: sidebar` and the scrolling layout, three
# foot windows a b c in one column each on workspace 1, columns 0.4 wide
# and b's widened to 0.6, b focused. On: b floating on workspace 1, centred
# on the output within 2px, the sidebar folded to its rail, and a and c
# still on workspace 1 but wholly off the output (their tape of 0.8 is
# narrower than the output, the case Hyprland centres on every
# recalculate). Under formalwm (wm_zen=layout) b stays tiled instead, a goes
# off the left edge and c off the right, and the columns keep the settings'
# width, there being no socket call for one. Off: b tiled again between a and c (by x), at the width it
# had within 2px, focused and on the output, the sidebar open. Then zen on
# again and b closed under it: zen off, the sidebar open, and the window
# that took focus back on the output. The on and off frames are pulled.
leg_zen_flag="--zen"
leg_zen_order=239
leg_zen_compositors="hyprland shoji"
leg_zen_needs="jq foot"
leg_zen_claims="layout=sidebar frame=none"

zen_status=("$shot_dir"/zen-status-{before,on,off,closed}.json)
zen_clients=("$shot_dir"/zen-clients-{before,on,off,closed}.json)
zen_sidebar=("$shot_dir"/zen-sidebar-{before,on,off,closed}.json)
zen_monitors="$shot_dir/zen-monitors.json"
zen_on_png="$shot_dir/zen-on.png"
zen_off_png="$shot_dir/zen-off.png"

leg_zen_fixture() {
  settings_pin '.layout = "sidebar"'
}

leg_zen_timing() {
  leg_timing 80
}

leg_zen_drive() {
  local script="$shot_dir/zen-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
for _ in \$(seq 1 60); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.25
done
"$wmctl_bin" eval 'hl.config({ general = { layout = "scrolling" }, scrolling = { column_width = 0.4 } })' > /dev/null 2>&1
for n in a b c; do
  "$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-zen-\$n sh -c 'sleep 300']==])" > /dev/null 2>&1
  for _ in \$(seq 1 40); do
    "$wmctl_bin" -j clients | "$jq_bin" -e --arg c "formalshell-zen-\$n" 'any(.[]; .class == \$c)' > /dev/null 2>&1 && break
    sleep 0.1
  done
  sleep 0.4
done
"$wmctl_bin" dispatch 'hl.dsp.focus({ window = "class:formalshell-zen-b" })' > /dev/null 2>&1
"$wmctl_bin" dispatch 'hl.dsp.layout("colresize 0.6")' > /dev/null 2>&1
sleep 1
"$wmctl_bin" -j monitors > "$zen_monitors" 2>&1
$ipc call zen status > "${zen_status[0]}" 2>&1
$ipc call sidebar status > "${zen_sidebar[0]}" 2>&1
"$wmctl_bin" -j clients > "${zen_clients[0]}" 2>&1
$ipc call zen enter > /dev/null 2>&1
sleep 2
$ipc call zen status > "${zen_status[1]}" 2>&1
$ipc call sidebar status > "${zen_sidebar[1]}" 2>&1
"$wmctl_bin" -j clients > "${zen_clients[1]}" 2>&1
"$grim_bin" "$zen_on_png" > /dev/null 2>&1
$ipc call zen exit > /dev/null 2>&1
sleep 2
$ipc call zen status > "${zen_status[2]}" 2>&1
$ipc call sidebar status > "${zen_sidebar[2]}" 2>&1
"$wmctl_bin" -j clients > "${zen_clients[2]}" 2>&1
"$wmctl_bin" -j activewindow > "$shot_dir/zen-active.json" 2>&1
"$grim_bin" "$zen_off_png" > /dev/null 2>&1
$ipc call zen toggle > /dev/null 2>&1
sleep 2
"$wmctl_bin" dispatch 'hl.dsp.window.close({ window = "class:formalshell-zen-b" })' > /dev/null 2>&1
sleep 2
$ipc call zen status > "${zen_status[3]}" 2>&1
$ipc call sidebar status > "${zen_sidebar[3]}" 2>&1
"$wmctl_bin" -j clients > "${zen_clients[3]}" 2>&1
"$wmctl_bin" -j activewindow > "$shot_dir/zen-active-closed.json" 2>&1
EOS
  wm_exec_once "bash $script"
}

zen_client() {
  "$jq_bin" -c --arg c "formalshell-zen-$2" '.[] | select(.class == $c)' "$1"
}

zen_on_output() {
  "$jq_bin" -e -n --argjson w "$1" --argjson m "$2" '$w.at[0] >= $m.x and $w.at[0] + $w.size[0] <= $m.x + $m.w' > /dev/null
}

leg_zen_assert() {
  local f b b0 mon cx order n
  for f in "${zen_status[@]}" "${zen_sidebar[@]}" "${zen_clients[@]}" "$zen_monitors"; do
    [ -s "$f" ] || fail "zen: nothing written to $f"
  done
  for f in "${zen_status[@]}"; do echo "zen status $(basename "$f"): $(cat "$f")"; done
  mon=$("$jq_bin" -c '.[0] | {x, y, w: (.width / .scale), h: (.height / .scale)}' "$zen_monitors")

  "$jq_bin" -e '.on == false' "${zen_status[0]}" > /dev/null || fail "zen: on before enter: $(cat "${zen_status[0]}")"
  "$jq_bin" -e '.collapsed == false' "${zen_sidebar[0]}" > /dev/null || fail "zen: the sidebar started folded"
  b0=$(zen_client "${zen_clients[0]}" b)
  "$jq_bin" -e '.floating == false and .workspace.id == 1' <<< "$b0" > /dev/null || fail "zen: b did not start tiled on workspace 1: $b0"

  # On.
  if [ "$wm_zen" = float ]; then
    "$jq_bin" -e '.on == true and .before != null' "${zen_status[1]}" > /dev/null \
      || fail "zen: not on, or no column to go back before: $(cat "${zen_status[1]}")"
  else
    "$jq_bin" -e '.on == true' "${zen_status[1]}" > /dev/null || fail "zen: not on: $(cat "${zen_status[1]}")"
  fi
  "$jq_bin" -e '.collapsed == true' "${zen_sidebar[1]}" > /dev/null || fail "zen: the sidebar did not fold to its rail"
  b=$(zen_client "${zen_clients[1]}" b)
  echo "zen on b: $("$jq_bin" -c '{at, size, floating, ws: .workspace.id}' <<< "$b")"
  "$jq_bin" -e --argjson fl "$([ "$wm_zen" = float ] && echo true || echo false)" '.floating == $fl and .workspace.id == 1' <<< "$b" > /dev/null \
    || fail "zen: b is not $([ "$wm_zen" = float ] && echo floating || echo tiled) on workspace 1: $b"
  for n in a c; do
    f=$(zen_client "${zen_clients[1]}" "$n")
    echo "zen on $n: $("$jq_bin" -c '{at, size, ws: .workspace.id}' <<< "$f")"
    if [ "$wm_zen" = layout ] && [ "$n" = c ]; then
      "$jq_bin" -e --argjson m "$mon" '.workspace.id == 1 and .at[0] >= $m.x + $m.w' <<< "$f" > /dev/null \
        || fail "zen: c is not off the output's right edge on workspace 1"
      continue
    fi
    "$jq_bin" -e --argjson m "$mon" '.workspace.id == 1 and .at[0] + .size[0] <= $m.x' <<< "$f" > /dev/null \
      || fail "zen: $n is not off the output's left edge on workspace 1"
  done
  cx=$("$jq_bin" -n --argjson b "$b" --argjson m "$mon" \
    '[(($b.at[0] + $b.size[0] / 2) - ($m.x + $m.w / 2)), (($b.at[1] + $b.size[1] / 2) - ($m.y + $m.h / 2))] | map(fabs) | max')
  echo "zen: b off the output's centre by $cx px (monitor $mon)"
  "$jq_bin" -e -n --argjson d "$cx" '$d <= 2' > /dev/null || fail "zen: b is $cx px off the output's centre"

  # Off.
  "$jq_bin" -e '.on == false' "${zen_status[2]}" > /dev/null || fail "zen: still on after exit"
  "$jq_bin" -e '.collapsed == false' "${zen_sidebar[2]}" > /dev/null || fail "zen: the sidebar stayed folded after exit"
  b=$(zen_client "${zen_clients[2]}" b)
  echo "zen off b: $("$jq_bin" -c '{at, size, floating}' <<< "$b") (before $("$jq_bin" -c '.size' <<< "$b0"))"
  "$jq_bin" -e '.floating == false and .workspace.id == 1' <<< "$b" > /dev/null || fail "zen: b is not tiled on workspace 1 again: $b"
  "$jq_bin" -e -n --argjson b "$b" --argjson b0 "$b0" '($b.size[0] - $b0.size[0]) | fabs <= 2' > /dev/null \
    || fail "zen: b came back $("$jq_bin" -c '.size[0]' <<< "$b") wide, not $("$jq_bin" -c '.size[0]' <<< "$b0")"
  zen_on_output "$b" "$mon" || fail "zen: b is not on the output after exit"
  order=$("$jq_bin" -r '[.[] | select(.workspace.id == 1 and (.class | startswith("formalshell-zen-")))] | sort_by(.at[0]) | map(.class | ltrimstr("formalshell-zen-")) | join(" ")' "${zen_clients[2]}")
  echo "zen: tape after exit: $order"
  [ "$order" = "a b c" ] || fail "zen: b did not go back between a and c: $order"
  "$jq_bin" -e '.class == "formalshell-zen-b"' "$shot_dir/zen-active.json" > /dev/null || fail "zen: b is not focused after exit"

  "$jq_bin" -e '.on == false' "${zen_status[3]}" > /dev/null || fail "zen: still on after its window closed"
  "$jq_bin" -e '.collapsed == false' "${zen_sidebar[3]}" > /dev/null || fail "zen: the sidebar stayed folded after the zen window closed"
  n=$("$jq_bin" -r '.class' "$shot_dir/zen-active-closed.json" | sed 's/^formalshell-zen-//')
  f=$(zen_client "${zen_clients[3]}" "$n")
  echo "zen closed: focus on $n $("$jq_bin" -c '{at, size}' <<< "$f")"
  zen_on_output "$f" "$mon" || fail "zen: the tape stayed off the output after the zen window closed"

  echo "SMOKE_ZEN_ON $zen_on_png"
  echo "SMOKE_ZEN_OFF $zen_off_png"
}
