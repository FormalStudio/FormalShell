# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --panel-handoff proves what one panel replacing another has to do: not
# two cards crossing, one card. The incoming card is seeded on the outgoing
# one's rect, the outgoing window is cut on the tick the travel starts, while
# the two are still exactly on top of each other, and the one card left
# travels to its own place with its new contents coming up over the first
# part of that travel. So at no instant is there a card at both
# resting places and at some instant there is one between them.
#
# The default right region cannot show that: every panel-bearing cell there
# is within half a card's width of the screen edge, so the geometry clamp
# lands both cards on the same x and the whole travel is a height change.
# This leg therefore puts `audio` alone in the LEFT region and `network`
# alone in the RIGHT one, which parks the two cards ~1500px apart.
#
# Four boxes in the band just under the bar carry the claim, all cropped out
# of full frames afterwards rather than sampled live:
#   left  over the audio card's resting place
#   right over the network card's
#   gap   the 1120px between them (736 at 1.25), bare desktop at both rests
#   band  the whole strip, for the settled frame
# A card is 380 wide and the two end boxes are 1300 apart at scale 1 (916 at 1.25), so one card can
# never be in both: `left is bare OR right is bare` holds for every frame of
# a handoff and fails for a pair crossing. The gap box is what a card in
# transit is in, for most of the travel, and nothing at rest is in it, so it
# says the card really moved rather than swapping ends. It is a box rather
# than the whole band on purpose: the Wi-Fi list
# rescans during the run, so a comparison that takes in the network card's
# own rows answers "different" whether anything moved or not.
#
# Five samples, each armed BEFORE the ipc call rather than after it: grim's
# own start-up costs about as much as the travel does here, so a capture
# asked for once the call has returned lands past the end of it every time.
# Arming first spends that start-up during the round trip instead. The
# spread runs wide because what is left, the round trip itself, is worth a
# couple of hundred milliseconds on this rig and is not the same twice.
#
# The five are the incoming window's own map plus a fraction of the travel's
# clock (Theme.motion.spatial), so a change to the token moves the
# sampling with it rather than leaving it pinned to a duration that has gone.
# The travel is front-loaded, which is why three of the five sit inside its
# first third: that is where a card is between the two rests.
leg_panel_handoff_flag="--panel-handoff"
leg_panel_handoff_order=78
leg_panel_handoff_compositors="hyprland shoji"
leg_panel_handoff_needs="convert"
leg_panel_handoff_claims="bar.layout.left bar.layout.right bar.position=top panel=shared"

panel_handoff_audio_open_path="$shot_dir/panel-handoff-audio-open.txt"
panel_handoff_audio_close_path="$shot_dir/panel-handoff-audio-close.txt"
panel_handoff_network_open_path="$shot_dir/panel-handoff-network-open.txt"
panel_handoff_network_state_path="$shot_dir/panel-handoff-network-state.txt"
panel_handoff_reply_path="$shot_dir/panel-handoff-reply.txt"
panel_handoff_state_path="$shot_dir/panel-handoff-state.txt"

panel_handoff_audio_rest_path="$shot_dir/panel-handoff-audio-rest.png"
panel_handoff_network_rest_path="$shot_dir/panel-handoff-network-rest.png"
panel_handoff_settled_path="$shot_dir/panel-handoff-settled.png"
panel_handoff_mid_paths=(
  "$shot_dir/panel-handoff-mid-1.png"
  "$shot_dir/panel-handoff-mid-2.png"
  "$shot_dir/panel-handoff-mid-3.png"
  "$shot_dir/panel-handoff-mid-4.png"
  "$shot_dir/panel-handoff-mid-5.png"
)

