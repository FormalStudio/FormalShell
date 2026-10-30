# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --media-progress holds the media panel open over a real mpv and reads the
# progress fill it draws against the position it reports. The fill, the
# elapsed readout and `media status` all come off MediaService.position, so
# the pair says whether the panel is drawing what it already knows: a fill
# that stops following a position that keeps advancing is the stall this leg
# is here for, and it is invisible to IPC alone.
#
# Five phases, one session, the panel opened once at the start and never
# touched again: plain playback, a pause and a resume, a seek, a track change
# inside the one player, and a switch to a second player. Track A carries a
# seeded lyrics cache and the other two a miss marker, so the first three
# phases run with the lyrics pane's per-frame position clock up and the last
# two run without it.
#
# media.visualizer is pinned off in the fixture: cava on a silent source
# still moves a column or two, and no phase here has anything to say about
# the spectrum.
leg_media_progress_flag="--media-progress"
leg_media_progress_order=171
leg_media_progress_needs="mpv ffmpeg convert"

media_progress_track_a="$shot_dir/progress-track-a.flac"
media_progress_track_b="$shot_dir/progress-track-b.flac"
media_progress_track_c="$shot_dir/progress-track-c.flac"
media_progress_pid1_path="$shot_dir/progress-mpv1.pid"
media_progress_pid2_path="$shot_dir/progress-mpv2.pid"
media_progress_sock_path="$shot_dir/progress-mpv1.sock"
media_progress_lib_path="$shot_dir/progress-lib.sh"
media_progress_log_path="$shot_dir/progress-samples.log"

media_progress_title_a="Progress Track A"
media_progress_title_b="Progress Track B"
media_progress_title_c="Progress Track C"
media_progress_artist="FormalShell Progress Artist"

leg_media_progress_timing() {
  # The switch phase's last sample lands ~105s in.
  leg_timing 112 200 2
}

# Lyrics.cacheKey (shell/Lyrics/model.js) for one of these fixtures: artist,
# title and an empty album joined with spaces, lowercased, runs of
# non-alphanumerics collapsed to one "-", the edges stripped, the rounded
# duration appended.
media_progress_cache_key() {
  printf '%s %s ' "$media_progress_artist" "$1" \
    | tr '[:upper:]' '[:lower:]' \
    | sed -E 's/[^a-z0-9]+/-/g; s/^-+//; s/-+$//' \
    | sed 's/$/-400/'
}

leg_media_progress_fixture() {
  settings_fragment ', "media": {"visualizer": false}'
  local t
  for t in a b c; do
    local path title
    case "$t" in
      a) path="$media_progress_track_a"; title="$media_progress_title_a" ;;
      b) path="$media_progress_track_b"; title="$media_progress_title_b" ;;
      *) path="$media_progress_track_c"; title="$media_progress_title_c" ;;
    esac
    # Silent and long: nothing here listens, and mpv leaves the bus the
    # moment a track ends, so the file has to outlive the session.
    "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 400 \
      -metadata "title=$title" -metadata "artist=$media_progress_artist" \
      -c:a flac -y "$path"
  done

  # Track A gets a synced cache file and the other two an empty miss marker,
  # written before the session so no key is ever live with no file behind it
  # (the same ordering lyrics.sh explains at length). Track A therefore runs
  # the panel's per-frame position clock and the rest of the session runs
  # without it, so one run covers both.
  local cache_dir="$iso_home/.cache/formalshell/lyrics"
  mkdir -p "$cache_dir"
  cat > "$cache_dir/$(media_progress_cache_key "$media_progress_title_a").json" <<'JSON'
{"source": "lrclib", "lines": [
{"time": 0.0, "end": 200.0, "text": "Held open and counting", "parent": null, "background": false, "oppositeTurn": false, "estimated": false, "words": []},
{"time": 200.0, "end": 100000.0, "text": "Still counting", "parent": null, "background": false, "oppositeTurn": false, "estimated": false, "words": []}
]}
JSON
  : > "$cache_dir/$(media_progress_cache_key "$media_progress_title_b").miss"
  : > "$cache_dir/$(media_progress_cache_key "$media_progress_title_c").miss"

  write_script "$media_progress_lib_path" <<EOF
#!/usr/bin/env bash
# One sample: the position binding as IPC reports it, and the frame.
progress_sample() {
  local name="\$1"
  $ipc call media status > "$shot_dir/progress-\$name.json" 2>&1
  $ipc call media lyrics > "$shot_dir/progress-\$name-lyrics.json" 2>&1
  "$grim_bin" "$shot_dir/progress-\$name.png" > /dev/null 2>&1
}

