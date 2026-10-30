# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --launcher-glass: the see-through launcher card blurring what is behind
# it. The rig boots with no layer rules and blur off; this leg loads the
# shipped docs/examples/hyprland/formalshell.lua rules (`chrome.blur` true)
# and turns blur on, so the run reads the config the owner installs. A foot
# window full of high-contrast text covers the output; the launcher is
# photographed open over it, then again over the same flat wallpaper with
# the window moved to workspace 2. The difference between the two frames
# is what shows through: inside the card it has to be blurred (its edge
# energy under a tenth of the same difference under the scrim beside it,
# which is darkened and sharp by design), where a card with no blur under
# it lets the text through crisp at 1 - surfaceOpacity. Ridden by
# `--no-morph` it reads the popover card, which draws its look on
# viewport-cropped subsurfaces.
leg_launcher_glass_flag="--launcher-glass"
leg_launcher_glass_order=239
leg_launcher_glass_needs="jq convert foot python3"

launcher_glass_flat="$shot_dir/launcher-glass-flat.png"
launcher_glass_text="$shot_dir/launcher-glass-text.png"
launcher_glass_bare="$shot_dir/launcher-glass-bare.png"
launcher_glass_dump="$shot_dir/launcher-glass-dump.json"
launcher_glass_numbers="$shot_dir/launcher-glass-numbers.txt"

leg_launcher_glass_fixture() {
  $convert_bin -size 1920x1080 xc:'#303030' "$launcher_glass_flat"
}

leg_launcher_glass_timing() {
  leg_timing 30 60
}

leg_launcher_glass_drive() {
  # The shipped rules, as the owner's hyprland.lua loads them.
  echo 'local chrome = { blur = true }'
  awk '/^hl.layer_rule\(\{/ { on = 1 } on { print } on && /^\}\)/ { on = 0 }' docs/examples/hyprland/formalshell.lua
  echo 'hl.config({ decoration = { blur = { enabled = true, size = 8, passes = 2, new_optimizations = true } } })'
  local script="$shot_dir/launcher-glass-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 2
$ipc call wallpaper set "$launcher_glass_flat" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-glass sh -c 'for i in \\\$(seq 1 3000); do printf \"WMWMWM ||| ### IIII mmmm \"; done; sleep 300']==])" > /dev/null 2>&1
sleep 6
$ipc call menu toggle > /dev/null 2>&1
sleep 1.5
$ipc call debug dump > "$launcher_glass_dump" 2>&1
"$grim_bin" "$launcher_glass_text" > /dev/null 2>&1
$ipc call menu toggle > /dev/null 2>&1
sleep 0.5
"$hyprctl_bin" dispatch "hl.dsp.window.move({ window = \"class:formalshell-glass\", workspace = 2, follow = false })" > /dev/null 2>&1
sleep 1
$ipc call menu toggle > /dev/null 2>&1
sleep 1.5
"$grim_bin" "$launcher_glass_bare" > /dev/null 2>&1
$ipc call menu toggle > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# Edge energy of what changed between the two frames inside a box
# (`x y w h`).
launcher_glass_leak() {
  "$python3_bin" - "$launcher_glass_text" "$launcher_glass_bare" "$@" <<'PY'
import sys
from PIL import Image, ImageChops, ImageFilter, ImageStat
a, b = (Image.open(p).convert("L") for p in sys.argv[1:3])
x, y, w, h = map(int, sys.argv[3:7])
d = ImageChops.difference(a, b).crop((x, y, x + w, y + h))
print(ImageStat.Stat(d.filter(ImageFilter.FIND_EDGES)).mean[0])
PY
}

leg_launcher_glass_assert() {
  local f
  for f in "$launcher_glass_dump" "$launcher_glass_text" "$launcher_glass_bare"; do
    [ -s "$f" ] || fail "launcher-glass: $f missing or empty"
  done
  echo "SMOKE_LAUNCHER_GLASS_TEXT $launcher_glass_text"
  echo "SMOKE_LAUNCHER_GLASS_BARE $launcher_glass_bare"
  jq -e '.theme.blur == true and .theme.surfaceOpacity < 1' "$launcher_glass_dump" > /dev/null \
    || fail "launcher-glass: blur on did not make the cards translucent: $(jq -c '.theme | {blur, surfaceOpacity}' "$launcher_glass_dump")"
  local x y w h
  read -r x y w h <<< "$(jq -r '.modals[] | select(.namespace == "formalshell:menu") | .card | "\(.x) \(.y) \(.width) \(.height)"' "$launcher_glass_dump")"
  [ -n "$h" ] || fail "launcher-glass: no launcher card in the dump"
  # Inside the card, short of its border and its content's first rows: the
  # lower third, where the root lists nothing past its rows on a short
  # tree. Beside it, the scrim over the same text.
  local inside="$((x + 20)) $((y + h * 2 / 3)) $((w - 40)) $((h / 3 - 20))"
  local beside="$((x + w + 40)) $((y + h * 2 / 3)) 300 $((h / 3 - 20))"
  local card scrim
  # shellcheck disable=SC2086  # four numbers, split on purpose
  card=$(launcher_glass_leak $inside)
  # shellcheck disable=SC2086
  scrim=$(launcher_glass_leak $beside)
  echo "card $card scrim $scrim" | tee "$launcher_glass_numbers"
  echo "SMOKE_LAUNCHER_GLASS_LEAK card=$card scrim=$scrim"
  awk -v c="$card" -v s="$scrim" 'BEGIN { exit !(s > 0 && c < s / 10) }' \
    || fail "launcher-glass: the text behind the card shows through sharp (card $card against scrim $scrim): no blur under it"
}
