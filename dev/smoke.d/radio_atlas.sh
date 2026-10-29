# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --radio-atlas opens Radio Atlas with `panel open radio` over a world cache
# the fixture writes: one station, served on loopback the way --radio serves
# its favourite, placed on the western Tibetan plateau, mountains and lakes
# with no real station Radio Browser adds within a click of it. Enter on a real key plays the
# list's first station, which turns the globe from its starting pose to
# centre it: the globe's own box has to differ between the open frame and
# that one. After `radio stop`, a real pointer
# click on the globe's centre (where the playing station now sits) picks the
# station off the globe and plays it again, read back off `radio status`.
# Then dev/vpointer.py holds the button across the globe: a drag that
# stops for 250 ms before its release must leave `radio status`'s
# atlas.globe.longitude where the release left it, and the same drag let
# go while moving must coast, less than 2.5 times the drag's own turn.
# Escape closes the atlas, read back off `panel state`. Before that, the
# pointer parks on the header's close button, and the formalshell:tooltip
# layer has to be absent before and present after, the way --tooltip reads a
# panel header. The example Hyprland config has to carry a layer rule for
# formalshell:radio (this rig runs with blur off, so a frame cannot show it).
#
# The globe wears NASA's Blue Marble (the release tarball's, dev/tarball.sh):
# `radio status` has to report atlas.globe.imagery true before the open
# frame is taken, and a square inside the disc has to carry colour the flat
# globe never had (mean HSL saturation 0.05 and some 450 colours over the
# same square in the flat globe's frame), and the whole disc's mean
# luminance has to clear 0.18: the picture as NASA masters it read 0.127,
# and the tone `earth::load` lays over each tile lifts that to about 0.26.
# The picture is a tile pyramid (FS_EARTH_TILES, packaging/earth-tiles.py), and
# the leg points FS_EARTH_TILES at a link of its own to the package's tiles.
# After the rest, the station plays again to bring the globe back over it,
# and a real wheel over the globe zooms it to its limit: `radio status` has
# to report scale 24 drawn at the 21600 level with no tile missing. VmRSS is
# read with it open there and again after Escape, which has to give back at
# least 16 MB. Then the link goes and the atlas opens again at the same pose
# on the single 4096 picture FS_EARTH_IMAGE names, reported as level 4096.
# A square of the disc off the station, the same in both frames, has to
# carry at least twice the high-frequency detail on the tiles (the mean
# absolute difference from a 1.5px gaussian blur of its luminance).
#
# Every globe build the shell traces while the leg runs (`atlas globe
# build_us=`, the sampler's share in sample_us) and every commit of the
# card (render_us) is summarised as SMOKE_RADIO_ATLAS_FRAME, with
# FS_CPU_QUOTA the e1504g stand-in.
#
# The globe's centre is worked out from the panel's own layout and the
# spacing tokens (panelPadding 12, controlHeight 32, lg 8, a 16px caption
# line), since nothing over IPC reports it.
leg_radio_atlas_flag="--radio-atlas"
leg_radio_atlas_order=176
leg_radio_atlas_needs="ffmpeg wtype wlrctl convert jq python3"

