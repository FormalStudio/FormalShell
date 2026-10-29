# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --notify-emerge proves how one notification arrives (M60 T4), which the
# --notify leg cannot: that leg's two frames land seconds after the last
# notify-send, by which time every entrance is long over. So one sticky
# critical notification is fired at a tenth of its speed (`debug motionScale`,
# the same instrument --menu-emerge and --join use) and photographed frame by
# frame, back to back, with the wall clock stamped after each capture.
#
# Read as a ladder of probes down the card's own centre column, each covered
# when it is NOT byte-identical to the same probe in the frame taken before
# the notification existed: something the desktop does not draw is being drawn
# there, which is the card. The desktop here carries no scrim, so that is the
# whole classifier.
#
# The claim: the card unfolds off its own top edge (a rotation about it from
# face down, through a few degrees past upright, to nothing), so its drawn
# height is a FRACTION of its resting height for most of the entrance. At
# least one frame covers some of the ladder and not all of it, the covered
# rungs are always the shallow ones, and the depth never goes back up.
#
# Then, with motion at full speed and the card at rest, the toast surface
# commits nothing for four seconds, read off the shell's own commit log
# between two `notifications status` calls.
#
# The card's rect is not hardcoded: it is the difference between the settled
# frame and the one taken before the notification existed, measured over the
# output below the bar so the bell cell's own unread mark is not in the box.
#
# Critical rather than normal on purpose: a sticky popup (expiresAt 0, see
# model.js's expire) is still there for the settled frame at the end of a
# ladder that runs for several seconds, and its urgency changes the rim's
# colour and nothing about the geometry this leg reads.
leg_notify_emerge_flag="--notify-emerge"
leg_notify_emerge_order=31
leg_notify_emerge_needs="notify-send convert"

notify_emerge_scale_path="$shot_dir/notify-emerge-scale.txt"
notify_emerge_status_path="$shot_dir/notify-emerge-status.txt"
notify_emerge_rest_path="$shot_dir/notify-emerge-rest.txt"
notify_emerge_t0_path="$shot_dir/notify-emerge-t0.txt"
notify_emerge_stamp_path="$shot_dir/notify-emerge-stamps.txt"
notify_emerge_bare_path="$shot_dir/notify-emerge-bare.png"
notify_emerge_settled_path="$shot_dir/notify-emerge-settled.png"

# The scale the arrival runs at, as a percent, and the divisor the stamps are
# reported in real milliseconds through.
notify_emerge_scale=1000

# Back to back rather than armed at fractions of the clock, for the reason
# menu_emerge.sh documents: grim's own start-up costs about as much as the
# entrance does at full speed, and a batch armed at once comes back out of
# order. A frame lands every fifth of a second or so, which covers a 400ms
# unfold stretched to four seconds several times over.
notify_emerge_frames=30
notify_emerge_frame_gap=0.09

# Where the card can be: the output below the bar, so the bell cell's own mark
# is outside the box the card's rect is measured in.
notify_emerge_body_top=60

leg_notify_emerge_validate() {
  local other
  for other in notify center reminder capture_edit menu_emerge panel_emerge \
    panel_handoff panel_morph join deform chevron_quiet osd polkit screensaver; do
    if leg_on "$other"; then
      echo "usage: --notify-emerge measures one notification's own frames and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_notify_emerge_timing() {
  leg_timing 28 76
}

