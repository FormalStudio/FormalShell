# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --media plays a real MPRIS player in-session (mpv carrying its own
# mpris.lua, a generated fixture track tagged with a title/artist and a real
# embedded cover) and reads the panel back off it rather than off a stub.
# `media status` is cross-checked against the fixture's own tags; the marquee
# gets both its states (short title parked, long title scrolling); shuffle,
# LoopStatus and Volume are set over IPC and read straight back out of mpv,
# which is the player answering rather than the shell repeating itself; and a
# SECOND mpv joins the bus so the PLAYERS switcher exists at all (it is
# hidden by design with one player), with `media select` handed the id the
# pick did NOT choose.
#
# MEDIA_NO_ART=1 mutes the embedded cover for one run, reproducing the
# artless MPRIS export browsers publish: the panel must then collapse the art
# slot out of its row rather than reserve a blank square.
leg_media_flag="--media"
leg_media_order=170
leg_media_needs="mpv ffmpeg convert"

# Neither binary is one the scaffold resolves for itself. Same shape as its
# own need_* helpers: set once, absolute path, since every one of these is
# embedded in a generated script.
need_mpv() {
  need_tool mpv_bin mpv mpv
  mpv_mpris=/usr/lib64/mpv/mpris.so
  [ -f "$mpv_mpris" ] || { echo "SMOKE_FAIL: no MPRIS script for mpv at $mpv_mpris; install the openSUSE package mpv-mpris" >&2; exit 1; }
}

need_ffmpeg() { need_tool ffmpeg_bin ffmpeg ffmpeg; }

media_status_path="$shot_dir/media-status.json"
media_status_long_path="$shot_dir/media-status-long.json"
media_marquee_static_path="$shot_dir/media-marquee-static.png"
media_marquee_scroll_path="$shot_dir/media-marquee-scroll.png"
media_controls_status_path="$shot_dir/media-controls-status.json"
media_controls_png_path="$shot_dir/media-controls.png"
media_players_path="$shot_dir/media-players.json"
media_select_status_path="$shot_dir/media-select-status.json"
media_players_png_path="$shot_dir/media-players.png"
media_pid_path="$shot_dir/mpv.pid"
media_pid2_path="$shot_dir/mpv-2.pid"
media_art_path="$shot_dir/smoke-art.png"
media_track_path="$shot_dir/smoke-track.flac"
media_track_long_path="$shot_dir/smoke-track-long.flac"
media_track_title="FormalShell Smoke Track"
media_track_title_long="FormalShell Marquee Autoscroll Verification Overflow Track"
media_track_artist="FormalShell Test Artist"

leg_media_timing() {
  # media-marquee.sh's worst case (7s static shot, kill/respawn, up to 8s
  # retitle poll, 5s into the scroll) lands ~21s in, and media-controls.sh's
  # last step (second player at 21, its select and status at 25-27) ~28s in.
  leg_timing 32 55
}

leg_media_fixture() {
  local art_args=()
  # Six saturated stripes, not one flat colour: the panel's art slot has to
  # show real hue variety survived whatever it does to the image.
  if [ "${MEDIA_NO_ART:-}" != "1" ]; then
    $convert_bin -size 64x64 "xc:#E03131" -size 64x64 "xc:#F08C00" -size 64x64 "xc:#FFD700" \
      -size 64x64 "xc:#2F9E44" -size 64x64 "xc:#1971C2" -size 64x64 "xc:#9C36B5" \
      +append -resize "64x64!" "$media_art_path"
    # A real FLAC PICTURE block, so mpv's mpris.lua extracts a genuine
    # mpris:artUrl through avformat's own attached_pic path.
    art_args=(-i "$media_art_path" -map 0:a -map 1:0 -c:v png -disposition:v attached_pic)
  fi
  # Silent by design: nothing in this leg listens, and a tone would only give
  # the visualizer something to read.
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" \
    ${art_args[@]+"${art_args[@]}"} -t 20 \
    -metadata "title=$media_track_title" -metadata "artist=$media_track_artist" \
    -c:a flac -y "$media_track_path"
  # Deliberately past NowPlaying's maxWidth at the shell's body size, so the
  # marquee's overflow gate actually trips.
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" \
    ${art_args[@]+"${art_args[@]}"} -t 20 \
    -metadata "title=$media_track_title_long" -metadata "artist=$media_track_artist" \
    -c:a flac -y "$media_track_long_path"
}

