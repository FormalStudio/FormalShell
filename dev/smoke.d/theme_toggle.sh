# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --theme-toggle drives `theme mode toggle` twice and photographs both sides.
#
# On its own, with no wallpaper set: the round trip runs against the
# mode-matched Flexoki fallback, so `theme status` has to report mode:"light"
# with wallpaper still "" and a theme.json present, then mode:"dark" again.
# No matugen is involved at all.
#
# Combined with --wallpaper the same round trip runs WITH the wallpaper set,
# and the rendered theme.json is dumped at every step: each dump must carry
# matugen-derived (non-Flexoki) values and the final one must be byte
# identical to the first. That is what proves a mode toggle re-runs matugen
# against the set wallpaper instead of resetting to the fallback palette.
leg_theme_toggle_flag="--theme-toggle"
leg_theme_toggle_order=220
leg_theme_toggle_compositors="hyprland shoji"
leg_theme_toggle_claims="theme.mode=shared"

theme_dark_png="$shot_dir/theme-dark.png"
theme_light_png="$shot_dir/theme-light.png"
theme_toggle_status_path="$shot_dir/theme-toggle-status.json"
theme_toggle_status2_path="$shot_dir/theme-toggle-status-2.json"
theme_json_dump1_path="$shot_dir/theme-json-1.json"
theme_json_dump2_path="$shot_dir/theme-json-2.json"
theme_json_dump3_path="$shot_dir/theme-json-3.json"
theme_toggle_json="$iso_home/.local/state/formalshell/theme.json"

leg_theme_toggle_timing() {
  if leg_on wallpaper; then
    # Each toggle gets 4s for a full matugen rerun rather than 2s for a
    # fallback write.
    leg_timing 49
  else
    leg_timing 44
  fi
}

leg_theme_toggle_drive() {
  local gap="2" dump_pre="" dump_mid="" dump_post="" script="$shot_dir/theme-toggle-drive.sh"
  if leg_on wallpaper; then
    gap="4"
    dump_pre="cat \"$theme_toggle_json\" > \"$theme_json_dump1_path\" 2>&1"
    dump_mid="cat \"$theme_toggle_json\" > \"$theme_json_dump2_path\" 2>&1"
    dump_post="cat \"$theme_toggle_json\" > \"$theme_json_dump3_path\" 2>&1"
  fi
  write_script "$script" <<EOF
#!/usr/bin/env bash
$dump_pre
"$grim_bin" "$theme_dark_png" > /dev/null 2>&1
$ipc call theme mode toggle > /dev/null 2>&1
sleep $gap
$ipc call theme status > "$theme_toggle_status_path" 2>&1
$dump_mid
"$grim_bin" "$theme_light_png" > /dev/null 2>&1
$ipc call theme mode toggle > /dev/null 2>&1
sleep $gap
$ipc call theme status > "$theme_toggle_status2_path" 2>&1
$dump_post
EOF
  wm_exec_once "bash $script"
}

leg_theme_toggle_assert() {
  if leg_on wallpaper; then
    # Flexoki hexes only ever enter theme.json through the fallback palette
    # write, so any appearance of one means a toggle reset the theme instead
    # of re-running matugen. Matched -i because that write is uppercase.
    if [ ! -s "$theme_json_dump1_path" ] || ! "$jq_bin" -e '.mode == "dark" and (.background | ascii_downcase) != "#100f0f"' "$theme_json_dump1_path" > /dev/null 2>&1; then
      [ -f "$theme_json_dump1_path" ] && cat "$theme_json_dump1_path" >&2
      fail "pre-toggle theme.json is missing or not matugen-derived dark"
    fi
    if [ ! -s "$theme_toggle_status_path" ] \
      || ! "$jq_bin" -e --arg wp "$wp2_path" '.wallpaper == $wp and .mode == "light"' "$theme_toggle_status_path" > /dev/null 2>&1; then
      [ -f "$theme_toggle_status_path" ] && cat "$theme_toggle_status_path" >&2
      fail "theme status after toggle did not report mode:light with the wallpaper kept"
    fi
    cat "$theme_toggle_status_path"; echo
    if [ ! -s "$theme_json_dump2_path" ] || ! "$jq_bin" -e '.mode == "light" and (.background | ascii_downcase) != "#fffcf0"' "$theme_json_dump2_path" > /dev/null 2>&1; then
      [ -f "$theme_json_dump2_path" ] && cat "$theme_json_dump2_path" >&2
      fail "theme.json after the dark to light toggle is not matugen-derived light (toggle reset to fallback?)"
    fi
    if [ ! -s "$theme_toggle_status2_path" ] \
      || ! "$jq_bin" -e --arg wp "$wp2_path" '.wallpaper == $wp and .mode == "dark"' "$theme_toggle_status2_path" > /dev/null 2>&1; then
      [ -f "$theme_toggle_status2_path" ] && cat "$theme_toggle_status2_path" >&2
      fail "theme status after the second toggle did not report mode:dark with the wallpaper kept"
    fi
    cat "$theme_toggle_status2_path"; echo
    if ! cmp -s "$theme_json_dump1_path" "$theme_json_dump3_path"; then
      diff "$theme_json_dump1_path" "$theme_json_dump3_path" >&2 || true
      fail "theme.json did not round-trip byte-identical through light and back"
    fi
  else
    if [ ! -s "$theme_toggle_status_path" ] \
      || ! "$jq_bin" -e '.wallpaper == "" and .mode == "light" and .themeJsonPresent == true' "$theme_toggle_status_path" > /dev/null 2>&1; then
      [ -f "$theme_toggle_status_path" ] && cat "$theme_toggle_status_path" >&2
      fail "theme status after toggle did not report mode:light with no wallpaper"
    fi
    cat "$theme_toggle_status_path"; echo
    if [ ! -s "$theme_toggle_status2_path" ] \
      || ! "$jq_bin" -e '.wallpaper == "" and .mode == "dark"' "$theme_toggle_status2_path" > /dev/null 2>&1; then
      [ -f "$theme_toggle_status2_path" ] && cat "$theme_toggle_status2_path" >&2
      fail "theme status after the second toggle did not report mode:dark"
    fi
    cat "$theme_toggle_status2_path"; echo
  fi
  if [ ! -s "$theme_dark_png" ] || [ ! -s "$theme_light_png" ]; then
    fail "missing theme-toggle screenshot pair ($theme_dark_png / $theme_light_png)"
  fi
  # The pair exists to be compared: two identical frames mean the mode
  # changed in the shell's own bookkeeping and nowhere on screen.
  if cmp -s "$theme_dark_png" "$theme_light_png"; then
    fail "theme-dark and theme-light screenshots are byte-identical: nothing recoloured"
  fi
  echo "SMOKE_THEME_DARK $theme_dark_png"
  echo "SMOKE_THEME_LIGHT $theme_light_png"
}
