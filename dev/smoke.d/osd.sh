# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --osd drives the bottom-centre pill four ways off one timeline. It opens
# with the entrance itself, at a tenth of its speed (`debug motionScale`),
# sampled across the travel into osd-emerge-1..N.png over a 400x90 band at
# the bottom of the output, with a bare-desktop frame of the same band taken
# first and the pill's resting pose at the end (osd-rest.png): the pill buds
# off the bottom line (M57 D8), so an early frame has its ink on the output's
# very last row, and at rest it is a plain pill a screenPadding clear of it.
# The pill is retriggered on every frame, which is what holds it up through
# a travel many times its own 1.6s auto-hide.
#
# Then, back at full speed, the three kinds off the original timeline: a
# manual `osd volume` (osd-manual.png), a real `wpctl set-volume` (the
# AudioService.changed auto-show trigger, this run's own SMOKE_OK frame),
# then `osd brightness` (osd-brightness.png). The VM has a pipewire null
# sink but no backlight device, so the brightness frame proves the surface
# renders that kind honestly (0%, empty track), not that hardware exists.
# The sink is set to a second value first, because pipewire outlives the
# session: re-setting an already-30% sink changes nothing, `changed` never
# fires, and the auto-show leg would photograph an empty screen. That half of
# the timeline is anchored 26s into the drive rather than run straight off
# the entrance, so its own spacing is what it always was however long the
# sampling took.
#
# The entrance probes are pinned to the bare rig's own numbers, so
# `--bar-position bottom --osd` and `--frame --osd` photograph the two cases
# with a real line under the pill and leave them out; those combinations are
# read by eye.
#
# Ridden by --no-morph the claim turns over (M60 T2): a popover has no line to
# come out of, so the pill fades in a few pixels under its rest and the
# output's last row stays bare through the whole entrance. Which one runs is
# read off `debug dump`'s `theme.morph`, not off the rider, and both are then
# read against the same resting pose.
leg_osd_flag="--osd"
leg_osd_order=50
# The brightness frame, the last of the desktop with the pill on it.
leg_osd_desktop_until=37
leg_osd_needs="wpctl convert jq"

osd_manual_path="$shot_dir/osd-manual.png"
osd_brightness_path="$shot_dir/osd-brightness.png"
osd_desktop_path="$shot_dir/osd-desktop.png"
osd_rest_path="$shot_dir/osd-rest.png"
osd_dump_path="$shot_dir/osd-dump.json"

# The band the entrance is read in: 400px of the output's own bottom 90 rows,
# centred, so the 320-wide pill sits inside it with the output's last row at
# the crop's own last. Column 200 of the crop is the output's centre, which is
# the pill's own.
osd_region="760,990 400x90"
osd_column_probe=200
osd_last_row=89
osd_frames=12

leg_osd_timing() {
  # The auto-show trigger lands at 31, and this run's own SMOKE_OK frame is
  # the one it produces. The brightness frame lands at 36.
  leg_timing 32 0 6
}

