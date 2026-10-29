# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --chevron-quiet asks the one thing --chevron does not: that the chevron's
# second bar OPENS quiet. The card is built fresh on every open, so its cells
# measure themselves for the first time while it is arriving, and with the
# layout rule armed (M53 D2) every one of them grew from nothing, slid its
# neighbours along the rail behind it and inflated the card around the lot,
# on every single open (owner, 2026-09-09: "every time there's an animation
# moving the icons and text, which is annoying"). The card's own entrance
# stays; what must not be there is a second movement inside it.
#
# Same fixture as --chevron, so the group in the card is the same five cells,
# which is why the two cannot share a session: both own bar.layout and both
# drive the same chevron.
#
# The card is opened and shut three times under a burst of captures of the
# band it hangs in, and two things are read off them:
#
#   width  no capture taken while the card was OPENING caught it narrower
#          than the card it settles on. A card that opens around its cells
#          starts at its padding alone and grows, so a capture inside the
#          entrance catches it.
#
#          "the card it settles on" is the settled silhouette less its two
#          fillets (M54 D6): a joined card carries a concave shoulder a
#          `radiusXl` outside each of its sides, in the first `radiusXl`
#          under the bar's line, and those are the last part of it to come
#          out from behind that line. So a frame caught mid-entrance is the
#          card's own width and a settled one is the card plus both fillets,
#          and comparing the two raw would call a perfectly rigid card 16%
#          narrow. The radius comes off `debug dump` rather than a constant,
#          so a change to the radius moves it.
#   quiet  three captures taken back to back once the entrance is over, and
#          two more half a second apart after them, are byte-identical: once
#          the card is up, nothing inside it moves on its own.
#
# Only the opening window counts toward the width claim, and the window is
# read off the expand calls' own stamps rather than off the pictures: a card
# on its way OUT measures narrow too, but for a reason that is not this
# leg's (it is fading, and a card at a third of its opacity has only its
# border left over the threshold below).
#
# ⚠️ The width claim is a guard here, not the proof. This rig commits the
# card's first frame ~130ms behind the ipc call and cannot sample faster than
# ~60ms even capturing the band alone (`grim -g`, a tenth of the pixels, runs
# no quicker than a full frame did). The entrance is `spatial` (M54 D2), long
# enough that several samples land inside it, but a card growing around its
# cells is still only measurably narrow while its cells are measuring
# themselves, which is the first frame or two of it.
# Do not tighten the sampling to chase it; the lanes below already lock in step
# under load.
#
# The quiet claim only means anything if the band is quiet at rest, so the
# settled capture is checked against a second one half a second later before
# any of it is believed: a cell repainting on its own would otherwise read as
# layout motion that is not there.
leg_chevron_quiet_flag="--chevron-quiet"
leg_chevron_quiet_order=181
leg_chevron_quiet_needs="convert jq"

# The band the card hangs in: the bar is `barCellHeight` 28 plus a
# `barMargin` 6 either side, so the strip ends at y=40 and the card's own top
# is one more barMargin under it. Starting two pixels past the strip keeps
# the chevron cell's own panel mark, which changes with the card, out of
# every comparison below.
chevron_quiet_band_box="1920x110+0+42"
# The same box in grim's own geometry spelling.
chevron_quiet_band_region="0,42 1920x110"
chevron_quiet_cycles=3
# Long enough for the card to be committed and sampled a few times, and for
# the exit to finish before the next open, and no longer: the burst has to
# cover every cycle. Both are the entrance's own clock (Theme.motion.spatial,
# M54 D2) plus this rig's map, rounded up.
chevron_quiet_open_hold_ms=800
chevron_quiet_shut_hold=0.6
# One capture's own length, cut off the tail of every opening window so a
# frame whose screencopy landed in the exit is never read as a narrow card.
chevron_quiet_capture_ms=80
# Two lanes half a band capture apart. A third lane makes it worse rather
# than better: screencopies in flight stretch each other's runs and the
# spacing with them. Enough frames to outlast the three cycles, no more.
chevron_quiet_burst_frames=80
chevron_quiet_burst_lanes="a b"
chevron_quiet_burst_offset=0.012

