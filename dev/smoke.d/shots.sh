# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2086  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail(); $convert_bin expands unquoted
# --shots rides --showcase and writes the README's and the website's images
# into docs/shots/ (scripts/shots runs it once per theme). Alone it takes
# every one of them under the default preset; under --pantheon or --retro it
# takes that preset's theme frame and nothing else, since the preset is read
# at startup and needs a session of its own.
#
# Demo data only: a pink noise track tagged as a demo, notifications from
# made-up apps, and USER=kyan in the shell's own environment, which is where
# the lock screen's name comes from. The launcher and panel clips are grim
# bursts at a third of the session's speed (`debug motionScale 300`); the
# screensaver is frame-stepped the way
# --screensaver-gif steps it, since a ttfx run on llvmpipe is too slow to
# film in real time. A camera, not a test: it fails only when a file is
# missing.
leg_shots_flag="--shots"
leg_shots_order=235
leg_shots_needs="wtype convert mpv ffmpeg notify-send"
leg_shots_fixture_window=none

shots_dir="$shot_dir/shots"
shots_done="$shot_dir/shots-done"
shots_track="$shot_dir/shots-demo.flac"
shots_title="Midnight Drive"
shots_artist="Demo Artist"

shots_theme() {
  if leg_on pantheon; then echo pantheon
  elif leg_on retro; then echo retro
  else echo metamorphosis
  fi
}

leg_shots_validate() {
  if ! leg_on showcase; then
    echo "usage: $0 --showcase [--pantheon|--retro] --shots" >&2
    exit 1
  fi
}

leg_shots_fixture() {
  shell_env="${shell_env:+$shell_env }USER=kyan"
  settings_fragment ', "screensaver": {"effect": "decrypt"}'
  # Pink noise, as --visualizer-styles plays it: every band moves.
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi \
    -i "anoisesrc=color=pink:amplitude=0.5:sample_rate=48000:duration=400" \
    -ac 2 -metadata "title=$shots_title" -metadata "artist=$shots_artist" \
    -c:a flac -y "$shots_track"
}

leg_shots_timing() {
  if [ "$(shots_theme)" = metamorphosis ]; then
    leg_timing 420 500
  else
    leg_timing 30 70
  fi
}

