# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --sky: the sky wallpaper (`wallpaper.color: "sky"`, fs_theme::sky). The
# fixture pins Amsterdam as `location` and the key itself, so the session
# starts on the sky for the sun now: `debug dump`'s `sky` block located,
# unpinned and holding a next change. A picture set over IPC takes the
# desktop back (the block null), then `wallpaper set sky`, the path the
# picker commits through, puts the sky back with state.json's
# `wallpaperColor` reading `sky`. Then `debug skyAt` pins four instants of
# 2026-10-30 (UTC, so the rig's own zone plays no part) and each is read off
# the dump, the frame and theme.json:
#   11:45 noon, 23.6 degrees    step 24   blue from the top to the foot
#   15:50 low sun, 2.6          step 2    the foot warmer than the top
#   16:40 dusk, -4.5            step -4   the same, darker
#   21:00 night, -42            step -18  near black at the foot
# The palette has to follow: theme.json's primary at noon differs from the
# low sun's, matugen having run on the sky as a picture. At night, pinned,
# the desktop commits nothing for six seconds: nothing redraws a sky that
# did not move.
leg_sky_flag="--sky"
leg_sky_order=236
leg_sky_needs="convert jq"

sky_picture="$shot_dir/sky-picture.png"
sky_state_json="$iso_home/.local/state/formalshell/state.json"
sky_theme_json="$iso_home/.local/state/formalshell/theme.json"
sky_phases="noon low dusk night"

leg_sky_fixture() {
  settings_pin '.location = {"latitude": 52.37, "longitude": 4.9}'
  settings_pin '.wallpaper.color = "sky"'
  $convert_bin -size 1920x1080 xc:'#5a8a3c' "$sky_picture"
}

leg_sky_timing() {
  leg_timing 55 120
}

leg_sky_drive() {
  local script="$shot_dir/sky-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
sleep 5
$ipc call debug dump > "$shot_dir/sky-dump-start.json" 2>&1
$ipc call wallpaper set "$sky_picture" > /dev/null 2>&1
sleep 3
$ipc call debug dump > "$shot_dir/sky-dump-picture.json" 2>&1
$ipc call debug skyAt 2026-10-30T11:45:00Z > /dev/null 2>&1
$ipc call wallpaper set sky > "$shot_dir/sky-set.txt" 2>&1
phase() {
  sleep 7
  $ipc call debug dump > "$shot_dir/sky-dump-\$1.json" 2>&1
  cp "$sky_theme_json" "$shot_dir/sky-theme-\$1.json" 2>/dev/null
  "$grim_bin" "$shot_dir/sky-\$1.png" > /dev/null 2>&1
}
phase noon
cp "$sky_state_json" "$shot_dir/sky-state.json" 2>/dev/null
$ipc call debug skyAt 2026-10-30T15:50:00Z > /dev/null 2>&1
phase low
$ipc call debug skyAt 2026-10-30T16:40:00Z > /dev/null 2>&1
phase dusk
$ipc call debug skyAt 2026-10-30T21:00:00Z > /dev/null 2>&1
phase night
grep -c 'commit surface=wallpaper' "$shell_log_path" > "$shot_dir/sky-commits-before.txt"
sleep 6
grep -c 'commit surface=wallpaper' "$shell_log_path" > "$shot_dir/sky-commits-after.txt"
EOS
  hypr_exec_once "bash $script"
}

# One pixel of a frame as "r g b".
_sky_px() {
  $convert_bin "$1" -crop "1x1+$2+$3" +repage -format '%[fx:int(r*255+0.5)] %[fx:int(g*255+0.5)] %[fx:int(b*255+0.5)]' info: 2>/dev/null
}

_sky_primary() {
  "$jq_bin" -r '.primary // empty' "$1" 2>/dev/null
}