chevron_quiet_burst_dir="$shot_dir/chevron-quiet-burst"
chevron_quiet_burst_log="$shot_dir/chevron-quiet-burst.tsv"
chevron_quiet_call_path="$shot_dir/chevron-quiet-calls.tsv"
chevron_quiet_reply_path="$shot_dir/chevron-quiet-replies.txt"
chevron_quiet_status_path="$shot_dir/chevron-quiet-status.json"
chevron_quiet_dump_path="$shot_dir/chevron-quiet-dump.json"
chevron_quiet_shut_path="$shot_dir/chevron-quiet-shut.png"
chevron_quiet_settled_path="$shot_dir/chevron-quiet-settled.png"
chevron_quiet_rest_path="$shot_dir/chevron-quiet-rest.png"
# The one full frame, for reading the card back rather than measuring it.
chevron_quiet_open_path="$shot_dir/chevron-quiet-open.png"
chevron_quiet_tail_paths=(
  "$shot_dir/chevron-quiet-tail-1.png"
  "$shot_dir/chevron-quiet-tail-2.png"
  "$shot_dir/chevron-quiet-tail-3.png"
)

leg_chevron_quiet_validate() {
  local other
  for other in bar_layout bar_position chevron panel_handoff tray_overflow; do
    if leg_on "$other"; then
      echo "usage: --chevron-quiet carries its own bar.layout and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_chevron_quiet_fixture() {
  settings_fragment ', "bar": {"layout": {"right": ["bluetooth", "weather", "tray", "bell", "indicators", "chevron", "battery", "audio", "network"]}}'
}

leg_chevron_quiet_timing() {
  # The drive below runs about seventeen seconds; the run's own frame is taken
  # past it, on the card left open.
  leg_timing 22 70
}

leg_chevron_quiet_drive() {
  local script="$shot_dir/chevron-quiet-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
mkdir -p "$chevron_quiet_burst_dir"
sleep 5
$ipc call debug dump > "$chevron_quiet_dump_path" 2>&1
"$grim_bin" -g "$chevron_quiet_band_region" "$chevron_quiet_shut_path" > /dev/null 2>&1
sleep 1
burst() {
  local lane=\$1 i t0 t1
  for i in \$(seq 1 $chevron_quiet_burst_frames); do
    t0=\$(date +%s%3N)
    "$grim_bin" -g "$chevron_quiet_band_region" "$chevron_quiet_burst_dir/burst-\$lane\$i.png" > /dev/null 2>&1
    t1=\$(date +%s%3N)
    printf '%s\t%s\t%s\n' "\$lane\$i" "\$t0" "\$t1" >> "$chevron_quiet_burst_log"
  done
}
burst_pids=()
for lane in $chevron_quiet_burst_lanes; do
  burst "\$lane" &
  burst_pids+=("\$!")
  sleep $chevron_quiet_burst_offset
done
sleep 0.15
for cycle in \$(seq 1 $chevron_quiet_cycles); do
  call_start=\$(date +%s%3N)
  $ipc call bar chevron expand >> "$chevron_quiet_reply_path" 2>&1
  printf '%s\t%s\n' "\$call_start" "\$(date +%s%3N)" >> "$chevron_quiet_call_path"
  sleep 0.$chevron_quiet_open_hold_ms
  $ipc call bar chevron collapse >> "$chevron_quiet_reply_path" 2>&1
  sleep $chevron_quiet_shut_hold
done
wait "\${burst_pids[@]}"
# One more open, on its own, for the settled half of the claim: three frames
# back to back once the entrance is over, then the card the run settles on.
$ipc call bar chevron expand >> "$chevron_quiet_reply_path" 2>&1
sleep 0.9
"$grim_bin" -g "$chevron_quiet_band_region" "${chevron_quiet_tail_paths[0]}" > /dev/null 2>&1
"$grim_bin" -g "$chevron_quiet_band_region" "${chevron_quiet_tail_paths[1]}" > /dev/null 2>&1
"$grim_bin" -g "$chevron_quiet_band_region" "${chevron_quiet_tail_paths[2]}" > /dev/null 2>&1
sleep 1
$ipc call bar chevron status > "$chevron_quiet_status_path" 2>&1
"$grim_bin" -g "$chevron_quiet_band_region" "$chevron_quiet_settled_path" > /dev/null 2>&1
"$grim_bin" "$chevron_quiet_open_path" > /dev/null 2>&1
sleep 0.5
"$grim_bin" -g "$chevron_quiet_band_region" "$chevron_quiet_rest_path" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# How wide the card is in one band, in pixels: the bounding box of everything
# that differs from the shut band, which in this band is the card and nothing
# else. 0 when there is no card in the frame, so one taken between two of the
# cycles reads as one rather than as a failure. Cut at 2%: the opaque card's
# fill sits about 6% over this rig's near-black desktop.
chevron_quiet_card_width() {
  local band="$1" box
  box=$($convert_bin "$band" "$chevron_quiet_shut_path" -compose difference -composite \
    -colorspace Gray -threshold 2% -format "%@" info: 2>/dev/null)
  case "$box" in
    [0-9]*x[0-9]*) ;;
    *) echo 0; return 0 ;;
  esac
  local width="${box%%x*}"
  # A trim over an all-black difference answers with the whole image on some
  # builds and with 1x1 on others; either way there is no card in it.
  if [ "$width" -le 4 ] || [ "$width" -ge 1900 ]; then
    echo 0
  else
    echo "$width"
  fi
}