panel_handoff_clock_ms=500
# The rust shell has its incoming window up within a frame or two of the
# call rather than Qt's couple of hundred milliseconds, so the travel starts
# that much sooner.
panel_handoff_map_ms=20
panel_handoff_mid_fractions=(0 16 32 72 130)
panel_handoff_mid_sleeps=()
for panel_handoff_f in "${panel_handoff_mid_fractions[@]}"; do
  panel_handoff_at=$((panel_handoff_map_ms + panel_handoff_f * panel_handoff_clock_ms / 100))
  panel_handoff_mid_sleeps+=("$(printf '%d.%03d' $((panel_handoff_at / 1000)) $((panel_handoff_at % 1000)))")
done

# Logical boxes, cut out of the saved PNGs as the frame's own pixels once
# the run has read the output's scale (panel_handoff_boxes). The band is
# below the 40px bar and inside any panel card: a card's own top is one
# barMargin under the bar, and its padding, header row and seam alone
# already carry it past y=160. The right box and the gap's end hang off the
# output's logical width, the network card resting against its right edge.
panel_handoff_boxes() {
  panel_handoff_left_box=$(logical_box 200 100 100 60)
  panel_handoff_right_box=$(logical_box 200 100 $((logical_w - 320)) 60)
  panel_handoff_gap_box=$(logical_box $((logical_w - 800)) 100 400 60)
  panel_handoff_band_box=$(logical_box "$logical_w" 100 0 60)
}

leg_panel_handoff_fixture() {
  settings_pin '.bar.layout.left = ["audio"] | .bar.layout.right = ["network"]'
}

leg_panel_handoff_timing() {
  leg_timing 50
}

