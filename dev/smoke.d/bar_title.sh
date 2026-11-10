# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --bar-title focuses a window whose title is far longer than any strip, in
# a strip crowded with six `custom:` CommandModules on the right, and reads
# the result off `bar room`: the title's budget is the room
# its region actually has, the label is drawn
# at exactly that budget with the marquee running, and no two cells on the
# strip intersect, none is cut by its region's clip, none runs past the
# strip's own end insets and no region hid a cell to make room. "At its
# budget" is read off the rects too: the strip has no slack left, the
# title sits at its own ceiling, or the title cell stops one gap short of
# the centre region's first cell.
#
# A real MPRIS player (mpv) plays a track whose title also outgrows its cap
# the whole time, so the two labels share the room: the track keeps at
# least its own minimum (or half the room the cells leave, on a strip too
# short for both minimums), the title gets no more than the track while the
# track is short of its want, both budgets are non-zero, and the centre
# region's middle sits within 2px of the strip's whenever the minimums
# leave the centre centrable (the leg prints why when they do not). Last,
# settings.json is retargeted to a layout carrying the title cell
# alone, and the title stops at its own ceiling with the marquee still
# running on a strip with room to spare.
#
# Rides `--bar-position <edge>`, which pins the edge: a left or right bar
# turns the title and runs every region down the strip, which is where
# cells used to land on top of each other. Two frames
# a second apart, `bar-title.png` and `bar-title-later.png`, are the marquee
# moving, read by eye.
leg_bar_title_flag="--bar-title"
leg_bar_title_order=196
leg_bar_title_needs="foot jq mpv ffmpeg"
leg_bar_title_claims="bar.layout bar.modules media.player"

bar_title_json_path="$shot_dir/bar-title.json"
bar_title_poll_path="$shot_dir/bar-title-poll.json"
bar_title_png_path="$shot_dir/bar-title.png"
bar_title_later_png_path="$shot_dir/bar-title-later.png"
bar_title_cmd_dir="$shot_dir/bar-title-cmd"
bar_title_samples_dir="$shot_dir/bar-title-samples"
bar_title_alone_path="$shot_dir/bar-title-alone.json"
bar_title_alone_png_path="$shot_dir/bar-title-alone.png"
bar_title_status_path="$shot_dir/bar-title-media.json"
bar_title_track_path="$shot_dir/bar-title-track.flac"
bar_title_pid_path="$shot_dir/bar-title-mpv.pid"
bar_title_track="FormalShell Bar Title Now Playing Track Long Enough To Scroll On Any Strip"
bar_title_text="FormalShell bar title verification with a window title far longer than any strip it could ever sit on so the only thing that decides how much of it shows is the room the bar hands the cell"

leg_bar_title_fixture() {
  local i modules="" layout_right='"battery", "audio", "network", "bluetooth", "weather", "tray", "bell", "indicators"'
  mkdir -p "$bar_title_cmd_dir"
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 10 \
    -metadata "title=$bar_title_track" -metadata "artist=FormalShell Test Artist" \
    -c:a flac -y "$bar_title_track_path"
  for i in 1 2 3 4 5 6; do
    write_script "$bar_title_cmd_dir/cmd$i.sh" <<EOF
#!/usr/bin/env bash
printf '{"text": "ROOM $i", "tooltip": "", "class": ""}'
EOF
    modules="$modules{\"id\": \"bartitle$i\", \"type\": \"command\", \"command\": [\"bash\", \"$bar_title_cmd_dir/cmd$i.sh\"], \"interval\": 30000},"
    layout_right="$layout_right, \"custom:bartitle$i\""
  done
  modules="${modules%,}"
  settings_pin '. * {"bar": {"layout": {"left": ["launcher", "workspaces", "activeWindow"], "center": ["clock", "nowPlaying"], "right": ['"$layout_right"']}, "modules": ['"$modules"']}}'
}

leg_bar_title_timing() {
  leg_timing 75
}