leg_shots_drive() {
  local script="$shot_dir/shots-drive.sh" kill_script="$shot_dir/shots-kill.sh" theme
  theme=$(shots_theme)
  mkdir -p "$shots_dir"
  write_script "$script" <<EOF
#!/usr/bin/env bash
start=\$(date +%s%3N)
ms() { echo \$(( \$(date +%s%3N) - start )); }
shot() { "$grim_bin" "$shots_dir/\$1.png" >> "$shot_dir/grim.log" 2>&1; }
# grim into \$1's directory for \$2 ms, each frame named by its stamp.
burst() {
  local dir="$shots_dir/\$1" until=\$(( \$(ms) + \$2 ))
  mkdir -p "\$dir"
  while [ "\$(ms)" -lt "\$until" ]; do
    "$grim_bin" "\$dir/\$(printf '%06d' "\$(ms)").png" >> "$shot_dir/grim.log" 2>&1
  done
}
slow() { $ipc call debug motionScale "\$1" > /dev/null 2>&1; }
sleep 4
"$mpv_bin" --no-video --really-quiet --loop "$shots_track" &
echo \$! > "$shot_dir/shots-mpv.pid"
for _ in \$(seq 10); do
  $ipc call media status 2>/dev/null | grep -qF '"title":"$shots_title"' && break
  sleep 1
done
sleep 2

$ipc call menu toggle > /dev/null 2>&1
sleep 3
shot theme-$theme
$ipc call menu toggle > /dev/null 2>&1
sleep 2
if [ "$theme" != metamorphosis ]; then
  touch "$shots_done"
  exit 0
fi

slow 300
$ipc call menu toggle > /dev/null 2>&1 &
burst launcher 9000 &
burst_pid=\$!
sleep 4.5
for k in Down Down Down; do "$wtype_bin" -k \$k; sleep 1.2; done
# Not a bare wait: mpv is a child too, looping until it is killed.
wait "\$burst_pid"
$ipc call menu toggle > /dev/null 2>&1
slow 100
sleep 2

slow 300
$ipc call panel open calendar > /dev/null 2>&1 &
burst panel 6000
slow 100
sleep 1
shot panel-poster
$ipc call panel close > /dev/null 2>&1
sleep 2

shot media-closed
$ipc call panel open media > /dev/null 2>&1
sleep 3
shot media
for id in bars led scope retro firework redsector; do
  $ipc call visualizer style \$id > /dev/null 2>&1
  sleep 1.5
  shot vis-\$id
done
$ipc call visualizer style bars > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
sleep 2

"$notify_send_bin" -a Calendar "Design review" "Starts in 10 minutes, room 2"
sleep 0.5
"$notify_send_bin" -a Downloads "Download finished" "wallpapers-autumn.zip, 48 MB"
sleep 0.5
"$notify_send_bin" -a Terminal "Build finished" "All 214 tests passed"
sleep 0.5
"$notify_send_bin" -a Battery "Battery low" "10% left, plug in soon"
sleep 2
$ipc call notifications expand on > /dev/null 2>&1
sleep 2
shot notifications
$ipc call notifications expand off > /dev/null 2>&1
# Past the popups' 8s timeout, so the centre lists them as pending.
sleep 12
$ipc call notifications showHistory > /dev/null 2>&1
sleep 2
shot center
$ipc call notifications showHistory > /dev/null 2>&1
sleep 2
kill "\$(cat "$shot_dir/shots-mpv.pid")" 2>/dev/null
sleep 1

$ipc call lock lock > /dev/null 2>&1
sleep 4
shot lock
"$wtype_bin" "formalshell-test"
"$wtype_bin" -k Return
sleep 4

$ipc call screensaver start > /dev/null 2>&1
sleep 1
$ipc call screensaver frame 0 > /dev/null 2>&1
sleep 2
convergence=\$($ipc call screensaver frameInfo 2>/dev/null | grep -o '"convergenceFrame":[0-9]*' | cut -d: -f2)
stride=\$(( (\${convergence:-0} + 89) / 90 ))
[ "\$stride" -lt 1 ] && stride=1
echo "\$stride" > "$shots_dir/screensaver-stride"
mkdir -p "$shots_dir/screensaver"
idx=0
for ((i = 0; i < \${convergence:-0}; i += stride)); do
  $ipc call screensaver frame "\$i" > /dev/null 2>&1
  sleep 0.4
  "$grim_bin" "$shots_dir/screensaver/\$(printf '%04d' "\$idx").png" >> "$shot_dir/grim.log" 2>&1
  idx=\$((idx + 1))
done
$ipc call screensaver stop > /dev/null 2>&1
touch "$shots_done"
EOF
  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
[ -f "$shot_dir/shots-mpv.pid" ] && kill "\$(cat "$shot_dir/shots-mpv.pid")" 2>/dev/null
true
EOF
  add_cleanup "bash $kill_script"
  hypr_exec_once "bash $script"
}

# A concat list of stamped frames to H.264. concat drops the last entry's
# duration, so the closing hold is a tpad clone of the final frame instead.
_shots_encode() {
  "$ffmpeg_bin" -nostdin -loglevel error -f concat -safe 0 -i "$1" \
    -vf "fps=30,tpad=stop_mode=clone:stop_duration=$2,scale=1280:-2:flags=lanczos,format=yuv420p" \
    -c:v libx264 -preset slow -crf 26 -movflags +faststart -an -y "$3"
}

