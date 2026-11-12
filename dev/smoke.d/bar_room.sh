# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --bar-room crowds a 1920px strip past its own length: the default right
# region's eight cells plus twelve `custom:` CommandModules, each a fixed
# 40-character label, behind a long-title track so the now-playing cell
# wants its full 220px budget at the same time the right region wants more
# room than the strip has. Two claims,
# read off `bar room`: the now-playing cell gives ground
# before a cell hides (`nowPlaying.budget` under 220), and what still does
# not fit hides whole cells from the right region's own inner edge rather
# than cutting one in half (`regions.right.hidden` at least 1). The frame is
# read by hand for the same claim: a whole cell sitting at the boundary,
# never a sliced one, and held against the rects `bar room` reports: no
# two drawn cells intersect and none is cut by its region clip
# (bar_title.sh's bar_cells_defects).
leg_bar_room_flag="--bar-room"
leg_bar_room_order=195
leg_bar_room_compositors="hyprland shoji"
leg_bar_room_needs="mpv ffmpeg jq"
leg_bar_room_claims="bar.layout.right bar.modules"

bar_room_json_path="$shot_dir/bar-room.json"
bar_room_png_path="$shot_dir/bar-room.png"
bar_room_status_path="$shot_dir/bar-room-status.json"
bar_room_cmd_dir="$shot_dir/bar-room-cmd"
bar_room_track_path="$shot_dir/bar-room-track.flac"
bar_room_title="FormalShell Bar Room Smoke Verification Extremely Long Now Playing Title"
bar_room_artist="FormalShell Test Artist"
bar_room_pid_path="$shot_dir/bar-room-mpv.pid"

# A fixed 40-character label per cell (`BAR ROOM CELL NN ` plus enough `X`
# to reach 40), the same CommandModule shape bar_layout.sh's fixture uses.
bar_room_label() {
  # $n already carries seq -w's own zero-padding (01..12): %s, not %d, so
  # printf never reads a leading-zero argument as octal ("08"/"09" are not
  # valid octal digits and would error).
  local n="$1" prefix pad
  prefix=$(printf 'BAR ROOM CELL %s ' "$n")
  pad=$((40 - ${#prefix}))
  [ "$pad" -lt 0 ] && pad=0
  printf '%s%s' "$prefix" "$(printf '%*s' "$pad" '' | tr ' ' 'X')"
}

leg_bar_room_fixture() {
  mkdir -p "$bar_room_cmd_dir"
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 25 \
    -metadata "title=$bar_room_title" -metadata "artist=$bar_room_artist" \
    -c:a flac -y "$bar_room_track_path"

  local i label modules="" layout_right='"battery", "audio", "network", "bluetooth", "weather", "tray", "bell", "indicators"'
  for i in $(seq -w 1 12); do
    label=$(bar_room_label "$i")
    write_script "$bar_room_cmd_dir/cmd$i.sh" <<EOF
#!/usr/bin/env bash
printf '{"text": "%s", "tooltip": "", "class": ""}' "$label"
EOF
    modules="$modules{\"id\": \"barroom$i\", \"type\": \"command\", \"command\": [\"bash\", \"$bar_room_cmd_dir/cmd$i.sh\"], \"interval\": 30000},"
    layout_right="$layout_right, \"custom:barroom$i\""
  done
  modules="${modules%,}"
  settings_pin '. * {"bar": {"layout": {"right": ['"$layout_right"']}, "modules": ['"$modules"']}}'
}

leg_bar_room_timing() {
  # A tag-match poll (up to 8s) plus a 3s settle for NowPlaying's own
  # refit timer (Theme.motion.spatial + 32ms, well under a second): worst
  # case lands ~13s in.
  leg_timing 40
}

leg_bar_room_drive() {
  local script="$shot_dir/bar-room-drive.sh"
  local kill_script="$shot_dir/bar-room-kill.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$mpv_bin" --no-video --really-quiet "$bar_room_track_path" &
echo \$! > "$bar_room_pid_path"
SECONDS=0
while [ "\$SECONDS" -lt 8 ]; do
  $ipc call media status > "$bar_room_status_path" 2>&1
  "$jq_bin" -e --arg t "$bar_room_title" '.title == \$t' "$bar_room_status_path" > /dev/null 2>&1 && break
  sleep 1
done
sleep 3
$ipc call bar room > "$bar_room_json_path" 2>&1
"$grim_bin" "$bar_room_png_path" > /dev/null 2>&1
EOF

  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
[ -f "$bar_room_pid_path" ] && kill "\$(cat "$bar_room_pid_path")" 2>/dev/null
true
EOF
  add_cleanup "bash $kill_script"
  wm_exec_once "bash $script"
}

leg_bar_room_assert() {
  local hidden budget natural
  "$jq_bin" -e --arg t "$bar_room_title" '.title == $t' "$bar_room_status_path" > /dev/null 2>&1 \
    || fail "media status never showed the bar-room long-title track, got: $(cat "$bar_room_status_path" 2>/dev/null)"
  if [ ! -s "$bar_room_json_path" ]; then
    fail "no bar room output produced"
  fi
  cat "$bar_room_json_path"; echo
  echo "SMOKE_BAR_ROOM_STATE $bar_room_json_path"
  hidden=$("$jq_bin" -r '.[0].regions.right.hidden' "$bar_room_json_path" 2>/dev/null)
  budget=$("$jq_bin" -r '.[0].nowPlaying.budget' "$bar_room_json_path" 2>/dev/null)
  natural=$("$jq_bin" -r '.[0].nowPlaying.natural' "$bar_room_json_path" 2>/dev/null)
  if [ -z "$hidden" ] || [ "$hidden" = "null" ] || [ "$hidden" -lt 1 ]; then
    fail "bar room does not report a hidden cell in the crowded right region, got hidden=$hidden"
  fi
  if ! awk -v b="${budget:--1}" 'BEGIN { exit !(b >= 0 && b < 220) }'; then
    fail "bar room does not report a now-playing budget under 220 while the right region is crowded, got budget=$budget (natural=$natural)"
  fi
  local defects
  defects=$(bar_cells_defects "$bar_room_json_path")
  if [ -n "$defects" ]; then
    fail "cells on the crowded strip do not sit clear of each other: $defects"
  fi
  if [ ! -f "$bar_room_png_path" ]; then
    fail "no bar-room screenshot produced"
  fi
  echo "SMOKE_BAR_ROOM $bar_room_png_path"
}
