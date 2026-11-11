# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --workspaces proves the bar's workspace indicator is ONE pill that travels
# rather than a fill each slot owns. With Spaces the slot on
# screen also opens wide for its icons, so between two settled frames the
# pill and the two slots it left and reached are what moves.
#
# A second foot window is spawned on workspace 2 first, so the slot it
# leaves has an icon of its own to fold away rather than only a label.
#
# `hl.dsp.focus({ workspace = 1 })` then fires and the run takes a burst of
# frames across the pill's own travel, plus a settled one three seconds on.
# Every frame is cropped to the workspace cell alone, never the whole bar:
# the switch changes the active-window title two cells over, so a wider crop
# would differ between any two frames whether or not the indicator moved.
#
# Two assertions. The settled cells either side of the switch must differ,
# which is the pill occupying a different slot. And at least one burst frame
# must differ from the settled cell, which says the indicator was still
# resolving after the switch had landed: at `motion.enabled=false` every one
# of these tokens is 0 and all four frames would be the settled cell exactly.
#
# The burst frames also carry the picture the assertions do not claim: the
# pill's two edges run `emphasized`, the trailing one over twice that clock,
# so between roughly a quarter and a whole clock past the switch
# the pill is one shape stretched across both slots rather than a pill at
# either end. That is what the crops are for reading by eye. It is left out
# of the assertions on purpose: a nested software-rendered session cannot
# promise which frame a screencopy commits, and the durations themselves are
# pinned in the theme tokens.
leg_workspaces_flag="--workspaces"
leg_workspaces_order=195
leg_workspaces_needs="foot convert"
leg_workspaces_fixture_window=keep
leg_workspaces_claims="workspaces=shared"

workspaces_two_path="$shot_dir/workspaces-two.png"
workspaces_settled_path="$shot_dir/workspaces-settled.png"
workspaces_dispatch_path="$shot_dir/workspaces-dispatch.txt"
workspaces_crop_dir="$shot_dir/workspaces-crops"
# The workspace cell alone, at the left end of the bar's left region, past
# the launcher cell beside it: five persistent slots with one of them open,
# short of the active-window cell after it.
workspaces_crop_geometry="150x24+52+8"
# The pill's own clock (Theme.motion.emphasized) and the delay
# this rig costs between the dispatch and the shell's first frame carrying
# it, both in milliseconds. Every burst frame is that delay plus a fraction
# of the clock, so moving the token moves the sampling with it instead of
# leaving the burst pinned to a duration that has gone.
#
# The fractions straddle the widest part of the stretch, which is earlier
# and narrower than the clock alone suggests: the emphasized curve puts
# 86% of the travel in its first third, so the leading edge is nearly home
# by 0.3 of a clock while the trailing edge, on twice that, is barely a
# third of the way. The gap between them peaks around a quarter of a clock
# past the shell's first frame and is closed again by three quarters.
#
# A fraction says when a capture is ASKED for, not when the compositor
# commits the frame it answers with, and all four are in flight at once, so
# the frames read later than they are armed and not always in that order.
# That is why the first one is negative and still lands inside the travel.
workspaces_clock_ms=400
workspaces_react_ms=130
workspaces_burst_fractions=(-6 8 18 28)
workspaces_burst_sleeps=()
workspaces_burst=""
for workspaces_f in "${workspaces_burst_fractions[@]}"; do
  workspaces_at=$((workspaces_react_ms + workspaces_f * workspaces_clock_ms / 100))
  workspaces_burst_sleeps+=("$(printf '%d.%03d' $((workspaces_at / 1000)) $((workspaces_at % 1000)))")
  workspaces_burst="$workspaces_burst ${#workspaces_burst_sleeps[@]}"
done

workspaces_burst_path() {
  echo "$shot_dir/workspaces-moving-$1.png"
}

workspaces_crop_path() {
  echo "$workspaces_crop_dir/cell-$(basename "$1")"
}

leg_workspaces_timing() {
  leg_timing 50
}

leg_workspaces_drive() {
  local script="$shot_dir/workspaces-drive.sh" i arm=""
  # Each burst frame is armed off the switch rather than chained behind the
  # one before it: a full-screen screencopy costs a good fraction of the
  # clock being sampled, so chained sleeps would drag every later frame past
  # the stretch it was meant to catch.
  for i in "${!workspaces_burst_sleeps[@]}"; do
    arm+="( sleep ${workspaces_burst_sleeps[$i]}; \"$grim_bin\" \"$(workspaces_burst_path $((i + 1)))\" > /dev/null 2>&1 ) &"$'\n'
  done
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$wmctl_bin" dispatch "hl.dsp.focus({ workspace = 2 })" > "$workspaces_dispatch_path" 2>&1
sleep 1
"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-smoke-workspace --title='formalshell workspace two' sh -c 'sleep 300']==])" >> "$workspaces_dispatch_path" 2>&1
sleep 3
"$grim_bin" "$workspaces_two_path" > /dev/null 2>&1
"$wmctl_bin" dispatch "hl.dsp.focus({ workspace = 1 })" >> "$workspaces_dispatch_path" 2>&1
$arm
wait
sleep 3
"$grim_bin" "$workspaces_settled_path" > /dev/null 2>&1
EOF
  add_cleanup "pkill -f formalshell-smoke-workspace 2>/dev/null || true"
  wm_exec_once "bash $script"
}

leg_workspaces_assert() {
  local f n crop crop_two crop_settled caught
  mkdir -p "$workspaces_crop_dir"
  for f in "$workspaces_two_path" "$workspaces_settled_path"; do
    if [ ! -f "$f" ]; then
      fail "no workspace frame produced at $f"
    fi
  done
  cat "$workspaces_dispatch_path"
  echo "SMOKE_WORKSPACES_TWO $workspaces_two_path"
  echo "SMOKE_WORKSPACES_SETTLED $workspaces_settled_path"
  for n in $workspaces_burst; do
    f=$(workspaces_burst_path "$n")
    if [ ! -f "$f" ]; then
      fail "no workspace burst frame produced at $f"
    fi
    echo "SMOKE_WORKSPACES_MOVING_$n $f"
  done

  for f in "$workspaces_two_path" "$workspaces_settled_path" \
      $(for n in $workspaces_burst; do workspaces_burst_path "$n"; done); do
    crop=$(workspaces_crop_path "$f")
    if ! $convert_bin "$f" -crop "$workspaces_crop_geometry" +repage "$crop"; then
      fail "could not crop the workspace cell out of $f"
    fi
  done
  crop_two=$(workspaces_crop_path "$workspaces_two_path")
  crop_settled=$(workspaces_crop_path "$workspaces_settled_path")
  echo "SMOKE_WORKSPACES_CELL_TWO $crop_two"
  echo "SMOKE_WORKSPACES_CELL_SETTLED $crop_settled"

  if cmp -s "$crop_two" "$crop_settled"; then
    fail "the workspace cell is identical before and after the switch: the indicator did not move"
  fi

  # Still resolving after the switch landed. With every motion token at 0
  # the cell would already be the settled one in all three burst frames.
  caught=""
  for n in $workspaces_burst; do
    crop=$(workspaces_crop_path "$(workspaces_burst_path "$n")")
    echo "SMOKE_WORKSPACES_CELL_MOVING_$n $crop"
    if [ -z "$caught" ] && ! cmp -s "$crop" "$crop_settled"; then
      caught="$crop"
    fi
  done
  if [ -z "$caught" ]; then
    fail "every burst frame is already the settled cell: the indicator changed with no transition at all"
  fi
}