leg_panel_handoff_drive() {
  local script="$shot_dir/panel-handoff-drive.sh" i arm=""
  for i in "${!panel_handoff_mid_paths[@]}"; do
    arm+="( sleep ${panel_handoff_mid_sleeps[$i]}; \"$grim_bin\" \"${panel_handoff_mid_paths[$i]}\" > /dev/null 2>&1 ) &"$'\n'
  done
  write_script "$script" <<EOF
#!/usr/bin/env bash
# The audio card's own resting place, captured before anything hands over,
# so the settled frame at the end has something to be equal to.
$ipc call panel toggle audio > "$panel_handoff_audio_open_path" 2>&1
until_settled
"$grim_bin" "$panel_handoff_audio_rest_path" > /dev/null 2>&1
$ipc call panel toggle audio > "$panel_handoff_audio_close_path" 2>&1
until_settled
$ipc call panel toggle network > "$panel_handoff_network_open_path" 2>&1
until_settled
"$grim_bin" "$panel_handoff_network_rest_path" > /dev/null 2>&1
$ipc call panel state > "$panel_handoff_network_state_path" 2>&1
$arm
$ipc call panel toggle audio > "$panel_handoff_reply_path" 2>&1
wait
sleep 2
$ipc call panel state > "$panel_handoff_state_path" 2>&1
"$grim_bin" "$panel_handoff_settled_path" > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

# One box out of a saved frame, named after the two, so a comparison below
# reads as the claim it is making. -strip because ImageMagick writes the
# wall clock into a PNG's tIME chunk, which would leave two byte-identical
# pictures comparing unequal.
panel_handoff_crop() {
  local source="$1" box="$2" name="$3" out
  out="$shot_dir/panel-handoff-crop-$name.png"
  $convert_bin "$source" -crop "$box" +repage -strip "$out" > /dev/null 2>&1
  [ -s "$out" ] || fail "could not crop $box out of $source"
  echo "$out"
}

leg_panel_handoff_assert() {
  local f i
  panel_handoff_boxes
  for f in "$panel_handoff_audio_open_path" "$panel_handoff_audio_close_path" \
    "$panel_handoff_network_open_path" "$panel_handoff_reply_path"; do
    if ! grep -q '^ok$' "$f" 2>/dev/null; then
      fail "a panel route did not answer ok at $f, got: $(cat "$f" 2>/dev/null)"
    fi
  done
  if ! grep -q '^network$' "$panel_handoff_network_state_path" 2>/dev/null; then
    fail "panel state before the handoff is not network, got: $(cat "$panel_handoff_network_state_path" 2>/dev/null)"
  fi
  # Exactly one panel, by the shell's own bookkeeping: `panel state` names
  # the first open one it finds, so a second surviving the handoff would
  # either show up here or leave nothing named at all.
  if ! grep -q '^audio$' "$panel_handoff_state_path" 2>/dev/null; then
    fail "panel state after the handoff is not audio, got: $(cat "$panel_handoff_state_path" 2>/dev/null)"
  fi
  for f in "$panel_handoff_audio_rest_path" "$panel_handoff_network_rest_path" \
    "$panel_handoff_settled_path" "${panel_handoff_mid_paths[@]}"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done

  echo "SMOKE_PANEL_HANDOFF_AUDIO_REST $panel_handoff_audio_rest_path"
  echo "SMOKE_PANEL_HANDOFF_NETWORK_REST $panel_handoff_network_rest_path"
  echo "SMOKE_PANEL_HANDOFF_SETTLED $panel_handoff_settled_path"
  for i in "${!panel_handoff_mid_paths[@]}"; do
    echo "SMOKE_PANEL_HANDOFF_MID_$((i + 1)) ${panel_handoff_mid_paths[$i]}"
  done

  local audio_band settled_band bare_left bare_right audio_gap network_gap
  audio_band=$(panel_handoff_crop "$panel_handoff_audio_rest_path" "$panel_handoff_band_box" "audio-band")
  settled_band=$(panel_handoff_crop "$panel_handoff_settled_path" "$panel_handoff_band_box" "settled-band")
  # Bare desktop in each end box: the box the OTHER panel's rest frame leaves
  # empty. The gap between them is bare in both.
  bare_left=$(panel_handoff_crop "$panel_handoff_network_rest_path" "$panel_handoff_left_box" "bare-left")
  bare_right=$(panel_handoff_crop "$panel_handoff_audio_rest_path" "$panel_handoff_right_box" "bare-right")
  audio_gap=$(panel_handoff_crop "$panel_handoff_audio_rest_path" "$panel_handoff_gap_box" "audio-gap")
  network_gap=$(panel_handoff_crop "$panel_handoff_network_rest_path" "$panel_handoff_gap_box" "network-gap")

  if ! cmp -s "$audio_gap" "$network_gap"; then
    fail "the box at $panel_handoff_gap_box is not bare desktop at both resting places, so a card in it proves nothing"
  fi
  if ! cmp -s "$settled_band" "$audio_band"; then
    fail "the handoff did not settle on the audio card's own rect: $panel_handoff_settled_path differs from $panel_handoff_audio_rest_path over $panel_handoff_band_box"
  fi

  local travelled=false mid_gap mid_left mid_right
  for i in "${!panel_handoff_mid_paths[@]}"; do
    mid_gap=$(panel_handoff_crop "${panel_handoff_mid_paths[$i]}" "$panel_handoff_gap_box" "mid-$((i + 1))-gap")
    mid_left=$(panel_handoff_crop "${panel_handoff_mid_paths[$i]}" "$panel_handoff_left_box" "mid-$((i + 1))-left")
    mid_right=$(panel_handoff_crop "${panel_handoff_mid_paths[$i]}" "$panel_handoff_right_box" "mid-$((i + 1))-right")
    if ! cmp -s "$mid_gap" "$audio_gap"; then
      travelled=true
    fi
    if ! cmp -s "$mid_left" "$bare_left" && ! cmp -s "$mid_right" "$bare_right"; then
      fail "sample $((i + 1)) has a card at BOTH resting places: ${panel_handoff_mid_paths[$i]} is two cards crossing, not one travelling"
    fi
  done
  if ! $travelled; then
    fail "no mid-flight sample caught a card between the two resting places: the handoff swapped ends rather than travelling"
  fi
}
