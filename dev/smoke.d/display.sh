# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --display: the Display panel's three output controls reaching the running
# compositor. Hyprland refuses `hyprctl keyword` under a Lua config, so each
# of scale, mirror and enable goes out as `hyprctl eval 'hl.monitor{...}'`,
# and the only proof it took effect is `hyprctl monitors all -j` reading the
# change back.
#
# The rig has one output, so the mirror and enable legs run against a second,
# headless one created for the purpose and removed again before the frame is
# taken. The IPC verbs (`display scale|mirror|enable`) reach the same backend
# calls the panel's slider, mirror switch and output switch do.
leg_display_flag="--display"
leg_display_scale1="sets the output's scale itself"
leg_display_order=216
leg_display_needs="jq"
leg_display_claims="outputs=shared"

display_scale_path="$shot_dir/display-scale.txt"
display_create_path="$shot_dir/display-create.txt"
display_mirror_path="$shot_dir/display-mirror.txt"
display_monitors_base="$shot_dir/display-monitors-base.json"
display_monitors_scaled="$shot_dir/display-monitors-scaled.json"
display_monitors_two="$shot_dir/display-monitors-two.json"
display_monitors_mirrored="$shot_dir/display-monitors-mirrored.json"
display_monitors_unmirrored="$shot_dir/display-monitors-unmirrored.json"
display_monitors_disabled="$shot_dir/display-monitors-disabled.json"
display_monitors_enabled="$shot_dir/display-monitors-enabled.json"
display_panel_png="$shot_dir/display-panel.png"
display_second="FS-DISPLAY"

leg_display_timing() {
  leg_timing 50
}

leg_display_drive() {
  local script="$shot_dir/display-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
mon() { "$hyprctl_bin" monitors all -j > "\$1" 2>&1; }
mon "$display_monitors_base"
out=\$("$jq_bin" -r '.[0].name' "$display_monitors_base")
call display scale "\$out" 1.5 > "$display_scale_path" 2>&1
sleep 1
mon "$display_monitors_scaled"
call display scale "\$out" 1 >> "$display_scale_path" 2>&1
sleep 1
"$hyprctl_bin" output create headless "$display_second" > "$display_create_path" 2>&1
sleep 2
mon "$display_monitors_two"
call display mirror "$display_second" "\$out" > "$display_mirror_path" 2>&1
sleep 1
mon "$display_monitors_mirrored"
call display mirror "$display_second" "" >> "$display_mirror_path" 2>&1
sleep 1
mon "$display_monitors_unmirrored"
call display enable "$display_second" false >> "$display_mirror_path" 2>&1
sleep 1
mon "$display_monitors_disabled"
call display enable "$display_second" true >> "$display_mirror_path" 2>&1
sleep 1
mon "$display_monitors_enabled"
"$hyprctl_bin" output remove "$display_second" >> "$display_create_path" 2>&1
sleep 1
call panel open display > /dev/null 2>&1
until_settled
"$grim_bin" "$display_panel_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_display_assert() {
  local f out
  for f in "$display_scale_path" "$display_create_path" "$display_mirror_path" \
    "$display_monitors_base" "$display_monitors_scaled" "$display_monitors_two" \
    "$display_monitors_mirrored" "$display_monitors_unmirrored" \
    "$display_monitors_disabled" "$display_monitors_enabled"; do
    [ -s "$f" ] || fail "display leg produced no $(basename "$f")"
  done
  cat "$display_scale_path" "$display_create_path" "$display_mirror_path"

  [ "$(grep -c '^ok$' "$display_scale_path")" = "2" ] \
    || fail "display scale did not answer ok twice: $(cat "$display_scale_path")"
  [ "$(grep -c '^ok$' "$display_mirror_path")" = "4" ] \
    || fail "display mirror/enable did not answer ok four times: $(cat "$display_mirror_path")"
  grep -qxF 'ok' "$display_create_path" \
    || fail "hyprctl output create headless failed: $(cat "$display_create_path")"

  out=$("$jq_bin" -r '.[0].name' "$display_monitors_base")

  "$jq_bin" -e --arg o "$out" '.[] | select(.name == $o) | .scale == 1' "$display_monitors_base" > /dev/null \
    || fail "the rig's output does not start at scale 1: $(cat "$display_monitors_base")"
  "$jq_bin" -e --arg o "$out" '.[] | select(.name == $o) | (.scale - 1.5 | fabs) < 0.01' "$display_monitors_scaled" > /dev/null \
    || fail "display scale 1.5 did not reach the compositor: $("$jq_bin" -c '.[] | {name,scale}' "$display_monitors_scaled")"
  echo "SMOKE_DISPLAY_SCALE $out at scale 1.5 in hyprctl monitors"

  "$jq_bin" -e --arg s "$display_second" '.[] | select(.name == $s) | .mirrorOf == "none"' "$display_monitors_two" > /dev/null \
    || fail "the headless output did not appear unmirrored: $("$jq_bin" -c '.[] | {name,mirrorOf}' "$display_monitors_two")"
  # `mirrorOf` is the mirrored monitor's numeric id in Hyprland's JSON.
  "$jq_bin" -e --arg s "$display_second" --arg o "$out" \
    '. as $all | .[] | select(.name == $s) | .mirrorOf == ($all[] | select(.name == $o) | .id | tostring)' "$display_monitors_mirrored" > /dev/null \
    || fail "display mirror did not reach the compositor: $("$jq_bin" -c '.[] | {name,mirrorOf}' "$display_monitors_mirrored")"
  echo "SMOKE_DISPLAY_MIRROR $display_second mirrors $out in hyprctl monitors"
  "$jq_bin" -e --arg s "$display_second" '.[] | select(.name == $s) | .mirrorOf == "none"' "$display_monitors_unmirrored" > /dev/null \
    || fail "clearing the mirror did not reach the compositor: $("$jq_bin" -c '.[] | {name,mirrorOf}' "$display_monitors_unmirrored")"

  "$jq_bin" -e --arg s "$display_second" '.[] | select(.name == $s) | .disabled == true' "$display_monitors_disabled" > /dev/null \
    || fail "display enable false did not reach the compositor: $("$jq_bin" -c '.[] | {name,disabled}' "$display_monitors_disabled")"
  "$jq_bin" -e --arg s "$display_second" '.[] | select(.name == $s) | .disabled == false and .width > 0' "$display_monitors_enabled" > /dev/null \
    || fail "display enable true did not bring the output back: $("$jq_bin" -c '.[] | {name,disabled,width}' "$display_monitors_enabled")"
  echo "SMOKE_DISPLAY_ENABLE $display_second disabled and enabled in hyprctl monitors"

  [ -f "$display_panel_png" ] || fail "no display panel screenshot produced"
  echo "SMOKE_DISPLAY_PANEL $display_panel_png"
}