leg_bar_title_drive() {
  local script="$shot_dir/bar-title-drive.sh"
  local kill_script="$shot_dir/bar-title-kill.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$mpv_bin" --no-video --really-quiet --loop-file=inf "$bar_title_track_path" &
echo \$! > "$bar_title_pid_path"
for _ in \$(seq 1 16); do
  $ipc call media status > "$bar_title_status_path" 2>&1
  grep -qF "\"title\":\"$bar_title_track\"" "$bar_title_status_path" && break
  sleep 0.5
done
spawn_foot formalshell-bar-title "" "$bar_title_text"
# Waits for the title to reach the cell rather than sleeping at it; the
# beat after is the label refit (Theme.motion.spatial past the last change)
# and the cells' own width glide settling behind it.
for _ in \$(seq 1 40); do
  $ipc call bar room > "$bar_title_poll_path" 2>&1
  "$jq_bin" -e '.[0].activeWindow.natural > 1000' "$bar_title_poll_path" > /dev/null 2>&1 && break
  sleep 0.5
done
sleep 3
$ipc call bar room > "$bar_title_json_path" 2>&1
"$grim_bin" "$bar_title_png_path" > /dev/null 2>&1
sleep 1
"$grim_bin" "$bar_title_later_png_path" > /dev/null 2>&1
# Focus to a short title and back, sampling the strip as fast as the IPC
# answers while the title cell, the centre and both region clips travel.
# Twelve samples take a few tens of ms, far less than foot takes to map, so
# each burst starts once Hyprland has the focus it asked for; otherwise the
# short window maps after the focus back and keeps the focus for the rest.
mkdir -p "$bar_title_samples_dir"
focused() {
  for _ in \$(seq 1 40); do
    [ "\$("$hyprctl_bin" -j activewindow | "$jq_bin" -r .class)" = "\$1" ] && return 0
    sleep 0.1
  done
  return 1
}
spawn_foot formalshell-bar-short "" short
focused formalshell-bar-short
for i in \$(seq 1 12); do
  $ipc call bar room > "$bar_title_samples_dir/to-short-\$i.json" 2>&1
done
"$hyprctl_bin" dispatch "hl.dsp.focus({ window = 'class:formalshell-bar-title' })" > /dev/null 2>&1
focused formalshell-bar-title
for i in \$(seq 1 12); do
  $ipc call bar room > "$bar_title_samples_dir/to-long-\$i.json" 2>&1
done
# The title cell alone on the strip.
sleep 2
settings_retarget '.bar.layout = {"left": ["activeWindow"], "center": [], "right": []}'
for _ in \$(seq 1 20); do
  $ipc call bar room > "$bar_title_alone_path" 2>&1
  "$jq_bin" -e '.[0].cells | length == 1' "$bar_title_alone_path" > /dev/null 2>&1 && break
  sleep 0.5
done
sleep 3
$ipc call bar room > "$bar_title_alone_path" 2>&1
"$grim_bin" "$bar_title_alone_png_path" > /dev/null 2>&1
EOF

  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
[ -f "$bar_title_pid_path" ] && kill "\$(cat "$bar_title_pid_path")" 2>/dev/null
true
EOF
  add_cleanup "bash $kill_script"
  hypr_exec_once "bash $script"
}

# Shared with --bar-room: every drawn cell in one `bar room` answer held
# against every other and against the strip. Prints one line per defect and
# nothing when the strip is sound.
bar_cells_defects() {
  "$jq_bin" -r '
    .[0] as $b
    | ($b.edge == "left" or $b.edge == "right") as $v
    | $b.cells as $c
    | ([range(0; $c | length) as $i | range($i + 1; $c | length) as $j
        | select($c[$i].x < $c[$j].x + $c[$j].width - 0.5 and $c[$j].x < $c[$i].x + $c[$i].width - 0.5
            and $c[$i].y < $c[$j].y + $c[$j].height - 0.5 and $c[$j].y < $c[$i].y + $c[$i].height - 0.5)
        | "overlap: \($c[$i].name) (\($c[$i].region)) and \($c[$j].name) (\($c[$j].region))"]
      + [$c[] | select(.whole | not) | "cut by its region clip: \(.name)"]
      + [$c[] | (if $v then .y else .x end) as $s | (if $v then .height else .width end) as $w
          | select($s < $b.edgeInset - 0.5 or $s + $w > $b.along - $b.edgeInset + 0.5)
          | "past the strip end: \(.name) at \($s)+\($w) on \($b.along)"])
    | .[]' "$1"
}