radio_atlas_track_path="$shot_dir/radio-atlas-station.mp3"
radio_atlas_open_png="$shot_dir/radio-atlas-open.png"
radio_atlas_turned_png="$shot_dir/radio-atlas-turned.png"
radio_atlas_picked_png="$shot_dir/radio-atlas-picked.png"
radio_atlas_open_path="$shot_dir/radio-atlas-open.txt"
radio_atlas_imagery_path="$shot_dir/radio-atlas-imagery.json"
radio_atlas_state_path="$shot_dir/radio-atlas-state.txt"
radio_atlas_played_path="$shot_dir/radio-atlas-played.json"
radio_atlas_stopped_path="$shot_dir/radio-atlas-stopped.json"
radio_atlas_picked_path="$shot_dir/radio-atlas-picked.json"
radio_atlas_closed_path="$shot_dir/radio-atlas-closed.txt"
radio_atlas_globe_path="$shot_dir/radio-atlas-globe.txt"
radio_atlas_tip_before_path="$shot_dir/radio-atlas-tip-before.json"
radio_atlas_tip_after_path="$shot_dir/radio-atlas-tip-after.json"
radio_atlas_tip_png="$shot_dir/radio-atlas-tooltip.png"
radio_atlas_loop_pid_path="$shot_dir/radio-atlas-loop.pid"
radio_atlas_drag_before_path="$shot_dir/radio-atlas-drag-before.json"
radio_atlas_drag_released_path="$shot_dir/radio-atlas-drag-released.json"
radio_atlas_drag_settled_path="$shot_dir/radio-atlas-drag-settled.json"
radio_atlas_fling_released_path="$shot_dir/radio-atlas-fling-released.json"
radio_atlas_fling_settled_path="$shot_dir/radio-atlas-fling-settled.json"
radio_atlas_tiles_link="$shot_dir/radio-atlas-tiles"
radio_atlas_zoom_tiles_png="$shot_dir/radio-atlas-zoom-tiles.png"
radio_atlas_zoom_single_png="$shot_dir/radio-atlas-zoom-single.png"
radio_atlas_zoom_tiles_path="$shot_dir/radio-atlas-zoom-tiles.json"
radio_atlas_zoom_single_path="$shot_dir/radio-atlas-zoom-single.json"
radio_atlas_rss_path="$shot_dir/radio-atlas-rss.txt"
radio_atlas_vpointer="$PWD/dev/vpointer.py"
# Twelve 15px steps a frame apart, about 940 px/s.
radio_atlas_steps=$(for _ in $(seq 12); do printf 'move 15 0 wait 16 '; done)
radio_atlas_name="FormalShell Atlas Radio"
radio_atlas_port=18098

leg_radio_atlas_timing() {
  leg_timing 110 140
}

leg_radio_atlas_fixture() {
  local cache_dir="$iso_home/.cache/formalshell/radio-atlas"
  mkdir -p "$cache_dir"
  local now_ms
  now_ms=$(($(date +%s) * 1000))
  cat > "$cache_dir/world.json" <<EOF
{"fetchedAt":$now_ms,"stations":[{"uuid":"smoke-atlas-1","name":"$radio_atlas_name","url":"http://127.0.0.1:$radio_atlas_port/station.mp3","homepage":"","favicon":"","country":"France","countryCode":"FR","state":"","language":"","tags":"","codec":"MP3","bitrate":128,"votes":0,"clicks":0,"latitude":31.5,"longitude":84.5}]}
EOF
  local tiles
  tiles=$(readlink -f "$PWD/result/share/formalshell/tiles")
  [ -d "$tiles" ] || fail "--radio-atlas: the result carries no blue-marble tiles"
  ln -sfn "$tiles" "$radio_atlas_tiles_link"
  ln -sfn "$tiles" "$radio_atlas_tiles_link.saved"
  shell_env="${shell_env:+$shell_env }FS_EARTH_TILES=$radio_atlas_tiles_link"
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=44100" -t 60 \
    -c:a libmp3lame -b:a 128k -y "$radio_atlas_track_path"
}

leg_radio_atlas_drive() {
  local script="$shot_dir/radio-atlas-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# ffmpeg's listener takes one client and exits; the station is played twice.
# The loop closes every inherited fd above stderr first (the caller's flock
# fd among them) and its pid is recorded so cleanup can stop the respawning.
( for fd in /proc/\$BASHPID/fd/*; do n=\${fd##*/}; [ "\$n" -gt 2 ] && eval "exec \$n>&-"; done 2>/dev/null
  while true; do
  "$ffmpeg_bin" -nostdin -loglevel error -re -i "$radio_atlas_track_path" -c copy -f mp3 \
    -listen 1 "http://127.0.0.1:$radio_atlas_port/station.mp3"
done ) &
echo \$! > "$radio_atlas_loop_pid_path"
sleep 5
$ipc call panel open radio > "$radio_atlas_open_path" 2>&1
sleep 4
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  $ipc call radio status > "$radio_atlas_imagery_path" 2>&1
  grep -q '"imagery":true' "$radio_atlas_imagery_path" && break
  sleep 0.5
