# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --dock-density: the dock's icons at the scale of the output it is on. The
# dock lives on the live bar's output, and the live bar follows focus from
# one output to another with no output changing, so the scale the shell
# read when the outputs last changed is the wrong one to draw its icons at.
# dev/mac-stub.py is the Mac window that gives the dock its surface, with a
# desktop entry and a 128px icon of its own. A headless second output is
# made at twice the scale beside the session's and focused: `dock status`
# reads that output's scale and the stub's icon is held at it, and with
# focus back on the first output both follow it back.
leg_dock_density_flag="--dock-density"
leg_dock_density_order=176
leg_dock_density_needs="python3 jq convert"
leg_dock_density_claims="outputs=shared"

dock_density_second="HEADLESS-DOCK"
dock_density_prefix="$shot_dir/dock-density"
dock_density_log="$shot_dir/dock-density.log"

leg_dock_density_fixture() {
  local icons="$iso_home/.local/share/icons/hicolor"
  mkdir -p "$iso_home/.local/share/applications" "$icons/128x128/apps"
  $convert_bin -size 128x128 xc:none -fill '#E0A021' -draw 'roundrectangle 6,6,121,121,28,28' "$icons/128x128/apps/org.atoll.stub.png"
  [ -f "$icons/index.theme" ] || cat > "$icons/index.theme" <<'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=128x128/apps

[128x128/apps]
Size=128
Context=Applications
Type=Threshold
EOF
  printf '[Desktop Entry]\nType=Application\nName=Stub\nExec=true\nIcon=org.atoll.stub\n' \
    > "$iso_home/.local/share/applications/org.atoll.stub.desktop"
}

leg_dock_density_timing() {
  leg_timing 50
}

leg_dock_density_drive() {
  local script="$shot_dir/dock-density-drive.sh"
  local stub="$PWD/dev/mac-stub.py"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$dock_density_prefix"
log="$dock_density_log"
held='any(.apps[]; .app == "org.atoll.stub" and .icon)'
"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$python3_bin $stub --name Stub]==])" > /dev/null 2>&1
until_window org.atoll.stub 15
until_jq 15 "\$m-first.json" ".exists and \$held" call dock status
"$wmctl_bin" -j monitors > "\$m-monitors.json" 2>&1
first=\$("$jq_bin" -r '.[0].name' "\$m-monitors.json")
width=\$("$jq_bin" -r '.[0].width' "\$m-monitors.json")
"$wmctl_bin" output create headless "$dock_density_second" > "\$log" 2>&1
"$wmctl_bin" eval "hl.monitor({ output = \\"$dock_density_second\\", mode = \\"1600x1000@60\\", position = \\"\${width}x0\\", scale = 2 })" >> "\$log" 2>&1
sleep 3
"$wmctl_bin" dispatch "hl.dsp.focus({ monitor = '$dock_density_second' })" >> "\$log" 2>&1
until_jq 15 "\$m-second.json" ".scale == 2 and \$held" call dock status
call dock open > /dev/null 2>&1
until_settled
"$grim_bin" -o "$dock_density_second" "\$m-second.png" >> "\$log" 2>&1
call dock close > /dev/null 2>&1
"$wmctl_bin" dispatch "hl.dsp.focus({ monitor = '\$first' })" >> "\$log" 2>&1
until_jq 15 "\$m-back.json" ".scale == $FS_SMOKE_SCALE and \$held" call dock status
"$wmctl_bin" output remove "$dock_density_second" >> "\$log" 2>&1
EOF
  write_script "$shot_dir/dock-density-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -f "$stub --name Stub" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/dock-density-kill.sh"
  wm_exec_once "bash $script"
}

leg_dock_density_assert() {
  local m="$dock_density_prefix" held='any(.apps[]; .app == "org.atoll.stub" and .icon)'
  local show='{exists, scale, apps: [.apps[] | {app, icon}]}'
  [ ! -f "$m-second.png" ] || echo "SMOKE_DOCK_DENSITY $m-second.png"
  expect_jq "$m-first.json" ".exists and .scale == $FS_SMOKE_SCALE and $held" \
    "--dock-density: the dock on the session's output does not hold the stub's icon at $FS_SMOKE_SCALE: $(cat "$dock_density_log" 2>/dev/null)" "$show"
  expect_jq "$m-second.json" ".scale == 2 and $held" \
    "--dock-density: with focus on the output at scale 2 the dock does not hold the stub's icon at 2: $(cat "$dock_density_log" 2>/dev/null)" "$show"
  expect_jq "$m-back.json" ".scale == $FS_SMOKE_SCALE and $held" \
    "--dock-density: with focus back on the session's output the dock does not hold the stub's icon at $FS_SMOKE_SCALE" "$show"
  echo "SMOKE_DOCK_DENSITY ok the dock's icons follow its output from $FS_SMOKE_SCALE to 2 and back"
}
