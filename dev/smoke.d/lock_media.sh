# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --lock-media: the lock surface's now-playing card and the clock's ink off
# the wallpaper. One real MPRIS player (mpv carrying
# mpris.lua, the --media leg's own need_mpv) loops a tagged fixture track
# with a striped cover while the session locks in dark mode over a flat
# white wallpaper; the card is photographed with its cover and title, then
# driven by real
# keys through the password field's own filter (Tab onto the transport,
# Right to play/pause, Return) and `media status` has to say the player
# paused. The wallpaper is then swapped for a flat dark one under the same
# lock, and the ink `lock status` reports has to flip: dark over the white
# field (black at 0.5 over white is mid grey, where black out-contrasts
# white) and light over the dark one. The clock itself sits on the login
# card (`surfaces::login`), so the frame inside its rect carries the card's
# own ink whatever the wallpaper: light words in dark mode over both
# fields, dark ones in light mode. Then the
# mode goes light under the same lock: the card is the theme's `card` box,
# so a band of its top padding (`mediaCard` off the report) is one flat
# fill in both modes, dark in dark mode and light in light mode over the
# same dark wallpaper. The real
# password typed last unlocks, which is the proof its first character still
# reached the field with the transport cursor on.
#
# Its clock starts after --lock's whole sequence when both run, since both
# lock the one session.
leg_lock_media_flag="--lock-media"
leg_lock_media_order=111
leg_lock_media_needs="mpv ffmpeg convert wtype jq"

lock_media_track="$shot_dir/lock-media-track.flac"
lock_media_art="$shot_dir/lock-media-art.png"
lock_media_title="FormalShell Lock Track"
lock_media_artist="FormalShell Lock Artist"
lock_media_bright_wp="$shot_dir/lock-media-bright.png"
lock_media_dark_wp="$shot_dir/lock-media-dark.png"
lock_media_pid="$shot_dir/lock-media-mpv.pid"

lock_media_before_json="$shot_dir/lock-media-before.json"
lock_media_bright_json="$shot_dir/lock-media-bright-status.json"
lock_media_bright_png="$shot_dir/lock-media-bright-frame.png"
lock_media_cursor_json="$shot_dir/lock-media-cursor.json"
lock_media_cursor_png="$shot_dir/lock-media-cursor.png"
lock_media_paused_json="$shot_dir/lock-media-paused.json"
lock_media_dark_json="$shot_dir/lock-media-dark-status.json"
lock_media_dark_png="$shot_dir/lock-media-dark-frame.png"
lock_media_light_json="$shot_dir/lock-media-light-status.json"
lock_media_light_png="$shot_dir/lock-media-light-frame.png"
lock_media_unlocked_txt="$shot_dir/lock-media-unlocked.txt"

lock_media_t0() {
  if leg_on lock; then echo $(($(lock_t0) + 26)); else echo 5; fi
}

leg_lock_media_fixture() {
  $convert_bin -size 64x64 "xc:#E03131" -size 64x64 "xc:#F08C00" -size 64x64 "xc:#FFD700" \
    -size 64x64 "xc:#2F9E44" -size 64x64 "xc:#1971C2" -size 64x64 "xc:#9C36B5" \
    +append -resize "64x64!" "$lock_media_art"
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" \
    -i "$lock_media_art" -map 0:a -map 1:0 -c:v png -disposition:v attached_pic -t 30 \
    -metadata "title=$lock_media_title" -metadata "artist=$lock_media_artist" \
    -c:a flac -y "$lock_media_track"
  $convert_bin -size 1920x1080 xc:'#ffffff' "$lock_media_bright_wp"
  $convert_bin -size 1920x1080 xc:'#1e1e1e' "$lock_media_dark_wp"
}

leg_lock_media_timing() {
  local t0
  t0=$(lock_media_t0)
  leg_timing $((t0 + 50)) $((t0 + 80))
}