done
sleep 0.5
$ipc call panel state > "$radio_atlas_state_path" 2>&1
"$grim_bin" "$radio_atlas_open_png" > /dev/null 2>&1
"$wtype_bin" -k Return
SECONDS=0
while [ "\$SECONDS" -lt 12 ]; do
  $ipc call radio status > "$radio_atlas_played_path" 2>&1
  grep -q '"loaded":true' "$radio_atlas_played_path" && break
  sleep 1
done
sleep 1
"$grim_bin" "$radio_atlas_turned_png" > /dev/null 2>&1
$ipc call radio stop > /dev/null 2>&1
sleep 3
$ipc call radio status > "$radio_atlas_stopped_path" 2>&1
read -r out_w out_h < <($convert_bin "$radio_atlas_open_png" -format '%w %h' info:)
cw=\$(( out_w * 85 / 100 < 1180 ? out_w * 85 / 100 : 1180 ))
ch=\$(( out_h * 85 / 100 < 760 ? out_h * 85 / 100 : 760 ))
ox=\$(( (out_w - cw - 24) / 2 + 12 ))
oy=\$(( (out_h - ch - 24) / 2 + 12 ))
side=\$(( cw * 39 / 100 < 390 ? cw * 39 / 100 : 390 ))
gl=\$ox
gr=\$(( ox + cw - side - 12 - 1 - 12 ))
gt=\$(( oy + 32 + 12 + 1 + 12 ))
gb=\$(( oy + ch - 16 - 8 ))
gx=\$(( (gl + gr) / 2 ))
gy=\$(( (gt + gb) / 2 ))
echo "\$gl \$gt \$gr \$gb \$gx \$gy" > "$radio_atlas_globe_path"
"$wlrctl_bin" pointer move -4000 -4000
sleep 0.3
"$wlrctl_bin" pointer move \$gx \$gy
sleep 1
"$wlrctl_bin" pointer click left
SECONDS=0
while [ "\$SECONDS" -lt 12 ]; do
  $ipc call radio status > "$radio_atlas_picked_path" 2>&1
  grep -q '"loaded":true' "$radio_atlas_picked_path" && break
  sleep 1
done
sleep 1
"$grim_bin" "$radio_atlas_picked_png" > /dev/null 2>&1
# A held drag across the globe that stops, holds still, then lets go: read
# the pose before, just after the release and once a coast would be over.
$ipc call radio status > "$radio_atlas_drag_before_path" 2>&1
"$python3_bin" "$radio_atlas_vpointer" down $radio_atlas_steps wait 250 up
sleep 0.1
$ipc call radio status > "$radio_atlas_drag_released_path" 2>&1
sleep 1.5
$ipc call radio status > "$radio_atlas_drag_settled_path" 2>&1
# The same drag let go while still moving: it coasts, carrying only its
# own speed.
"$python3_bin" "$radio_atlas_vpointer" down $radio_atlas_steps up
$ipc call radio status > "$radio_atlas_fling_released_path" 2>&1
sleep 2.5
$ipc call radio status > "$radio_atlas_fling_settled_path" 2>&1
"$hyprctl_bin" -j layers > "$radio_atlas_tip_before_path" 2>&1
"$wlrctl_bin" pointer move -4000 -4000
sleep 0.5
"$wlrctl_bin" pointer move \$(( ox + cw - 16 )) \$(( oy + 16 ))
sleep 2
"$hyprctl_bin" -j layers > "$radio_atlas_tip_after_path" 2>&1
"$grim_bin" -c "$radio_atlas_tip_png" > /dev/null 2>&1
"$wtype_bin" -k Escape
sleep 2
$ipc call panel state > "$radio_atlas_closed_path" 2>&1
$ipc call radio stop > /dev/null 2>&1
# Max zoom over the station, on the tiles and then on the single picture.
pid=\$(cat "$shot_dir/shell.pid")
rss() { awk '/^VmRSS/{print \$2}' "/proc/\$pid/status"; }
echo "closed_before \$(rss)" > "$radio_atlas_rss_path"
$ipc call panel open radio > /dev/null 2>&1
sleep 3
"$wtype_bin" -k Return
sleep 5
"$wlrctl_bin" pointer move -4000 -4000
sleep 0.3
"$wlrctl_bin" pointer move \$gx \$gy
sleep 0.5
for _ in \$(seq 22); do "$wlrctl_bin" pointer scroll -15 0; sleep 0.05; done
SECONDS=0
while [ "\$SECONDS" -lt 20 ]; do
  sleep 1
  $ipc call radio status > "$radio_atlas_zoom_tiles_path" 2>&1
  "$jq_bin" -e '.atlas.globe.scale >= 23.9 and .atlas.globe.earth.level == 21600 and .atlas.globe.earth.missing == 0' "$radio_atlas_zoom_tiles_path" > /dev/null 2>&1 && break
