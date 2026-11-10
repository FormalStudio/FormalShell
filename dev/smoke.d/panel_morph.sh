# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --panel-morph proves a card whose content changes size travels to the new
# one instead of jumping to it (any panel that
# suddenly resizes, like wifi or now playing). Three cases, all at `debug
# motionScale 1000` so a screencopy lands inside the morph rather than after
# it, and all read off the card's own far edge:
#
#   settled   the Wi-Fi card is open and standing still when the radio goes
#             off and its list collapses. A sample has to catch the far edge
#             inside the middle half of the two resting heights.
#   emerging  the same change issued two seconds into a five-second emerge,
#             while the card is still coming out of the bar's line. This is
#             the one that needs the morph armed from the surface being
#             mapped rather than from its enter having settled: a clock that
#             is off for the whole travel lets the change through in one
#             frame, and since the emerge's own displacement is a function of
#             the card's height, the far edge jumps mid-travel rather than
#             retargeting.
#   media     and the now-playing card growing from "No player" to a real
#             MPRIS track, the other surface the owner named, which is the
#             same claim on a growth rather than a shrink.
#
# Everything is read off the rig's own pixels. Each frame is reduced to one
# column of per-row coverage against the closed output (`panel_morph_rows`):
# 0 is bare desktop, 255 a row painted right across the band. The band takes
# the widest of the three cards with a little desktop either side, so a row
# the narrowest of them covers still reads well over `panel_morph_body`, and
# the far edge is the last row above it. The corner arcs fall off from that
# over the last `radiusXl` rows, so what counts as painted "below the card"
# is `panel_morph_skirt` rows past the flat bottom.
#
# The tolerance on the far edge is neither slack nor a guess: the spatial
# curves overshoot their rest by design and the deform stretches the
# card along its travel on top of that, so the edge legitimately
# passes a rest by a few pixels. What it may not do is land on a size nobody
# is travelling to, which is tens of pixels away in every case here.
#
# The band is read as a top bar's, the card's height down the rows from the
# edge it rests against. A bottom bar's frames are flipped. On a side bar the
# card hangs off the line at the line's far end, the output's bottom, and its
# height runs along the line: the frames are turned to put that end on top
# and the line on the right (flipped for a right bar, turned half round for a
# left one), and the band is the 512 columns inside the line. Where the band
# starts and ends come off `debug dump` taken with the network card at rest
# (`theme.edgeInset`, the panel's `rest` rect).
leg_panel_morph_flag="--panel-morph"
leg_panel_morph_order=76
leg_panel_morph_vm_only="it turns the Wi-Fi radio off and on through NetworkManager, with sudo nmcli in its fixture"
leg_panel_morph_needs="convert mpv ffmpeg"
leg_panel_morph_claims="screen panel launcher wifi.radios media.player bar.layout=default"

panel_morph_bare_path="$shot_dir/panel-morph-bare.png"
panel_morph_rest_full_path="$shot_dir/panel-morph-rest-full.png"
panel_morph_rest_off_path="$shot_dir/panel-morph-rest-off.png"
panel_morph_rest_end_path="$shot_dir/panel-morph-rest-end.png"
panel_morph_rest_quiet_path="$shot_dir/panel-morph-rest-quiet.png"
panel_morph_rest_playing_path="$shot_dir/panel-morph-rest-playing.png"
panel_morph_replies_path="$shot_dir/panel-morph-replies.txt"
panel_morph_scan_path="$shot_dir/panel-morph-scan.json"
panel_morph_track_path="$shot_dir/panel-morph-track.flac"
panel_morph_pid_path="$shot_dir/panel-morph-mpv.pid"
panel_morph_dump_path="$shot_dir/panel-morph-dump.json"
# Where the probes read the frames: the grabs themselves on a top bar, else
# each one turned (panel_morph_turn).
panel_morph_read="$shot_dir"