leg_media_drive() {
  local play_script="$shot_dir/media-play.sh"
  local open_script="$shot_dir/media-open.sh"
  local marquee_script="$shot_dir/media-marquee.sh"
  local controls_script="$shot_dir/media-controls.sh"
  local kill_script="$shot_dir/media-kill.sh"

  # The pid is written before the exec: exec replaces this shell's process
  # image without forking, so $$ recorded here is mpv's eventual pid exactly.
  # `pkill -f <fixture path>` was tried first and self-matched, killing its
  # own invoking shell.
  write_script "$play_script" <<EOF
#!/usr/bin/env bash
sleep 2
echo \$\$ > "$media_pid_path"
exec "$mpv_bin" --no-video --really-quiet "$media_track_path"
EOF

  write_script "$open_script" <<EOF
#!/usr/bin/env bash
sleep 5
$ipc call panel open media > /dev/null 2>&1
sleep 1
$ipc call media status > "$media_status_path" 2>&1
EOF

  # The marquee's two states off one timeline: photograph the short title
  # first (the non-overflowing baseline), then kill and respawn on the long
  # one, poll `media status` for the retitle to actually land, and only then
  # wait past the marquee's hold and well into its scroll.
  write_script "$marquee_script" <<EOF
#!/usr/bin/env bash
sleep 7
"$grim_bin" "$media_marquee_static_path" > /dev/null 2>&1
if [ -f "$media_pid_path" ]; then
  kill "\$(cat "$media_pid_path")" 2>/dev/null || true
fi
sleep 1
"$mpv_bin" --no-video --really-quiet "$media_track_long_path" &
echo \$! > "$media_pid_path"
SECONDS=0
while [ "\$SECONDS" -lt 8 ]; do
  $ipc call media status > "$media_status_long_path" 2>&1
  grep -qF "\"title\":\"$media_track_title_long\"" "$media_status_long_path" && break
  sleep 1
done
sleep 5
"$grim_bin" "$media_marquee_scroll_path" > /dev/null 2>&1
EOF

  # Starts at 16, after the marquee's own kill/respawn has settled on the
  # long-title track, so these act on the player still there at the end.
  # `select` is handed the id that is NOT the one status just reported
  # active, read out of the two-entry players dump rather than assumed from
  # mpv's bus naming.
  write_script "$controls_script" <<EOF
#!/usr/bin/env bash
sleep 16
$ipc call media shuffle on > /dev/null 2>&1
$ipc call media loop track > /dev/null 2>&1
$ipc call media volume 30 > /dev/null 2>&1
sleep 1
$ipc call media status > "$media_controls_status_path" 2>&1
"$grim_bin" "$media_controls_png_path" > /dev/null 2>&1
sleep 4
"$mpv_bin" --no-video --really-quiet "$media_track_path" &
echo \$! > "$media_pid2_path"
sleep 4
$ipc call media players > "$media_players_path" 2>&1
active=\$(grep -o '"id":"[^"]*"' "$media_controls_status_path" | head -1 | cut -d'"' -f4)
other=\$(grep -o '"id":"[^"]*"' "$media_players_path" | cut -d'"' -f4 | grep -v "^\$active\$" | head -1)
$ipc call media select "\$other" > /dev/null 2>&1
sleep 2
$ipc call media status > "$media_select_status_path" 2>&1
"$grim_bin" "$media_players_png_path" > /dev/null 2>&1
EOF

  # Neither mpv has an auto-close of its own and both would outlive the run.
  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
for pidfile in "$media_pid_path" "$media_pid2_path"; do
  [ -f "\$pidfile" ] && kill "\$(cat "\$pidfile")" 2>/dev/null
done
true
EOF
  add_cleanup "bash $kill_script"

  hypr_exec_once "bash $play_script"
  hypr_exec_once "bash $open_script"
  hypr_exec_once "bash $marquee_script"
  hypr_exec_once "bash $controls_script"
}

