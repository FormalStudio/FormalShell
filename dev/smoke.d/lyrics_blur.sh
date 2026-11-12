# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --lyrics-blur: one real MPRIS player, a cache seeded
# with six main lines 6s apart so several sit far enough down the
# depth-of-field ramp, which is spent over the rows the pane has room for,
# to carry most of the blur it can ask for once the first one is lit. The cache file is written
# before the player is ever launched (a lookup starts a second
# after any key change, panel open or closed, so a live key with no file
# behind it would otherwise reach the real providers), and `media playPause`
# freezes playback right after the panel opens so both frames below are the
# same position and the same lit set, and the same crop lands on the same
# row twice.
#
# The first frame is `media.lyricsBlur`'s own default (true); the settings
# symlink is then retargeted the way `config_reload.sh` proved a running
# shell actually notices one (a plain rewrite is not the same case: inotify
# resolves a watch to the inode at watch-setup time, and home-manager's own
# activation only ever swaps the symlink beside it), pointing at a second
# generation with the key set false, and the second frame is taken once
# `media lyrics`'s own `blur` field agrees. A crop over a far, unlit row
# (never the active line, which never blurs) is compared by edge energy: a
# 3x1 morphological gradient, ImageMagick's own built-in and already on this
# rig's PATH, its `-format "%[fx:mean]"` read as one scalar per frame, which
# is what "sum of absolute horizontal neighbour differences" comes down to
# for one image. Sharp glyphs move it more than a Gaussian-blurred version
# of the same pixels, in either direction the eye would call "more edges",
# so the blur-on frame has to read measurably lower.
leg_lyrics_blur_flag="--lyrics-blur"
leg_lyrics_blur_order=176
leg_lyrics_blur_compositors="hyprland shoji"
leg_lyrics_blur_needs="mpv ffmpeg convert jq"
leg_lyrics_blur_claims="media.player=shared"

lyrics_blur_track_path="$shot_dir/lyrics-blur-track.flac"
lyrics_blur_pid_path="$shot_dir/lyrics-blur-mpv.pid"
lyrics_blur_status_path="$shot_dir/lyrics-blur-status.json"
lyrics_blur_bare_path="$shot_dir/lyrics-blur-bare.png"
lyrics_blur_on_png_path="$shot_dir/lyrics-blur-on.png"
lyrics_blur_off_png_path="$shot_dir/lyrics-blur-off.png"
lyrics_blur_status_on_path="$shot_dir/lyrics-blur-status-on.json"
lyrics_blur_status_off_path="$shot_dir/lyrics-blur-status-off.json"
lyrics_blur_lib_path="$shot_dir/lyrics-blur-lib.sh"

lyrics_blur_track_title="FormalShell Lyrics Blur Track"
lyrics_blur_track_artist="FormalShell Lyrics Blur Artist"
lyrics_blur_track_album="FormalShell Lyrics Blur Album"

leg_lyrics_blur_timing() {
  leg_timing 110
}