leg_osd_drive() {
  # The entrance first, at a tenth speed, then the three kinds at full speed
  # from the 26s anchor. Each of those is screenshotted a second later, well
  # inside the pill's 1.6s auto-hide window, with enough gap between them that
  # the previous card is long gone before the next fires. The 29s set is not
  # photographed: it exists so the 31s one is always a real change.
  local script="$shot_dir/osd-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
start=\$SECONDS
call() { $ipc call "\$@" > /dev/null 2>&1; }
sleep 4
$ipc call debug dump > "$osd_dump_path" 2>&1
call debug motionScale 1000
"$grim_bin" -g "$osd_region" "$osd_desktop_path" > /dev/null 2>&1
call osd volume
for i in \$(seq 1 $osd_frames); do
  sleep 0.3
  "$grim_bin" -g "$osd_region" "$shot_dir/osd-emerge-\$i.png" > /dev/null 2>&1
  call osd volume
done
call osd volume
sleep 1.2
call osd volume
sleep 0.6
"$grim_bin" -g "$osd_region" "$osd_rest_path" > /dev/null 2>&1
call debug motionScale 100
call osd close
while [ \$((SECONDS - start)) -lt 26 ]; do sleep 0.2; done
call osd volume
sleep 1
"$grim_bin" "$osd_manual_path" > /dev/null 2>&1
sleep 2
"$wpctl_bin" set-volume @DEFAULT_AUDIO_SINK@ 80% > /dev/null 2>&1
sleep 2
"$wpctl_bin" set-volume @DEFAULT_AUDIO_SINK@ 30% > /dev/null 2>&1
sleep 4
call osd brightness
sleep 1
"$grim_bin" "$osd_brightness_path" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# One whole column of a frame, as six hex digits per row, top to bottom.
osd_column() {
  $convert_bin "$1" -crop "1x4096+$2+0" +repage -depth 8 txt:- 2>/dev/null \
    | awk 'NR > 1 { print toupper(substr($3, 2, 6)) }'
}

# The bottom-most row of column $2 in frame $1 that is not the bare desktop's
# own ink, or -1 for a column with nothing drawn on it at all.
osd_ink_bottom() {
  local i
  local -a shot desk
  mapfile -t shot < <(osd_column "$1" "$2")
  mapfile -t desk < <(osd_column "$osd_desktop_path" "$2")
  for ((i = ${#shot[@]} - 1; i >= 0; i--)); do
    if [ -n "${shot[i]}" ] && [ "${shot[i]}" != "${desk[i]}" ]; then
      echo "$i"
      return 0
    fi
  done
  echo -1
}

leg_osd_assert() {
  local i path
  if [ ! -f "$osd_manual_path" ]; then
    fail "no osd-manual screenshot produced"
  fi
  echo "SMOKE_OSD_MANUAL $osd_manual_path"
  if [ ! -f "$osd_brightness_path" ]; then
    fail "no osd-brightness screenshot produced"
  fi
  echo "SMOKE_OSD_BRIGHTNESS $osd_brightness_path"

  [ -f "$osd_desktop_path" ] || fail "no bare desktop frame at $osd_desktop_path"
  echo "SMOKE_OSD_DESKTOP $osd_desktop_path"
  [ -f "$osd_rest_path" ] || fail "no resting frame at $osd_rest_path"
  echo "SMOKE_OSD_REST $osd_rest_path"
  for i in $(seq 1 $osd_frames); do
    path="$shot_dir/osd-emerge-$i.png"
    [ -f "$path" ] || fail "no osd entrance frame $path"
    echo "SMOKE_OSD_EMERGE_$i $path"
  done

  if leg_on bar_position || leg_on frame; then
    echo "SMOKE_OSD_LINE read by eye: the probes below are pinned to the bare rig's own numbers"
    return 0
  fi

  [ -s "$osd_dump_path" ] || fail "no debug dump produced for the osd check"
  local emerge
  emerge=$("$jq_bin" -r 'if .theme.morph == false then "popover" else "join" end' "$osd_dump_path" 2>/dev/null)
  echo "SMOKE_OSD_MORPH emerge=$emerge"

  # The entrance: the pill comes out of the bottom line, so on at least one
  # frame its ink reaches the output's own last row at the pill's centre
  # column. A pill that appeared at its resting place instead would leave
  # every one of those rows bare. Under the popover that is
  # exactly the claim, the other way up: the pill comes in a few pixels off
  # its rest and never reaches the edge at all.
  local out=-1 hit=""
  for i in $(seq 1 $osd_frames); do
    path="$shot_dir/osd-emerge-$i.png"
    out=$(osd_ink_bottom "$path" "$osd_column_probe")
    if [ "$out" -eq "$osd_last_row" ]; then
      hit="$path"
      break
    fi
  done
  if [ "$emerge" != "join" ]; then
    [ -z "$hit" ] || fail \
      "$hit has ink on row $osd_last_row at column $osd_column_probe: a $emerge card comes out of nothing, it does not come off the output's bottom edge"
    echo "SMOKE_OSD_EMERGE ok row $osd_last_row stayed bare at column $osd_column_probe through osd-emerge-1..$osd_frames"
  else
    [ -n "$hit" ] || fail \
      "no entrance frame among osd-emerge-1..$osd_frames reaches row $osd_last_row at column $osd_column_probe: the pill never touches the output's bottom edge on its way out"
    echo "SMOKE_OSD_EMERGE ok row $osd_last_row at column $osd_column_probe $hit"
  fi

  # And at rest it is the plain pill at its resting rect, one screenPadding
  # of bare desktop between its own bottom and the output's edge. The pill's
  # cast lands nearer the edge by the few rows it reaches past the rect,
  # which is the card's own ink and not the pill sitting lower.
  local rest gap floor=5
  rest=$(osd_ink_bottom "$osd_rest_path" "$osd_column_probe")
  [ "$rest" -ge 0 ] || fail \
    "column $osd_column_probe of $osd_rest_path is bare desktop from top to bottom: the pill never settled"
  gap=$((osd_last_row - rest))
  if [ "$gap" -lt "$floor" ] || [ "$gap" -gt 16 ]; then
    fail "the pill's own bottom sits ${gap}px off the output's last row in $osd_rest_path against the screenPadding (12) it rests at: it has not let go of the line"
  fi
  echo "SMOKE_OSD_RESTING ok ${gap}px of desktop under the pill at column $osd_column_probe"
}