leg_sky_assert() {
  local start="$shot_dir/sky-dump-start.json" picture="$shot_dir/sky-dump-picture.json"
  [ -s "$start" ] || fail "--sky: no dump at startup"
  "$jq_bin" -e '.sky.located == true and .sky.pinned == false and (.sky.nextMs | type) == "number" and (.sky.source | startswith("sky:"))' "$start" > /dev/null \
    || fail "--sky: wallpaper.color sky did not start the session on a located, unpinned sky: $("$jq_bin" -c .sky "$start")"
  "$jq_bin" -e '.sky == null' "$picture" > /dev/null || fail "--sky: a picture set over IPC left the sky up: $("$jq_bin" -c .sky "$picture")"
  [ "$(cat "$shot_dir/sky-set.txt")" = ok ] || fail "--sky: wallpaper set sky answered '$(cat "$shot_dir/sky-set.txt")'"
  "$jq_bin" -e '.wallpaperColor == "sky"' "$shot_dir/sky-state.json" > /dev/null \
    || fail "--sky: state.json holds wallpaperColor $("$jq_bin" -c .wallpaperColor "$shot_dir/sky-state.json")"

  local phase want dump png top foot tr tg tb fr fg fb summary=""
  for phase in $sky_phases; do
    case $phase in
      noon) want=24 ;;
      low) want=2 ;;
      dusk) want=-4 ;;
      night) want=-18 ;;
    esac
    dump="$shot_dir/sky-dump-$phase.json" png="$shot_dir/sky-$phase.png"
    [ -s "$dump" ] && [ -s "$png" ] || fail "--sky $phase: no dump or frame"
    "$jq_bin" -e --argjson s "$want" '.sky.step == $s and .sky.pinned and .sky.located and .sky.source == "sky:\($s)" and .sky.nextMs == null' "$dump" > /dev/null \
      || fail "--sky $phase: want step $want, the dump says $("$jq_bin" -c .sky "$dump")"
    # Below the bar and above the foot, in the middle of the output.
    top=$(_sky_px "$png" 960 120) foot=$(_sky_px "$png" 960 1070)
    read -r tr tg tb <<< "$top"
    read -r fr fg fb <<< "$foot"
    echo "--sky $phase: elevation $("$jq_bin" .sky.elevation "$dump") top $top foot $foot primary $(_sky_primary "$shot_dir/sky-theme-$phase.json")"
    case $phase in
      noon) [ "$tb" -gt $((tr + 80)) ] && [ "$fb" -gt "$fr" ] || fail "--sky noon: not blue down the frame (top $top, foot $foot)" ;;
      low) [ "$fr" -gt $((tr + 80)) ] && [ "$fr" -gt "$fb" ] || fail "--sky low: the foot is not the warm horizon (top $top, foot $foot)" ;;
      dusk) [ "$fr" -gt $((tr + 40)) ] && [ "$tb" -gt "$tr" ] || fail "--sky dusk: no warm foot under a blue top (top $top, foot $foot)" ;;
      night) [ "$fr" -lt 40 ] && [ "$fg" -lt 40 ] && [ "$fb" -lt 50 ] || fail "--sky night: the foot is not near black ($foot)" ;;
    esac
    summary="$summary $phase=$want"
  done
  local noon_primary low_primary
  noon_primary=$(_sky_primary "$shot_dir/sky-theme-noon.json")
  low_primary=$(_sky_primary "$shot_dir/sky-theme-low.json")
  [ -n "$noon_primary" ] && [ "$noon_primary" != "$low_primary" ] \
    || fail "--sky: the palette did not follow the sky (noon $noon_primary, low sun $low_primary)"
  local before after
  before=$(cat "$shot_dir/sky-commits-before.txt") after=$(cat "$shot_dir/sky-commits-after.txt")
  [ "$before" = "$after" ] || fail "--sky: the desktop committed $((after - before)) times in 6s under a sky that did not move"

  echo "SMOKE_SKY ok$summary primary noon=$noon_primary low=$low_primary"
  for phase in $sky_phases; do
    echo "SMOKE_SKY_$(echo "$phase" | tr '[:lower:]' '[:upper:]') $shot_dir/sky-$phase.png"
  done
}