leg_lyrics_blur_fixture() {
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 90 \
    -metadata "title=$lyrics_blur_track_title" -metadata "artist=$lyrics_blur_track_artist" \
    -metadata "album=$lyrics_blur_track_album" -c:a flac -y "$lyrics_blur_track_path"

  write_script "$lyrics_blur_lib_path" <<'EOF'
#!/usr/bin/env bash
# Mirrors the lyrics service's cache key, same as lyrics.sh's own copy.
lyrics_blur_cache_key() {
  local artist="$1" title="$2" album="$3" duration="$4"
  local raw lower slug seconds
  raw="$artist $title $album"
  lower=$(printf '%s' "$raw" | tr '[:upper:]' '[:lower:]')
  slug=$(printf '%s' "$lower" | sed -E 's/[^a-z0-9]+/-/g; s/^-+//; s/-+$//')
  seconds=$(awk -v v="$duration" 'BEGIN { if (v < 0) v = 0; printf "%d", (v - int(v) >= 0.5) ? int(v) + 1 : int(v) }')
  printf '%s-%s' "$slug" "$seconds"
}

# Same technique as lyrics.sh's own `lyrics_pane_rect`: a pixel diff between
# a frame with the panel closed and one with it open, bar cropped away
# first. Echoes "x y w h bodyImagePath"; y is relative to that crop.
lyrics_blur_pane_rect() {
  local bare="$1" open="$2" bar open_body rect w h x y
  bar=$(logical_px 60)
  open_body="${open%.png}-body.png"
  "$convert_bin" "$open" -chop "0x${bar}" "$open_body" > /dev/null 2>&1
  rect=$(card_box "$bare" "$open" "$bar" 8%)
  IFS='x+' read -r w h x y <<< "$rect"
  printf '%s %s %s %s %s\n' "${x:-0}" "$(( ${y:-$bar} - bar ))" "${w:-0}" "${h:-0}" "$open_body"
}

# A 3x1 morphological gradient (max-min over each pixel's own row of three),
# ImageMagick's own built-in rather than a hand-rolled convolution: a signed
# Sobel kernel's mean cancels to ~0 over any roughly-symmetric edge (checked
# by hand before this leg was written), where the gradient operator is
# already the non-negative magnitude "sum of absolute neighbour differences"
# comes down to for one image.
lyrics_blur_edge_energy() {
  local img="$1" conv="$2"
  "$conv" "$img" -colorspace Gray -statistic Gradient 3x1 -format '%[fx:mean]' info: 2>/dev/null
}
EOF

  # Six lines, 6s apart (2s gap between each, well under the 5s interlude
  # floor), each word 0.8s on so the lit line wipes and glows chunk by chunk
  # through the audited stretch, and several lines sit past the blur ramp's
  # 3-row cap once playback is frozen.
  cat > "$shot_dir/lyrics-blur-cache.json" <<'JSON'
{"source": "apple", "lines": [
{"time": 2.0, "end": 6.0, "text": "Copper wire hums low", "parent": null, "background": false, "oppositeTurn": false, "estimated": false, "words": [{"time": 2.0, "text": "Copper", "joinsNext": false}, {"time": 2.8, "text": "wire", "joinsNext": false}, {"time": 3.6, "text": "hums", "joinsNext": false}, {"time": 4.4, "text": "low", "joinsNext": false}]},
{"time": 8.0, "end": 12.0, "text": "Slate roofs hold the frost", "parent": null, "background": false, "oppositeTurn": false, "estimated": false, "words": [{"time": 8.0, "text": "Slate", "joinsNext": false}, {"time": 8.8, "text": "roofs", "joinsNext": false}, {"time": 9.6, "text": "hold", "joinsNext": false}, {"time": 10.4, "text": "the", "joinsNext": false}, {"time": 11.2, "text": "frost", "joinsNext": false}]},
{"time": 14.0, "end": 18.0, "text": "Empty platform waits", "parent": null, "background": false, "oppositeTurn": false, "estimated": false, "words": [{"time": 14.0, "text": "Empty", "joinsNext": false}, {"time": 14.8, "text": "platform", "joinsNext": false}, {"time": 15.6, "text": "waits", "joinsNext": false}]},
{"time": 20.0, "end": 24.0, "text": "Signal fades to static", "parent": null, "background": false, "oppositeTurn": false, "estimated": false, "words": [{"time": 20.0, "text": "Signal", "joinsNext": false}, {"time": 20.8, "text": "fades", "joinsNext": false}, {"time": 21.6, "text": "to", "joinsNext": false}, {"time": 22.4, "text": "static", "joinsNext": false}]},
{"time": 26.0, "end": 30.0, "text": "Distant engine turns over", "parent": null, "background": false, "oppositeTurn": false, "estimated": false, "words": [{"time": 26.0, "text": "Distant", "joinsNext": false}, {"time": 26.8, "text": "engine", "joinsNext": false}, {"time": 27.6, "text": "turns", "joinsNext": false}, {"time": 28.4, "text": "over", "joinsNext": false}]},
{"time": 32.0, "end": 36.0, "text": "Morning finds the platform empty", "parent": null, "background": false, "oppositeTurn": false, "estimated": false, "words": [{"time": 32.0, "text": "Morning", "joinsNext": false}, {"time": 32.8, "text": "finds", "joinsNext": false}, {"time": 33.6, "text": "the", "joinsNext": false}, {"time": 34.4, "text": "platform", "joinsNext": false}, {"time": 35.2, "text": "empty", "joinsNext": false}]}
]}
JSON

  # The lyrics service starts a lookup a second after any key change,
  # panel open or closed, so the cache file has to exist before the track
  # is ever selected, not after (lyrics.sh's own fixture carries the full
  # reasoning). Written here, straight into the isolated XDG_CACHE_HOME,
  # off the fixture's own known 90s ffmpeg duration.
  # shellcheck source=/dev/null
  . "$lyrics_blur_lib_path"
  local cache_dir="$iso_home/.cache/formalshell/lyrics"
  mkdir -p "$cache_dir"
  local key
  key=$(lyrics_blur_cache_key "$lyrics_blur_track_artist" "$lyrics_blur_track_title" "$lyrics_blur_track_album" 90)
  cp "$shot_dir/lyrics-blur-cache.json" "$cache_dir/$key.json"
}

