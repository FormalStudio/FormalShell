# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mic: the opt-in microphone cell, named in bar.layout so it exists at all.
# THE HONEST NO MIC STATE IS THE PASSING RESULT: this VM has a pipewire null
# sink and no capture device, so AudioService.sourceAvailable is genuinely
# false and the widget renders its one dim NO MIC label instead of a glyph,
# staying visible because the user opted in. A live-or-muted glyph on this
# rig would mean the widget invented a device.
#
# `debug dump` carries the resolved Config.settings, the one place the layout
# the shell actually resolved is readable from outside the process: the frame
# proves what the cell rendered, the dump proves it was there because
# bar.layout asked for it.
leg_mic_flag="--mic"
leg_mic_order=180
leg_mic_compositors="hyprland shoji"
leg_mic_claims="bar.layout.right"

mic_bar_png="$shot_dir/mic-bar.png"
# The same file --dump's own leg writes, deliberately: this leg reads the
# resolved settings back rather than adding a second dump path.
mic_dump_path="$shot_dir/dump.json"

leg_mic_fixture() {
  # Leading the right region rather than appended, so the cell cannot be
  # clipped off the end of a full row.
  settings_pin '.bar.layout.right = ["microphone", "battery", "audio", "network", "bluetooth", "weather", "tray", "bell", "indicators"]'
}

leg_mic_timing() {
  leg_timing 45
}

leg_mic_drive() {
  local script="$shot_dir/mic-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call debug dump > "$mic_dump_path" 2>&1
"$grim_bin" "$mic_bar_png" > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_mic_assert() {
  if [ ! -s "$mic_dump_path" ]; then
    fail "no debug dump produced"
  fi
  expect_jq "$mic_dump_path" '.configLoaded.bar.layout.right[0] == "microphone"' \
    "the resolved settings do not lead bar.layout's right region with microphone, the opt-in cell was never placed" .configLoaded.bar.layout
  [ -f "$mic_bar_png" ] || fail "no mic screenshot produced at $mic_bar_png"
  echo "SMOKE_MIC $mic_bar_png (this VM has no capture device, so a dim NO MIC cell is the correct rendering; a glyph here would mean an invented device)"
}
