# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --visualizer-styles: pink noise rather than one sine, so every
# cava band actually differs and a style that shapes the spectrum
# (butterfly, mirror, terrain) has something to shape. The media panel opens
# once and every id `visualizer styles` reports is set over IPC in turn, one
# crop of the spectrum box per style saved to
# visualizer-style-<id>.png. The crop rect itself is never hardcoded: it
# comes from two pairwise pixel diffs of the running spectrum a few frames
# apart (the same trimmed-bounding-box idiom media_progress.sh's own rect
# finder uses), intersected so a stray once-a-second tick of the elapsed
# readout in one of the two diffs can't widen the box.
#
# The second half is the AGC settling claim (f4e5885): with
# the player volume forced to 100, `visualizer status`'s levels (exposed
# there for this leg) must not all sit above 0.9 once cava's had a couple of
# seconds to converge. Before that fix a loud track pegged every band.
leg_visualizer_styles_flag="--visualizer-styles"
leg_visualizer_styles_order=222
leg_visualizer_styles_claims="visualizer.style media.player=shared panel=shared"
leg_visualizer_styles_needs="mpv ffmpeg convert"

visualizer_styles_track_path="$shot_dir/visualizer-styles-tone.flac"
visualizer_styles_pid_path="$shot_dir/visualizer-styles-mpv.pid"
visualizer_styles_status_path="$shot_dir/visualizer-styles-status.json"
visualizer_styles_ids_path="$shot_dir/visualizer-styles-ids.txt"
visualizer_styles_rect_path="$shot_dir/visualizer-styles-rect.txt"
visualizer_styles_loud_status_path="$shot_dir/visualizer-styles-loud-status.json"
visualizer_styles_track_title="FormalShell Visualizer Styles Smoke Tone"
visualizer_styles_track_artist="FormalShell Test Artist"

leg_visualizer_styles_fixture() {
  # Pink noise, not a sine: a single frequency only ever lights one or two
  # of the twelve columns, and half these styles (butterfly, terrain,
  # mirror) have nothing to show without a spread across the bands.
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi \
    -i "anoisesrc=color=pink:amplitude=0.7:sample_rate=48000:duration=70" \
    -ac 2 -t 70 \
    -metadata "title=$visualizer_styles_track_title" -metadata "artist=$visualizer_styles_track_artist" \
    -c:a flac -y "$visualizer_styles_track_path"
}

leg_visualizer_styles_timing() {
  # 2s pre-roll, up to 8s for the tag-match poll, 2s panel-open settle, ~1s
  # for the three rect-finding frames, ~31 styles at ~1s each including the
  # IPC round trip and the crop, then the volume-100 phase (~4s): worst case
  # lands ~52s in (sand's own dwell included).
  leg_timing 110
}

leg_visualizer_styles_drive() {
  local script="$shot_dir/visualizer-styles-drive.sh"
  local kill_script="$shot_dir/visualizer-styles-kill.sh"

  write_script "$script" <<EOF
#!/usr/bin/env bash
set -u
"$mpv_bin" --no-video --really-quiet "$visualizer_styles_track_path" &
mpv_pid=\$!
echo "\$mpv_pid" > "$visualizer_styles_pid_path"

until_jq 8 "$visualizer_styles_status_path" '.title == "$visualizer_styles_track_title"' call media status

$ipc call panel open media > /dev/null 2>&1
# The running spectrum keeps the card moving, so it never reads at rest.
sleep 2

# Three frames of the running spectrum, close enough together that the
# elapsed readout below it can tick over at most once across the pair it
# lands in, never both.
f0="$shot_dir/visualizer-styles-rect-0.png"
f1="$shot_dir/visualizer-styles-rect-1.png"
f2="$shot_dir/visualizer-styles-rect-2.png"
"$grim_bin" "\$f0" > /dev/null 2>&1
sleep 0.2
"$grim_bin" "\$f1" > /dev/null 2>&1
sleep 0.2
"$grim_bin" "\$f2" > /dev/null 2>&1

# The bar's own nowPlaying marquee scrolls this title continuously, which
# would otherwise widen the diff below to span the whole screen height.
# Leaving the 60px bar out (the same constant media_progress.sh's own rect
# finder uses) keeps the diff to the panel alone.
bar=\$(logical_px 60)
rect_a=\$(card_box "\$f0" "\$f1" "\$bar" 8%)
rect_b=\$(card_box "\$f1" "\$f2" "\$bar" 8%)

IFS='x+' read -r wa ha xa ya <<< "\$rect_a"
IFS='x+' read -r wb hb xb yb <<< "\$rect_b"
if [ -n "\${wa:-}" ] && [ -n "\${wb:-}" ]; then
  x1=\$(( xa > xb ? xa : xb ))
  y1=\$(( ya > yb ? ya : yb ))
  x2a=\$(( xa + wa )); x2b=\$(( xb + wb ))
  y2a=\$(( ya + ha )); y2b=\$(( yb + hb ))
  x2=\$(( x2a < x2b ? x2a : x2b ))
  y2=\$(( y2a < y2b ? y2a : y2b ))
  if [ "\$x2" -gt "\$x1" ] && [ "\$y2" -gt "\$y1" ]; then
    pad=6
    x1=\$(( x1 - pad > 0 ? x1 - pad : 0 ))
    y1=\$(( y1 - pad > bar ? y1 - pad : bar ))
    w=\$(( x2 - x1 + pad * 2 ))
    h=\$(( y2 - y1 + pad * 2 ))
    printf '%sx%s+%s+%s' "\$w" "\$h" "\$x1" "\$y1" > "$visualizer_styles_rect_path"
  fi
fi

$ipc call visualizer styles > "$visualizer_styles_ids_path" 2>&1

rect=\$(cat "$visualizer_styles_rect_path" 2>/dev/null)
prev=""
while IFS= read -r id; do
  [ -z "\$id" ] && continue
  $ipc call visualizer style "\$id" > /dev/null 2>&1
  # sand starts from an empty bed and needs a few seconds to pile up.
  case "\$id" in
    sand) sleep 4 ;;
    *) sleep 0.5 ;;
  esac
  frame="$shot_dir/visualizer-styles-frame-\$id.png"
  "$grim_bin" "\$frame" > /dev/null 2>&1
  crop="$shot_dir/visualizer-style-\$id.png"
  if [ -n "\$rect" ]; then
    $convert_bin "\$frame" -crop "\$rect" +repage "\$crop" > /dev/null 2>&1
  fi
  rm -f "\$frame"
  prev="\$crop"