# One clip out of a burst: each frame held until the next one's stamp,
# divided back to real time by \$3 (the motion scale), the last held 1.5s.
_shots_clip() {
  local dir="$1" out="$2" scale="$3" list="$1/frames.txt" frames=() i stamp next
  mapfile -t frames < <(find "$dir" -maxdepth 1 -name '[0-9]*.png' | sort)
  [ "${#frames[@]}" -ge 2 ] || return 1
  : > "$list"
  for ((i = 0; i < ${#frames[@]}; i++)); do
    stamp=$((10#$(basename "${frames[$i]}" .png)))
    printf "file '%s'\n" "${frames[$i]}" >> "$list"
    if [ $((i + 1)) -lt ${#frames[@]} ]; then
      next=$((10#$(basename "${frames[$((i + 1))]}" .png)))
      printf "duration %s\n" "$(awk -v d=$((next - stamp)) -v s="$scale" 'BEGIN { printf "%.3f", d / 1000 * 100 / s }')" >> "$list"
    fi
  done
  _shots_encode "$list" 1.5 "$out"
}

leg_shots_assert() {
  [ -f "$shots_done" ] || fail "the shots drive never finished"
  local out="$PWD/docs/shots" theme f made=0 want ids=() last stride delay
  theme=$(shots_theme)
  mkdir -p "$out"
  cp "$shots_dir/theme-$theme.png" "$out/" 2>/dev/null && made=$((made + 1))
  if [ "$theme" != metamorphosis ]; then
    [ "$made" -eq 1 ] || fail "no theme-$theme.png"
    echo "SMOKE_SHOTS bytes=$(stat -c %s "$out/theme-$theme.png") theme-$theme.png"
    return
  fi
  for f in panel-poster media notifications center lock; do
    cp "$shots_dir/$f.png" "$out/" 2>/dev/null && made=$((made + 1))
  done

  # The media card alone, found as what opening it changed below the bar,
  # then six styles in a 3x2 grid on the wallpaper's own ground.
  local rect
  rect=$($convert_bin "$shots_dir/media-closed.png" "$shots_dir/media.png" -compose difference -composite \
    -crop "1920x1020+0+60" +repage -colorspace Gray -threshold 8% -format '%@' info: 2>/dev/null)
  if [ -n "$rect" ]; then
    IFS='x+' read -r w h x y <<< "$rect"
    rect="$((w + 24))x$((h + 24))+$((x - 12))+$((y + 60 - 12))"
    for f in bars led scope retro firework redsector; do
      $convert_bin "$shots_dir/vis-$f.png" -crop "$rect" +repage "$shots_dir/card-$f.png"
      ids+=("$shots_dir/card-$f.png")
    done
    $convert_bin \( "${ids[0]}" "${ids[1]}" "${ids[2]}" -bordercolor none -border 16 +append \) \
      \( "${ids[3]}" "${ids[4]}" "${ids[5]}" -bordercolor none -border 16 +append \) -append \
      -resize '1840x1000>' -background '#101428' -gravity center -extent 1920x1080 "$out/visualizers.png" \
      && made=$((made + 1))
  fi

  _shots_clip "$shots_dir/launcher" "$out/launcher.mp4" 300 && made=$((made + 1))
  last=$(find "$shots_dir/launcher" -maxdepth 1 -name '[0-9]*.png' | sort | tail -1)
  cp "$last" "$out/launcher-poster.png" && made=$((made + 1))
  _shots_clip "$shots_dir/panel" "$out/panel.mp4" 300 && made=$((made + 1))

  # Frame-stepped: one captured frame stands for `stride` of ttfx's 60fps,
  # and the converged banner holds for two seconds.
  stride=$(cat "$shots_dir/screensaver-stride" 2>/dev/null || echo 1)
  delay=$(awk -v s="$stride" 'BEGIN { printf "%.4f", s / 60 }')
  last=$(find "$shots_dir/screensaver" -maxdepth 1 -name '*.png' | sort | tail -1)
  if [ -n "$last" ]; then
    {
      find "$shots_dir/screensaver" -maxdepth 1 -name '*.png' | sort | while read -r f; do
        printf "file '%s'\nduration %s\n" "$f" "$delay"
      done
    } > "$shots_dir/screensaver/frames.txt"
    _shots_encode "$shots_dir/screensaver/frames.txt" 2 "$out/screensaver.mp4" && made=$((made + 1))
    cp "$last" "$out/screensaver-poster.png" && made=$((made + 1))
  fi

  for f in "$out"/*; do
    echo "SMOKE_SHOTS bytes=$(stat -c %s "$f") $(basename "$f")"
  done
  tar -cf "$shot_dir/shots-frames.tar" -C "$shots_dir" . && echo "SMOKE_SHOTS_FRAMES $shot_dir/shots-frames.tar"
  want=12
  [ "$made" -eq "$want" ] || fail "wrote $made of $want shots"
}
