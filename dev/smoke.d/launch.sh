# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --launch proves launch feedback (`services/launch.rs`, Plan.md phase 6):
# a desktop entry the shell spawns is a running `launch:<id>` item in
# Activity until a window of it maps, and one that maps none reads failed
# once fs_activity's LAUNCH_MS (15s) has passed.
#
# Two staged entries: Launch Probe, a foot window of its own app id that
# maps two seconds after the spawn, and Launch Ghost, a `sleep` that never
# maps one. The probe is launched twice, by `activity launch` (the verb a
# keybind calls) and by the launcher's own row, and read off `activity
# status` and `bar room` while it waits (running, the bar's indicators cell
# up, no toast card) and after its window maps (gone, nothing folded into
# the history). The ghost is read running, then failed with its sentence
# 16s on. An id with no entry answers the error string.
leg_launch_flag="--launch"
leg_launch_order=36
leg_launch_needs="jq foot"

launch_probe=formalshell-launch-probe
launch_ghost=formalshell-launch-ghost

leg_launch_fixture() {
  mkdir -p "$iso_home/.local/share/applications"
  cat > "$iso_home/.local/share/applications/$launch_probe.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Launch Probe
Exec=sh -c "sleep 2; exec $foot_bin --app-id=$launch_probe sleep 300"
EOF
  cat > "$iso_home/.local/share/applications/$launch_ghost.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Launch Ghost
Exec=sleep 60
EOF
}

leg_launch_timing() {
  leg_timing 45 75
}

leg_launch_drive() {
  local script="$shot_dir/launch-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
status() { call activity status > "$shot_dir/launch-\$1.json" 2>&1; call bar room > "$shot_dir/launch-room-\$1.json" 2>&1; }
shoot() { "$grim_bin" "$shot_dir/launch-\$1.png" > /dev/null 2>&1; }
sleep 4
status idle
call activity launch $launch_probe > "$shot_dir/launch-reply-ipc.txt" 2>&1
sleep 0.5
status ipc-running
shoot ipc-running
sleep 4
status ipc-mapped
"$hyprctl_bin" clients -j > "$shot_dir/launch-clients-ipc.json" 2>&1
pkill -f -- "--app-id=$launch_probe"
sleep 1
call menu summon "" > /dev/null 2>&1
sleep 0.4
call menu filter "Launch Probe" > /dev/null 2>&1
sleep 0.6
call menu status > "$shot_dir/launch-menu.json" 2>&1
call menu activate 0 > /dev/null 2>&1
sleep 0.5
status menu-running
sleep 4
status menu-mapped
call activity launch $launch_ghost > /dev/null 2>&1
sleep 1
status ghost-running
sleep 16
status ghost-failed
shoot ghost-failed
call activity launch nothing-here > "$shot_dir/launch-reply-unknown.txt" 2>&1
EOF
  hypr_exec_once "bash $script"
}

# The group of launch:<id> in one status capture, or null.
launch_group() {
  jq -c --arg s "launch:$2" '[.groups[] | select(.source == $s)][0]' "$shot_dir/launch-$1.json" 2>/dev/null
}

launch_expect() {
  local name=$1 filter=$2 why=$3
  jq -e "$filter" "$shot_dir/launch-$name.json" > /dev/null 2>&1 || fail "launch: $why: $(cat "$shot_dir/launch-$name.json" 2>/dev/null)"
}

leg_launch_assert() {
  local indicators='[.[0].cells[] | select(.name == "indicators")] | length'
  for f in ipc-running ghost-failed; do
    [ -f "$shot_dir/launch-$f.png" ] || fail "no screenshot produced at $shot_dir/launch-$f.png"
    f=${f//-/_}
    echo "SMOKE_LAUNCH_${f^^} $shot_dir/launch-${f//_/-}.png"
  done
  grep -qx ok "$shot_dir/launch-reply-ipc.txt" || fail "activity launch answered '$(cat "$shot_dir/launch-reply-ipc.txt")', not ok"
  jq -e "$indicators == 0" "$shot_dir/launch-room-idle.json" > /dev/null || fail "launch: an indicators cell showed before any launch"

  local probe="[.groups[] | select(.source == \"launch:$launch_probe\")]"
  for phase in ipc menu; do
    echo "launch: $phase running: $(launch_group "$phase-running" "$launch_probe")"
    launch_expect "$phase-running" "$probe | length == 1 and .[0].state == \"running\" and .[0].items[0].kind == \"launch\"" \
      "the $phase launch is not a running launch item while its window is still to map"
    launch_expect "$phase-running" "(.toasts | index(\"launch:$launch_probe\")) == null" "the $phase launch raised a toast card"
    jq -e "$indicators == 1" "$shot_dir/launch-room-$phase-running.json" > /dev/null \
      || fail "launch: no indicators cell in the bar while the $phase launch waits: $(cat "$shot_dir/launch-room-$phase-running.json")"
    launch_expect "$phase-mapped" "$probe | length == 0" "the $phase launch is still live after its window mapped"
    launch_expect "$phase-mapped" "[.history[] | select(.id == \"launch:$launch_probe\")] | length == 0" "a launch that mapped folded into the history"
  done
  jq -e "[.[] | select(.class == \"$launch_probe\")] | length >= 1" "$shot_dir/launch-clients-ipc.json" > /dev/null \
    || fail "launch: the probe's window never mapped: $(cat "$shot_dir/launch-clients-ipc.json")"

  local ghost="[.groups[] | select(.source == \"launch:$launch_ghost\")]"
  launch_expect ghost-running "$ghost | length == 1 and .[0].state == \"running\"" "the ghost is not running a second in"
  echo "launch: ghost after 16s: $(launch_group ghost-failed "$launch_ghost")"
  launch_expect ghost-failed "$ghost | length == 1 and .[0].state == \"failed\" and .[0].items[0].summary == \"Launch Ghost has not opened a window\"" \
    "the ghost does not read failed with its sentence 16s on"
  grep -qx "error: no desktop entry 'nothing-here'" "$shot_dir/launch-reply-unknown.txt" \
    || fail "activity launch on an unknown id answered '$(cat "$shot_dir/launch-reply-unknown.txt")'"
}
