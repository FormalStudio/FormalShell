# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --monitor: the opt-in bar cell, its compact panel, and the launcher's full
# monitor route, off one
# timeline. Never-faked data is the actual claim: `monitor gpu` has to name
# exactly the cards this machine's own /sys/class/drm holds (here that is
# the vkms card the session renders on, which reports no utilisation counter
# at all and so renders as the view's NO METRICS cell) while `monitor
# status`, taken in the same breath, carries real CPU and memory numbers off
# the VM's own /proc.
#
# The cell is also read off `bar room`, whole on the
# strip.
leg_monitor_flag="--monitor"
leg_monitor_order=140
leg_monitor_needs="jq"
leg_monitor_claims="launcher=shared bar.layout.right"

monitor_status_path="$shot_dir/monitor-status.json"
monitor_gpu_path="$shot_dir/monitor-gpu.json"
monitor_panel_reply_path="$shot_dir/monitor-panel-reply.txt"
monitor_panel_state_path="$shot_dir/monitor-panel-state.txt"
monitor_menu_reply_path="$shot_dir/monitor-menu-reply.txt"
monitor_menu_status_path="$shot_dir/monitor-menu-status.json"
monitor_bar_png="$shot_dir/monitor-bar.png"
monitor_panel_png="$shot_dir/monitor-panel.png"
monitor_view_png="$shot_dir/monitor-view.png"
# The same file --dump's own leg writes, and deliberately so: this leg reads
# the resolved settings back to prove the opt-in cell was placed at all.
monitor_dump_path="$shot_dir/dump.json"
monitor_room_path="$shot_dir/monitor-room.json"

leg_monitor_fixture() {
  # `monitor` is an opt-in builtin (absent from DEFAULT_LAYOUT), so naming
  # it here is the whole install of the bar cell. Leading the right region
  # rather than appended, so a wide CPU/MEM/GPU cell cannot be clipped off
  # the end.
  settings_pin '.bar.layout.right = ["monitor", "battery", "audio", "network", "bluetooth", "weather", "tray", "bell", "indicators"]'
}

leg_monitor_timing() {
  leg_timing 50
}

leg_monitor_drive() {
  local script="$shot_dir/monitor-drive.sh"
  local menu_lines="sleep 1
$ipc call menu summon monitor > \"$monitor_menu_reply_path\" 2>&1
sleep 3
$ipc call menu status > \"$monitor_menu_status_path\" 2>&1
\"$grim_bin\" \"$monitor_view_png\" > /dev/null 2>&1"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# CPU is a delta: the aggregate is null until the second sample.
until_jq 15 "$monitor_status_path" '.cpu.aggregate != null' call monitor status
$ipc call monitor gpu > "$monitor_gpu_path" 2>&1
$ipc call bar room > "$monitor_room_path" 2>&1
$ipc call debug dump > "$monitor_dump_path" 2>&1
"$grim_bin" "$monitor_bar_png" > /dev/null 2>&1
sleep 1
$ipc call panel open monitor > "$monitor_panel_reply_path" 2>&1
sleep 2
$ipc call panel state > "$monitor_panel_state_path" 2>&1
"$grim_bin" "$monitor_panel_png" > /dev/null 2>&1
sleep 1
$ipc call panel close > /dev/null 2>&1
$menu_lines
EOF
  hypr_exec_once "bash $script"
}

leg_monitor_assert() {
  local sys_card_count gpu_card_count
  if [ ! -s "$monitor_status_path" ]; then
    fail "no monitor status produced"
  fi
  cat "$monitor_status_path"; echo
  # cpuDelta returns null until it has two /proc/stat samples (the surfaces
  # render that as a dash, never a fabricated 0%), so a real fraction here
  # proves the collector is ticking, not merely that the service loaded.
  if ! grep -q '"cpu":{"available":true,"aggregate":[0-9]' "$monitor_status_path"; then
    fail "monitor status carries no real CPU sample: $(cat "$monitor_status_path")"
  fi
  if ! grep -q '"mem":{"available":true,"totalBytes":[1-9]' "$monitor_status_path"; then
    fail "monitor status carries no real memory sample: $(cat "$monitor_status_path")"
  fi
  if [ ! -s "$monitor_gpu_path" ]; then
    fail "no monitor gpu produced"
  fi
  cat "$monitor_gpu_path"; echo
  # The card list has to be the machine's own, counted straight off /sys
  # rather than pinned to a rig: the vkms session here holds one card, a
  # nested run on a real host holds that host's, and a headless box holds
  # none. Connector entries (card0-Virtual-1) are not cards, hence the
  # anchored match.
  sys_card_count=$(ls /sys/class/drm 2>/dev/null | grep -cE '^card[0-9]+$' || true)
  gpu_card_count=$(grep -oF '"card":"card' "$monitor_gpu_path" | wc -l | tr -d ' ')
  if [ "$sys_card_count" != "$gpu_card_count" ]; then
    fail "monitor gpu reported $gpu_card_count cards, /sys/class/drm holds $sys_card_count: $(cat "$monitor_gpu_path")"
  fi
  if [ "$sys_card_count" = "0" ] && ! grep -qF '"available":false,"cards":[]' "$monitor_gpu_path"; then
    fail "a machine with no card did not report the honest no-GPU state: $(cat "$monitor_gpu_path")"
  fi
  # The cell is opt-in, so the screenshot below only means something if the
  # layout the shell resolved is the one the fixture asked for.
  if [ ! -s "$monitor_dump_path" ]; then
    fail "no debug dump produced"
  fi
  if ! grep -qF '"right":["monitor"' "$monitor_dump_path"; then
    fail "the resolved settings do not lead bar.layout's right region with monitor, the opt-in cell was never placed"
  fi
  if [ ! -f "$monitor_bar_png" ]; then
    fail "no monitor-bar screenshot produced"
  fi
  echo "SMOKE_MONITOR_BAR $monitor_bar_png"
  if ! grep -q '^ok$' "$monitor_panel_reply_path" 2>/dev/null; then
    fail "panel open monitor did not answer ok, got: $(cat "$monitor_panel_reply_path" 2>/dev/null)"
  fi
  if ! grep -q '^monitor$' "$monitor_panel_state_path" 2>/dev/null; then
    fail "panel state did not report monitor open, got: $(cat "$monitor_panel_state_path" 2>/dev/null)"
  fi
  if [ ! -f "$monitor_panel_png" ]; then
    fail "no monitor-panel screenshot produced"
  fi
  echo "SMOKE_MONITOR_PANEL $monitor_panel_png"
  jq -e '.[].cells[] | select(.name == "monitor" and .whole == true and .width > 0)' "$monitor_room_path" > /dev/null \
    || fail "the monitor cell is not whole on the strip: $(cat "$monitor_room_path" 2>/dev/null)"
  if ! grep -q '^ok$' "$monitor_menu_reply_path" 2>/dev/null; then
    fail "menu summon monitor did not answer ok, got: $(cat "$monitor_menu_reply_path" 2>/dev/null)"
  fi
  # The claim the frame cannot make on its own: the launcher is sitting on
  # the monitor route itself (an app view), not on a row list that happens
  # to mention it.
  if ! grep -qF '"isOpen":true,"level":"monitor"' "$monitor_menu_status_path" 2>/dev/null; then
    fail "the launcher is not on the monitor app view: $(cat "$monitor_menu_status_path" 2>/dev/null)"
  fi
  cat "$monitor_menu_status_path"; echo
  if [ ! -f "$monitor_view_png" ]; then
    fail "no monitor-view screenshot produced"
  fi
  echo "SMOKE_MONITOR_VIEW $monitor_view_png"
}
