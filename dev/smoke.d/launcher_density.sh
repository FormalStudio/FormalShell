# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --launcher-density: the launcher on an output denser than the bar's. The
# session's own output stays at scale 1 (the bar's), a headless second one is
# made at 1.25 (g815's panel beside its scale-1 monitor) and focused, and the
# app grid opened there as a popover (`motion.morph` false, g815's setting).
# Hyprland sends no preferred scale to a subsurface, and the popover's look
# and content are subsurfaces of its card: every buffer the card, its look
# (whose four corners log as the card) and its content commit has to be
# drawn at 1.25, read off the shell's commit log, or it is a 1x picture
# stretched over the denser panel. The frame is pulled for
# reading, its icon a 128px picture of two flat halves.
leg_launcher_density_flag="--launcher-density"
leg_launcher_density_order=176
leg_launcher_density_needs="jq convert"

launcher_density_second="HEADLESS-DENSITY"
launcher_density_colour="#E0218A"
launcher_density_other="#21C8E0"
launcher_density_dir="$iso_home/.local/share/applications"
launcher_density_icons="$iso_home/.local/share/icons/hicolor"
launcher_density_png="$shot_dir/launcher-density.png"
launcher_density_log="$shot_dir/launcher-density.log"
launcher_density_status="$shot_dir/launcher-density-status.json"

leg_launcher_density_validate() {
  if [ "${FS_SMOKE_SCALE:-1}" != 1 ]; then
    echo "usage: --launcher-density needs the session's output at scale 1 (unset FS_SMOKE_SCALE)" >&2
    exit 1
  fi
  if leg_on multi_output || leg_on display || leg_on brightness; then
    echo "usage: --launcher-density makes its own second output and cannot combine with legs that add or remove outputs" >&2
    exit 1
  fi
}

leg_launcher_density_fixture() {
  settings_fragment ', "menu": {"appGrid": true}, "motion": {"morph": false}'
  mkdir -p "$launcher_density_dir" "$launcher_density_icons/128x128/apps"
  $convert_bin -size 64x128 "xc:$launcher_density_colour" -size 64x128 "xc:$launcher_density_other" +append \
    "$launcher_density_icons/128x128/apps/formalshell-density.png"
  [ -f "$launcher_density_icons/index.theme" ] || cat > "$launcher_density_icons/index.theme" <<'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=128x128/apps

[128x128/apps]
Size=128
Context=Applications
Type=Threshold
EOF
  printf '[Desktop Entry]\nType=Application\nName=Density Probe\nExec=true\nIcon=formalshell-density\n' \
    > "$launcher_density_dir/formalshell-density.desktop"
}

leg_launcher_density_timing() {
  leg_timing 20 45
}

leg_launcher_density_drive() {
  local script="$shot_dir/launcher-density-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 5
width=\$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0].width')
"$hyprctl_bin" output create headless "$launcher_density_second" > "$launcher_density_log" 2>&1
"$hyprctl_bin" eval "hl.monitor({ output = \\"$launcher_density_second\\", mode = \\"1600x1000@60\\", position = \\"\${width}x0\\", scale = 1.25 })" >> "$launcher_density_log" 2>&1
sleep 3
"$hyprctl_bin" dispatch "hl.dsp.focus({ monitor = '$launcher_density_second' })" >> "$launcher_density_log" 2>&1
sleep 1
$ipc call menu summon apps > /dev/null 2>&1
$ipc call menu filter density > /dev/null 2>&1
sleep 3
"$grim_bin" -o "$launcher_density_second" "$launcher_density_png" >> "$launcher_density_log" 2>&1
$ipc call menu status > "$launcher_density_status" 2>&1
"$hyprctl_bin" -j monitors >> "$launcher_density_log" 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 1
"$hyprctl_bin" output remove "$launcher_density_second" >> "$launcher_density_log" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_launcher_density_assert() {
  [ -s "$launcher_density_png" ] || fail "no frame of the second output: $(cat "$launcher_density_log" 2>/dev/null)"
  echo "SMOKE_LAUNCHER_DENSITY $launcher_density_png"
  local commits
  commits=$(grep -E 'commit surface=menu(-content)? n=' "$shell_log_path" 2>/dev/null \
    | awk '{ s = "scale=1"; for (i = 1; i <= NF; i++) { if ($i ~ /^surface=/) n = $i; if ($i ~ /^scale=/) s = $i } print n, s }' | sort | uniq -c)
  echo "-- the popover's subsurfaces, commits per scale --"
  echo "${commits:-(none)}"
  [ -n "$commits" ] || fail "the popover's look and content never committed"
  if echo "$commits" | grep -qv 'scale=1.25$'; then
    fail "a launcher subsurface on the 1.25 output committed at another scale"
  fi
}
