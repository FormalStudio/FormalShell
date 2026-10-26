# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --depth rides --lock: a wallpaper with a depth map beside it, the way
# formalshell-depth writes one (`.depth/<name>.png`, not older than the
# picture), seeded into state.json before the shell starts. The picture is a
# dark field with an orange band down the screen through the default
# theme's clock digits (x 588 to 767 at 1920x1080), and the map is white over
# that band alone, so the band is the subject.
#
# Two frames of the same lock: the first under the defaults, the band drawn
# over the clock's digits; the second after settings.json is retargeted to
# `lock.depth: false`, the digits back over the band. `lock status` has to
# say `depth` true then false. The foreground carries the card's own fill,
# so inside the band the rows just inside and just outside the top and
# bottom of the drawn rect (`depthRect`) match within 2 per channel, and the
# band down that rect is one flat fill with depth on and carries the digits
# with it off.
#
# FS_DEPTH_PICTURE=<path in the VM> swaps the band for a real wallpaper and
# the map beside it (`.depth/<name>.png`); only the status checks apply then.
leg_depth_flag="--depth"
leg_depth_order=111
leg_depth_needs="convert jq"

leg_depth_validate() {
  if ! leg_on lock; then
    echo "usage: $0 --depth rides --lock, which locks the session it photographs" >&2
    exit 1
  fi
  if leg_on showcase || leg_on wallpaper || leg_on config_reload || leg_on lyrics_blur; then
    echo "usage: $0 --depth seeds its own wallpaper and retargets settings.json; run it without --showcase, --wallpaper, --config-reload or --lyrics-blur" >&2
    exit 1
  fi
}

depth_dir="$shot_dir/depth-wallpapers"
depth_real="${FS_DEPTH_PICTURE:-}"
if [ -n "$depth_real" ]; then
  depth_picture_path="$depth_dir/$(basename "$depth_real")"
else
  depth_picture_path="$depth_dir/band.png"
fi
depth_on_path="$shot_dir/depth-on.png"
depth_off_path="$shot_dir/depth-off.png"
depth_status_on_path="$shot_dir/depth-status-on.json"
depth_status_off_path="$shot_dir/depth-status-off.json"
depth_gen1_path="$shot_dir/depth-settings-gen1.json"
depth_gen2_path="$shot_dir/depth-settings-gen2.json"

leg_depth_fixture() {
  mkdir -p "$depth_dir/.depth"
  if [ -n "$depth_real" ]; then
    local map
    map="$(dirname "$depth_real")/.depth/$(basename "$depth_real").png"
    [ -s "$depth_real" ] && [ -s "$map" ] || fail "FS_DEPTH_PICTURE needs $depth_real and $map"
    cp "$depth_real" "$depth_picture_path"
    cp "$map" "$depth_dir/.depth/$(basename "$depth_real").png"
    touch -r "$depth_picture_path" "$depth_dir/.depth/$(basename "$depth_real").png"
  else
  $convert_bin -size 1920x1080 xc:'#101418' -fill '#ff7a1a' -draw 'rectangle 620,0 739,1079' "$depth_picture_path"
  $convert_bin -size 1920x1080 xc:black -fill white -draw 'rectangle 620,0 739,1079' "$depth_dir/.depth/band.png.png"
  touch -r "$depth_picture_path" "$depth_dir/.depth/band.png.png"
  fi
  mkdir -p "$iso_home/.local/state/formalshell"
  printf '{"wallpaper": "%s"}\n' "$depth_picture_path" > "$iso_home/.local/state/formalshell/state.json"
}

# Inside --lock's own window: locked at t0+1, typing from t0+6.
leg_depth_drive() {
  local t0 script="$shot_dir/depth-drive.sh" settings_path="$iso_home/.config/formalshell/settings.json"
  t0=$(lock_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
sleep 2
$ipc call lock status > "$depth_status_on_path" 2>&1
"$grim_bin" "$depth_on_path" > /dev/null 2>&1
cp "\$(readlink -f "$settings_path")" "$depth_gen1_path"
"$jq_bin" '.lock.depth = false' "$depth_gen1_path" > "$depth_gen2_path"
ln -sfn "$depth_gen2_path" "$settings_path"
sleep 1.5
$ipc call lock status > "$depth_status_off_path" 2>&1
"$grim_bin" "$depth_off_path" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_depth_assert() {
  local f on off
  for f in "$depth_on_path" "$depth_off_path" "$depth_status_on_path" "$depth_status_off_path"; do
    [ -s "$f" ] || fail "--depth produced no $f"
  done
  echo "SMOKE_DEPTH_ON $depth_on_path"
  echo "SMOKE_DEPTH_OFF $depth_off_path"
  cat "$depth_status_on_path"; echo
  grep -q '"depth":true' "$depth_status_on_path" || fail "lock status did not report depth:true with a map beside the wallpaper. Got: $(cat "$depth_status_on_path")"
  grep -q '"depth":true' "$depth_status_off_path" && fail "lock status still reported depth:true after lock.depth was set false. Got: $(cat "$depth_status_off_path")"
  [ -n "$depth_real" ] && return 0
  local rect x y bottom crop
  rect=$("$jq_bin" -c 'first(.. | objects | select(.depthRect != null) | .depthRect)' "$depth_status_on_path")
  [ -n "$rect" ] || fail "lock status reported no depthRect with depth on"
  x=680
  y=$("$jq_bin" '.y' <<<"$rect")
  bottom=$("$jq_bin" '.y + .height' <<<"$rect")
  for pair in "$((y - 3)) $((y + 3))" "$((bottom - 4)) $((bottom + 2))"; do
    set -- $pair
    local a b
    a=$($convert_bin "$depth_on_path" -format "%[fx:int(255*p{$x,$1}.r)] %[fx:int(255*p{$x,$1}.g)] %[fx:int(255*p{$x,$1}.b)]" info:)
    b=$($convert_bin "$depth_on_path" -format "%[fx:int(255*p{$x,$2}.r)] %[fx:int(255*p{$x,$2}.g)] %[fx:int(255*p{$x,$2}.b)]" info:)
    echo "depth edge at x $x: y $1 ($a) y $2 ($b)"
    read -r ar ag ab <<<"$a"
    read -r br bg bb <<<"$b"
    for d in $((ar - br)) $((ag - bg)) $((ab - bb)); do
      [ "${d#-}" -le 2 ] || fail "a step at the depth rect's edge inside the band: y $1 is ($a), y $2 is ($b)"
    done
  done
  # The band clear of its feathered sides, down the drawn rect: one flat
  # fill with the digits hidden, the digits back with depth off.
  crop="104x$((bottom - y))+628+$y"
  on=$($convert_bin "$depth_on_path" -crop "$crop" +repage -format '%[fx:standard_deviation]' info:)
  off=$($convert_bin "$depth_off_path" -crop "$crop" +repage -format '%[fx:standard_deviation]' info:)
  echo "depth band under the clock, spread: on $on off $off"
  awk -v on="$on" -v off="$off" 'BEGIN { exit !(on < 0.01 && off > 0.05) }' \
    || fail "the band did not hide the clock with a depth map in use (spread on $on, off $off)"
}
