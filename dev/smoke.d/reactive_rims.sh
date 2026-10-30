# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --reactive-rims: a card's rim taking light off what is around it
# (docs/superpowers/specs/2026-10-30-reactive-rims.md). A wallpaper red on
# its left half and blue on its right, `audio` alone in the left region and
# `network` alone in the right, so one card rests over each half:
#
# - The audio card's rim leans red and the network card's blue, in `debug
#   dump`'s `rims.panel` and in the frame: the 1px column on each card's
#   left and right edge, read in both frames, carries more red than blue
#   over the red half and the other way over the blue, by more than the
#   card's own fill a few pixels inside it does.
# - Five seconds with the network card open and nothing moving: no commit
#   on the panel's surface (the shell's own `commit surface=panel` lines),
#   while the rims keep asking the compositor for the windows.
# - A floating foot window moved by Hyprland's own dispatcher to just left
#   of the network card, which raises no event of its own: the card's left
#   side lifts in the dump and its left rim column brightens in the frame,
#   read along the stretch the window spans. The light spills onto the
#   fill: a column 10px inside the lit edge is brighter than one 10px
#   inside the far edge, moved by less than the rim itself was, and 48px
#   in (past the spill's depth) not moved at all.
# - settings.json retargeted to `theme.reactiveRims: false` under the
#   running shell: `rims.enabled` false and the panel's rim gone.
leg_reactive_rims_flag="--reactive-rims"
leg_reactive_rims_order=79
leg_reactive_rims_needs="convert jq foot"

reactive_rims_wp="$shot_dir/reactive-rims-wallpaper.png"
reactive_rims_left_png="$shot_dir/reactive-rims-left.png"
reactive_rims_right_png="$shot_dir/reactive-rims-right.png"
reactive_rims_lit_png="$shot_dir/reactive-rims-window.png"
reactive_rims_off_png="$shot_dir/reactive-rims-off.png"
reactive_rims_left_json="$shot_dir/reactive-rims-left.json"
reactive_rims_right_json="$shot_dir/reactive-rims-right.json"
reactive_rims_lit_json="$shot_dir/reactive-rims-window.json"
reactive_rims_off_json="$shot_dir/reactive-rims-off.json"
reactive_rims_rest_path="$shot_dir/reactive-rims-rest.txt"
reactive_rims_dispatch_path="$shot_dir/reactive-rims-dispatch.txt"
reactive_rims_gen1_path="$shot_dir/reactive-rims-settings-1.json"
reactive_rims_gen2_path="$shot_dir/reactive-rims-settings-2.json"

leg_reactive_rims_validate() {
  local other
  for other in bar_layout bar_position chevron config_reload lyrics_blur mic monitor panel_anchor panel_handoff systemupdate tray_overflow visualizer; do
    if leg_on "$other"; then
      echo "usage: --reactive-rims carries its own bar.layout and settings.json and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_reactive_rims_fixture() {
  $convert_bin -size 1920x1080 xc:'#ff0000' -fill '#0000ff' \
    -draw "rectangle 960,0 1919,1079" "$reactive_rims_wp"
  settings_fragment ', "bar": {"layout": {"left": ["audio"], "right": ["network"]}}'
}

leg_reactive_rims_timing() {
  leg_timing 40 90
}