leg_notify_emerge_drive() {
  local script="$shot_dir/notify-emerge-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
sleep 4
call debug motionScale $notify_emerge_scale > "$notify_emerge_scale_path" 2>&1
sleep 1
# The same output with nothing in the corner: what every probe below reads as
# before any card is drawn over it.
"$grim_bin" "$notify_emerge_bare_path" > /dev/null 2>&1
"$notify_send_bin" -u critical 'Unfold' 'One bubble arriving'
date +%s%N > "$notify_emerge_t0_path"
for i in \$(seq 1 $notify_emerge_frames); do
  "$grim_bin" "$shot_dir/notify-emerge-\$i.png" > /dev/null 2>&1
  echo "\$i \$(date +%s%N)" >> "$notify_emerge_stamp_path"
  sleep $notify_emerge_frame_gap
done
sleep 4
"$grim_bin" "$notify_emerge_settled_path" > /dev/null 2>&1
call notifications status > "$notify_emerge_status_path" 2>&1
call debug motionScale 100 > /dev/null 2>&1
sleep 2
# Two status calls bracket the rest window in the shell's own log.
call notifications status > "$notify_emerge_rest_path" 2>&1
sleep 4
call notifications status >> "$notify_emerge_rest_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

# One box out of a saved frame, named after what it is looking at. -strip
# because ImageMagick writes the wall clock into a PNG's tIME chunk, which
# would leave two byte-identical pictures comparing unequal.
notify_emerge_crop() {
  local source="$1" box="$2" name="$3" out
  out="$shot_dir/notify-emerge-crop-$name.png"
  $convert_bin "$source" -crop "$box" +repage -strip "$out" > /dev/null 2>&1
  [ -s "$out" ] || fail "could not crop $box out of $source"
  echo "$out"
}

# Which rungs of the ladder this frame covers, as a string of 0s and 1s from
# the card's own top downward.
notify_emerge_rungs() {
  local frame="$1" name="$2" i probe out=""
  for i in "${!notify_emerge_probe_boxes[@]}"; do
    probe=$(notify_emerge_crop "$frame" "${notify_emerge_probe_boxes[$i]}" "$name-$i")
    if cmp -s "$probe" "$shot_dir/notify-emerge-crop-bare-$i.png"; then
      out+="0"
    else
      out+="1"
    fi
  done
  echo "$out"
}

# How many rungs a reading covers, and whether the ones it covers are the
# shallow ones: a card unfolding off its top edge can only ever cover a prefix
# of the ladder.
notify_emerge_depth() {
  echo "${1//0/}" | tr -d '\n' | wc -c | tr -d ' '
}

notify_emerge_is_prefix() {
  case "$1" in
    *01*) return 1 ;;
    *) return 0 ;;
  esac
}