# The band the cards are read in: an IPC open with no cell rests one
# screenPadding (12) off the line's far end, and the widest card here is the
# media panel's `popupWidthWide` (480), so the band starts a little ahead of
# its leading side and runs a screenPadding past the network card's end. Its
# first row is the one under the bar's line. The rest is cut in the assert,
# off the dump and the turned frame's size.
panel_morph_band_w=512
# A row a card covers. The narrowest here is the network panel's
# `popupWidthDefault` (380), which is 189 of the band's 255, and the attached
# silhouette's fillets make it wider still; nothing else paints two thirds of
# the band under the bar.
panel_morph_body=170
# How far past a card's flat bottom its corner arcs reach, the one place a
# painted row narrower than the body legitimately sits (radiusXl 14).
panel_morph_skirt=16
# What counts as painted at all: a couple of columns out of the band.
panel_morph_ink=2
# And what counts as a pixel differing from the bare desktop. The card is a
# translucent fill over the rig's own near-black desktop and lifts it by about
# 5%, so a cut above that leaves the card's body reading as desktop and only
# its own 1px bottom border clearing `panel_morph_body`. A card resting on a
# fractional row splits that line across two pixel rows at half strength, the
# far edge then falls back to a row inside the card, and the card's own
# contents read as painted outside it: the rows a card covers have to be the
# thing being measured, not the one line at the bottom of it.
panel_morph_lift=2%
# The far edge may pass a rest by this much and no more; see the header.
panel_morph_overshoot=40
# And two rests have to differ by at least this much, or the action drove no
# size change and there is nothing to measure.
panel_morph_delta=40

panel_morph_settled_sleeps=(0.4 0.8 1.1 1.4 1.9 2.4 3.0 3.8 4.6)
# The emerging case's own burst, timed from the `panel open` rather than from
# the change: the first three land while the card is still travelling out on
# its original height and the rest after the radio goes off at two seconds,
# by which point the emerge has all but stopped and what the far edge does
# next is the morph's alone.
panel_morph_emerging_sleeps=(0.5 1.2 1.8 2.3 2.6 2.9 3.2 3.5 4.0 4.8 6.2)
panel_morph_emerging_change=3
panel_morph_media_sleeps=(0.3 0.6 0.9 1.2 1.6 2.0 2.5 3.0 3.8 4.8 6.0)

leg_panel_morph_timing() {
  # Both scan waits below break within a few seconds of the radio being on,
  # which the fixture below has already seen to; the ceilings are there so a
  # station that never scans fails the claim instead of eating the run's own
  # frame.
  leg_timing 235
}

leg_panel_morph_fixture() {
  # Before the session, because the shell enumerates the station's access
  # points when it starts: with the radio off at that moment the list stays
  # empty for the whole run however often it is asked, and turning it back on
  # from inside never repopulates it. The radio is NetworkManager's own
  # setting and outlives any one run, so this leg both finds it however the
  # last one left it and puts it back (the cleanup in the drive below).
  sudo nmcli radio wifi on > /dev/null 2>&1 || true

  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 300 \
    -metadata "title=FormalShell Morph Track" -metadata "artist=FormalShell Morph Artist" \
    -metadata "album=FormalShell Morph Album" -c:a flac -y "$panel_morph_track_path"
}

# The grim calls for one burst, armed before the call that starts the morph:
# grim's own start-up costs about as much as a travel does, so a capture asked
# for once the call has returned lands past the end of it (panel_handoff.sh's
# finding).
panel_morph_arm() {
  local name=$1 arm="" i n=0
  shift
  for i in "$@"; do
    n=$((n + 1))
    arm+="( sleep $i; \"$grim_bin\" \"$shot_dir/panel-morph-$name-$n.png\" > /dev/null 2>&1 ) &"$'\n'
  done
  printf '%s' "$arm"
}