leg_reactive_rims_drive() {
  local script="$shot_dir/reactive-rims-drive.sh"
  local settings_path="$iso_home/.config/formalshell/settings.json"
  local win="class:^(formalshell-rims)\$"
  write_script "$script" <<EOS
#!/usr/bin/env bash
commits() { grep -c '^commit surface=panel ' "$shell_log_path"; }
sleep 4
# The window first, floated well clear of both cards, so nothing later
# takes focus from an open panel.
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-rims sh -c 'sleep 300']==])" >> "$reactive_rims_dispatch_path" 2>&1
sleep 1.5
"$hyprctl_bin" dispatch "hl.dsp.window.float({ window = '$win', action = 'toggle' })" >> "$reactive_rims_dispatch_path" 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.resize({ x = 300, y = 200, window = '$win' })" >> "$reactive_rims_dispatch_path" 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.move({ x = 860, y = 820, window = '$win' })" >> "$reactive_rims_dispatch_path" 2>&1
$ipc call wallpaper set "$reactive_rims_wp" > /dev/null 2>&1
sleep 6
$ipc call panel toggle audio > /dev/null 2>&1
sleep 2
$ipc call debug dump > "$reactive_rims_left_json" 2>&1
"$grim_bin" "$reactive_rims_left_png" > /dev/null 2>&1
$ipc call panel toggle network > /dev/null 2>&1
sleep 2.5
$ipc call debug dump > "$reactive_rims_right_json" 2>&1
"$grim_bin" "$reactive_rims_right_png" > /dev/null 2>&1
c0=\$(commits)
sleep 5
c1=\$(commits)
echo "\$c0 \$c1" > "$reactive_rims_rest_path"
# Just left of the network card, its top a little under the card's.
read -r x0 y0 < <("$jq_bin" -r '.rims.panel.rect | "\(.[0] | floor) \(.[1] | floor)"' "$reactive_rims_right_json")
"$hyprctl_bin" dispatch "hl.dsp.window.move({ x = \$((x0 - 312)), y = \$((y0 + 24)), window = '$win' })" >> "$reactive_rims_dispatch_path" 2>&1
sleep 1.5
"$hyprctl_bin" -j getoption general:col.active_border >> "$reactive_rims_dispatch_path" 2>&1
$ipc call debug dump > "$reactive_rims_lit_json" 2>&1
"$grim_bin" "$reactive_rims_lit_png" > /dev/null 2>&1
mv "$settings_path" "$reactive_rims_gen1_path"
ln -sfn "$reactive_rims_gen1_path" "$settings_path"
"$jq_bin" '.theme.reactiveRims = false' "$reactive_rims_gen1_path" > "$reactive_rims_gen2_path"
ln -sfn "$reactive_rims_gen2_path" "$settings_path"
sleep 2
$ipc call debug dump > "$reactive_rims_off_json" 2>&1
"$grim_bin" "$reactive_rims_off_png" > /dev/null 2>&1
EOS
  hypr_exec_once "bash $script"
}

# Mean red, green and blue (0..255) inside one rect of a frame.
_reactive_rims_mean() {
  $convert_bin "$1" -crop "$2" +repage \
    -format '%[fx:int(mean.r*255+0.5)] %[fx:int(mean.g*255+0.5)] %[fx:int(mean.b*255+0.5)]\n' info: 2>/dev/null
}

# A 1px column `inset` pixels in from the card's left (or right) edge,
# clear of its corners.
_reactive_rims_column() {
  local json="$1" side="$2" inset="$3" x0 y0 x1 y1 x h
  read -r x0 y0 x1 y1 < <("$jq_bin" -r '.rims.panel.rect | map(round) | "\(.[0]) \(.[1]) \(.[2]) \(.[3])"' "$json")
  h=$(((y1 - y0) / 2))
  if [ "$side" = left ]; then x=$((x0 + inset)); else x=$((x1 - 1 - inset)); fi
  echo "1x$h+$x+$((y0 + h / 2))"
}

# The same column cut to the stretch the window beside the card spans
# (its top 24px under the card's, 200px tall), 16px clear of either end.
_reactive_rims_span() {
  local json="$1" side="$2" inset="$3" x0 y0 x1 y1 x top bottom
  read -r x0 y0 x1 y1 < <("$jq_bin" -r '.rims.panel.rect | map(round) | "\(.[0]) \(.[1]) \(.[2]) \(.[3])"' "$json")
  top=$((y0 + 40))
  bottom=$((y0 + 208))
  [ "$bottom" -le $((y1 - 16)) ] || bottom=$((y1 - 16))
  if [ "$side" = left ]; then x=$((x0 + inset)); else x=$((x1 - 1 - inset)); fi
  echo "1x$((bottom - top))+$x+$top"
}

