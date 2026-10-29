# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --picker: the wallpaper picker, which is the launcher's grid route rather
# than a surface of its own. `summon` opens it, `choose` is the same action
# Enter or a click on a cell takes (exposed over IPC because no synthetic
# pointer or key delivery into an OnDemand layer surface exists here), and
# `theme status` is what proves the pick actually became the wallpaper.
#
# The second half of the route is the generic image selector: `picker select`
# opens the same grid against a caller's directory and token, and the choose
# that follows has to answer that request with `{token, value}` in
# picker-selection.txt, the same request/answer handshake MenuIpc's select()
# writes to menu-selection.txt.
#
# Then the Dark/Light half: the staged subdirectories are moved in
# underneath the running shell, which also proves the scan re-runs per entry
# (a listing cached at startup would still report the flat set), and `picker
# variant light` is the segmented switcher's own action over IPC. The route
# is left open so the run's own screenshot is the grid.
#
# picker_done_path is this leg's own finish marker: the picker and the
# clipboard routes are the same launcher surface, so a run carrying both has
# the clipboard leg wait on this file rather than summoning over the grid.
leg_picker_flag="--picker"
leg_picker_order=120
leg_picker_needs="convert"

# This leg's own clock, the same rule menu_t0 and lock_t0 draw: the grid
# covers the whole output, so under --wallpaper it starts after that leg's
# last frame (t=14).
picker_t0() {
  if leg_on wallpaper; then echo 16; else echo 4; fi
}

picker_done_path="$shot_dir/picker-drive.done"
picker_grid_png="$shot_dir/picker-grid.png"
picker_variant_png="$shot_dir/picker-variant.png"
picker_flat_status_path="$shot_dir/picker-status-flat.json"
picker_dark_status_path="$shot_dir/picker-status-dark.json"
picker_light_status_path="$shot_dir/picker-status-light.json"
picker_variant_reply_path="$shot_dir/picker-variant-reply.txt"
picker_theme_status_path="$shot_dir/picker-theme-status.json"
picker_cached_status_path="$shot_dir/picker-status-cached.json"
picker_cache_listing_path="$shot_dir/picker-thumb-cache.txt"
picker_select_reply_path="$shot_dir/picker-select-reply.txt"
picker_selection_path="$shot_dir/picker-selection.txt"
picker_dir="$iso_home/.local/share/formalshell/pictures"

leg_picker_fixture() {
  # The route points at a directory of generated solid-colour fixtures, so
  # the grid renders real images and `choose` picks a real file rather than
  # a path that merely happens to parse. 960x540 rather than the old leg's
  # 1920x1080: that size is there to feed a resident-memory bracket this
  # port leaves behind, and anything past the grid cell's own ~210px decode
  # cap already exercises the cap.
  local i
  local picker_colors=(c0392b 27ae60 2980b9 f1c40f 8e44ad e67e22 16a085 2c3e50 d35400 c2185b 00838f 5d4037)
  mkdir -p "$picker_dir"
  for i in "${!picker_colors[@]}"; do
    $convert_bin -size 960x540 "xc:#${picker_colors[$i]}" "$picker_dir/img-$i.png"
  done
  # The Dark/Light variant sets, STAGED rather than in place: every leg
  # before the switcher runs against the flat listing, which is what a
  # directory with no variant pair does, and the drive moves these in
  # mid-run to cover the other half. `.stage` is not one of the scan's
  # starting points, so nothing here is visible until it is moved.
  # Deliberately different counts per variant, so a status dump reporting
  # the wrong set is unambiguous rather than a coincidence.
  local picker_dark_colors=(1b2a4a 24344f 2f3f5c 3a4a68 111c33)
  local picker_light_colors=(f5efe0 e8dcc3 fbf7ee)
  mkdir -p "$picker_dir/.stage/Dark" "$picker_dir/.stage/Light"
  for i in "${!picker_dark_colors[@]}"; do
    $convert_bin -size 960x540 "xc:#${picker_dark_colors[$i]}" "$picker_dir/.stage/Dark/dark-$i.png"
  done
  for i in "${!picker_light_colors[@]}"; do
    $convert_bin -size 960x540 "xc:#${picker_light_colors[$i]}" "$picker_dir/.stage/Light/light-$i.png"
  done
  settings_fragment ', "picker": {"directory": "'"$picker_dir"'"}'
}

leg_picker_timing() {
  # Past picker-drive.sh's own last step, which leaves the light-variant
  # grid up, so the run's own frame is that grid.
  local t0
  t0=$(picker_t0)
  leg_timing $((24 + t0 - 4)) $((56 + t0 - 4))
}