done
sleep 1
"$grim_bin" "$radio_atlas_zoom_tiles_png" > /dev/null 2>&1
echo "open_zoomed \$(rss)" >> "$radio_atlas_rss_path"
"$wtype_bin" -k Escape
sleep 3
echo "closed_after \$(rss)" >> "$radio_atlas_rss_path"
rm -f "$radio_atlas_tiles_link"
$ipc call panel open radio > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  sleep 1
  $ipc call radio status > "$radio_atlas_zoom_single_path" 2>&1
  "$jq_bin" -e '.atlas.globe.earth.level == 4096' "$radio_atlas_zoom_single_path" > /dev/null 2>&1 && break
done
sleep 1
"$grim_bin" "$radio_atlas_zoom_single_png" > /dev/null 2>&1
# A drag at max zoom on each picture, for the frame times: the single one
# here, the tiles once they are back.
"$python3_bin" "$radio_atlas_vpointer" down $radio_atlas_steps up
sleep 2
"$wtype_bin" -k Escape
sleep 2
ln -sfn "\$(readlink "$radio_atlas_tiles_link.saved")" "$radio_atlas_tiles_link"
$ipc call panel open radio > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 15 ]; do
  sleep 1
  $ipc call radio status > "$shot_dir/radio-atlas-zoom-again.json" 2>&1
  "$jq_bin" -e '.atlas.globe.earth.level == 21600 and .atlas.globe.earth.missing == 0' "$shot_dir/radio-atlas-zoom-again.json" > /dev/null 2>&1 && break
done
"$python3_bin" "$radio_atlas_vpointer" down $radio_atlas_steps up
sleep 3
"$wtype_bin" -k Escape
sleep 1
$ipc call radio stop > /dev/null 2>&1
EOF
  add_cleanup "kill \$(cat '$radio_atlas_loop_pid_path' 2>/dev/null) 2>/dev/null || true"
  add_cleanup "pkill -f 'listen 1 http://127.0.0.1:$radio_atlas_port' 2>/dev/null || true"
  add_cleanup "pkill -f 'radio-atlas-drive.sh' 2>/dev/null || true"
  hypr_exec_once "bash $script"
}