leg_lock_media_drive() {
  local t0 script="$shot_dir/lock-media-drive.sh" kill_script="$shot_dir/lock-media-kill.sh"
  t0=$(lock_media_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
"$mpv_bin" --no-video --really-quiet --loop-file=inf "$lock_media_track" &
echo \$! > "$lock_media_pid"
$ipc call theme mode dark > /dev/null 2>&1
$ipc call wallpaper set "$lock_media_bright_wp" > /dev/null 2>&1
sleep 6
$ipc call media status > "$lock_media_before_json" 2>&1
$ipc call lock lock > /dev/null 2>&1
sleep 5
$ipc call lock status > "$lock_media_bright_json" 2>&1
"$grim_bin" "$lock_media_bright_png" > /dev/null 2>&1
"$wtype_bin" -k Tab
sleep 0.5
"$wtype_bin" -k Right
sleep 1
$ipc call lock status > "$lock_media_cursor_json" 2>&1
"$grim_bin" "$lock_media_cursor_png" > /dev/null 2>&1
"$wtype_bin" -k Return
sleep 2
$ipc call media status > "$lock_media_paused_json" 2>&1
$ipc call wallpaper set "$lock_media_dark_wp" > /dev/null 2>&1
sleep 7
$ipc call lock status > "$lock_media_dark_json" 2>&1
"$grim_bin" "$lock_media_dark_png" > /dev/null 2>&1
$ipc call theme mode light > /dev/null 2>&1
sleep 7
$ipc call lock status > "$lock_media_light_json" 2>&1
"$grim_bin" "$lock_media_light_png" > /dev/null 2>&1
"$wtype_bin" "formalshell-test"
"$wtype_bin" -k Return
sleep 4
$ipc call lock isLocked > "$lock_media_unlocked_txt" 2>&1
EOF
  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
[ -f "$lock_media_pid" ] && kill "\$(cat "$lock_media_pid")" 2>/dev/null
true
EOF
  add_cleanup "bash $kill_script"
  hypr_exec_once "bash $script"
}

# The first output's report out of a `lock status` reply.
_lock_media_out() {
  "$jq_bin" -r ".outputs | to_entries | .[0].value.$2 // \"none\"" "$1" 2>/dev/null
}

# How many pixels of the clock's own rect are near black and near white, as
# "dark light". The rect comes off the report, so the crop is the block the
# shell sampled rather than a guess at where the clock landed.
_lock_media_ink_px() {
  local json="$1" png="$2" geom dark light
  geom=$("$jq_bin" -r '.outputs | to_entries | .[0].value.clockRect
    | "\(.width | floor)x\(.height | floor)+\(.x | floor)+\(.y | floor)"' "$json" 2>/dev/null)
  dark=$($convert_bin "$png" -crop "$geom" +repage -colorspace gray \
    -threshold 27% -negate -format '%[fx:int(mean*w*h)]' info: 2>/dev/null)
  light=$($convert_bin "$png" -crop "$geom" +repage -colorspace gray \
    -threshold 78% -format '%[fx:int(mean*w*h)]' info: 2>/dev/null)
  echo "${dark:-0} ${light:-0}"
}

# Mean and standard deviation, in thousandths, of a band of the card's top
# padding: inside its border and clear of the rounded corners.
_lock_media_card_band() {
  local json="$1" png="$2" geom
  geom=$("$jq_bin" -r '.outputs | to_entries | .[0].value.mediaCard // empty
    | "\((.width - 48) | floor)x6+\((.x + 24) | floor)+\((.y + 3) | floor)"' "$json" 2>/dev/null)
  [ -n "$geom" ] || return 0
  $convert_bin "$png" -crop "$geom" +repage -colorspace gray \
    -format '%[fx:int(mean*1000)] %[fx:int(standard_deviation*1000)]' info: 2>/dev/null
}

leg_lock_media_assert() {
  local f ink shown title playing cursor px dark light stripes dmean dsd lmean lsd
  for f in "$lock_media_before_json" "$lock_media_bright_json" "$lock_media_cursor_json" \
    "$lock_media_paused_json" "$lock_media_dark_json"; do
    [ -s "$f" ] || fail "lock-media: no reply produced at $f"
  done
  for f in "$lock_media_bright_png" "$lock_media_cursor_png" "$lock_media_dark_png"; do
    [ -f "$f" ] || fail "lock-media: no frame produced at $f"
  done
  echo "SMOKE_LOCK_MEDIA_BRIGHT $lock_media_bright_png"
  echo "SMOKE_LOCK_MEDIA_CURSOR $lock_media_cursor_png"
  echo "SMOKE_LOCK_MEDIA_DARK $lock_media_dark_png"

  title=$("$jq_bin" -r '.title' "$lock_media_before_json")
  playing=$("$jq_bin" -r '.isPlaying' "$lock_media_before_json")
  echo "lock-media: before the lock title='$title' playing=$playing"
  [ "$title" = "$lock_media_title" ] || fail "lock-media: media status names '$title', not the fixture track"
  [ "$playing" = "true" ] || fail "lock-media: the fixture player was not playing before the lock"

  # The block on the locked frame: the report says it is shown, and the
  # cover's own stripes are in the picture (the red one, nothing else on the
  # lock paints it).
  shown=$(_lock_media_out "$lock_media_bright_json" nowPlaying)
  stripes=$($convert_bin "$lock_media_bright_png" -fuzz 6% -fill black +opaque '#E03131' \
    -fill white -opaque '#E03131' -format '%[fx:int(mean*w*h)]' info: 2>/dev/null)
  echo "lock-media: nowPlaying=$shown cover-red-px=${stripes:-0}"
  [ "$shown" = "true" ] || fail "lock-media: the lock reports no now-playing block with a player playing"
  [ "${stripes:-0}" -ge 40 ] || fail "lock-media: the cover's red stripe is not in the locked frame (${stripes:-0} px)"

  # Tab then Right puts the cursor on play/pause without the field losing
  # the keys; Return pressed it.
  cursor=$(_lock_media_out "$lock_media_cursor_json" transportCursor)
  playing=$("$jq_bin" -r '.isPlaying' "$lock_media_paused_json")
  echo "lock-media: transportCursor=$cursor after Tab+Right, playing=$playing after Return"
  [ "$cursor" = "1" ] || fail "lock-media: Tab then Right left the transport cursor on '$cursor', not play/pause (1)"
  [ "$playing" = "false" ] || fail "lock-media: Return on play/pause did not pause the player (isPlaying=$playing)"

  # The ink the report asks for follows the wallpaper; the card's words
  # follow the mode.
  ink=$(_lock_media_out "$lock_media_bright_json" clockInk)
  px=$(_lock_media_ink_px "$lock_media_bright_json" "$lock_media_bright_png")
  read -r dark light <<< "$px"
  echo "lock-media: white wallpaper clockInk=$ink luma=$(_lock_media_out "$lock_media_bright_json" clockLuma) clock-dark-px=$dark clock-light-px=$light"
  [ "$ink" = "dark" ] || fail "lock-media: the clock took '$ink' ink over a white wallpaper, not dark"
  [ "$light" -ge 400 ] || fail "lock-media: the clock's rect carries $light light pixels in dark mode over a white wallpaper; the card's words never reached the frame"

  ink=$(_lock_media_out "$lock_media_dark_json" clockInk)
  px=$(_lock_media_ink_px "$lock_media_dark_json" "$lock_media_dark_png")
  read -r dark light <<< "$px"
  echo "lock-media: dark wallpaper clockInk=$ink luma=$(_lock_media_out "$lock_media_dark_json" clockLuma) clock-dark-px=$dark clock-light-px=$light"
  [ "$ink" = "light" ] || fail "lock-media: the clock took '$ink' ink over a dark wallpaper, not light"
  [ "$light" -ge 400 ] || fail "lock-media: the clock's rect carries $light light pixels in dark mode over a dark wallpaper; the card's words never reached the frame"

  # The card over the same dark wallpaper in each mode.
  [ -s "$lock_media_light_json" ] || fail "lock-media: no reply produced at $lock_media_light_json"
  [ -f "$lock_media_light_png" ] || fail "lock-media: no frame produced at $lock_media_light_png"
  echo "SMOKE_LOCK_MEDIA_LIGHT $lock_media_light_png"
  local band_dark band_light
  band_dark=$(_lock_media_card_band "$lock_media_dark_json" "$lock_media_dark_png")
  band_light=$(_lock_media_card_band "$lock_media_light_json" "$lock_media_light_png")
  echo "lock-media: card padding band (mean sd, thousandths) dark mode=$band_dark light mode=$band_light"
  read -r dmean dsd <<< "$band_dark"
  read -r lmean lsd <<< "$band_light"
  [[ "$dmean" =~ ^[0-9]+$ && "$lmean" =~ ^[0-9]+$ ]] || fail "lock-media: the report carries no mediaCard rect"
  [ "$dsd" -le 30 ] && [ "$lsd" -le 30 ] || fail "lock-media: the card's padding is not one flat fill (sd $dsd, $lsd)"
  [ "$lmean" -ge $((dmean + 300)) ] \
    || fail "lock-media: the card did not follow the mode: dark-mode fill $dmean, light-mode fill $lmean"

  if [ ! -s "$lock_media_unlocked_txt" ] || ! grep -q "^false$" "$lock_media_unlocked_txt"; then
    fail "lock-media: the real password typed with the transport cursor on did not unlock. Got: $(cat "$lock_media_unlocked_txt" 2>/dev/null)"
  fi
  echo "SMOKE_LOCK_MEDIA ok"
}