leg_picker_drive() {
  local t0 script="$shot_dir/picker-drive.sh"
  t0=$(picker_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
$ipc call picker summon > /dev/null 2>&1
sleep 2
"$grim_bin" "$picker_grid_png" > /dev/null 2>&1
$ipc call picker status > "$picker_flat_status_path" 2>&1
$ipc call picker choose "$picker_dir/img-3.png" > /dev/null 2>&1
sleep 3
$ipc call theme status > "$picker_theme_status_path" 2>&1
sleep 1
$ipc call picker select "$picker_dir" tok-picker > "$picker_select_reply_path" 2>&1
sleep 2
$ipc call picker choose "$picker_dir/img-1.png" > /dev/null 2>&1
sleep 1
cat "$iso_home/.local/state/formalshell/picker-selection.txt" > "$picker_selection_path" 2>&1
mv "$picker_dir/.stage/Dark" "$picker_dir/Dark"
mv "$picker_dir/.stage/Light" "$picker_dir/Light"
$ipc call picker summon > /dev/null 2>&1
sleep 2
$ipc call picker status > "$picker_dark_status_path" 2>&1
$ipc call picker variant light > "$picker_variant_reply_path" 2>&1
sleep 2
"$grim_bin" "$picker_variant_png" > /dev/null 2>&1
$ipc call picker status > "$picker_light_status_path" 2>&1
sleep 3
$ipc call picker status > "$picker_cached_status_path" 2>&1
ls -1 "$iso_home/.cache/formalshell/thumbnails" > "$picker_cache_listing_path" 2>&1
touch "$picker_done_path"
EOF
  hypr_exec_once "bash $script"
}

leg_picker_assert() {
  if [ ! -f "$picker_grid_png" ]; then
    fail "no picker-grid screenshot produced"
  fi
  echo "SMOKE_PICKER_GRID $picker_grid_png"
  # The flat listing first: with no subdirectory pair the route lists the
  # directory itself and raises no switcher, which is what every setup that
  # does not use variants must keep doing.
  if [ ! -s "$picker_flat_status_path" ]; then
    fail "no flat picker status produced"
  fi
  cat "$picker_flat_status_path"; echo
  if ! grep -q '"hasVariants":false' "$picker_flat_status_path" \
    || ! grep -q '"variant":"none"' "$picker_flat_status_path" \
    || ! grep -q '"count":12' "$picker_flat_status_path" \
    || ! grep -q '"colors":0' "$picker_flat_status_path"; then
    fail "the flat listing did not report all 12 fixtures and no colours with no variants, got: $(cat "$picker_flat_status_path")"
  fi
  # Choosing over IPC has to have run State.setWallpaper() through to the
  # ThemeEngine, which is what `theme status` reports back.
  if [ ! -s "$picker_theme_status_path" ]; then
    fail "no picker theme status produced"
  fi
  cat "$picker_theme_status_path"; echo
  if ! grep -q "\"wallpaper\":\"$picker_dir/img-3.png\"" "$picker_theme_status_path"; then
    fail "theme status did not report the picker-chosen wallpaper, got: $(cat "$picker_theme_status_path")"
  fi
  if ! grep -q "\"desktop\":\"$picker_dir/img-3.png\"" "$picker_theme_status_path"; then
    fail "the picked picture is not what the desktop shows, got: $(cat "$picker_theme_status_path")"
  fi
  # Generic image-selector proof: the second choose(), made against a
  # select()-mode request, must resolve that request's token with the chosen
  # path.
  if ! grep -qx 'ok' "$picker_select_reply_path" 2>/dev/null; then
    fail "picker select was refused, got: $(cat "$picker_select_reply_path" 2>/dev/null)"
  fi
  if [ ! -s "$picker_selection_path" ]; then
    fail "no picker-selection.txt produced"
  fi
  cat "$picker_selection_path"; echo
  if ! grep -q '"token":"tok-picker"' "$picker_selection_path" \
    || ! grep -q "\"value\":\"$picker_dir/img-1.png\"" "$picker_selection_path"; then
    fail "picker-selection.txt did not resolve tok-picker with the chosen path, got: $(cat "$picker_selection_path")"
  fi
  # Then the same directory with the pair moved in underneath the running
  # shell: hasVariants, entered on the theme's own mode, listing that set.
  if [ ! -s "$picker_dark_status_path" ]; then
    fail "no dark-variant picker status produced"
  fi
  cat "$picker_dark_status_path"; echo
  if ! grep -q '"hasVariants":true' "$picker_dark_status_path" \
    || ! grep -q '"darkCount":5' "$picker_dark_status_path" \
    || ! grep -q '"lightCount":3' "$picker_dark_status_path"; then
    fail "the variant listing did not report the staged sets, got: $(cat "$picker_dark_status_path")"
  fi
  if ! grep -qx 'ok' "$picker_variant_reply_path" 2>/dev/null; then
    fail "picker variant light was refused, got: $(cat "$picker_variant_reply_path" 2>/dev/null)"
  fi
  if [ ! -s "$picker_light_status_path" ]; then
    fail "no light-variant picker status produced"
  fi
  cat "$picker_light_status_path"; echo
  if ! grep -q '"variant":"light"' "$picker_light_status_path" \
    || ! grep -q "\"count\":3" "$picker_light_status_path"; then
    fail "the switcher did not swap the listing to the light set, got: $(cat "$picker_light_status_path")"
  fi
  if [ ! -f "$picker_variant_png" ]; then
    fail "no picker-variant screenshot produced"
  fi
  echo "SMOKE_PICKER_VARIANT $picker_variant_png"
  # ThumbnailService: the grid draws prerendered 512px crops rather than
  # decoding the wallpapers themselves, and a warm and a fallback paint the
  # same picture, so the frame above cannot tell the two apart. The cache
  # directory and `cachedThumbnails` can. All twenty fixtures, the twelve
  # flat ones warmed off the configured directory at startup and the eight
  # staged ones warmed when the second summon re-scanned.
  if [ ! -s "$picker_cache_listing_path" ]; then
    fail "no thumbnail cache directory produced"
  fi
  cat "$picker_cache_listing_path"
  local cached
  cached=$(grep -c '\.jpg$' "$picker_cache_listing_path" || true)
  if [ "$cached" -ne 20 ]; then
    fail "expected 20 cached thumbnails, got $cached: $(cat "$picker_cache_listing_path")"
  fi
  if grep -q '\.part\.jpg$' "$picker_cache_listing_path"; then
    fail "the thumbnail warm left partial files behind: $(cat "$picker_cache_listing_path")"
  fi
  cat "$picker_cached_status_path"; echo
  if ! grep -q "\"count\":3" "$picker_cached_status_path" \
    || ! grep -q "\"cachedThumbnails\":3" "$picker_cached_status_path"; then
    fail "the light listing is not fully backed by cached thumbnails, got: $(cat "$picker_cached_status_path")"
  fi
}
