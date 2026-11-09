# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --wallpaper-pair: a light and a dark picture set as one pair
# (`wallpaper pair`, docs/superpowers/specs/2026-11-04-wallpaper-picker.md
# §3), the mode choosing the half the desktop shows.
#
# The halves are 960x540 fields of known mean, each with a band down its
# left quarter: cream with an orange band, navy with a blue one, so each
# seeds matugen a different hue (warm and cool) and the two means sit far
# apart. `theme status` names the dark half as `desktop` in the default dark
# mode with `wallpaperDark` set, then the light half after `theme mode
# toggle`, and the dark one again after a second. Each toggle runs at a
# tenth of the shell's speed (`debug motionScale 1000`) with a grim burst
# through the backdrop's crossfade, read on a patch of desktop under the
# bar: no frame's mean outside the two halves' (a flash), at least one part
# way between (where a cut has none). theme.json's primary is read in each
# mode: different, warm under the light half and cool under the dark.
# Locked in dark mode the lock's frame carries the dark half's blue band,
# and the real password unlocks it. Last, settings.json is retargeted to
# `theme.mode: "auto"`, and `desktop` has to be the half `effective` names.
leg_wallpaper_pair_flag="--wallpaper-pair"
leg_wallpaper_pair_order=102
leg_wallpaper_pair_needs="convert jq wtype"

wallpaper_pair_light="$shot_dir/pair-light.png"
wallpaper_pair_dark="$shot_dir/pair-dark.png"
wallpaper_pair_frames=50
wallpaper_pair_gap=0.05
# The patch every mean is read on: below the bar at any scale, the whole
# width, so the band and the field both count.
wallpaper_pair_crop="1920x700+0+300"
wallpaper_pair_theme_json="$iso_home/.local/state/formalshell/theme.json"
wallpaper_pair_state_json="$iso_home/.local/state/formalshell/state.json"

leg_wallpaper_pair_validate() {
  local other
  for other in wallpaper theme_toggle theme_auto lock unlock depth showcase sky config_reload; do
    if leg_on "$other"; then
      echo "usage: --wallpaper-pair sets its own pair, toggles the mode, locks and retargets settings.json; run it without --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_wallpaper_pair_fixture() {
  $convert_bin -size 960x540 xc:'#efe3d0' -fill '#e0601a' -draw 'rectangle 80,0 239,539' "png:$wallpaper_pair_light"
  $convert_bin -size 960x540 xc:'#0d1424' -fill '#2f7fff' -draw 'rectangle 80,0 239,539' "png:$wallpaper_pair_dark"
}

leg_wallpaper_pair_timing() {
  leg_timing 70 110
}

