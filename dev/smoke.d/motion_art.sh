# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --motion-art proves the animated album cover with no
# network: `media.appleMusicArt` on, a track tagged with an artist and album
# whose motion art is already in the cache the lookup would have filled
# (`$XDG_CACHE_HOME/formalshell/applemusic-art/<key>.mp4`), so the decode is
# the only thing under test. That mp4 alternates flat red and flat blue every
# half second; the track's own static art is flat green, so which picture a
# cover slot is drawing is read off three colour counts. The bar's cover is
# read off a crop around the now-playing cell, placed by `bar room`.
#
#   bar      with the panel shut, crops of the bar's cover carry red in some
#            and blue in others, one ffmpeg child decodes, and the bar commits
#            while it moves.
#   playing  frames taken while the media panel is open carry red in some and
#            blue in others, still off one decoder shared by both slots.
#   paused   after `media playPause`, two frames apart carry the same counts
#            and one of red or blue, in the panel and in the bar crop alike:
#            the held frame, not the static art. The bar does not commit.
#   closed   with the panel shut the bar alone keeps its decoder.
#   off      `media.animatedBarCover: false` written into settings.json under
#            the running shell, playing again: no decoder, the bar crop back
#            on the green static art and the bar not committing; then the
#            panel open again on its own decoder, the bar still quiet.
#   sidebar  `layout: sidebar` and the bar cover back on, written the same
#            way: the column's media card and then the rail's, each read
#            off its `panel:media` hit in `sidebar status`, carry red in
#            some frames and blue in others.
leg_motion_art_flag="--motion-art"
leg_motion_art_order=36
leg_motion_art_compositors="hyprland shoji"
leg_motion_art_needs="mpv ffmpeg convert jq"
leg_motion_art_claims="layout"

motion_art_artist="FormalShell Motion Artist"
motion_art_album="Motion Album"
motion_art_key="formalshell-motion-artist-motion-album"
motion_art_dir="$iso_home/.cache/formalshell/applemusic-art"
motion_art_track_path="$shot_dir/motion-art-track.flac"
motion_art_cover_path="$shot_dir/motion-art-cover.png"
motion_art_frames=8
motion_art_results="$shot_dir/motion-art-results.txt"
motion_art_room="$shot_dir/motion-art-room.json"
motion_art_sidebar="$shot_dir/motion-art-sidebar.json"
motion_art_rail="$shot_dir/motion-art-rail.json"

leg_motion_art_timing() {
  leg_timing 110
}

leg_motion_art_fixture() {
  settings_pin '.media.appleMusicArt = true'
  mkdir -p "$motion_art_dir"
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "nullsrc=s=128x128:r=25:d=2" \
    -vf "geq=r='if(lt(mod(T,1),0.5),255,0)':g=0:b='if(lt(mod(T,1),0.5),0,255)',format=yuv420p" \
    -c:v mpeg4 -q:v 2 -y "$motion_art_dir/$motion_art_key.mp4"
  $convert_bin -size 64x64 "xc:#00C000" "$motion_art_cover_path"
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" \
    -i "$motion_art_cover_path" -map 0:a -map 1:0 -c:v png -disposition:v attached_pic -t 60 \
    -metadata "title=Motion" -metadata "artist=$motion_art_artist" -metadata "album=$motion_art_album" \
    -c:a flac -y "$motion_art_track_path"
}

leg_motion_art_drive() {
  local play="$shot_dir/motion-art-play.sh" drive="$shot_dir/motion-art-drive.sh"
  write_script "$play" <<EOF
#!/usr/bin/env bash
exec "$mpv_bin" --no-video --really-quiet --loop-file=inf "$motion_art_track_path"
EOF
  write_script "$drive" <<EOF
#!/usr/bin/env bash
note() { echo "\$1 \$2" >> "$motion_art_results"; }
decoders() { note "decoders_\$1" "\$(pgrep -fc 'applemusic-art')"; }
# commits <name> <seconds>: the bar's commits over that window.
commits() {
  local c0 c1
  c0=\$(grep -c '^commit surface=bar ' "$shell_log_path")
  sleep "\$2"
  c1=\$(grep -c '^commit surface=bar ' "$shell_log_path")
  note "commits_\$1" "\$((c1 - c0))"
}
: > "$motion_art_results"
sleep 4
call bar room > "$motion_art_room" 2>&1
burst "$shot_dir/motion-art-bar" $motion_art_frames 0.15
decoders bar
commits bar 3
call panel open media > /dev/null 2>&1
# The animated cover keeps the card moving and its decoder is a child of its own: a fixed wait.
sleep 4
burst "$shot_dir/motion-art-play" $motion_art_frames 0.15
decoders open
call media playPause > /dev/null 2>&1
sleep 1.5
"$grim_bin" "$shot_dir/motion-art-paused-1.png" > /dev/null 2>&1
sleep 0.8
"$grim_bin" "$shot_dir/motion-art-paused-2.png" > /dev/null 2>&1
commits paused 2
call panel close > /dev/null 2>&1
sleep 1.5
decoders closed
call media playPause > /dev/null 2>&1
settings_retarget '.media.animatedBarCover = false'
sleep 2.5
decoders off
"$grim_bin" "$shot_dir/motion-art-off.png" > /dev/null 2>&1
commits off 3
call panel open media > /dev/null 2>&1
# The decoder is a child of its own, started or not on the open.
sleep 4
decoders off_panel
commits off_panel 3
call panel close > /dev/null 2>&1
settings_retarget '.media.animatedBarCover = true | .layout = "sidebar"'
sleep 4
call sidebar status > "$motion_art_sidebar" 2>&1
burst "$shot_dir/motion-art-sidebar" $motion_art_frames 0.15
call sidebar collapse > /dev/null 2>&1
sleep 3
call sidebar status > "$motion_art_rail" 2>&1
burst "$shot_dir/motion-art-rail" $motion_art_frames 0.15
EOF
  wm_exec_once "bash $play"
  wm_exec_once "bash $drive"
}

