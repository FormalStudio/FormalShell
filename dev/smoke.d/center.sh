# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --center: a notify-send of this leg's own, then a long enough wait for the
# model's 8s popup timeout to move it (and --notify's, when the two flags
# combine) into the `pending` tier the centre lists. The wait is 14s rather
# than the 9 that timeout alone would need: on the VM's software renderer
# the shell reaches the bus several seconds after the drive script starts,
# so a notification's own clock begins well after its notify-send. It fires
# at sleep 2, ahead of --notify's own four, so that when the two legs
# combine it is the oldest group and therefore the one the popup cap evicts
# into pending: --notify's frames need all four of its cards alive.
# Combining also leaves the critical popup sticky in the popup layer, which
# is what makes the suppression claim readable: the centre hangs off the
# bar under the bell wherever notifications.position puts the toast stack,
# so the toast stack drops out for as long as the centre is open and
# center.png shows the centre alone.
#
# `notifications status` brackets a showHistory round trip (the IPC stand-in
# for the bell cell's own click, which calls the same center.open()/close()),
# and `dismissOne` is asserted here rather than in --notify: it drops the
# front toast, which would change what that leg's own frames show.
#
# Then the M48 D3 claim, which needs a history longer than the output is
# tall: thirty notifications, the centre reopened over them, and a real
# scroll wheel through the row list. The card's own height is read off
# `notifications status` rather than measured in the frame, since the
# surface behind it is the whole output either way and a short card and a
# clipped one photograph the same.
#
# Last, one open at a tenth of its speed (`debug motionScale`, the instrument
# --join and --menu-emerge sample with) over a band down the output's trailing
# side, where the bell sits: the centre comes out of the bar's line under it
# like any panel, and this is the only place that travel is photographed.
# Frames only, read by eye: the line it joins moves with --frame and
# --bar-position, so no one claim here would hold whatever the run carries.
leg_center_flag="--center"
leg_center_order=40
leg_center_needs="notify-send jq wlrctl convert"

# The band the entrance is read in: the output's trailing 560 columns, whole
# height, so a `popupWidthWide` (480) card a `screenPadding` in from the edge
# sits inside it with both of its ends in the frame.
center_region="1360,0 560x1080"
center_frames=10
center_emerge_desktop_path="$shot_dir/center-emerge-desktop.png"
center_emerge_rest_path="$shot_dir/center-emerge-rest.png"

center_path="$shot_dir/center.png"
center_long_path="$shot_dir/center-long.png"
center_scrolled_path="$shot_dir/center-scrolled.png"
center_status_before_path="$shot_dir/center-status-before.json"
center_status_open_path="$shot_dir/center-status-open.json"
center_status_closed_path="$shot_dir/center-status-closed.json"
center_status_long_path="$shot_dir/center-status-long.json"
center_dismiss_one_path="$shot_dir/center-dismiss-one.txt"
center_scroll_path="$shot_dir/center-scroll.txt"

leg_center_timing() {
  # center-drive.sh's own last frame lands around 45s, and thirty
  # notify-sends are thirty processes on a software-rendered VM, so the
  # margin is deliberately wide: an earlier 48 put the last leg past the
  # session's own teardown. This run's generic smoke.png is taken after all
  # of it, showing the session with the centre closed again and the front
  # toast gone. The slow-motion entrance at the end costs another ~13s.
  leg_timing 70 150
}

