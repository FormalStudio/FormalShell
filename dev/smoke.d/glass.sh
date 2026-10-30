# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --glass: the cards' translucency following the compositor's own blur
# switch (docs/superpowers/specs/2026-10-30-activity.md section 6). The rig
# boots Hyprland with `decoration.blur.enabled` false, so `debug dump`
# reports `blur` false and `surfaceOpacity` 1 whatever the default asks.
# Then the session's own config is rewritten with blur on and reloaded:
# the shell reads `getoption` again off `configreloaded` and reports `blur`
# true and 0.85. Over a flat wallpaper the far side of the mode's cards (bright
# under dark mode, dark under light), the audio panel and a sticky toast are
# photographed in both states, and their cards letting the wallpaper through
# changes at least 2% of the screen's right half.
leg_glass_flag="--glass"
leg_glass_order=236
leg_glass_needs="jq convert notify-send"

glass_bright="$shot_dir/glass-bright.png"
glass_dark="$shot_dir/glass-dark.png"
glass_opaque_path="$shot_dir/glass-opaque.png"
glass_clear_path="$shot_dir/glass-clear.png"
glass_dump_opaque="$shot_dir/glass-dump-opaque.json"
glass_dump_clear="$shot_dir/glass-dump-clear.json"

leg_glass_fixture() {
  $convert_bin -size 1920x1080 xc:'#e6e6e6' "$glass_bright"
  $convert_bin -size 1920x1080 xc:'#202020' "$glass_dark"
}

leg_glass_timing() {
  leg_timing 30 60
}

leg_glass_drive() {
  local script="$shot_dir/glass-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3
wp="$glass_bright"
$ipc call theme status 2>/dev/null | jq -e '.mode == "light"' > /dev/null && wp="$glass_dark"
$ipc call wallpaper set "\$wp" > /dev/null 2>&1
sleep 8
"$notify_send_bin" -t 0 "Glass" "A card over a flat field"
$ipc call panel open audio > /dev/null 2>&1
sleep 1.5
$ipc call debug dump > "$glass_dump_opaque" 2>&1
"$grim_bin" "$glass_opaque_path" > /dev/null 2>&1
sed -i 's/blur = { enabled = false }/blur = { enabled = true }/' "$cfg"
"$hyprctl_bin" reload > /dev/null 2>&1
sleep 2
$ipc call debug dump > "$glass_dump_clear" 2>&1
"$grim_bin" "$glass_clear_path" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_glass_assert() {
  local f
  for f in "$glass_dump_opaque" "$glass_dump_clear" "$glass_opaque_path" "$glass_clear_path"; do
    [ -s "$f" ] || fail "glass: $f missing or empty"
  done
  echo "SMOKE_GLASS_OPAQUE $glass_opaque_path"
  echo "SMOKE_GLASS_CLEAR $glass_clear_path"
  jq -e '.theme.blur == false and .theme.surfaceOpacity == 1' "$glass_dump_opaque" > /dev/null \
    || fail "glass: with blur off the cards are not opaque: $(jq -c '.theme | {blur, surfaceOpacity}' "$glass_dump_opaque")"
  jq -e '.theme.blur == true and .theme.surfaceOpacity == 0.85' "$glass_dump_clear" > /dev/null \
    || fail "glass: blur on after the reload did not reach the theme: $(jq -c '.theme | {blur, surfaceOpacity}' "$glass_dump_clear")"
  # The share of the right half that changed between the two frames, a
  # fuzz past the encoder's own noise: the cards and nothing else move.
  local crop="960x1040+960+40" changed
  changed=$($convert_bin "$glass_opaque_path" "$glass_clear_path" -crop "$crop" +repage -compose difference -composite \
    -colorspace Gray -fx 'p > 0.05' -format '%[fx:mean]' info:)
  echo "SMOKE_GLASS_CHANGED $changed"
  awk -v c="$changed" 'BEGIN { exit !(c > 0.02) }' \
    || fail "glass: the cards did not let the wallpaper through with blur on (changed share $changed)"
}