leg_lyrics_blur_drive() {
  local script="$shot_dir/lyrics-blur-drive.sh"
  local kill_script="$shot_dir/lyrics-blur-kill.sh"

  write_script "$script" <<EOF
#!/usr/bin/env bash
. "$lyrics_blur_lib_path"
"$mpv_bin" --no-video --really-quiet "$lyrics_blur_track_path" &
disown
echo \$! > "$lyrics_blur_pid_path"

until_jq 10 "$lyrics_blur_status_path" '.title == "$lyrics_blur_track_title"' call media status

# The isolated session's own audio bring-up can pop a volume OSD once,
# early, unrelated to anything this leg does (seen once as a stray large
# diff region between the bare and open frames, on a run where the bare
# shot landed while it was still up); the OSD's own hide delay is 1.6s, so
# this clears it before the bare reference is taken.
sleep 3
"$grim_bin" "$lyrics_blur_bare_path" > /dev/null 2>&1
$ipc call panel open media > /dev/null 2>&1
until_jq 15 "$lyrics_blur_status_on_path" '.state == "synced"' call media lyrics

# Played through two line changes (8s and 14s) under the stale audit: the
# lit line's wipe and glow redraw small boxes reaching the blurred rows
# beside it every frame, and none of those redraws may leave a pixel a
# whole redraw does not draw.
$(stale_audit_start)
SECONDS=0
while [ "\$SECONDS" -lt 25 ]; do
  pos=\$($ipc call media lyrics 2>/dev/null | "$jq_bin" -r '.position // 0')
  awk -v p="\$pos" 'BEGIN { exit !(p >= 16) }' && break
  sleep 0.5
done
$(stale_audit_stop lyrics-blur)

# Frozen here, on the third line, so every frame below shares one position
# and one lit set.
$ipc call media playPause > /dev/null 2>&1
sleep 1
$ipc call media lyrics > "$lyrics_blur_status_on_path" 2>&1
"$grim_bin" "$lyrics_blur_on_png_path" > /dev/null 2>&1

settings_retarget '.media.lyricsBlur = false'

until_jq 15 "$lyrics_blur_status_off_path" '.blur == false' call media lyrics
# IPC flips with the key; the rows leave their blur on the effects clock after
# it, and a frame taken in between is the blurred one again.
sleep 1
"$grim_bin" "$lyrics_blur_off_png_path" > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"

  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
[ -f "$lyrics_blur_pid_path" ] && kill "\$(cat "$lyrics_blur_pid_path")" 2>/dev/null
true
EOF
  add_cleanup "bash $kill_script"
}

