# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --hypr-motion: the compositor's window animations on the shell's own two
# clock families. Under Hyprland the rig boots with animations off; this leg
# appends the shipped docs/examples/hyprland/formalshell.lua motion block to
# its session config, reading the formalshell-chrome.lua the shell publishes
# into the session's config dir the way the owner's hyprland.lua does, so
# the run reads what installs. Once the shell has published and reloaded the
# compositor, `hyprctl animations` has to carry the six
# `formalshell_<family>` beziers at motion.rs's control points
# (MOTION_CURVES) and every leaf the block sets on its family's bezier and
# speed (the family's duration in tenths of a second), with
# `animations:enabled` on. formalwm takes the families in the shell's
# `theme.set` and has no leaves: its state's `chrome.motion` holds what it
# runs on, which shoji-ctl answers as the six beziers and
# `animations:enabled`. A foot window's arrival is grabbed as a burst of
# one-row strips across the output's middle: tiled over the output's width
# (Hyprland's with no gaps, formalwm's a full width column), its middle is
# drawn from the first frame it maps on, its edge 20px in from where it
# settles only once it has finished growing, so some strip has to carry the
# window's middle over a bare edge. Then settings.json is rewritten with
# `motion.enabled: false`: the shell publishes again, and
# `animations:enabled` reads off.
leg_hypr_motion_flag="--hypr-motion"
leg_hypr_motion_order=241
leg_hypr_motion_compositors="hyprland shoji"
leg_hypr_motion_claims="motion.enabled hyprland.animations"
leg_hypr_motion_needs="jq foot convert"

hypr_motion_on="$shot_dir/hypr-motion-animations.json"
hypr_motion_enabled_on="$shot_dir/hypr-motion-enabled-on.json"
hypr_motion_enabled_off="$shot_dir/hypr-motion-enabled-off.json"
hypr_motion_chrome="$shot_dir/hypr-motion-chrome.lua"
hypr_motion_settled="$shot_dir/hypr-motion-settled.png"
hypr_motion_clients="$shot_dir/hypr-motion-clients.json"

leg_hypr_motion_timing() {
  leg_timing 80
}

leg_hypr_motion_drive() {
  local published="$iso_home/.config/hypr/formalshell-chrome.lua"
  if [ "$FS_COMPOSITOR" = hyprland ]; then
    # The published table, as the shipped file's `published` helper reads it.
    echo "local chrome = (function() local ok, t = pcall(dofile, \"$published\"); if ok and type(t) == \"table\" then return t end; return {} end)()"
    awk '/^local motion = chrome.motion/ { on = 1 } on { print } on && /^hl.animation\({ leaf = "monitorAdded"/ { on = 0 }' docs/examples/hyprland/formalshell.lua
  fi
  wm_full_width_columns
  local script="$shot_dir/hypr-motion-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
cp "$published" "$hypr_motion_chrome" 2>/dev/null
"$wmctl_bin" -j animations > "$hypr_motion_on" 2>&1
"$wmctl_bin" -j getoption animations:enabled > "$hypr_motion_enabled_on" 2>&1
"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-motion sh -c 'sleep 30']==])" > /dev/null 2>&1
for i in \$(seq 0 79); do
  "$grim_bin" -g "0,560 960x1" "$shot_dir/hypr-motion-strip-\$i.png" > /dev/null 2>&1
done
sleep 2
"$grim_bin" "$hypr_motion_settled" > /dev/null 2>&1
"$wmctl_bin" -j clients > "$hypr_motion_clients" 2>&1
settings_retarget '.motion.enabled = false'
sleep 5
"$wmctl_bin" -j getoption animations:enabled > "$hypr_motion_enabled_off" 2>&1
EOS
  wm_exec_once "bash $script"
}