# Pixels within 20% of one flat colour. White ones (every label on screen)
# count toward each, so only differences between the three counts mean
# anything.
motion_art_count() {
  $convert_bin "$1" -alpha off -fuzz 20% -fill white -opaque "$2" -fill black +opaque white \
    -format '%[fx:round(mean*w*h)]' info: 2>/dev/null
}

motion_art_counts() {
  echo "$(motion_art_count "$1" '#FF0000') $(motion_art_count "$1" '#0000FF') $(motion_art_count "$1" '#00C000')"
}

# The bar's now-playing cell off `bar room`, widened past the strip's own
# inset so the cover is inside it wherever the strip starts.
motion_art_bar_crop() {
  local box
  box=$(scaled_jq -r "$geometry_jq"'[.[0].cells[] | select(.name == "nowPlaying")][0] // empty | {x, y: 0, width: (.width + 64), height: (.height + 64)} | geometry' "$motion_art_room" 2>/dev/null)
  [ -n "$box" ] || return 1
  $convert_bin "$1" -crop "$box" +repage "$2"
}

# The sidebar's media card off its `panel:media` hit in `sidebar status`.
motion_art_card_crop() {
  local box
  box=$(scaled_jq -r "$geometry_jq"'[.hits[] | select(.on == "panel:media")][0] // empty | {x, y, width: .w, height: .h} | geometry' "$1" 2>/dev/null)
  [ -n "$box" ] || return 1
  $convert_bin "$2" -crop "$box" +repage "$3"
}

# motion_art_card_moves <phase> <status json>: the card's crops over the
# burst move between red and blue.
motion_art_card_moves() {
  local i f r b g reds=0 blues=0 trail=""
  for i in $(seq 1 $motion_art_frames); do
    f="$shot_dir/motion-art-$1-$i.png"
    [ -f "$f" ] || fail "no screenshot produced at $f"
    motion_art_card_crop "$2" "$f" "$shot_dir/motion-art-$1-crop-$i.png" || fail "sidebar status reported no media card ($(cat "$2"))"
    read -r r b g <<< "$(motion_art_counts "$shot_dir/motion-art-$1-crop-$i.png")"
    trail+=" $r/$b/$g"
    [ $((r - b)) -gt 100 ] && reds=$((reds + 1))
    [ $((b - r)) -gt 100 ] && blues=$((blues + 1))
  done
  echo "SMOKE_MOTION_ART_${1^^}_CROP $shot_dir/motion-art-$1-crop-1.png"
  echo "SMOKE_MOTION_ART_${1^^} red/blue/green$trail"
  [ "$reds" -gt 0 ] && [ "$blues" -gt 0 ] \
    || fail "the $1's media card did not move between red and blue while playing (red/blue/green$trail)"
}

motion_art_result() {
  awk -v n="$1" '$1 == n { print $2 }' "$motion_art_results"
}