leg_panel_morph_drive() {
  local script="$shot_dir/panel-morph-drive.sh" kill_script="$shot_dir/panel-morph-kill.sh"
  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
[ -f "$panel_morph_pid_path" ] && kill "\$(cat "$panel_morph_pid_path")" 2>/dev/null
# The radio goes back on however this run ended: it is NetworkManager's own
# setting, not part of the isolated HOME, and the next run needs a list.
$ipc call network wifi true > /dev/null 2>&1
true
EOF
  add_cleanup "bash $kill_script"

  write_script "$script" <<EOF
#!/usr/bin/env bash
logged() { $ipc call "\$@" >> "$panel_morph_replies_path" 2>&1; }
# Both of the rig's own hostapd SSIDs in the station's own scan list, which
# is what gives the card the two rows this leg measures the collapse of.
scanned() {
  local ceiling=\$1
  SECONDS=0
  while [ "\$SECONDS" -lt "\$ceiling" ]; do
    $ipc call network status > "$panel_morph_scan_path" 2>&1
    if "$jq_bin" -e '[.. | objects | .name? | strings] | any(. == "FORMALTEST") and any(. == "FORMALTEST-EAP")' "$panel_morph_scan_path" > /dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}
logged debug motionScale 1000
"$grim_bin" "$panel_morph_bare_path" > /dev/null 2>&1

# The radio is NetworkManager's own setting and lives outside the isolated
# HOME, so a run that ended while it was off leaves the next one with an
# empty list and nothing to measure. On, then wait for the station to see the
# rig's own APs again, which it does within a few seconds of the radio coming
# back. The cleanup below puts it back on whatever happens here.
logged network wifi true
scanned 20

# The Wi-Fi card at its full height, both networks listed.
logged panel open network
until_settled
"$grim_bin" "$panel_morph_rest_full_path" > /dev/null 2>&1
$ipc call debug dump > "$panel_morph_dump_path" 2>&1

$(panel_morph_arm settled "${panel_morph_settled_sleeps[@]}")
logged network wifi false
wait
sleep 10
"$grim_bin" "$panel_morph_rest_off_path" > /dev/null 2>&1

# And the same change landing while the card is still on its way out.
logged panel close
sleep 8
logged network wifi true
scanned 15
sleep 3

$(panel_morph_arm emerging "${panel_morph_emerging_sleeps[@]}")
logged panel open network
# Two seconds into the five-second emerge, on purpose.
sleep 2
logged network wifi false
wait
sleep 12
"$grim_bin" "$panel_morph_rest_end_path" > /dev/null 2>&1

# The now-playing card, growing from its honest empty state to a real track.
logged panel close
sleep 8
logged panel open media
until_settled
"$grim_bin" "$panel_morph_rest_quiet_path" > /dev/null 2>&1

$(panel_morph_arm media "${panel_morph_media_sleeps[@]}")
"$mpv_bin" --no-video --really-quiet "$panel_morph_track_path" &
echo \$! > "$panel_morph_pid_path"
# No `wait` here: the player is a background job of this script too, and it
# outlives the burst by five minutes.
sleep 14
"$grim_bin" "$panel_morph_rest_playing_path" > /dev/null 2>&1
logged network wifi true
logged debug motionScale 100
EOF
  hypr_exec_once "bash $script"
}

# One frame as per-row coverage over the closed output, top of the band down:
# the band cropped out of the difference, everything that differs at all made
# white, then averaged to a single column, so each row's value is the share of
# the band's own width that frame paints there.
panel_morph_rows() {
  $convert_bin "$panel_morph_read/${1##*/}" "$panel_morph_read/${panel_morph_bare_path##*/}" -compose difference -composite \
    -colorspace Gray \
    -crop "${panel_morph_band_w}x${panel_morph_band_h}+${panel_morph_band_x}+${panel_morph_band_y}" +repage \
    -threshold "$panel_morph_lift" -scale "1x${panel_morph_band_h}!" -depth 8 txt:- 2>/dev/null \
    | awk 'NR > 1 { v = $2; gsub(/[()]/, "", v); split(v, c, ","); print c[1] + 0 }'
}

# The card's far edge in frame $1, as a row of the band: the last one the card
# covers right across. Empty when the card is not in the frame at all.
panel_morph_far_edge() {
  panel_morph_rows "$1" | awk -v body="$panel_morph_body" \
    '$1 >= body { last = NR - 1 } END { if (last != "") print last }'
}

# Every row of frame $1 painted more than a skirt below that far edge, as
# "<row> <coverage>" lines: content the card's own cut should have held.
panel_morph_below() {
  panel_morph_rows "$1" | awk -v edge="$2" -v skirt="$panel_morph_skirt" -v ink="$panel_morph_ink" \
    'NR - 1 > edge + skirt && $1 > ink { print NR - 1, $1 }'
}

