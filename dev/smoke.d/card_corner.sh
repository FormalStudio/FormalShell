# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --card-corner: a card's 1px rim on an output at 1.25, g815's laptop panel.
# The session's own output stays at its scale and a headless second one is
# made at 1.25 beside it, `layout: sidebar` putting a column of cards on it
# over a flat light wallpaper. The column's cards sit 9 off the screen, so
# their right edge lands at 181.25 output pixels: drawn where the logical
# edge falls, the border spreads over two pixels at half depth and the
# card's lit inner line with it, which read as a soft doubled rim. The
# frame of that output is read by dev/card-corner.py, the shared check: the
# top and right borders one whole pixel each with the wallpaper side of them
# clearly lighter, the lit line one pixel, and nothing on the top-right arc
# lit past the top edge's line. The same read on the session's own output is
# printed beside it.
leg_card_corner_flag="--card-corner"
leg_card_corner_order=177
leg_card_corner_needs="jq convert python3"

card_corner_second="HEADLESS-CORNER"
card_corner_flat="$shot_dir/card-corner-flat.png"
card_corner_png="$shot_dir/card-corner.png"
card_corner_own="$shot_dir/card-corner-own.png"
card_corner_log="$shot_dir/card-corner.log"
card_corner_monitors="$shot_dir/card-corner-monitors.json"

leg_card_corner_validate() {
  if leg_on multi_output || leg_on display || leg_on brightness || leg_on launcher_density; then
    echo "usage: --card-corner makes its own second output and cannot combine with legs that add or remove outputs" >&2
    exit 1
  fi
}

leg_card_corner_fixture() {
  $convert_bin -size 1920x1080 xc:'#b0b0b0' "$card_corner_flat"
  settings_fragment ', "layout": "sidebar"'
}

leg_card_corner_timing() {
  leg_timing 20 50
}

leg_card_corner_drive() {
  local script="$shot_dir/card-corner-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
for _ in \$(seq 1 40); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.5
done
# First: the recolour reloads Hyprland's config, which drops an output
# rule made at runtime.
$ipc call wallpaper set "$card_corner_flat" > "$card_corner_log" 2>&1
sleep 4
width=\$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0].width')
"$hyprctl_bin" output create headless "$card_corner_second" >> "$card_corner_log" 2>&1
"$hyprctl_bin" eval "hl.monitor({ output = \\"$card_corner_second\\", mode = \\"1600x1000@60\\", position = \\"\${width}x0\\", scale = 1.25 })" >> "$card_corner_log" 2>&1
sleep 3
"$grim_bin" -o "$card_corner_second" "$card_corner_png" >> "$card_corner_log" 2>&1
own=\$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0].name')
"$grim_bin" -o "\$own" "$card_corner_own" >> "$card_corner_log" 2>&1
"$hyprctl_bin" -j monitors > "$card_corner_monitors" 2>> "$card_corner_log"
"$hyprctl_bin" output remove "$card_corner_second" >> "$card_corner_log" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_card_corner_assert() {
  [ -s "$card_corner_png" ] || fail "card-corner: no frame of the 1.25 output: $(cat "$card_corner_log" 2>/dev/null)"
  echo "SMOKE_CARD_CORNER $card_corner_png"
  [ -s "$card_corner_own" ] && echo "SMOKE_CARD_CORNER_OWN $card_corner_own"
  local scale out status=0
  scale=$("$jq_bin" -r --arg o "$card_corner_second" '.[] | select(.name == $o) | .scale' "$card_corner_monitors" 2>/dev/null)
  [ "$scale" = 1.25 ] || fail "card-corner: the second output is not at 1.25 (${scale:-none}): $(tr "\n" " " < "$card_corner_log")"
  # Inside the Spaces card, the column's second, 40 logical pixels in
  # from its left edge and 100 down the output, past its top corners.
  "$python3_bin" "$PWD/dev/card-corner.py" "$card_corner_own" 49 100 own || true
  out=$("$python3_bin" "$PWD/dev/card-corner.py" "$card_corner_png" 61 125 "1.25" 2>&1) || status=$?
  echo "$out"
  [ "$status" -eq 0 ] || fail "card-corner: $(grep FAIL <<< "$out" | head -1)"
}