leg_media_assert() {
  local expect volume active_before active_after selected_after
  if [ ! -s "$media_status_path" ]; then
    fail "no media status produced"
  fi
  cat "$media_status_path"; echo
  if ! grep -q '"available":true' "$media_status_path"; then
    fail "media status reports no player available, got: $(cat "$media_status_path")"
  fi
  if ! grep -qF "\"title\":\"$media_track_title\"" "$media_status_path"; then
    fail "media status title does not match the fixture track's tag ($media_track_title), got: $(cat "$media_status_path")"
  fi
  if ! grep -qF "\"artist\":\"$media_track_artist\"" "$media_status_path"; then
    fail "media status artist does not match the fixture track's tag ($media_track_artist), got: $(cat "$media_status_path")"
  fi
  [ -f "$media_marquee_static_path" ] || fail "no media-marquee-static screenshot produced"
  echo "SMOKE_MEDIA_MARQUEE_STATIC $media_marquee_static_path"
  # The scroll frame is only evidence of a marquee if the retitle landed
  # before it: without this it could be a stale static frame.
  if [ ! -s "$media_status_long_path" ]; then
    fail "no media status produced after the marquee retitle"
  fi
  if ! grep -qF "\"title\":\"$media_track_title_long\"" "$media_status_long_path"; then
    fail "media status after retitle does not show the long overflow title, got: $(cat "$media_status_long_path")"
  fi
  [ -f "$media_marquee_scroll_path" ] || fail "no media-marquee-scroll screenshot produced"
  echo "SMOKE_MEDIA_MARQUEE_SCROLL $media_marquee_scroll_path"

  if [ ! -s "$media_controls_status_path" ]; then
    fail "no media status produced after the controls leg"
  fi
  cat "$media_controls_status_path"; echo
  for expect in '"shuffleSupported":true' '"shuffle":true' '"loopSupported":true' '"loop":"track"' '"volumeSupported":true'; do
    if ! grep -qF "$expect" "$media_controls_status_path"; then
      fail "media status does not report $expect after the controls leg, got: $(cat "$media_controls_status_path")"
    fi
  done
  # 0.3 on the wire is 30 in mpv's own units and back; the window is for that
  # round trip's float, not a tolerance on what was asked for.
  volume=$(grep -o '"volume":[0-9.eE+-]*' "$media_controls_status_path" | head -1 | cut -d: -f2)
  if ! awk -v v="$volume" 'BEGIN { exit !(v > 0.28 && v < 0.32) }'; then
    fail "media volume 30 did not reach the player, status reports volume=$volume"
  fi
  [ -f "$media_controls_png_path" ] || fail "no media-controls screenshot produced"
  echo "SMOKE_MEDIA_CONTROLS $media_controls_png_path"

  if [ ! -s "$media_players_path" ]; then
    fail "no media players list produced"
  fi
  cat "$media_players_path"; echo
  if [ ! -s "$media_select_status_path" ]; then
    fail "no media status produced after select"
  fi
  cat "$media_select_status_path"; echo
  if ! grep -q '"playerCount":2' "$media_select_status_path"; then
    fail "second mpv never registered on MPRIS, got: $(cat "$media_select_status_path")"
  fi
  active_before=$(grep -o '"id":"[^"]*"' "$media_controls_status_path" | head -1 | cut -d'"' -f4)
  active_after=$(grep -o '"id":"[^"]*"' "$media_select_status_path" | head -1 | cut -d'"' -f4)
  selected_after=$(grep -o '"selectedId":"[^"]*"' "$media_select_status_path" | head -1 | cut -d'"' -f4)
  if [ -z "$active_after" ] || [ "$active_after" != "$selected_after" ] || [ "$active_after" = "$active_before" ]; then
    fail "media select did not move the active player (before=$active_before after=$active_after selected=$selected_after)"
  fi
  [ -f "$media_players_png_path" ] || fail "no media-players screenshot produced"
  echo "SMOKE_MEDIA_PLAYERS $media_players_png_path"
}