leg_bar_title_assert() {
  local defects edge budget natural extent scrolling hidden verdict
  if [ ! -s "$bar_title_json_path" ]; then
    fail "no bar room output produced for --bar-title"
  fi
  cat "$bar_title_json_path"; echo
  echo "SMOKE_BAR_TITLE_STATE $bar_title_json_path"
  edge=$("$jq_bin" -r '.[0].edge' "$bar_title_json_path")
  if leg_on bar_position && [ "$edge" != "$(leg_arg bar_position)" ]; then
    fail "bar room answered for a $edge bar, not the $(leg_arg bar_position) one asked for"
  fi
  read -r budget natural extent scrolling <<< "$("$jq_bin" -r '.[0].activeWindow | "\(.budget) \(.natural) \(.extent) \(.scrolling)"' "$bar_title_json_path")"
  echo "title on a $edge bar: budget=$budget natural=$natural extent=$extent scrolling=$scrolling"
  if ! awk -v b="$budget" -v n="$natural" -v e="$extent" 'BEGIN { exit !(b > 0 && n > b && e >= b - 1 && e <= b + 1) }'; then
    fail "the title is not drawn at a positive budget short of its natural width: budget=$budget natural=$natural extent=$extent"
  fi
  if [ "$scrolling" != "true" ]; then
    fail "the title overflows its budget but its marquee is not running"
  fi
  hidden=$("$jq_bin" -r '[.[0].regions[].hidden] | add' "$bar_title_json_path")
  if [ "$hidden" != "0" ]; then
    fail "a region hid $hidden cell(s) while the title still had room to give"
  fi
  defects=$(bar_cells_defects "$bar_title_json_path")
  if [ -n "$defects" ]; then
    fail "cells on the $edge strip do not sit clear of each other: $defects"
  fi
  # At its budget: the strip has no slack left, the title is at its own
  # ceiling, or the title cell ends one gap short of the centre region's
  # first cell (the centre held centred).
  verdict=$("$jq_bin" -r '
    .[0] as $b
    | ($b.edge == "left" or $b.edge == "right") as $v
    | ([$b.cells[] | select(.name == "activeWindow")] | first) as $t
    | ([$b.cells[] | select(.region == "center")] | first) as $c
    | (if $v then ($c.y - $t.y - $t.height) else ($c.x - $t.x - $t.width) end) as $gap
    | if $b.slack < 1 then "strip slack \($b.slack)"
      elif ($b.activeWindow.budget >= $b.activeWindow.cap - 1) then "title ceiling \($b.activeWindow.cap)"
      elif ($gap >= $b.gap - 1 and $gap <= $b.gap + 1) then "gap to the centre \($gap)"
      else "slack \($b.slack), gap to the centre \($gap)" end' "$bar_title_json_path")
  case "$verdict" in
    "strip slack "*|"title ceiling "*|"gap to the centre "*) echo "title cell at its budget: $verdict" ;;
    *) fail "the title cell stops short of the room it was given: $verdict" ;;
  esac
  # The two labels sharing the strip.
  if ! grep -qF "\"title\":\"$bar_title_track\"" "$bar_title_status_path" 2>/dev/null; then
    fail "media status never showed the bar-title track, got: $(cat "$bar_title_status_path" 2>/dev/null)"
  fi
  "$jq_bin" -c '.[0] | {nowPlaying, activeWindow}' "$bar_title_json_path"
  verdict=$("$jq_bin" -r '
    .[0] as $b | $b.nowPlaying as $p | $b.activeWindow as $w
    | ([$p.natural, $p.cap] | min) as $want
    | (($b.slack + $p.extent + $w.extent) / 2) as $even
    | ([$want, $p.min, $even] | min) as $least
    | if ($p.budget <= 0 or $w.budget <= 0) then "a label was squeezed to nothing: track \($p.budget), title \($w.budget)"
      elif ($p.natural <= 0) then "the track draws no label at all"
      elif ($p.budget < $least - 1) then "the track \($p.budget) is under its minimum \($least)"
      elif ($p.budget < $want - 1 and $w.budget > $p.budget + 1) then "the title \($w.budget) holds more than the track \($p.budget) while the track is short of its \($want)"
      elif ($w.budget > $w.cap + 1) then "the title \($w.budget) is past its ceiling \($w.cap)"
      else "ok track \($p.budget) of \($want), title \($w.budget) of \($w.cap)" end' "$bar_title_json_path")
  case "$verdict" in
    "ok "*) echo "labels share the strip: ${verdict#ok }" ;;
    *) fail "the labels do not share the $edge strip: $verdict" ;;
  esac
  # The centre centred: its cells' span against the strip's middle, unless
  # the cells and minimums alone already need more than half the strip on
  # one side (labelBudgets' own test, with the labels at their budgets).
  verdict=$("$jq_bin" -r '
    .[0] as $b
    | ($b.edge == "left" or $b.edge == "right") as $v
    | [$b.cells[] | select(.region == "center")] as $c
    | [$b.cells[] | select(.region == "left")] as $l
    | [$b.cells[] | select(.region == "right")] as $r
    | def lo($x): if $v then $x.y else $x.x end;
      def hi($x): if $v then $x.y + $x.height else $x.x + $x.width end;
    (($c | map(lo(.)) | min) as $s | ($c | map(hi(.)) | max) as $e
     | (($s + $e) / 2 - $b.along / 2) as $off
     | (($l | map(hi(.)) | max) - $b.edgeInset) as $left
     | ($b.along - $b.edgeInset - ($r | map(lo(.)) | min)) as $right
     | ($b.along - 2 * $b.edgeInset - 2 * $b.gap) as $inner
     | if ($off | fabs) <= 2 then "ok \($off)"
       elif (($e - $s) + 2 * ([$left, $right] | max) > $inner + 1) then "uncentrable: the centre \($e - $s) plus twice the wider end \([$left, $right] | max) needs more than \($inner)"
       else "off by \($off): centre \($s)..\($e) on \($b.along)" end)' "$bar_title_json_path")
  case "$verdict" in
    "ok "*) echo "centre centred, off the middle by ${verdict#ok }px" ;;
    uncentrable*) echo "centre not held on the middle, $verdict" ;;
    *) fail "the centre sits off the $edge strip's middle: $verdict" ;;
  esac
  local sample count=0 bad=0
  for sample in "$bar_title_samples_dir"/*.json; do
    [ -s "$sample" ] || continue
    count=$((count + 1))
    defects=$(bar_cells_defects "$sample")
    if [ -n "$defects" ]; then
      echo "mid-switch $(basename "$sample"): $defects"
      bad=$((bad + 1))
      "$jq_bin" -c '.[0] | {slack, cells: [.cells[] | [.name, .x, .y, .width, .height]]}' "$sample"
    fi
  done
  [ "$bad" -eq 0 ] || fail "cells on the $edge strip ran into each other in $bad of $count samples while the title changed"
  [ "$count" -ge 12 ] || fail "only $count mid-switch bar room samples came back"
  echo "mid-switch samples clear: $count"
  # The title cell alone: its ceiling, however much room is spare.
  if [ ! -s "$bar_title_alone_path" ]; then
    fail "no bar room output for the title cell alone"
  fi
  "$jq_bin" -c '.[0] | {slack, activeWindow, cells: [.cells[] | [.name, .x, .y, .width, .height]]}' "$bar_title_alone_path"
  verdict=$("$jq_bin" -r '
    .[0] as $b | $b.activeWindow as $w
    | ([$b.cells[] | select(.name == "activeWindow")] | first) as $t
    | (if ($b.edge == "left" or $b.edge == "right") then $t.height else $t.width end) as $len
    | if ($b.cells | length) != 1 then "\($b.cells | length) cells on a strip laid out with one"
      elif ($w.cap <= 0 or $w.budget < $w.cap - 1 or $w.budget > $w.cap + 1) then "budget \($w.budget) is not the ceiling \($w.cap)"
      elif ($w.extent < $w.cap - 1 or $w.extent > $w.cap + 1) then "the title draws \($w.extent), not its ceiling \($w.cap)"
      elif ($w.natural <= $w.cap) then "the title \($w.natural) never outgrew its ceiling \($w.cap)"
      elif ($w.scrolling | not) then "the marquee is not running"
      elif ($b.slack < 100) then "only \($b.slack) slack, not a strip with room to spare"
      else "ok ceiling \($w.cap), cell \($len) long, slack \($b.slack)" end' "$bar_title_alone_path")
  case "$verdict" in
    "ok "*) echo "title alone at its ${verdict#ok }" ;;
    *) fail "the title alone on the $edge strip does not stop at its ceiling: $verdict" ;;
  esac
  for f in "$bar_title_png_path" "$bar_title_later_png_path" "$bar_title_alone_png_path"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done
  echo "SMOKE_BAR_TITLE $bar_title_png_path"
  echo "SMOKE_BAR_TITLE_LATER $bar_title_later_png_path"
  echo "SMOKE_BAR_TITLE_ALONE $bar_title_alone_png_path"
}