# mpv's own JSON IPC, for the two things MPRIS gives no verb for here: an
# absolute seek and loading another file into the same player.
progress_mpv() {
  python3 - "$media_progress_sock_path" "\$1" <<'PY'
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n")
s.close()
PY
}
EOF
}

leg_media_progress_drive() {
  local play_script="$shot_dir/progress-play.sh"
  local watch_script="$shot_dir/progress-watch.sh"
  local kill_script="$shot_dir/progress-kill.sh"

  write_script "$play_script" <<EOF
#!/usr/bin/env bash
sleep 2
echo \$\$ > "$media_progress_pid1_path"
exec "$mpv_bin" --no-video --really-quiet \\
  --input-ipc-server="$media_progress_sock_path" "$media_progress_track_a"
EOF

  write_script "$watch_script" <<EOF
#!/usr/bin/env bash
. "$media_progress_lib_path"

sleep 7
# The bare desktop, for the pixel diff that finds the panel's own rect.
"$grim_bin" "$shot_dir/progress-bare.png" > /dev/null 2>&1
sleep 1
$ipc call panel open media > /dev/null 2>&1

# Plain playback, the panel untouched from here on.
sleep 2;  progress_sample play-1
sleep 5;  progress_sample play-2
sleep 5;  progress_sample play-3
sleep 5;  progress_sample play-4
sleep 5;  progress_sample play-5

# Pause and resume.
$ipc call media playPause > /dev/null 2>&1
sleep 3;  progress_sample paused-1
$ipc call media playPause > /dev/null 2>&1
sleep 3;  progress_sample resume-1
sleep 5;  progress_sample resume-2
sleep 5;  progress_sample resume-3

# A seek well past where playback had reached.
progress_mpv '{"command":["seek",200,"absolute"]}'
sleep 3;  progress_sample seek-1
sleep 5;  progress_sample seek-2
sleep 5;  progress_sample seek-3

# A track change inside the one player.
progress_mpv '{"command":["loadfile","$media_progress_track_b"]}'
sleep 4;  progress_sample track-1
sleep 5;  progress_sample track-2
sleep 5;  progress_sample track-3

# A second player, made active by hand.
"$mpv_bin" --no-video --really-quiet "$media_progress_track_c" &
echo \$! > "$media_progress_pid2_path"
sleep 4
$ipc call media players > "$shot_dir/progress-players.json" 2>&1
active=\$($ipc call media status 2>&1 | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
other=\$(grep -o '"id":"[^"]*"' "$shot_dir/progress-players.json" | cut -d'"' -f4 | grep -v "^\$active\$" | head -1)
$ipc call media select "\$other" > /dev/null 2>&1
sleep 3;  progress_sample switch-1
sleep 5;  progress_sample switch-2
sleep 5;  progress_sample switch-3
EOF

  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
for pidfile in "$media_progress_pid1_path" "$media_progress_pid2_path"; do
  [ -f "\$pidfile" ] && kill "\$(cat "\$pidfile")" 2>/dev/null
done
true
EOF
  add_cleanup "bash $kill_script"

  hypr_exec_once "bash $play_script"
  hypr_exec_once "bash $watch_script"
}

media_progress_field() {
  grep -o "\"$2\":[0-9.eE+-]*" "$shot_dir/progress-$1.json" 2>/dev/null | head -1 | cut -d: -f2
}

# One frame with the bar cropped off it. The bar's clock ticks between any
# two frames, and the rect below is a diff of two.
media_progress_body() {
  local h
  h=$($convert_bin "$shot_dir/progress-$1.png" -format '%h' info: 2>/dev/null)
  $convert_bin "$shot_dir/progress-$1.png" -crop "1920x$((h - 60))+0+60" +repage \
    "$shot_dir/progress-body-$1.png" > /dev/null 2>&1
  printf '%s' "$shot_dir/progress-body-$1.png"
}

# The panel's own rect, off a pixel diff between the bare desktop and a
# frame with the panel open, so nothing below reads a hardcoded screen
# coordinate.
media_progress_rect() {
  local bare open
  bare=$(media_progress_body bare)
  open=$(media_progress_body "$1")
  $convert_bin "$bare" "$open" -compose difference -composite \
    -colorspace Gray -threshold 8% -format '%@' info: 2>/dev/null
}

# The progress track's fill, as a fraction of its own groove, read off one
# frame. The track finds itself: every row of the panel is scanned for its
# longest unbroken run of groove-or-fill pixels, and the longest run in the
# panel is the position track, since it is the widest thing the panel draws
# in the primary hue. Taking the run rather than the row keeps the elapsed
# and total readouts either side of it out of the count. The fill is the
# far end from the card's own shade and the groove a step off it (darker
# than the card under the shipped table, lighter under others).
media_progress_fill() {
  local body rect
  body=$(media_progress_body "$1")
  rect="$2"
  $convert_bin "$body" -crop "$rect" +repage -depth 8 txt:- 2>/dev/null \
    | python3 -c '
import re, sys
rows = {}
for line in sys.stdin:
    m = re.match(r"(\d+),(\d+): \((\d+),(\d+),(\d+)", line)
    if not m:
        continue
    rows.setdefault(int(m.group(2)), {})[int(m.group(1))] = int(m.group(3))
# The card is the commonest shade; the fill stands far off it and the
# groove a little off it, darker or lighter as the table has it.
counts = {}
for row in rows.values():
    for r in row.values():
        counts[r] = counts.get(r, 0) + 1
card = max(counts, key=counts.get) if counts else 0
for row in rows.values():
    for x, r in row.items():
        d = abs(r - card)
        row[x] = 2 if d > 100 else (1 if d > 5 else 0)
# The track is a bar three rows deep or more; a separator or the lyrics
# pane border is one pixel.
segs = {}
for y, row in rows.items():
    out, seg = [], None
    for x in sorted(row):
        if row[x] and seg and x == seg[1] + 1:
            seg[1], seg[2] = x, seg[2] + (row[x] == 2)
            continue
        if seg:
            out.append(seg)
        seg = [x, x, int(row[x] == 2)] if row[x] else None
    if seg:
        out.append(seg)
    segs[y] = out
def under(y, start, end):
    return any(abs(s - start) <= 2 and abs(e - end) <= 2 for s, e, _ in segs.get(y, []))
best = (0, 0)
for y, out in segs.items():
    for start, end, fill in out:
        if end - start + 1 > best[0] and under(y + 1, start, end) and under(y + 2, start, end):
            best = (end - start + 1, fill)
print("%.4f %d" % (best[1] / best[0] if best[0] else 0.0, best[0]))
'
}

leg_media_progress_assert() {
  local rect name
  [ -s "$shot_dir/progress-bare.png" ] || fail "no bare frame produced"
  rect=$(media_progress_rect play-5)
  case "$rect" in
    [1-9]*x[1-9]*+*+*) ;;
    *) fail "could not find the media panel's rect against the bare desktop, got: $rect" ;;
  esac
  echo "media panel rect: $rect"

  # The claim: the fill the panel draws is the position the panel reports,
  # at every sample, with the panel never touched after it was opened. The
  # tolerance is a fiftieth of the track, wider than the groove's own
  # rounded caps and narrower than anything a stalled fill could hide in.
  local failures=""
  : > "$media_progress_log_path"
  for name in play-1 play-2 play-3 play-4 play-5 \
              paused-1 resume-1 resume-2 resume-3 \
              seek-1 seek-2 seek-3 \
              track-1 track-2 track-3 \
              switch-1 switch-2 switch-3; do
    local pos len title lyr drawn width want
    pos=$(media_progress_field "$name" position)
    len=$(media_progress_field "$name" length)
    title=$(grep -o '"title":"[^"]*"' "$shot_dir/progress-$name.json" 2>/dev/null | head -1 | cut -d'"' -f4)
    lyr=$(grep -o '"state":"[^"]*"' "$shot_dir/progress-$name-lyrics.json" 2>/dev/null | head -1 | cut -d'"' -f4)
    # The panel narrows once lyrics go, so each frame is read in its own rect.
    read -r drawn width < <(media_progress_fill "$name" "$(media_progress_rect "$name")")
    want=$(awk -v p="${pos:-0}" -v l="${len:-0}" 'BEGIN { printf "%.4f", (l > 0 ? p / l : 0) }')
    printf '%-10s position=%-9s want=%-8s drawn=%-8s track=%-5s lyrics=%-8s title=%s\n' \
      "$name" "${pos:-none}" "$want" "$drawn" "$width" "${lyr:-none}" "${title:-none}" \
      | tee -a "$media_progress_log_path"
    if [ -z "$pos" ] || [ "$width" -lt 100 ]; then
      failures="$failures $name(no track read)"
    elif awk -v w="$want" -v d="$drawn" 'BEGIN { exit !((w > d ? w - d : d - w) > 0.02) }'; then
      failures="$failures $name(want $want drawn $drawn)"
    fi
  done
  echo "SMOKE_MEDIA_PROGRESS_LOG $media_progress_log_path"
  echo "SMOKE_MEDIA_PROGRESS_SEEK $shot_dir/progress-seek-1.png"
  [ -z "$failures" ] || fail "the drawn progress does not follow the reported position at:$failures"
}
