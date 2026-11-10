# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --spectrum: the media panel's own spectrum band, with
# no `visualizer` cell anywhere in bar.layout, so the process gate's other
# consumer (`root._visibleBars > 0`) is off and only `panelWants` can start
# cava. A real tone through a real MPRIS player, visualizer.sh's own fixture,
# so cava has an actual spectrum to bin rather than silence.
#
# pgrep is the verification, the same discipline visualizer.sh's own header
# lays out: no cava child while the panel is shut even with the track
# playing, one the moment `panel open media` lands, none again the moment
# `panel close` does. A screenshot is still taken while the panel is open,
# read by hand for the band's fills under the now-playing block.
leg_spectrum_flag="--spectrum"
leg_spectrum_order=225
leg_spectrum_needs="mpv ffmpeg convert"
leg_spectrum_claims="media.player=shared bar.layout.center"

spectrum_track_path="$shot_dir/spectrum-tone.flac"
spectrum_art_path="$shot_dir/spectrum-art.png"
spectrum_pid_path="$shot_dir/spectrum-mpv.pid"
spectrum_status_path="$shot_dir/spectrum-status.json"
spectrum_pgrep_before_path="$shot_dir/spectrum-pgrep-before.txt"
spectrum_pgrep_open_path="$shot_dir/spectrum-pgrep-open.txt"
spectrum_pgrep_closed_path="$shot_dir/spectrum-pgrep-closed.txt"
spectrum_open_png_path="$shot_dir/spectrum-open.png"
spectrum_track_title="FormalShell Spectrum Smoke Tone"
spectrum_track_artist="FormalShell Test Artist"

leg_spectrum_fixture() {
  $convert_bin -size 64x64 "xc:#E03131" -size 64x64 "xc:#F08C00" -size 64x64 "xc:#FFD700" \
    -size 64x64 "xc:#2F9E44" -size 64x64 "xc:#1971C2" -size 64x64 "xc:#9C36B5" \
    +append -resize "64x64!" "$spectrum_art_path"
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=25" -i "$spectrum_art_path" \
    -map 0:a -map 1:0 -ac 2 -t 25 \
    -metadata "title=$spectrum_track_title" -metadata "artist=$spectrum_track_artist" \
    -c:a flac -c:v png -disposition:v attached_pic -y "$spectrum_track_path"
  # No `visualizer` anywhere in bar.layout: the bar cell's own
  # `_visibleBars` gate stays 0 all run, so a cava child appearing at all is
  # `panelWants` alone doing it.
  settings_pin '.bar.layout.center = ["clock", "nowPlaying"]'
}

leg_spectrum_timing() {
  # A tag-match poll (up to 8s), the two pgrep checks either side of
  # panel open/close with a 2s settle each: worst case lands ~22s in.
  leg_timing 50
}

leg_spectrum_drive() {
  local script="$shot_dir/spectrum-drive.sh"
  local kill_script="$shot_dir/spectrum-kill.sh"

  write_script "$script" <<EOF
#!/usr/bin/env bash
"$mpv_bin" --no-video --really-quiet "$spectrum_track_path" &
mpv_pid=\$!
echo "\$mpv_pid" > "$spectrum_pid_path"
SECONDS=0
while [ "\$SECONDS" -lt 8 ]; do
  $ipc call media status > "$spectrum_status_path" 2>&1
  grep -qF "\"title\":\"$spectrum_track_title\"" "$spectrum_status_path" && break
  sleep 1
done
pgrep -f -- "cava -p" > "$spectrum_pgrep_before_path" 2>&1 || true
$ipc call panel open media > /dev/null 2>&1
for _ in \$(seq 1 20); do pgrep -f -- "cava -p" > /dev/null && break; sleep 0.25; done
pgrep -f -- "cava -p" > "$spectrum_pgrep_open_path" 2>&1 || true
"$grim_bin" "$spectrum_open_png_path" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
sleep 2
pgrep -f -- "cava -p" > "$spectrum_pgrep_closed_path" 2>&1 || true
kill "\$mpv_pid" 2>/dev/null || true
wait "\$mpv_pid" 2>/dev/null || true
EOF

  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
if [ -f "$spectrum_pid_path" ]; then
  kill "\$(cat "$spectrum_pid_path")" 2>/dev/null || true
fi
EOF
  add_cleanup "bash $kill_script"
  hypr_exec_once "bash $script"
}

leg_spectrum_assert() {
  if [ ! -s "$spectrum_status_path" ] || ! grep -qF "\"title\":\"$spectrum_track_title\"" "$spectrum_status_path"; then
    fail "media status never showed the spectrum smoke tone, got: $(cat "$spectrum_status_path" 2>/dev/null)"
  fi
  if [ -s "$spectrum_pgrep_before_path" ]; then
    fail "cava is already running before the media panel opened (no visualizer cell in bar.layout, so panelWants alone should gate it), pgrep output: $(cat "$spectrum_pgrep_before_path")"
  fi
  echo "SMOKE_SPECTRUM_CAVA_BEFORE_OPEN none"
  if [ ! -s "$spectrum_pgrep_open_path" ]; then
    fail "cava never appeared once the media panel opened with a track playing, pgrep output: $(cat "$spectrum_pgrep_open_path" 2>/dev/null)"
  fi
  cat "$spectrum_pgrep_open_path"
  if [ ! -f "$spectrum_open_png_path" ]; then
    fail "no spectrum-open screenshot produced"
  fi
  echo "SMOKE_SPECTRUM_OPEN $spectrum_open_png_path"
  if [ -s "$spectrum_pgrep_closed_path" ]; then
    fail "cava is still running after the media panel closed, pgrep output: $(cat "$spectrum_pgrep_closed_path")"
  fi
  echo "SMOKE_SPECTRUM_CAVA_AFTER_CLOSE none"
}