# Luminance (0..255) inside one rect of a frame.
_reactive_rims_lum() {
  local lr lg lb
  read -r lr lg lb < <(_reactive_rims_mean "$1" "$2")
  echo $(((lr * 2126 + lg * 7152 + lb * 722) / 10000))
}

# Red minus blue at the rim, less red minus blue on the fill inside it.
_reactive_rims_lean() {
  local png="$1" json="$2" side="$3" rim fill
  read -r rr _ rb < <(_reactive_rims_mean "$png" "$(_reactive_rims_column "$json" "$side" 0)")
  read -r fr _ fb < <(_reactive_rims_mean "$png" "$(_reactive_rims_column "$json" "$side" 6)")
  echo $(((rr - rb) - (fr - fb)))
}

leg_reactive_rims_assert() {
  local f side lean_l lean_r rest c0 c1 lift_b lift_c lum_b lum_c lum_d near_b near_c far_c deep_b deep_c
  for f in "$reactive_rims_left_json" "$reactive_rims_right_json" "$reactive_rims_lit_json" "$reactive_rims_off_json"; do
    [ -s "$f" ] || fail "no debug dump at $f"
  done
  for f in "$reactive_rims_left_png" "$reactive_rims_right_png" "$reactive_rims_lit_png" "$reactive_rims_off_png"; do
    [ -f "$f" ] || fail "no frame at $f"
  done
  "$jq_bin" -e '.rims.enabled == true and .rims.panel != null' "$reactive_rims_left_json" > /dev/null \
    || fail "the audio card carries no rim: $("$jq_bin" -c .rims "$reactive_rims_left_json")"
  echo "left card: $("$jq_bin" -c .rims.panel "$reactive_rims_left_json")"
  echo "right card: $("$jq_bin" -c .rims.panel "$reactive_rims_right_json")"

  # The dump: each card's rim leans to the half it rests over.
  "$jq_bin" -e '.rims.panel.border.left | .[1:3] as $r | .[5:7] as $b | ($r | ascii_downcase) > ($b | ascii_downcase)' "$reactive_rims_left_json" > /dev/null \
    || fail "the audio card's left rim does not lean red: $("$jq_bin" -c .rims.panel.border "$reactive_rims_left_json")"
  "$jq_bin" -e '.rims.panel.border.right | .[1:3] as $r | .[5:7] as $b | ($b | ascii_downcase) > ($r | ascii_downcase)' "$reactive_rims_right_json" > /dev/null \
    || fail "the network card's right rim does not lean blue: $("$jq_bin" -c .rims.panel.border "$reactive_rims_right_json")"

  # The frame: the rim's own lean past whatever the fill carries.
  for side in left right; do
    lean_l=$(_reactive_rims_lean "$reactive_rims_left_png" "$reactive_rims_left_json" "$side")
    lean_r=$(_reactive_rims_lean "$reactive_rims_right_png" "$reactive_rims_right_json" "$side")
    echo "rim lean (red - blue past the fill), $side edge: over red $lean_l, over blue $lean_r"
    [ $((lean_l - lean_r)) -ge 30 ] || fail "the $side rim reads the same over the red and the blue half ($lean_l against $lean_r)"
  done

  # At rest: nothing commits.
  read -r c0 c1 < "$reactive_rims_rest_path" || fail "no rest count at $reactive_rims_rest_path"
  rest=$((c1 - c0))
  echo "panel commits in 5s at rest: $rest"
  [ "$rest" -eq 0 ] || fail "the panel committed $rest times in 5s with nothing moving"

  # A window moved next to the card lights the side facing it.
  lift_b=$("$jq_bin" -r '.rims.panel.lift.left' "$reactive_rims_right_json")
  lift_c=$("$jq_bin" -r '.rims.panel.lift.left' "$reactive_rims_lit_json")
  echo "left lift: $lift_b alone, $lift_c with the window beside it; windows=$("$jq_bin" -r .rims.windows "$reactive_rims_lit_json")"
  "$jq_bin" -en --argjson b "$lift_b" --argjson c "$lift_c" '$c >= $b + 0.15' > /dev/null \
    || fail "the window beside the card did not lift its left side ($lift_b to $lift_c); dispatch: $(tr '\n' ' ' < "$reactive_rims_dispatch_path")"
  lum_b=$(_reactive_rims_lum "$reactive_rims_right_png" "$(_reactive_rims_span "$reactive_rims_right_json" left 0)")
  lum_c=$(_reactive_rims_lum "$reactive_rims_lit_png" "$(_reactive_rims_span "$reactive_rims_lit_json" left 0)")
  echo "left rim luminance: $lum_b alone, $lum_c with the window"
  [ $((lum_c - lum_b)) -ge 12 ] || fail "the left rim did not brighten with the window beside it ($lum_b to $lum_c)"

  # The spill: the fill near the lit edge brighter than near the far one,
  # and moved by less than the rim.
  near_b=$(_reactive_rims_lum "$reactive_rims_right_png" "$(_reactive_rims_span "$reactive_rims_right_json" left 10)")
  near_c=$(_reactive_rims_lum "$reactive_rims_lit_png" "$(_reactive_rims_span "$reactive_rims_lit_json" left 10)")
  far_c=$(_reactive_rims_lum "$reactive_rims_lit_png" "$(_reactive_rims_span "$reactive_rims_lit_json" right 10)")
  echo "fill luminance 10px inside: lit edge $near_b alone, $near_c with the window; far edge $far_c"
  [ $((near_c - far_c)) -ge 3 ] || fail "the fill by the lit edge ($near_c) is no brighter than by the far one ($far_c)"
  [ $((near_c - near_b)) -ge 2 ] || fail "the fill by the lit edge did not take the window's light ($near_b to $near_c)"
  [ $((near_c - near_b)) -lt $((lum_c - lum_b)) ] \
    || fail "the fill moved as much as the rim ($near_b to $near_c against $lum_b to $lum_c)"
  # Past the spill's 36px the window changes nothing.
  deep_b=$(_reactive_rims_lum "$reactive_rims_right_png" "$(_reactive_rims_span "$reactive_rims_right_json" left 48)")
  deep_c=$(_reactive_rims_lum "$reactive_rims_lit_png" "$(_reactive_rims_span "$reactive_rims_lit_json" left 48)")
  echo "fill luminance 48px inside the lit edge: $deep_b alone, $deep_c with the window"
  [ $((deep_c - deep_b)) -le 1 ] || fail "the window's light reached 48px into the fill ($deep_b to $deep_c)"

  # Off: the flat border back on the same edge, the window still beside it.
  "$jq_bin" -e '.rims.enabled == false and .rims.panel == null' "$reactive_rims_off_json" > /dev/null \
    || fail "theme.reactiveRims false left the rims on: $("$jq_bin" -c .rims "$reactive_rims_off_json")"
  lum_d=$(_reactive_rims_lum "$reactive_rims_off_png" "$(_reactive_rims_span "$reactive_rims_lit_json" left 0)")
  echo "left rim luminance with rims off: $lum_d"
  [ $((lum_c - lum_d)) -ge 12 ] && [ "$lum_d" -le $((lum_b + 12)) ] \
    || fail "the left rim with rims off ($lum_d) is not the flat border ($lum_b alone, $lum_c lit)"

  echo "active border: $(tail -1 "$reactive_rims_dispatch_path")"
  echo "SMOKE_REACTIVE_RIMS ok rest_commits=$rest lift=$lift_b->$lift_c lum=$lum_b->$lum_c->$lum_d fill=$near_b->$near_c far=$far_c"
  echo "SMOKE_REACTIVE_RIMS_LEFT $reactive_rims_left_png"
  echo "SMOKE_REACTIVE_RIMS_RIGHT $reactive_rims_right_png"
  echo "SMOKE_REACTIVE_RIMS_WINDOW $reactive_rims_lit_png"
  echo "SMOKE_REACTIVE_RIMS_OFF $reactive_rims_off_png"
}
