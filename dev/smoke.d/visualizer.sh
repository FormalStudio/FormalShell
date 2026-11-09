# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --visualizer plays a real tone through a real MPRIS player so cava has an
# actual spectrum to bin, puts the widget where the owner asked for it (next
# to nowPlaying, leaving left and right on their own DEFAULT_LAYOUT fallback),
# and proves the process gate from outside the shell: VisualizerService owns a
# real `cava` child, so pgrep is the verification, not a screenshot. The
# second half is the half that matters, DESIGN.md section 4 rule 8: once
# playback stops the process has to be GONE, not paused-looking.
#
# The fixture is a sine tone rather than --media's silent track, which would
# leave cava with no signal and the widget stuck on its baseline row all run.
leg_visualizer_flag="--visualizer"
leg_visualizer_order=220
leg_visualizer_needs="mpv ffmpeg convert"
leg_visualizer_claims="media.player=shared bar.layout.center"

visualizer_pid_path="$shot_dir/visualizer-mpv.pid"
visualizer_playing_path="$shot_dir/visualizer-playing.png"
visualizer_pgrep_playing_path="$shot_dir/visualizer-pgrep-playing.txt"
visualizer_pgrep_after_path="$shot_dir/visualizer-pgrep-after.txt"
visualizer_track_path="$shot_dir/smoke-tone.flac"
visualizer_art_path="$shot_dir/smoke-art.png"

leg_visualizer_fixture() {
  # Cover art on the fixture so the NowPlaying mini cover in the same frame
  # renders its real dithered art beside the bars, not the no-art fallback.
  $convert_bin -size 64x64 "xc:#E03131" -size 64x64 "xc:#F08C00" -size 64x64 "xc:#FFD700" \
    -size 64x64 "xc:#2F9E44" -size 64x64 "xc:#1971C2" -size 64x64 "xc:#9C36B5" \
    +append -resize "64x64!" "$visualizer_art_path"
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=20" -i "$visualizer_art_path" \
    -map 0:a -map 1:0 -ac 2 -t 20 \
    -metadata "title=FormalShell Visualizer Smoke Tone" -metadata "artist=FormalShell Test Artist" \
    -c:a flac -c:v png -disposition:v attached_pic -y "$visualizer_track_path"
  settings_pin '.bar.layout.center = ["clock", "nowPlaying", "visualizer"]'
}

leg_visualizer_timing() {
  # visualizer-drive.sh's worst case (2s before mpv, a 10s cava-appears poll,
  # a 2s settle, the frame, the kill, a 6s cava-gone poll) lands ~20s in; the
  # run's own frame is taken past that, showing the widget hidden again with
  # no player, the same way nowPlaying goes.
  leg_timing 50
}

leg_visualizer_drive() {
  local script="$shot_dir/visualizer-drive.sh"
  local kill_script="$shot_dir/visualizer-kill.sh"
  # mpv in the background rather than exec'd: this script keeps running after
  # it, to poll for cava, capture, then kill and poll again. The first poll
  # waits out VisualizerService's whole bootstrap (mkdir, config write, PATH
  # probe) plus MediaService.isPlaying flipping true.
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$mpv_bin" --no-video --really-quiet "$visualizer_track_path" &
mpv_pid=\$!
echo "\$mpv_pid" > "$visualizer_pid_path"
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  pgrep -f -- "cava -p" > "$visualizer_pgrep_playing_path" 2>&1 || true
  [ -s "$visualizer_pgrep_playing_path" ] && break
  sleep 1
done
sleep 2
"$grim_bin" "$visualizer_playing_path" > /dev/null 2>&1
kill "\$mpv_pid" 2>/dev/null || true
wait "\$mpv_pid" 2>/dev/null || true
SECONDS=0
: > "$visualizer_pgrep_after_path"
while [ "\$SECONDS" -lt 6 ]; do
  pgrep -f -- "cava -p" > "$visualizer_pgrep_after_path" 2>&1 || true
  [ ! -s "$visualizer_pgrep_after_path" ] && break
  sleep 1
done
EOF

  # Safety net only: the drive script above already kills mpv well before the
  # run's own frame, so this is the same "harmless if already dead" shape the
  # tray stubs' kill script has.
  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
if [ -f "$visualizer_pid_path" ]; then
  kill "\$(cat "$visualizer_pid_path")" 2>/dev/null || true
fi
EOF
  add_cleanup "bash $kill_script"
  hypr_exec_once "bash $script"
}

leg_visualizer_assert() {
  if [ ! -s "$visualizer_pgrep_playing_path" ]; then
    fail "cava never appeared while the visualizer smoke tone was playing, pgrep output: $(cat "$visualizer_pgrep_playing_path" 2>/dev/null)"
  fi
  cat "$visualizer_pgrep_playing_path"
  # An empty (or absent) pgrep file after the post-kill poll is the honest
  # positive result here.
  if [ -s "$visualizer_pgrep_after_path" ]; then
    fail "cava is still running after mpv was killed, pgrep output: $(cat "$visualizer_pgrep_after_path")"
  fi
  echo "SMOKE_VISUALIZER_CAVA_KILLED_AFTER_PAUSE ok"
  if [ ! -f "$visualizer_playing_path" ]; then
    fail "no visualizer-playing screenshot produced"
  fi
  echo "SMOKE_VISUALIZER_PLAYING $visualizer_playing_path"
}
