# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --showcase seeds state.json with a generated wallpaper (three soft colour
# fields on a dark ground) before the shell starts, so whatever leg it rides
# photographs a matugen palette over a real desktop instead of the bare
# output. It drives and asserts nothing; the README's screenshots and GIFs
# are taken under it. Seeded rather than set over IPC so the first frame of
# any leg already carries the palette.
#
# Legs whose probes read brightness against the bare output (--menu-emerge)
# fail under it and keep their frames in the VM's shot_dir; it is a camera,
# not a test.
leg_showcase_flag="--showcase"
leg_showcase_order=5
leg_showcase_needs="convert"
leg_showcase_fixture_window=keep

leg_showcase_fixture() {
  local wp="$shot_dir/showcase-wallpaper.png"
  $convert_bin -size 1920x1080 xc:'#101428' \
    \( -size 1400x1400 radial-gradient:'#e0703a-none' \) -geometry +900+250 -composite \
    \( -size 1500x1500 radial-gradient:'#4a3fb0-none' \) -geometry -500-500 -composite \
    \( -size 1100x1100 radial-gradient:'#2a8f9a-none' \) -geometry +100+500 -composite \
    -blur 0x40 "$wp"
  mkdir -p "$iso_home/.local/state/formalshell"
  printf '{"wallpaper": "%s"}\n' "$wp" > "$iso_home/.local/state/formalshell/state.json"
}