done < "$visualizer_styles_ids_path"

# The AGC settling claim (f4e5885): a loud track, player volume
# forced to 100, must not peg every band near the ceiling.
$ipc call media volume 100 > /dev/null 2>&1
sleep 3
$ipc call visualizer status > "$visualizer_styles_loud_status_path" 2>&1

kill "\$mpv_pid" 2>/dev/null || true
wait "\$mpv_pid" 2>/dev/null || true
EOF

  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
if [ -f "$visualizer_styles_pid_path" ]; then
  kill "\$(cat "$visualizer_styles_pid_path")" 2>/dev/null || true
fi
EOF
  add_cleanup "bash $kill_script"
  wm_exec_once "bash $script"
}

leg_visualizer_styles_assert() {
  "$jq_bin" -e --arg t "$visualizer_styles_track_title" '.title == $t' "$visualizer_styles_status_path" > /dev/null 2>&1 \
    || fail "media status never showed the visualizer styles smoke tone, got: $(cat "$visualizer_styles_status_path" 2>/dev/null)"
  if [ ! -s "$visualizer_styles_rect_path" ]; then
    fail "could not find the spectrum box's rect from two frames of the running spectrum"
  fi
  local rect
  rect=$(cat "$visualizer_styles_rect_path")
  case "$rect" in
    [1-9]*x[1-9]*+*+*) ;;
    *) fail "spectrum box rect looks wrong: $rect" ;;
  esac
  echo "spectrum box rect: $rect"

  if [ ! -s "$visualizer_styles_ids_path" ]; then
    fail "visualizer styles returned no ids"
  fi

  local id crop prev_crop colors
  prev_crop=""
  while IFS= read -r id; do
    [ -z "$id" ] && continue
    crop="$shot_dir/visualizer-style-$id.png"
    if [ ! -s "$crop" ]; then
      fail "no crop produced for style $id"
    fi
    colors=$($convert_bin "$crop" -format '%k' info: 2>/dev/null)
    if [ -z "$colors" ] || [ "$colors" -le 1 ]; then
      fail "crop for style $id is empty ($colors color(s)): $crop"
    fi
    if [ -n "$prev_crop" ] && cmp -s "$prev_crop" "$crop"; then
      fail "style $id's crop is byte-identical to the previous style's, $crop vs $prev_crop"
    fi
    echo "SMOKE_VISUALIZER_STYLE_$(printf '%s' "$id" | tr '[:lower:]' '[:upper:]') $crop"
    prev_crop="$crop"
  done < "$visualizer_styles_ids_path"

  if [ ! -s "$visualizer_styles_loud_status_path" ]; then
    fail "visualizer status never answered for the loud-track check"
  fi
  local levels_summary rc
  levels_summary=$(python3 -c '
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception as e:
    print("parse error: %s" % e)
    sys.exit(2)
levels = d.get("levels") or []
if not levels:
    print("no levels in status: %s" % d)
    sys.exit(2)
left = d.get("levelsLeft") or []
right = d.get("levelsRight") or []
if len(left) != len(levels) or len(right) != len(levels) or not any(left) or not any(right):
    print("stereo levels missing or silent: left=%s right=%s" % (left, right))
    sys.exit(2)
print(",".join("%.3f" % v for v in levels))
sys.exit(1 if all(v > 0.9 for v in levels) else 0)
' "$visualizer_styles_loud_status_path")
  rc=$?
  if [ "$rc" -eq 2 ]; then
    fail "visualizer status at volume 100 gave no usable levels: $levels_summary"
  fi
  if [ "$rc" -eq 1 ]; then
    fail "every band is above 0.9 at volume 100, the AGC is not giving a loud track headroom: $levels_summary"
  fi
  echo "SMOKE_VISUALIZER_LOUD_LEVELS $visualizer_styles_loud_status_path"
  echo "loud-track levels: $levels_summary"
}
