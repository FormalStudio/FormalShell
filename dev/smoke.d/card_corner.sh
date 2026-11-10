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
# lit past the top edge's line. Then the network panel hanging off the
# column (the joined shape, at rest) and the launcher, each opened on that
# output and found as what went dark over the frame before it. The same
# read of the column's card on the session's own output is printed beside
# it. The wallpaper is light enough to stay lighter than the card under the
# launcher's scrim, and the reactive rims are off, which would light the
# border off that wallpaper: the line read is the table's own black.
leg_card_corner_flag="--card-corner"
leg_card_corner_order=177
leg_card_corner_needs="jq convert python3 pil"
leg_card_corner_claims="outputs=shared layout=sidebar"

card_corner_second="HEADLESS-CORNER"
card_corner_flat="$shot_dir/card-corner-flat.png"
card_corner_png="$shot_dir/card-corner.png"
card_corner_own="$shot_dir/card-corner-own.png"
card_corner_bare="$shot_dir/card-corner-bare.png"
card_corner_panel="$shot_dir/card-corner-panel.png"
card_corner_menu="$shot_dir/card-corner-menu.png"
card_corner_log="$shot_dir/card-corner.log"
card_corner_monitors="$shot_dir/card-corner-monitors.json"

leg_card_corner_fixture() {
  $convert_bin -size 1920x1080 xc:'#e0e0e0' "$card_corner_flat"
  settings_pin '.layout = "sidebar" | .theme.reactiveRims = false'
}

leg_card_corner_timing() {
  leg_timing 50
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
"$hyprctl_bin" dispatch "hl.dsp.focus({ monitor = '$card_corner_second' })" >> "$card_corner_log" 2>&1
sleep 1
"$grim_bin" -o "$card_corner_second" "$card_corner_bare" >> "$card_corner_log" 2>&1
$ipc call panel open network >> "$card_corner_log" 2>&1
sleep 3
"$grim_bin" -o "$card_corner_second" "$card_corner_panel" >> "$card_corner_log" 2>&1
$ipc call panel close >> "$card_corner_log" 2>&1
sleep 2
$ipc call menu toggle >> "$card_corner_log" 2>&1
sleep 3
"$grim_bin" -o "$card_corner_second" "$card_corner_menu" >> "$card_corner_log" 2>&1
$ipc call menu toggle >> "$card_corner_log" 2>&1
sleep 1
own=\$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0].name')
"$grim_bin" -o "\$own" "$card_corner_own" >> "$card_corner_log" 2>&1
"$hyprctl_bin" -j monitors > "$card_corner_monitors" 2>> "$card_corner_log"
"$hyprctl_bin" output remove "$card_corner_second" >> "$card_corner_log" 2>&1
EOF
  hypr_exec_once "bash $script"
}

# One frame read, its line printed and a miss kept for the verdict.
card_corner_failed=""
card_corner_read() {
  local out status=0
  [ -s "$1" ] || { card_corner_failed+="no frame $1; "; return; }
  echo "SMOKE_CARD_CORNER_$(echo "$2" | tr a-z A-Z) $1"
  out=$("$python3_bin" "$PWD/dev/card-corner.py" "$1" "$2" "${@:3}" 2>&1) || status=$?
  echo "$out"
  [ "$status" -eq 0 ] || card_corner_failed+="$(grep FAIL <<< "$out" | head -1); "
}

leg_card_corner_assert() {
  [ -s "$card_corner_png" ] || fail "card-corner: no frame of the 1.25 output: $(cat "$card_corner_log" 2>/dev/null)"
  echo "SMOKE_CARD_CORNER $card_corner_png"
  [ -s "$card_corner_own" ] && echo "SMOKE_CARD_CORNER_OWN $card_corner_own"
  [ -s "$card_corner_bare" ] && echo "SMOKE_CARD_CORNER_BARE $card_corner_bare"
  local scale
  scale=$("$jq_bin" -r --arg o "$card_corner_second" '.[] | select(.name == $o) | .scale' "$card_corner_monitors" 2>/dev/null)
  [ "$scale" = 1.25 ] || fail "card-corner: the second output is not at 1.25 (${scale:-none}): $(tr "\n" " " < "$card_corner_log")"
  # Inside the Spaces card, the column's second, 40 logical pixels in
  # from its left edge and 100 down the output, past its top corners.
  "$python3_bin" "$PWD/dev/card-corner.py" "$card_corner_own" own 49 100 || true
  card_corner_read "$card_corner_png" column 61 125
  card_corner_read "$card_corner_panel" panel --over "$card_corner_bare"
  card_corner_read "$card_corner_menu" launcher --over "$card_corner_bare"
  [ -z "$card_corner_failed" ] || fail "card-corner: $card_corner_failed"
}