leg_center_drive() {
  local script="$shot_dir/center-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 2
"$notify_send_bin" -u normal 'Second' 'World'
sleep 15
$ipc call notifications status > "$center_status_before_path" 2>&1
sleep 1
$ipc call notifications showHistory > /dev/null 2>&1
sleep 2
$ipc call notifications status > "$center_status_open_path" 2>&1
"$grim_bin" "$center_path" > /dev/null 2>&1
sleep 1
$ipc call notifications dismissOne > "$center_dismiss_one_path" 2>&1
sleep 1
$ipc call notifications showHistory > /dev/null 2>&1
sleep 1
$ipc call notifications status > "$center_status_closed_path" 2>&1
sleep 1
# Distinct summaries: identical ones would collapse into a single counted
# row (Model.groupEntries) and the list would be no longer than before. In
# parallel, since thirty sequential notify-sends each waiting on their own
# D-Bus reply cost this leg ten seconds it does not have.
for i in \$(seq 1 30); do
  "$notify_send_bin" -u normal "History row \$i" 'Filling the centre' &
done
wait
sleep 13
$ipc call notifications showHistory > /dev/null 2>&1
sleep 2
$ipc call notifications status > "$center_status_long_path" 2>&1
"$grim_bin" "$center_long_path" > /dev/null 2>&1
# wlrctl, a real zwlr_virtual_pointer_v1 client, for the same reason the
# tooltip leg uses it: the compositor's own warp dispatcher moves the cursor
# without the surface under it ever seeing a pointer event. Its protocol is
# relative only, so the pointer is slammed into a corner the compositor
# clamps and then moved once from that known origin. 1700x600 is inside the
# card, which hangs under the bell, clamped a padding in from the right edge.
# Each exit code is recorded: wlrctl aborts on an assertion rather than
# printing anything useful when it cannot reach the display, and a silent
# success looks the same as a silent nothing.
"$wlrctl_bin" pointer move -4000 -4000 > "$center_scroll_path" 2>&1
echo "corner exit \$?" >> "$center_scroll_path"
sleep 1
"$wlrctl_bin" pointer move 1700 600 >> "$center_scroll_path" 2>&1
echo "park exit \$?" >> "$center_scroll_path"
sleep 1
"$wlrctl_bin" pointer scroll 400 0 >> "$center_scroll_path" 2>&1
echo "scroll exit \$?" >> "$center_scroll_path"
sleep 2
"$grim_bin" "$center_scrolled_path" > /dev/null 2>&1
$ipc call notifications showHistory > /dev/null 2>&1
sleep 2
# And the same open again at a tenth speed, over the band alone: the card
# coming out of the bar's line under the bell.
$ipc call debug motionScale 1000 > /dev/null 2>&1
"$grim_bin" -g "$center_region" "$center_emerge_desktop_path" > /dev/null 2>&1
$ipc call notifications showHistory > /dev/null 2>&1
for i in \$(seq 1 $center_frames); do
  sleep 0.3
  "$grim_bin" -g "$center_region" "$shot_dir/center-emerge-\$i.png" > /dev/null 2>&1
done
sleep 5
"$grim_bin" -g "$center_region" "$center_emerge_rest_path" > /dev/null 2>&1
$ipc call debug motionScale 100 > /dev/null 2>&1
$ipc call notifications showHistory > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_center_assert() {
  local f short_height short_max short_capped long_height long_max long_capped
  for f in "$center_status_before_path" "$center_status_open_path" \
    "$center_status_closed_path" "$center_status_long_path"; do
    if [ -s "$f" ]; then
      cat "$f"; echo
    else
      fail "no notifications status produced at $f"
    fi
  done
  # "pending":0 would mean the expired notify-sends never reached the tier
  # the bell cell counts, and the summon would have shown an empty list.
  if grep -q '"pending":0,' "$center_status_before_path"; then
    fail "pending count was zero before the centre summon: $(cat "$center_status_before_path")"
  fi
  if ! grep -q '"centerOpen":false' "$center_status_before_path"; then
    fail "centre reported open before the summon: $(cat "$center_status_before_path")"
  fi
  if ! grep -q '"centerOpen":true' "$center_status_open_path"; then
    fail "showHistory did not open the centre: $(cat "$center_status_open_path")"
  fi
  if ! grep -q '"centerOpen":false' "$center_status_closed_path"; then
    fail "second showHistory did not close the centre: $(cat "$center_status_closed_path")"
  fi
  if [ ! -s "$center_dismiss_one_path" ]; then
    fail "no notifications dismissOne answer produced"
  fi
  cat "$center_dismiss_one_path"
  if leg_on notify; then
    # --notify's critical toast is sticky (expiresAt 0), so it is still the
    # only thing in the popup tier when dismissOne runs, and the tier has to
    # be empty afterwards.
    if ! grep -q "^ok$" "$center_dismiss_one_path"; then
      fail "dismissOne did not drop the sticky critical toast, got: $(cat "$center_dismiss_one_path")"
    fi
    if ! grep -q '"popups":0' "$center_status_closed_path"; then
      fail "a toast survived dismissOne: $(cat "$center_status_closed_path")"
    fi
  else
    # Run alone, every popup this leg fired has expired into pending by the
    # time dismissOne runs, and an empty popup tier answers `none` rather
    # than erroring.
    if ! grep -q "^none$" "$center_dismiss_one_path"; then
      fail "dismissOne on an empty popup tier did not answer none, got: $(cat "$center_dismiss_one_path")"
    fi
  fi

  # --- M48 D3: content-tall, then capped -------------------------------
  short_height=$("$jq_bin" -r '.centerHeight' "$center_status_open_path")
  short_max=$("$jq_bin" -r '.centerMaxHeight' "$center_status_open_path")
  short_capped=$("$jq_bin" -r '.centerCapped' "$center_status_open_path")
  long_height=$("$jq_bin" -r '.centerHeight' "$center_status_long_path")
  long_max=$("$jq_bin" -r '.centerMaxHeight' "$center_status_long_path")
  long_capped=$("$jq_bin" -r '.centerCapped' "$center_status_long_path")
  echo "centre card: short ${short_height}/${short_max} capped=$short_capped, long ${long_height}/${long_max} capped=$long_capped"

  # The cap itself, against the monitor dev/smoke.sh pins rather than
  # against the shell's own arithmetic: it has to leave the bar and both
  # paddings out of the 1080.
  if [ "$short_max" -le 0 ] || [ "$short_max" -ge 1056 ]; then
    fail "the centre's cap does not leave the bar and both paddings out of the output: $short_max"
  fi
  if [ "$short_capped" != "false" ]; then
    fail "a centre holding a handful of rows reported itself capped: $(cat "$center_status_open_path")"
  fi
  if [ "$short_height" -le 0 ] || [ "$short_height" -ge "$short_max" ]; then
    fail "a short history did not produce a short card ($short_height of $short_max)"
  fi
  if [ "$long_capped" != "true" ]; then
    fail "thirty notifications did not cap the centre: $(cat "$center_status_long_path")"
  fi
  if [ "$long_height" != "$long_max" ]; then
    fail "a capped centre is not at its cap ($long_height of $long_max)"
  fi

  cat "$center_scroll_path" 2>/dev/null || true
  for f in "$center_path" "$center_long_path" "$center_scrolled_path"; do
    if [ ! -f "$f" ]; then
      fail "no centre screenshot produced at $f"
    fi
  done
  # The cheapest honest proof the wheel reached the row list: nothing else
  # on screen moves between these two frames.
  if cmp -s "$center_long_path" "$center_scrolled_path"; then
    fail "the capped centre did not move under a scroll wheel"
  fi
  echo "SMOKE_CENTER $center_path"
  echo "SMOKE_CENTER_LONG $center_long_path"
  echo "SMOKE_CENTER_SCROLLED $center_scrolled_path"

  local i path
  for f in "$center_emerge_desktop_path" "$center_emerge_rest_path"; do
    [ -f "$f" ] || fail "no centre entrance frame at $f"
  done
  echo "SMOKE_CENTER_EMERGE_DESKTOP $center_emerge_desktop_path"
  echo "SMOKE_CENTER_EMERGE_REST $center_emerge_rest_path"
  for i in $(seq 1 $center_frames); do
    path="$shot_dir/center-emerge-$i.png"
    [ -f "$path" ] || fail "no centre entrance frame at $path"
    echo "SMOKE_CENTER_EMERGE_$i $path"
  done
}