leg_wallpaper_pair_drive() {
  local script="$shot_dir/wallpaper-pair-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
call wallpaper pair "$wallpaper_pair_light" "$wallpaper_pair_dark" > "$shot_dir/pair-call.txt" 2>&1
sleep 5
call theme status > "$shot_dir/pair-status-dark.json" 2>&1
call wallpaper get > "$shot_dir/pair-get-dark.txt" 2>&1
cp "$wallpaper_pair_theme_json" "$shot_dir/pair-theme-dark.json" 2>/dev/null
cp "$wallpaper_pair_state_json" "$shot_dir/pair-state.json" 2>/dev/null
"$grim_bin" "$shot_dir/pair-desk-dark.png" > /dev/null 2>&1
call debug motionScale 1000 > "$shot_dir/pair-scale.txt" 2>&1
call theme mode toggle > /dev/null 2>&1
burst "$shot_dir/pair-tolight" $wallpaper_pair_frames $wallpaper_pair_gap
sleep 2
call debug motionScale 100 > /dev/null 2>&1
sleep 4
call theme status > "$shot_dir/pair-status-light.json" 2>&1
call wallpaper get > "$shot_dir/pair-get-light.txt" 2>&1
cp "$wallpaper_pair_theme_json" "$shot_dir/pair-theme-light.json" 2>/dev/null
"$grim_bin" "$shot_dir/pair-desk-light.png" > /dev/null 2>&1
call debug motionScale 1000 > /dev/null 2>&1
call theme mode toggle > /dev/null 2>&1
burst "$shot_dir/pair-todark" $wallpaper_pair_frames $wallpaper_pair_gap
sleep 2
call debug motionScale 100 > /dev/null 2>&1
sleep 4
call theme status > "$shot_dir/pair-status-dark-2.json" 2>&1
call lock lock > /dev/null 2>&1
sleep 3
"$grim_bin" "$shot_dir/pair-lock-dark.png" > /dev/null 2>&1
"$wtype_bin" -s 300 "$rig_password"
"$wtype_bin" -s 300 -k Return
sleep 3
call lock isLocked > "$shot_dir/pair-islocked.txt" 2>&1
cp "\$(readlink -f "$settings_path")" "$shot_dir/pair-settings-1.json"
"$jq_bin" '.theme.mode = "auto"' "$shot_dir/pair-settings-1.json" > "$shot_dir/pair-settings-2.json"
ln -sfn "$shot_dir/pair-settings-2.json" "$settings_path"
sleep 5
call theme status > "$shot_dir/pair-status-auto.json" 2>&1
"$grim_bin" "$shot_dir/pair-desk-auto.png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# A frame's mean grey over the desktop patch, 0 to 1000.
wallpaper_pair_mean() {
  patch_mean "$1" "$wallpaper_pair_crop"
}

# A theme.json's primary as "r g b".
wallpaper_pair_primary() {
  local hex
  hex=$("$jq_bin" -r '.. | objects | .primary? // empty | strings' "$1" 2>/dev/null | head -1)
  hex=${hex#\#}
  [ ${#hex} -ge 6 ] || return 1
  echo "$((16#${hex:0:2})) $((16#${hex:2:2})) $((16#${hex:4:2}))"
}

# One toggle's burst between the frames `from` and `to`.
wallpaper_pair_fade() {
  local name="$1" from="$2" to="$3" lo hi tol=15 i f m means=() between=0 outside=0 mid=""
  lo=$from hi=$to
  [ "$lo" -gt "$hi" ] && { lo=$to; hi=$from; }
  for i in $(seq 1 $wallpaper_pair_frames); do
    f="$shot_dir/pair-$name-$i.png"
    [ -s "$f" ] || fail "no burst frame at $f"
    m=$(wallpaper_pair_mean "$f")
    means+=("$m")
    if [ "$m" -lt $((lo - tol)) ] || [ "$m" -gt $((hi + tol)) ]; then
      outside=$((outside + 1))
      echo "SMOKE_WALLPAPER_PAIR_OUTSIDE $f"
    fi
    if [ "$m" -gt $((lo + (hi - lo) / 10)) ] && [ "$m" -lt $((hi - (hi - lo) / 10)) ]; then
      between=$((between + 1))
      [ -n "$mid" ] || mid=$f
    fi
  done
  echo "wallpaper pair: $name means ${means[*]} (ends $from, $to)"
  [ "$outside" -eq 0 ] || fail "$outside frames of the $name toggle fall outside the two halves' means ($lo to $hi): a flash"
  [ "$between" -ge 1 ] || fail "no frame of the $name toggle lies part way between the halves ($lo to $hi): a cut, not a crossfade"
  echo "SMOKE_WALLPAPER_PAIR_MID_${name^^} $mid"
  m=${means[$((wallpaper_pair_frames - 1))]}
  [ "$m" -ge $((to - tol)) ] && [ "$m" -le $((to + tol)) ] || fail "the $name burst's last frame ($m) is not the half it toggled to ($to)"
}

leg_wallpaper_pair_assert() {
  local f desktop dark light auto effective
  grep -q '^ok$' "$shot_dir/pair-call.txt" 2>/dev/null || fail "wallpaper pair did not answer ok: $(cat "$shot_dir/pair-call.txt" 2>/dev/null)"
  grep -q '^ok$' "$shot_dir/pair-scale.txt" 2>/dev/null || fail "debug motionScale did not answer ok: $(cat "$shot_dir/pair-scale.txt" 2>/dev/null)"
  for f in pair-status-dark.json pair-status-light.json pair-status-dark-2.json pair-status-auto.json; do
    [ -s "$shot_dir/$f" ] || fail "--wallpaper-pair produced no $f"
    cat "$shot_dir/$f"; echo
  done
  "$jq_bin" -e --arg l "$wallpaper_pair_light" --arg d "$wallpaper_pair_dark" '.wallpaper == $l and .wallpaperDark == $d and .wallpaperColor == false' "$shot_dir/pair-state.json" > /dev/null \
    || fail "state.json does not hold the pair: $(cat "$shot_dir/pair-state.json" 2>/dev/null)"
  "$jq_bin" -e --arg l "$wallpaper_pair_light" --arg d "$wallpaper_pair_dark" '.mode == "dark" and .wallpaper == $l and .wallpaperDark == $d and .desktop == $d' "$shot_dir/pair-status-dark.json" > /dev/null \
    || fail "theme status in dark mode does not show the dark half"
  "$jq_bin" -e --arg l "$wallpaper_pair_light" '.mode == "light" and .desktop == $l' "$shot_dir/pair-status-light.json" > /dev/null \
    || fail "theme status after the toggle does not show the light half"
  "$jq_bin" -e --arg d "$wallpaper_pair_dark" '.mode == "dark" and .desktop == $d' "$shot_dir/pair-status-dark-2.json" > /dev/null \
    || fail "theme status after the second toggle does not show the dark half again"
  grep -qxF "$wallpaper_pair_dark" "$shot_dir/pair-get-dark.txt" || fail "wallpaper get in dark mode is not the dark half: $(cat "$shot_dir/pair-get-dark.txt")"
  grep -qxF "$wallpaper_pair_light" "$shot_dir/pair-get-light.txt" || fail "wallpaper get in light mode is not the light half: $(cat "$shot_dir/pair-get-light.txt")"
  echo "SMOKE_WALLPAPER_PAIR_STATUS desktop dark, light, dark across two toggles"

  for f in pair-desk-dark pair-desk-light pair-lock-dark pair-desk-auto; do
    [ -s "$shot_dir/$f.png" ] || fail "no frame at $shot_dir/$f.png"
    f=${f#pair-}
    f=${f//-/_}
    echo "SMOKE_WALLPAPER_PAIR_${f^^} $shot_dir/pair-${f//_/-}.png"
  done
  dark=$(wallpaper_pair_mean "$shot_dir/pair-desk-dark.png")
  light=$(wallpaper_pair_mean "$shot_dir/pair-desk-light.png")
  echo "wallpaper pair: desktop mean dark $dark, light $light (of 1000)"
  [ $((light - dark)) -ge 300 ] || fail "the light half's desktop ($light) is not well over the dark half's ($dark)"
  wallpaper_pair_fade tolight "$dark" "$light"
  wallpaper_pair_fade todark "$light" "$dark"

  local pd pl
  pd=$(wallpaper_pair_primary "$shot_dir/pair-theme-dark.json") || fail "no primary in the dark half's theme.json"
  pl=$(wallpaper_pair_primary "$shot_dir/pair-theme-light.json") || fail "no primary in the light half's theme.json"
  echo "wallpaper pair: primary dark ($pd), light ($pl)"
  [ "$pd" != "$pl" ] || fail "theme.json's primary is the same under both halves"
  read -r r g b <<<"$pd"
  [ "$b" -gt "$r" ] || fail "the dark half (a blue band) seeded a primary that is not cool: ($pd)"
  read -r r g b <<<"$pl"
  [ "$r" -gt "$b" ] || fail "the light half (an orange band) seeded a primary that is not warm: ($pl)"
  echo "SMOKE_WALLPAPER_PAIR_PRIMARY each half seeds its own palette"

  # The band sits in the picture's left quarter, x 80 to 239 of 960: 160
  # to 479 on the 1920 wide output, clear of the centred lock card.
  local px
  px=$($convert_bin "$shot_dir/pair-lock-dark.png" -crop 120x200+300+700 +repage -scale 1x1! -format '%[fx:int(255*r)] %[fx:int(255*g)] %[fx:int(255*b)]' info:)
  echo "wallpaper pair: the lock's band patch ($px)"
  read -r r g b <<<"$px"
  [ "$b" -gt $((r + 40)) ] || fail "the lock in dark mode does not carry the dark half's blue band: ($px)"
  grep -q '^false$' "$shot_dir/pair-islocked.txt" 2>/dev/null || fail "the real password did not unlock: $(cat "$shot_dir/pair-islocked.txt" 2>/dev/null)"
  echo "SMOKE_WALLPAPER_PAIR_LOCK the lock shows the dark half"

  effective=$("$jq_bin" -r '.effective' "$shot_dir/pair-status-auto.json")
  desktop=$("$jq_bin" -r '.desktop' "$shot_dir/pair-status-auto.json")
  "$jq_bin" -e '.modeKey == "auto"' "$shot_dir/pair-status-auto.json" > /dev/null || fail "settings.json's theme.mode auto did not reach the shell"
  case "$effective" in
    dark) auto=$wallpaper_pair_dark ;;
    light) auto=$wallpaper_pair_light ;;
    *) fail "theme status under auto has no effective mode: $effective" ;;
  esac
  [ "$desktop" = "$auto" ] || fail "under theme.mode auto the effective mode is $effective and the desktop is $desktop"
  echo "SMOKE_WALLPAPER_PAIR_AUTO effective $effective shows $desktop"
}