leg_motion_art_assert() {
  local i f r b g reds=0 blues=0 trail=""
  [ -s "$motion_art_results" ] || fail "no motion-art checkpoints were written"
  cat "$motion_art_results"

  for i in $(seq 1 $motion_art_frames); do
    f="$shot_dir/motion-art-bar-$i.png"
    [ -f "$f" ] || fail "no screenshot produced at $f"
    motion_art_bar_crop "$f" "$shot_dir/motion-art-bar-crop-$i.png" || fail "bar room reported no nowPlaying cell ($(cat "$motion_art_room"))"
    read -r r b g <<< "$(motion_art_counts "$shot_dir/motion-art-bar-crop-$i.png")"
    trail+=" $r/$b/$g"
    [ $((r - b)) -gt 100 ] && reds=$((reds + 1))
    [ $((b - r)) -gt 100 ] && blues=$((blues + 1))
  done
  echo "SMOKE_MOTION_ART_BAR_CROP $shot_dir/motion-art-bar-crop-1.png"
  echo "SMOKE_MOTION_ART_BAR red/blue/green$trail"
  [ "$reds" -gt 0 ] && [ "$blues" -gt 0 ] \
    || fail "the bar's cover did not move between red and blue while playing (red/blue/green$trail)"
  [ "$(motion_art_result decoders_bar)" = 1 ] || fail "the bar's cover wants one decoder, pgrep counted $(motion_art_result decoders_bar)"
  [ "$(motion_art_result commits_bar)" -ge 6 ] || fail "the bar committed $(motion_art_result commits_bar) times in 3s of an animated cover"

  reds=0 blues=0 trail=""
  for i in $(seq 1 $motion_art_frames); do
    f="$shot_dir/motion-art-play-$i.png"
    [ -f "$f" ] || fail "no screenshot produced at $f"
    read -r r b g <<< "$(motion_art_counts "$f")"
    trail+=" $r/$b/$g"
    [ $((r - b)) -gt 4000 ] && reds=$((reds + 1))
    [ $((b - r)) -gt 4000 ] && blues=$((blues + 1))
  done
  echo "SMOKE_MOTION_ART_PLAY_FRAME $shot_dir/motion-art-play-1.png"
  echo "SMOKE_MOTION_ART_PLAYING red/blue/green$trail"
  [ "$reds" -gt 0 ] && [ "$blues" -gt 0 ] \
    || fail "the cover did not move between red and blue frames while playing (red/blue/green$trail)"
  [ "$(motion_art_result decoders_open)" = 1 ] || fail "the panel and the bar share one decoder, pgrep counted $(motion_art_result decoders_open)"

  local p1 p2 c1 c2
  p1=$(motion_art_counts "$shot_dir/motion-art-paused-1.png")
  p2=$(motion_art_counts "$shot_dir/motion-art-paused-2.png")
  echo "SMOKE_MOTION_ART_PAUSED $p1 | $p2"
  echo "SMOKE_MOTION_ART_PAUSED_FRAME $shot_dir/motion-art-paused-2.png"
  [ "$p1" = "$p2" ] || fail "the cover kept moving after the track paused ($p1 then $p2)"
  read -r r b g <<< "$p2"
  [ $((r - g)) -gt 4000 ] || [ $((b - g)) -gt 4000 ] || fail "the paused cover fell back to the static art ($p2)"
  motion_art_bar_crop "$shot_dir/motion-art-paused-1.png" "$shot_dir/motion-art-paused-bar-1.png"
  motion_art_bar_crop "$shot_dir/motion-art-paused-2.png" "$shot_dir/motion-art-paused-bar-2.png"
  c1=$(motion_art_counts "$shot_dir/motion-art-paused-bar-1.png")
  c2=$(motion_art_counts "$shot_dir/motion-art-paused-bar-2.png")
  echo "SMOKE_MOTION_ART_PAUSED_BAR $c1 | $c2"
  [ "$c1" = "$c2" ] || fail "the bar's cover kept moving after the track paused ($c1 then $c2)"
  read -r r b g <<< "$c2"
  [ $((r - g)) -gt 100 ] || [ $((b - g)) -gt 100 ] || fail "the bar's paused cover fell back to the static art ($c2)"
  [ "$(motion_art_result commits_paused)" -le 2 ] || fail "the bar committed $(motion_art_result commits_paused) times in 2s over a paused cover"

  [ "$(motion_art_result decoders_closed)" = 1 ] || fail "with the panel shut the bar keeps one decoder, pgrep counted $(motion_art_result decoders_closed)"

  echo "SMOKE_MOTION_ART_OFF decoders=$(motion_art_result decoders_off) commits=$(motion_art_result commits_off) panel_decoders=$(motion_art_result decoders_off_panel) panel_commits=$(motion_art_result commits_off_panel)"
  [ "$(motion_art_result decoders_off)" = 0 ] || fail "a decoder was still running with animatedBarCover off and the panel shut ($(motion_art_result decoders_off))"
  motion_art_bar_crop "$shot_dir/motion-art-off.png" "$shot_dir/motion-art-off-bar.png"
  read -r r b g <<< "$(motion_art_counts "$shot_dir/motion-art-off-bar.png")"
  echo "SMOKE_MOTION_ART_OFF_BAR $shot_dir/motion-art-off-bar.png $r/$b/$g"
  [ $((g - r)) -gt 100 ] && [ $((g - b)) -gt 100 ] || fail "the bar's cover is not the static art with animatedBarCover off ($r/$b/$g)"
  [ "$(motion_art_result commits_off)" -le 2 ] || fail "the bar committed $(motion_art_result commits_off) times in 3s with animatedBarCover off"
  [ "$(motion_art_result decoders_off_panel)" = 1 ] || fail "the panel's own cover wants one decoder, pgrep counted $(motion_art_result decoders_off_panel)"
  [ "$(motion_art_result commits_off_panel)" -le 2 ] || fail "the bar committed $(motion_art_result commits_off_panel) times in 3s under the panel's animated cover with animatedBarCover off"

  motion_art_card_moves sidebar "$motion_art_sidebar"
  motion_art_card_moves rail "$motion_art_rail"
}