# The rest a case starts at and the rest it ends at, and every sample's far
# edge in between: one claim, made three times. `$1` names the case, `$2` and
# `$3` are the two rests in either order, `$4` is how many of the samples were
# taken before the change (none, for a case whose change is its first act),
# and `$5` is how many there are.
panel_morph_claim() {
  local name=$1 from=$2 to=$3 before=$4 count=$5
  local low=$from high=$to i path edge travelled="" seen="" below margin
  if [ "$low" -gt "$high" ]; then low=$to; high=$from; fi
  if [ $((high - low)) -lt "$panel_morph_delta" ]; then
    fail "the $name case moved the card's far edge from row $from to row $to, under the $panel_morph_delta rows this leg needs to measure a travel: the card drew the same size either way (the station's own scan list: $(cat "$panel_morph_scan_path" 2>/dev/null))"
  fi
  for ((i = 1; i <= count; i++)); do
    path="$shot_dir/panel-morph-$name-$i.png"
    [ -f "$path" ] || fail "no $name sample at $path"
    echo "SMOKE_PANEL_MORPH_$(echo "$name" | tr 'a-z' 'A-Z')_$i $path"
    edge=$(panel_morph_far_edge "$path")
    [ -n "$edge" ] || continue
    seen="$seen $i:$edge"
    if [ "$edge" -gt $((high + panel_morph_overshoot)) ]; then
      fail "$path puts the card's far edge at row $edge, past the deeper rest's own row $high by more than the $panel_morph_overshoot rows the overshoot and the deform account for"
    fi
    below=$(panel_morph_below "$path" "$edge")
    [ -z "$below" ] || fail \
      "$path paints below the card's own far edge at row $edge (row/coverage: $(echo "$below" | tr '\n' ';')): the contents are drawn outside the card while it travels"
    [ "$i" -gt "$before" ] || continue
    # A quarter of the change in from either rest, so that a card which
    # crossed the whole distance inside one frame and is sitting on its new
    # size, a few pixels off it because the emerge is still unwinding, cannot
    # pass for one that travelled.
    margin=$(((high - low) / 4))
    if [ "$edge" -gt $((low + margin)) ] && [ "$edge" -lt $((high - margin)) ]; then
      travelled="$travelled $i:$edge"
    fi
  done
  [ -n "$seen" ] || fail "no card in any $name sample: the panel never opened"
  [ -n "$travelled" ] || fail \
    "no $name sample past the change caught the card inside the middle half of the two rests (rows $low and $high), far edges seen:$seen: the card jumped from one size to the other"
  echo "SMOKE_PANEL_MORPH_$(echo "$name" | tr 'a-z' 'A-Z') ok$travelled between rows $low and $high (all:$seen)"
}

# Every frame of the leg turned for the bar's edge $1 (see the header), into
# a directory of its own.
panel_morph_turn() {
  local turn src
  case $1 in
    bottom | right) turn="-flip" ;;
    left) turn="-rotate 180" ;;
  esac
  panel_morph_read="$shot_dir/panel-morph-read"
  mkdir -p "$panel_morph_read"
  for src in "$shot_dir"/panel-morph-*.png; do
    # shellcheck disable=SC2086  # the turn is one or two words
    $convert_bin "$src" $turn +repage "$panel_morph_read/${src##*/}" || fail "could not turn $src for the $1 edge"
  done
}