leg_chevron_quiet_assert() {
  local f
  for f in "$chevron_quiet_shut_path" "$chevron_quiet_settled_path" \
    "$chevron_quiet_rest_path" "$chevron_quiet_open_path" "${chevron_quiet_tail_paths[@]}"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done
  [ -s "$chevron_quiet_burst_log" ] || fail "the burst produced no frames at all"
  [ -s "$chevron_quiet_call_path" ] || fail "no expand call was stamped"

  echo "SMOKE_CHEVRON_QUIET_OPEN $chevron_quiet_open_path"

  local replies
  replies=$(grep -c '^ok$' "$chevron_quiet_reply_path" 2>/dev/null || true)
  if [ "$replies" -ne $((chevron_quiet_cycles * 2 + 1)) ]; then
    fail "the chevron did not answer ok to all $((chevron_quiet_cycles * 2 + 1)) expand/collapse calls, got $replies. Replies: $(cat "$chevron_quiet_reply_path" 2>/dev/null)"
  fi
  if ! grep -q '"open":true' "$chevron_quiet_status_path" 2>/dev/null; then
    fail "the last expand did not leave the group's own bar up. Got: $(cat "$chevron_quiet_status_path" 2>/dev/null)"
  fi

  if ! cmp -s "$chevron_quiet_settled_path" "$chevron_quiet_rest_path"; then
    fail "the card's own band is not still at rest: $chevron_quiet_settled_path and $chevron_quiet_rest_path differ half a second apart, so nothing below could tell layout motion from a cell repainting"
  fi
  local settled_width
  settled_width=$(chevron_quiet_card_width "$chevron_quiet_settled_path")
  if [ "$settled_width" -lt 100 ]; then
    fail "no card found in the settled band: the diff against the shut band is ${settled_width}px wide, so either the card is not in $chevron_quiet_band_box or it never opened"
  fi
  # The card's own rect inside that silhouette: the fillets are the only part
  # of it that is not there until the entrance has landed (see the header).
  local fillet card_width
  fillet=$($jq_bin -r '.theme.radiusXl' "$chevron_quiet_dump_path" 2>/dev/null)
  case "$fillet" in
    ''|*[!0-9]*) fail "the dump did not carry theme.radiusXl, got '$fillet' in $chevron_quiet_dump_path" ;;
  esac
  card_width=$((settled_width - fillet * 2))
  echo "chevron-quiet: settled card ${card_width}px plus two ${fillet}px fillets over $chevron_quiet_cycles openings"

  # Nothing moved once the entrance was over.
  local i
  for i in "${!chevron_quiet_tail_paths[@]}"; do
    if ! cmp -s "${chevron_quiet_tail_paths[$i]}" "$chevron_quiet_settled_path"; then
      fail "the card was still moving $((i + 1)) frames past its own entrance: ${chevron_quiet_tail_paths[$i]} differs from the card the run settled on"
    fi
  done

  # Every expand's own window, in milliseconds from the first one. A capture
  # is inside one while the card is opening and standing there; the tail of
  # `open_hold` is cut so a frame whose screencopy landed in the exit that
  # follows cannot be read as a narrow card.
  local -a open_from=() open_to=()
  local first_call="" call_start call_end
  while IFS=$'\t' read -r call_start call_end; do
    [ -n "$first_call" ] || first_call="$call_start"
    open_from+=("$((call_start - first_call))")
    open_to+=("$((call_end - first_call + chevron_quiet_open_hold_ms - chevron_quiet_capture_ms))")
  done < "$chevron_quiet_call_path"

  # Read every burst frame first and print the table, so a run that fails the
  # claim below still hands back what it saw.
  local idx t0 t1 width n inside
  local -a burst_index=() burst_offset=() burst_width=() burst_opening=()
  while IFS=$'\t' read -r idx t0 t1; do
    [ -f "$chevron_quiet_burst_dir/burst-$idx.png" ] || continue
    width=$(chevron_quiet_card_width "$chevron_quiet_burst_dir/burst-$idx.png")
    burst_index+=("$idx")
    burst_offset+=("$((t0 - first_call))")
    burst_width+=("$width")
    local inside=no
    for n in "${!open_from[@]}"; do
      if [ "$((t0 - first_call))" -ge "${open_from[$n]}" ] && [ "$((t0 - first_call))" -le "${open_to[$n]}" ]; then
        inside=yes
      fi
    done
    burst_opening+=("$inside")
  done < <(sort -k2,2n "$chevron_quiet_burst_log")

  local shown=0 bare=0 opening=0 narrowest=0
  for i in "${!burst_index[@]}"; do
    echo "chevron-quiet: burst ${burst_index[$i]} +${burst_offset[$i]}ms card ${burst_width[$i]}px opening=${burst_opening[$i]}"
    if [ "${burst_width[$i]}" -eq 0 ]; then
      bare=$((bare + 1))
      continue
    fi
    shown=$((shown + 1))
    [ "${burst_opening[$i]}" = yes ] || continue
    opening=$((opening + 1))
    if [ "$narrowest" -eq 0 ] || [ "${burst_width[$i]}" -lt "$narrowest" ]; then
      narrowest="${burst_width[$i]}"
    fi
  done
  if [ "$shown" -lt $((chevron_quiet_cycles * 2)) ] || [ "$bare" -lt "$chevron_quiet_cycles" ]; then
    fail "the burst covered $shown frames with a card and $bare without over $chevron_quiet_cycles openings: it did not span the cycles it was meant to sample"
  fi
  # Three frames an opening is what the sampling rate is chosen to give; far
  # fewer means the burst outran the cycles and the claim below is thin.
  if [ "$opening" -lt $((chevron_quiet_cycles * 3)) ]; then
    fail "only $opening frames caught a card inside an opening window over $chevron_quiet_cycles openings: too few to claim anything about how they open"
  fi
  echo "chevron-quiet: narrowest card caught while opening ${narrowest}px of ${card_width}px, over $opening frames"

  for i in "${!burst_index[@]}"; do
    if [ "${burst_width[$i]}" -eq 0 ] || [ "${burst_opening[$i]}" != yes ]; then continue; fi
    if [ $(( burst_width[i] * 100 )) -lt $(( card_width * 90 )) ]; then
      fail "burst frame ${burst_index[$i]}, +${burst_offset[$i]}ms and inside an opening, caught the card ${burst_width[$i]}px wide against the ${card_width}px it settled on: the card opened around its cells instead of with them ($chevron_quiet_burst_dir/burst-${burst_index[$i]}.png)"
    fi
  done
}