leg_lyrics_blur_assert() {
  [ -s "$lyrics_blur_status_path" ] || fail "no media status produced for the blur track"
  expect_jq "$lyrics_blur_status_path" ".title == \"$lyrics_blur_track_title\"" "media status never showed the blur track" '.title'
  [ -f "$lyrics_blur_bare_path" ] || fail "no bare (panel closed) screenshot produced"

  if [ ! -s "$lyrics_blur_status_on_path" ]; then
    fail "no media lyrics status produced with blur on"
  fi
  cat "$lyrics_blur_status_on_path"; echo
  echo "SMOKE_LYRICS_BLUR_STATUS_ON $lyrics_blur_status_on_path"
  expect_jq "$lyrics_blur_status_on_path" '.state == "synced"' "media lyrics never reached synced" '{state, blur}'
  expect_jq "$lyrics_blur_status_on_path" '.blur == true' "media.lyricsBlur's own default did not read true over IPC" '{state, blur}'
  stale_audit_assert lyrics-blur
  [ -f "$lyrics_blur_on_png_path" ] || fail "no blur-on screenshot produced"
  echo "SMOKE_LYRICS_BLUR_ON $lyrics_blur_on_png_path"

  if [ ! -s "$lyrics_blur_status_off_path" ]; then
    fail "no media lyrics status produced after the settings retarget"
  fi
  cat "$lyrics_blur_status_off_path"; echo
  echo "SMOKE_LYRICS_BLUR_STATUS_OFF $lyrics_blur_status_off_path"
  expect_jq "$lyrics_blur_status_off_path" '.blur == false' "the retargeted settings symlink (media.lyricsBlur: false) was never noticed" '{state, blur}'
  [ -f "$lyrics_blur_off_png_path" ] || fail "no blur-off screenshot produced"
  echo "SMOKE_LYRICS_BLUR_OFF $lyrics_blur_off_png_path"

  local pos_on pos_off
  pos_on=$("$jq_bin" -r '.position' "$lyrics_blur_status_on_path" 2>/dev/null)
  pos_off=$("$jq_bin" -r '.position' "$lyrics_blur_status_off_path" 2>/dev/null)
  if ! awk -v a="${pos_on:--1}" -v b="${pos_off:--2}" 'BEGIN { d = a - b; if (d < 0) d = -d; exit !(d < 1.0) }'; then
    fail "playback moved between the two frames (on: $pos_on, off: $pos_off), so they are not the same position"
  fi

  # shellcheck source=/dev/null
  . "$lyrics_blur_lib_path"
  local rx ry rw rh on_body off_body energy_on energy_off
  read -r rx ry rw rh on_body < <(lyrics_blur_pane_rect "$lyrics_blur_bare_path" "$lyrics_blur_on_png_path")
  read -r _ _ _ _ off_body < <(lyrics_blur_pane_rect "$lyrics_blur_bare_path" "$lyrics_blur_off_png_path")
  local lyrics_x0=$((rx + 480)) lyrics_w=$((rw - 480))
  # The bottom third of the pane's own viewport: with the first line lit and
  # frozen, everything back there is most of the way from the anchor to the
  # pane's own edge, which is where the blur ramp reaches its cap, so it is
  # unambiguously the far end rather than a guess at exactly which row.
  local band_y=$((ry + rh * 60 / 100)) band_h=$((rh * 30 / 100))
  local on_crop="$shot_dir/lyrics-blur-on-crop.png" off_crop="$shot_dir/lyrics-blur-off-crop.png"
  "$convert_bin" "$on_body" -crop "${lyrics_w}x${band_h}+${lyrics_x0}+${band_y}" +repage "$on_crop" > /dev/null 2>&1
  "$convert_bin" "$off_body" -crop "${lyrics_w}x${band_h}+${lyrics_x0}+${band_y}" +repage "$off_crop" > /dev/null 2>&1
  energy_on=$(lyrics_blur_edge_energy "$on_crop" "$convert_bin")
  energy_off=$(lyrics_blur_edge_energy "$off_crop" "$convert_bin")
  echo "SMOKE_LYRICS_BLUR_CROP_ON $on_crop"
  echo "SMOKE_LYRICS_BLUR_CROP_OFF $off_crop"
  echo "SMOKE_LYRICS_BLUR_ENERGY on=${energy_on:-0} off=${energy_off:-0} (rect: x=$rx y=$ry w=$rw h=$rh, band y=$band_y h=$band_h)"
  # Measured over eight VM runs: off/on ranged 1.15 to 1.24. The margin
  # below is pinned under the lowest of those with headroom, not at the
  # mean.
  if ! awk -v on="${energy_on:-0}" -v off="${energy_off:-0}" 'BEGIN { exit !(off > on * 1.10) }'; then
    fail "blur-off did not read measurably sharper than blur-on over the far row(s): on=$energy_on off=$energy_off"
  fi
}