leg_radio_atlas_assert() {
  grep -q 'formalshell:radio' "$radio_atlas_tip_before_path" \
    || fail "no formalshell:radio layer in the layer dump with the atlas open"
  grep -q 'formalshell:tooltip' "$radio_atlas_tip_before_path" \
    && fail "a tooltip layer was mapped before the pointer parked on a button"
  grep -q 'formalshell:tooltip' "$radio_atlas_tip_after_path" \
    || fail "no tooltip layer after the pointer parked on the close button"
  grep -q 'namespace = "formalshell:radio"' "docs/examples/hyprland/formalshell.lua" \
    || fail "the example Hyprland config has no layer rule for formalshell:radio"
  echo "SMOKE_RADIO_ATLAS_TOOLTIP $radio_atlas_tip_png"
  grep -q "^ok$" "$radio_atlas_open_path" 2>/dev/null \
    || fail "panel open radio did not answer ok, got: $(cat "$radio_atlas_open_path" 2>/dev/null)"
  grep -q "^radio$" "$radio_atlas_state_path" 2>/dev/null \
    || fail "panel state is not radio with the atlas open, got: $(cat "$radio_atlas_state_path" 2>/dev/null)"
  cat "$radio_atlas_played_path"; echo
  grep -q '"loaded":true' "$radio_atlas_played_path" && grep -qF "\"station\":\"$radio_atlas_name\"" "$radio_atlas_played_path" \
    || fail "Enter on the list did not play the loopback station"
  grep -q '"running":false' "$radio_atlas_stopped_path" \
    || fail "radio stop left the station running: $(cat "$radio_atlas_stopped_path")"
  cat "$radio_atlas_picked_path"; echo
  grep -q '"loaded":true' "$radio_atlas_picked_path" && grep -qF "\"station\":\"$radio_atlas_name\"" "$radio_atlas_picked_path" \
    || fail "a click on the globe's centre did not pick the station ($(cat "$radio_atlas_globe_path" 2>/dev/null))"
  [ -z "$(tr -d '[:space:]' < "$radio_atlas_closed_path" 2>/dev/null)" ] \
    || fail "Escape left a panel open: $(cat "$radio_atlas_closed_path")"
  for png in "$radio_atlas_open_png" "$radio_atlas_turned_png" "$radio_atlas_picked_png"; do
    [ -s "$png" ] || fail "no frame at $png"
  done
  local gl gt gr gb box diff
  read -r gl gt gr gb _ _ < "$radio_atlas_globe_path"
  box="$((gr - gl))x$((gb - gt))+$gl+$gt"
  $convert_bin "$radio_atlas_open_png" -crop "$box" +repage -strip "$shot_dir/radio-atlas-globe-open.png" > /dev/null 2>&1
  $convert_bin "$radio_atlas_turned_png" -crop "$box" +repage -strip "$shot_dir/radio-atlas-globe-turned.png" > /dev/null 2>&1
  diff=$($convert_bin "$shot_dir/radio-atlas-globe-open.png" "$shot_dir/radio-atlas-globe-turned.png" -compose difference -composite -colorspace gray -format '%[fx:mean*100]' info: 2>/dev/null)
  echo "globe box $box, mean difference after the turn: ${diff}%"
  "$jq_bin" -e '.atlas.globe.imagery == true' "$radio_atlas_imagery_path" > /dev/null 2>&1 \
    || fail "the globe had no satellite imagery 10s after opening: $(cat "$radio_atlas_imagery_path" 2>/dev/null)"
  local side half sat colors
  side=$(( (gr - gl) < (gb - gt) ? (gr - gl) : (gb - gt) ))
  half=$(( side / 4 ))
  $convert_bin "$radio_atlas_open_png" -crop "$((half * 2))x$((half * 2))+$(( (gl + gr) / 2 - half ))+$(( (gt + gb) / 2 - half ))" +repage \
    "$shot_dir/radio-atlas-disc.png" > /dev/null 2>&1
  sat=$($convert_bin "$shot_dir/radio-atlas-disc.png" -colorspace HSL -channel G -separate -format '%[fx:mean]' info: 2>/dev/null)
  colors=$($convert_bin "$shot_dir/radio-atlas-disc.png" -format '%k' info: 2>/dev/null)
  echo "disc square $((half * 2))px: mean saturation ${sat:-none}, ${colors:-0} colours"
  awk -v s="${sat:-0}" -v c="${colors:-0}" 'BEGIN { exit !(s > 0.2 && c > 3000) }' \
    || fail "the disc does not look like satellite imagery (saturation ${sat:-none}, ${colors:-0} colours; the flat globe reads 0.05 and ~450)"
  echo "SMOKE_RADIO_ATLAS_DISC $shot_dir/radio-atlas-disc.png"
  local disc_r lum
  disc_r=$(( side * 44 * 97 / 10000 ))
  $convert_bin "$radio_atlas_open_png" -crop "$((disc_r * 2))x$((disc_r * 2))+$(( (gl + gr) / 2 - disc_r ))+$(( (gt + gb) / 2 - disc_r ))" +repage \
    -colorspace gray "$shot_dir/radio-atlas-lum.png" > /dev/null 2>&1
  $convert_bin -size "$((disc_r * 2))x$((disc_r * 2))" xc:black -fill white -draw "circle $disc_r,$disc_r $disc_r,1" \
    "$shot_dir/radio-atlas-lum-mask.png" > /dev/null 2>&1
  lum=$($convert_bin "$shot_dir/radio-atlas-lum.png" "$shot_dir/radio-atlas-lum-mask.png" -compose multiply -composite -format '%[fx:mean]' info: 2>/dev/null)
  lum=$(awk -v l="${lum:-0}" -v m="$($convert_bin "$shot_dir/radio-atlas-lum-mask.png" -format '%[fx:mean]' info: 2>/dev/null)" 'BEGIN { printf "%.4f", (m > 0 ? l / m : 0) }')
  echo "disc mean luminance ${lum}"
  awk -v l="$lum" 'BEGIN { exit !(l > 0.18) }' \
    || fail "the disc reads dark: mean luminance ${lum}, where the untoned picture read 0.127 and the toned one 0.26"
  awk '
    function us(line, key) { if (!match(line, key "=[0-9]+")) return -1; return substr(line, RSTART + length(key) + 1, RLENGTH - length(key) - 1) + 0 }
    /^atlas globe build_us=.*imagery=true/ { b[++nb] = us($0, "build_us"); s[nb] = us($0, "sample_us"); seen = 1 }
    /^atlas globe build_us=.*imagery=true coarse=true/ { m[++nm] = us($0, "build_us") }
    seen && /^commit surface=radio / { r[++nr] = us($0, "render_us") }
    function sort(a, n,   i, j, t) { for (i = 2; i <= n; i++) { t = a[i]; for (j = i - 1; j > 0 && a[j] > t; j--) a[j + 1] = a[j]; a[j + 1] = t } }
    function q(a, n, p) { return n ? a[int((n - 1) * p) + 1] : -1 }
    END {
      sort(b, nb); sort(s, nb); sort(r, nr); sort(m, nm)
      printf "builds=%d build_us p50=%d p95=%d max=%d sample_us p50=%d p95=%d max=%d; commits=%d render_us p50=%d p95=%d max=%d\n", nb, q(b, nb, 0.5), q(b, nb, 0.95), q(b, nb, 1), q(s, nb, 0.5), q(s, nb, 0.95), q(s, nb, 1), nr, q(r, nr, 0.5), q(r, nr, 0.95), q(r, nr, 1)
      printf "moving builds=%d build_us p50=%d max=%d\n", nm, q(m, nm, 0.5), q(m, nm, 1)
    }' "$shell_log_path" | sed 's/^/SMOKE_RADIO_ATLAS_FRAME /'
  grep -E '^(atlas globe |commit surface=radio |event loop: )' "$shell_log_path" > "$shot_dir/radio-atlas-frames.log" || true
  echo "SMOKE_RADIO_ATLAS_FRAMES $shot_dir/radio-atlas-frames.log"
  awk -v d="${diff:-0}" 'BEGIN { exit !(d > 1.0) }' || fail "the globe did not turn to the station (mean difference ${diff}%)"
  local before released settled fling_released fling_settled coasting
  read -r before released settled fling_released fling_settled <<< "$(for f in "$radio_atlas_drag_before_path" "$radio_atlas_drag_released_path" \
    "$radio_atlas_drag_settled_path" "$radio_atlas_fling_released_path" "$radio_atlas_fling_settled_path"; do
    "$jq_bin" -r '.atlas.globe.longitude // "none"' "$f" 2>/dev/null || echo none
  done | tr '\n' ' ')"
  coasting=$("$jq_bin" -r '.atlas.globe.coasting' "$radio_atlas_drag_settled_path" 2>/dev/null)
  echo "drag longitudes: before $before, released $released, settled $settled; fling released $fling_released, settled $fling_settled"
  awk -v a="$before" -v b="$released" 'function d(x, y) { x = y - x; while (x > 180) x -= 360; while (x < -180) x += 360; return x < 0 ? -x : x }
    BEGIN { exit !(a != "none" && b != "none" && d(a, b) > 1) }' \
    || fail "the held drag did not turn the globe (longitude $before to $released)"
  [ "$released" = "$settled" ] && [ "$coasting" = "false" ] \
    || fail "a drag that stopped before its release coasted on (longitude $released to $settled, coasting $coasting)"
  awk -v a="$before" -v b="$released" -v c="$fling_released" -v e="$fling_settled" \
    'function d(x, y) { x = y - x; while (x > 180) x -= 360; while (x < -180) x += 360; return x < 0 ? -x : x }
    BEGIN { drag = d(a, b); coast = d(c, e); printf "drag turned %.2f deg, the moving release coasted %.2f deg after it\n", drag, coast
      exit !(coast > 0 && coast < drag * 2.5) }' \
    || fail "a release while moving coasted out of proportion to the drag (or not at all)"
  "$jq_bin" -c '.atlas.globe | {scale, earth}' "$radio_atlas_zoom_tiles_path" 2>/dev/null
  "$jq_bin" -e '.atlas.globe.scale >= 23.9 and .atlas.globe.earth.level == 21600 and .atlas.globe.earth.missing == 0' "$radio_atlas_zoom_tiles_path" > /dev/null 2>&1 \
    || fail "max zoom did not settle on the 21600 tiles: $(cat "$radio_atlas_zoom_tiles_path" 2>/dev/null)"
  "$jq_bin" -c '.atlas.globe | {scale, earth}' "$radio_atlas_zoom_single_path" 2>/dev/null
  "$jq_bin" -e '.atlas.globe.imagery == true and .atlas.globe.earth.level == 4096' "$radio_atlas_zoom_single_path" > /dev/null 2>&1 \
    || fail "without the tiles the atlas did not fall back to the 4096 picture: $(cat "$radio_atlas_zoom_single_path" 2>/dev/null)"
  local open_kb after_kb
  open_kb=$(awk '$1 == "open_zoomed" { print $2 }' "$radio_atlas_rss_path" 2>/dev/null)
  after_kb=$(awk '$1 == "closed_after" { print $2 }' "$radio_atlas_rss_path" 2>/dev/null)
  echo "VmRSS: $(tr '\n' ' ' < "$radio_atlas_rss_path" 2>/dev/null)(kB)"
  awk -v o="${open_kb:-0}" -v a="${after_kb:-0}" 'BEGIN { exit !(o > 0 && a > 0 && o - a >= 16384) }' \
    || fail "closing the zoomed atlas gave back $(( ${open_kb:-0} - ${after_kb:-0} )) kB, under 16 MB"
  local zx zy energy_tiles energy_single
  zx=$(( (gl + gr) / 2 - 250 ))
  zy=$(( (gt + gb) / 2 - 230 ))
  energy() {
    $convert_bin "$1" -crop "200x200+$zx+$zy" +repage -colorspace gray "$2" > /dev/null 2>&1
    $convert_bin "$2" \( +clone -blur 0x1.5 \) -compose difference -composite -format '%[fx:mean]' info: 2>/dev/null
  }
  energy_tiles=$(energy "$radio_atlas_zoom_tiles_png" "$shot_dir/radio-atlas-zoom-tiles-crop.png")
  energy_single=$(energy "$radio_atlas_zoom_single_png" "$shot_dir/radio-atlas-zoom-single-crop.png")
  echo "max zoom detail: tiles ${energy_tiles:-none}, single 4096 picture ${energy_single:-none}"
  awk -v t="${energy_tiles:-0}" -v s="${energy_single:-0}" 'BEGIN { exit !(t > 0 && s > 0 && t >= 2 * s) }' \
    || fail "the tiles at max zoom carry under twice the 4096 picture's detail (${energy_tiles:-none} vs ${energy_single:-none})"
  awk '
    function us(line, key) { if (!match(line, key "=[0-9]+")) return -1; return substr(line, RSTART + length(key) + 1, RLENGTH - length(key) - 1) + 0 }
    function sort(a, n,   i, j, t) { for (i = 2; i <= n; i++) { t = a[i]; for (j = i - 1; j > 0 && a[j] > t; j--) a[j + 1] = a[j]; a[j + 1] = t } }
    function q(a, n, p) { return n ? a[int((n - 1) * p) + 1] : -1 }
    # Commits count toward the level the build before them drew at.
    /^atlas globe build_us=.*imagery=true/ { cur = us($0, "level") }
    /^atlas globe build_us=.*imagery=true/ && cur == 21600 { tb[++nt] = us($0, "build_us"); ts[nt] = us($0, "sample_us"); if (/coarse=true/) tm++ }
    /^atlas globe build_us=.*imagery=true/ && cur == 4096 { sb[++ns] = us($0, "build_us"); ss[ns] = us($0, "sample_us"); if (/coarse=true/) sm++ }
    /^commit surface=radio / && cur == 21600 { tr[++ntr] = us($0, "render_us") }
    /^commit surface=radio / && cur == 4096 { sr[++nsr] = us($0, "render_us") }
    END {
      sort(tb, nt); sort(ts, nt); sort(tr, ntr); sort(sb, ns); sort(ss, ns); sort(sr, nsr)
      printf "max zoom tiles builds=%d (moving %d) build_us p50=%d p95=%d sample_us p50=%d p95=%d; commits=%d render_us p50=%d p95=%d\n", nt, tm, q(tb, nt, 0.5), q(tb, nt, 0.95), q(ts, nt, 0.5), q(ts, nt, 0.95), ntr, q(tr, ntr, 0.5), q(tr, ntr, 0.95)
      printf "max zoom single builds=%d (moving %d) build_us p50=%d p95=%d sample_us p50=%d p95=%d; commits=%d render_us p50=%d p95=%d\n", ns, sm, q(sb, ns, 0.5), q(sb, ns, 0.95), q(ss, ns, 0.5), q(ss, ns, 0.95), nsr, q(sr, nsr, 0.5), q(sr, nsr, 0.95)
    }' "$shell_log_path" | sed 's/^/SMOKE_RADIO_ATLAS_ZOOM_FRAME /'
  echo "SMOKE_RADIO_ATLAS_ZOOM_TILES $shot_dir/radio-atlas-zoom-tiles-crop.png"
  echo "SMOKE_RADIO_ATLAS_ZOOM_SINGLE $shot_dir/radio-atlas-zoom-single-crop.png"
  echo "SMOKE_RADIO_ATLAS_ZOOM_TILES_FRAME $radio_atlas_zoom_tiles_png"
  echo "SMOKE_RADIO_ATLAS_ZOOM_SINGLE_FRAME $radio_atlas_zoom_single_png"
  echo "SMOKE_RADIO_ATLAS_OPEN $radio_atlas_open_png"
  echo "SMOKE_RADIO_ATLAS_TURNED $radio_atlas_turned_png"
  echo "SMOKE_RADIO_ATLAS_PICKED $radio_atlas_picked_png"
}