leg_hypr_motion_assert() {
  [ -s "$hypr_motion_on" ] || fail "--hypr-motion: no hyprctl animations dump"
  if [ "$FS_COMPOSITOR" = hyprland ]; then
    grep -q 'motion = {' "$hypr_motion_chrome" 2>/dev/null || fail "--hypr-motion: the shell published no motion table into formalshell-chrome.lua"
  fi
  local family want got
  # MOTION_CURVES (crates/fs-theme/src/tokens.rs), x1 y1 x2 y2.
  for want in "spatialFast 0.42 1.67 0.21 0.9" "spatial 0.38 1.21 0.22 1" "spatialSlow 0.39 1.29 0.35 0.98" \
    "effectsFast 0.31 0.94 0.34 1" "effects 0.34 0.8 0.34 1" "effectsSlow 0.34 0.88 0.34 1"; do
    family=${want%% *}
    got=$("$jq_bin" -r --arg n "formalshell_$family" '.[1][] | select(.name == $n) | "\(.X0) \(.Y0) \(.X1) \(.Y1)"' "$hypr_motion_on" 2>/dev/null)
    [ -n "$got" ] || fail "--hypr-motion: no bezier formalshell_$family in hyprctl animations"
    awk -v a="${want#* }" -v b="$got" 'BEGIN { n = split(a, x, " "); split(b, y, " "); for (i = 1; i <= n; i++) if ((x[i] - y[i]) ^ 2 > 1e-6) exit 1 }' \
      || fail "--hypr-motion: formalshell_$family is ($got), not motion.rs's (${want#* })"
  done
  local leaf
  if [ "$FS_COMPOSITOR" = hyprland ]; then
    for want in "windows spatial 5" "windowsOut spatialFast 3.5" "layers spatial 5" "layersOut spatialFast 3.5" \
      "fade effects 2" "fadeOut effectsFast 1.5" "border effectsSlow 3" "workspaces spatial 5" "specialWorkspace spatial 5"; do
      read -r leaf family got <<< "$want"
      "$jq_bin" -e --arg l "$leaf" --arg b "formalshell_$family" --argjson s "$got" \
        '.[0][] | select(.name == $l) | .overridden and .enabled and .bezier == $b and ((.speed - $s) | fabs) < 0.01' "$hypr_motion_on" > /dev/null 2>&1 \
        || fail "--hypr-motion: leaf $leaf is $("$jq_bin" -c --arg l "$leaf" '.[0][] | select(.name == $l)' "$hypr_motion_on"), not formalshell_$family at $got"
    done
    "$jq_bin" -e '.[0][] | select(.name == "monitorAdded") | .overridden and (.enabled | not)' "$hypr_motion_on" > /dev/null 2>&1 \
      || fail "--hypr-motion: monitorAdded is $("$jq_bin" -c '.[0][] | select(.name == "monitorAdded")' "$hypr_motion_on"), not off"
  fi
  "$jq_bin" -e '.int == 1 or .bool == true' "$hypr_motion_enabled_on" > /dev/null 2>&1 \
    || fail "--hypr-motion: animations:enabled is not on with motion on: $(cat "$hypr_motion_enabled_on")"
  "$jq_bin" -e '.int == 0 or .bool == false' "$hypr_motion_enabled_off" > /dev/null 2>&1 \
    || fail "--hypr-motion: motion.enabled false left animations:enabled $(cat "$hypr_motion_enabled_off")"
  local window desk reads="" moving="" i edge mid at
  # 20px in from the settled window's left edge, in the pixels grim takes.
  at=$("$jq_bin" -r --argjson s "$out_scale" '[.[] | select(.class == "formalshell-motion")][0].at[0] * $s | ceil + 20' "$hypr_motion_clients" 2>/dev/null)
  [[ "$at" =~ ^[0-9]+$ ]] && [ "$at" -lt 900 ] || fail "--hypr-motion: no settled formalshell-motion window to sample: $(cat "$hypr_motion_clients" 2>/dev/null)"
  # The settled window's own fill, and the bare desktop the first strip
  # carries before foot has mapped.
  window=$(patch_mean "$hypr_motion_settled" "1x1+$at+560")
  desk=$(patch_mean "$shot_dir/hypr-motion-strip-0.png" "1x1+$at+0")
  # 24 thousandths is the 6 of 255 grey levels this compared before.
  near() { [ -n "$1" ] && [ -n "$2" ] && [ $(($1 > $2 ? $1 - $2 : $2 - $1)) -le 24 ]; }
  for i in $(seq 0 79); do
    [ -f "$shot_dir/hypr-motion-strip-$i.png" ] || continue
    edge=$(patch_mean "$shot_dir/hypr-motion-strip-$i.png" "1x1+$at+0")
    mid=$(patch_mean "$shot_dir/hypr-motion-strip-$i.png" 1x1+900+0)
    reads="$reads $edge/$mid"
    if [ -z "$moving" ] && near "$mid" "$window" && near "$edge" "$desk" && ! near "$desk" "$window"; then
      moving=$i
    fi
  done
  echo "--hypr-motion: edge/middle per strip:$reads (desktop $desk, window $window)"
  [ -n "$moving" ] || fail "--hypr-motion: no strip carried the window's middle over a bare edge, so it arrived at full size"
  echo "SMOKE_HYPR_MOTION ok six curves, $([ "$FS_COMPOSITOR" = hyprland ] && echo "nine leaves, monitorAdded off, ")off with motion.enabled false"
  echo "SMOKE_HYPR_MOTION_SETTLED $hypr_motion_settled"
}