leg_notify_emerge_assert() {
  local f i
  grep -q '^ok$' "$notify_emerge_scale_path" 2>/dev/null \
    || fail "debug motionScale did not answer ok, got: $(cat "$notify_emerge_scale_path" 2>/dev/null)"
  for f in "$notify_emerge_bare_path" "$notify_emerge_settled_path"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done
  echo "SMOKE_NOTIFY_EMERGE_BARE $notify_emerge_bare_path"
  echo "SMOKE_NOTIFY_EMERGE_SETTLED $notify_emerge_settled_path"
  # The sticky popup is still the one the settled frame is measured against.
  echo "notifications status after the arrival: $(cat "$notify_emerge_status_path" 2>/dev/null)"
  for i in $(seq 1 $notify_emerge_frames); do
    [ -f "$shot_dir/notify-emerge-$i.png" ] || fail "no screenshot produced at $shot_dir/notify-emerge-$i.png"
    echo "SMOKE_NOTIFY_EMERGE_$i $shot_dir/notify-emerge-$i.png"
  done

  # The stamps, in real milliseconds off the notify-send that started it, so
  # the ladder below can be read against the clock. Printed before anything is
  # asserted: it has to survive a run whose assertions fail.
  local t0 stamp frame at
  t0=$(cat "$notify_emerge_t0_path" 2>/dev/null)
  [ -n "$t0" ] || fail "no t0 stamped for the notification"
  while read -r frame stamp; do
    at=$(( (stamp - t0) / 1000000 / (notify_emerge_scale / 100) ))
    echo "SMOKE_NOTIFY_EMERGE_AT $frame ${at}ms"
  done < "$notify_emerge_stamp_path"

  # The card's own rect, off the two frames that differ by exactly one card.
  local body_h=$((1080 - notify_emerge_body_top)) bare_body settled_body rect
  bare_body="$shot_dir/notify-emerge-body-bare.png"
  settled_body="$shot_dir/notify-emerge-body-settled.png"
  $convert_bin "$notify_emerge_bare_path" -crop "1920x${body_h}+0+${notify_emerge_body_top}" +repage "$bare_body" > /dev/null 2>&1
  $convert_bin "$notify_emerge_settled_path" -crop "1920x${body_h}+0+${notify_emerge_body_top}" +repage "$settled_body" > /dev/null 2>&1
  # 2% rather than --panel-emerge's 8%: a card's translucent fill lifts this
  # rig's near-black desktop by about 5% (panel_morph.sh's own finding), so a
  # higher cut measures the card's border alone and puts the box in the wrong
  # place.
  rect=$($convert_bin "$bare_body" "$settled_body" -compose difference -composite \
    -threshold 2% -format "%@" info: 2>/dev/null)
  local card_w card_h card_x card_y
  card_w=${rect%%x*}
  card_h=${rect#*x}; card_h=${card_h%%+*}
  card_x=$(echo "$rect" | sed -n 's/.*+\([0-9]*\)+[0-9]*$/\1/p')
  card_y=$(echo "$rect" | sed -n 's/.*+\([0-9]*\)$/\1/p')
  card_y=$((card_y + notify_emerge_body_top))
  if [ -z "$card_w" ] || [ "${card_w:-0}" -lt 200 ] || [ "${card_h:-0}" -lt 40 ]; then
    fail "could not measure the notification off the bare/settled pair, got rect '$rect'"
  fi
  echo "SMOKE_NOTIFY_EMERGE_CARD ${card_w}x${card_h}+${card_x}+${card_y}"

  # The ladder, down the card's own centre column: a rotation about the top
  # edge holds that column's horizontal midpoint still, so a probe on it is
  # inside the card whenever the card has unfolded that far.
  local centre_x=$((card_x + card_w / 2 - 2)) depth_at
  notify_emerge_probe_boxes=()
  for depth_at in 12 30 50 70 88; do
    notify_emerge_probe_boxes+=("5x4+${centre_x}+$((card_y + card_h * depth_at / 100 - 2))")
  done

  for i in "${!notify_emerge_probe_boxes[@]}"; do
    notify_emerge_crop "$notify_emerge_bare_path" "${notify_emerge_probe_boxes[$i]}" "bare-$i" > /dev/null
  done

  local settled_rungs settled_depth
  settled_rungs=$(notify_emerge_rungs "$notify_emerge_settled_path" "settled")
  settled_depth=$(notify_emerge_depth "$settled_rungs")
  if [ "$settled_depth" -ne "${#notify_emerge_probe_boxes[@]}" ]; then
    fail "the settled card does not cover its own ladder ($settled_rungs), so the probes are not where the measurement thinks"
  fi

  local rungs depth previous=0 ladder="" fractional=0
  for i in $(seq 1 $notify_emerge_frames); do
    rungs=$(notify_emerge_rungs "$shot_dir/notify-emerge-$i.png" "frame-$i")
    depth=$(notify_emerge_depth "$rungs")
    ladder+=" $depth"
    notify_emerge_is_prefix "$rungs" \
      || fail "frame $i covers a deep rung under a bare one ($rungs): whatever is drawing there is not a card unfolding off its own top edge"
    if [ "$depth" -lt "$previous" ]; then
      fail "the card went back up between frames $((i - 1)) and $i: depths$ladder"
    fi
    if [ "$depth" -gt 0 ] && [ "$depth" -lt "$settled_depth" ]; then
      fractional=$((fractional + 1))
    fi
    previous=$depth
  done
  echo "SMOKE_NOTIFY_EMERGE_LADDER$ladder settled=$settled_depth"
  if [ "$previous" -ne "$settled_depth" ]; then
    fail "the last frame never reached the card's own height (depths$ladder): the arrival did not land"
  fi

  [ "$fractional" -gt 0 ] || fail \
    "no frame drew part of the card (depths$ladder): a bubble unfolds off its own top edge, it does not appear at its resting height"
  echo "SMOKE_NOTIFY_EMERGE_UNFOLD ok $fractional of $notify_emerge_frames frames carried a fraction of the card"

  local commits
  commits=$(awk '
    function t(line) { match(line, /t=[0-9]+ms/); return substr(line, RSTART + 2, RLENGTH - 4) + 0 }
    /^ipc t=.*target: "notifications", function: "status"/ { lo = hi; hi = t($0) }
    /^commit surface=toasts / { c[++n] = t($0) }
    END { k = 0; for (i = 1; i <= n; i++) if (c[i] > lo && c[i] < hi) k++; print (lo > 0 ? k : -1) }' "$shell_log_path")
  echo "SMOKE_NOTIFY_EMERGE_QUIET commits=$commits"
  [ "$commits" -ge 0 ] || fail "no rest window bracketed in the shell log"
  [ "$commits" -eq 0 ] || fail "the toast surface committed $commits times with the card at rest"
}
