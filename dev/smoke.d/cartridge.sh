# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --cartridge pins `theme.preset` to `cartridge` in the settings fixture and
# drives nothing itself: it rides whichever legs it is combined with, so
# `--cartridge --gallery` is the gallery sheet in the console look (square
# boxes with one chamfered corner, the cursor's block bar and brackets, the
# packaged Archivo, DotGothic16 and BIZ UD Mincho faces) and `--cartridge
# --menu` is the launcher sliding in under it. The preset is read at startup,
# which is why it is a fixture and not an IPC call, and why there is nothing
# to assert here beyond the frame the other leg takes.
leg_cartridge_flag="--cartridge"
leg_cartridge_order=7
# See pantheon.sh: a rider with no drive or assert of its own must never
# vote the base fixture window off through fixture_window_mode's AND.
leg_cartridge_fixture_window=keep

# The table's boot takes the screen and the keyboard for its first seconds,
# so every leg but --boot and --cartridge-media runs with it off.
leg_cartridge_fixture() {
  if leg_on boot || leg_on cartridge_media; then
    settings_fragment ', "theme": {"preset": "cartridge"}'
  else
    settings_fragment ', "theme": {"preset": "cartridge", "boot": false}'
  fi
}