leg_panel_morph_assert() {
  local path rest_full rest_off rest_end rest_quiet rest_playing edge inset end far w h

  for path in "$panel_morph_bare_path" "$panel_morph_rest_full_path" \
    "$panel_morph_rest_off_path" "$panel_morph_rest_end_path" \
    "$panel_morph_rest_quiet_path" "$panel_morph_rest_playing_path"; do
    [ -f "$path" ] || fail "no screenshot produced at $path"
  done
  echo "SMOKE_PANEL_MORPH_BARE $panel_morph_bare_path"
  echo "SMOKE_PANEL_MORPH_REST_FULL $panel_morph_rest_full_path"
  echo "SMOKE_PANEL_MORPH_REST_OFF $panel_morph_rest_off_path"
  echo "SMOKE_PANEL_MORPH_REST_END $panel_morph_rest_end_path"
  echo "SMOKE_PANEL_MORPH_REST_QUIET $panel_morph_rest_quiet_path"
  echo "SMOKE_PANEL_MORPH_REST_PLAYING $panel_morph_rest_playing_path"

  edge=$("$jq_bin" -r '.theme.barPosition' "$panel_morph_dump_path" 2>/dev/null)
  inset=$("$jq_bin" -r --arg e "$edge" '.theme.edgeInset[$e]' "$panel_morph_dump_path" 2>/dev/null)
  end=$("$jq_bin" -r --arg e "$edge" \
    '.panel.rest | if $e == "left" or $e == "right" then .y + .height else .x + .width end' "$panel_morph_dump_path" 2>/dev/null)
  [[ "$inset" =~ ^[0-9.]+$ && "$end" =~ ^[0-9.]+$ ]] || fail \
    "no edge inset or resting network card in $panel_morph_dump_path: $(head -c 400 "$panel_morph_dump_path" 2>/dev/null)"
  [ "$edge" = top ] || panel_morph_turn "$edge"
  read -r w h < <($convert_bin "$panel_morph_read/${panel_morph_bare_path##*/}" -format '%w %h\n' info: 2>/dev/null)
  case $edge in
    left | right)
      far=$((w - $(logical_px "${inset%.*}")))
      panel_morph_band_y=$(logical_px "$("$jq_bin" -r '.theme.edgeInset.bottom | floor' "$panel_morph_dump_path")")
      ;;
    *)
      far=$(logical_px "$(awk -v e="$end" 'BEGIN { printf "%d", e + 12 }')")
      [ "$far" -le "$w" ] || far=$w
      panel_morph_band_y=$(( $(logical_px "${inset%.*}") + 1 ))
      ;;
  esac
  panel_morph_band_x=$((far - panel_morph_band_w))
  panel_morph_band_h=$((h - panel_morph_band_y))
  echo "SMOKE_PANEL_MORPH_BAND edge $edge columns $panel_morph_band_x-$((far - 1)) from row $panel_morph_band_y of ${w}x$h"

  rest_full=$(panel_morph_far_edge "$panel_morph_rest_full_path")
  [ -n "$rest_full" ] || fail \
    "no card in $panel_morph_rest_full_path: nothing covers the band at columns $panel_morph_band_x-$((panel_morph_band_x + panel_morph_band_w - 1))"
  rest_off=$(panel_morph_far_edge "$panel_morph_rest_off_path")
  [ -n "$rest_off" ] || fail "no card in $panel_morph_rest_off_path once the radio was off"
  rest_end=$(panel_morph_far_edge "$panel_morph_rest_end_path")
  [ -n "$rest_end" ] || fail "no card in $panel_morph_rest_end_path at the end of the emerging case"
  rest_quiet=$(panel_morph_far_edge "$panel_morph_rest_quiet_path")
  [ -n "$rest_quiet" ] || fail "no media card in $panel_morph_rest_quiet_path"
  rest_playing=$(panel_morph_far_edge "$panel_morph_rest_playing_path")
  [ -n "$rest_playing" ] || fail "no media card in $panel_morph_rest_playing_path once the track was on the bus"
  echo "SMOKE_PANEL_MORPH_RESTS full=$rest_full off=$rest_off end=$rest_end quiet=$rest_quiet playing=$rest_playing"

  if [ $((rest_end > rest_off ? rest_end - rest_off : rest_off - rest_end)) -gt 8 ]; then
    fail "the emerging case settled at row $rest_end against the settled case's own radio-off rest at row $rest_off: the two runs did not end on the same card"
  fi

  panel_morph_claim settled "$rest_full" "$rest_off" 0 "${#panel_morph_settled_sleeps[@]}"
  panel_morph_claim emerging "$rest_full" "$rest_end" "$panel_morph_emerging_change" \
    "${#panel_morph_emerging_sleeps[@]}"
  panel_morph_claim media "$rest_quiet" "$rest_playing" 0 "${#panel_morph_media_sleeps[@]}"
}
