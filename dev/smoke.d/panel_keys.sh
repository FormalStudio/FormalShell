# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --panel-keys opens the audio panel and drives it with real `wtype`
# keystrokes rather than the IPC shortcuts every other panel leg uses. This is
# the one thing that can prove row-level keyboard navigation reaches the
# surface at all, since the shared cursor state only ever moves from a real Qt
# key event.
#
# Down, Down, Return lands on the sink's mute row, and the proof is the sink
# itself: `wpctl get-volume` is read before and after, and the mute state has
# to have flipped. A frame pair alone could not carry that claim here, because
# this VM's bar redraws something (a clock digit, an animated glyph) on almost
# any 1-2s window, so two frames differ whether or not a key landed.
#
# The old rig recorded the opposite outcome for the same sequence: four
# independently timed probes there never got a Down/Down/Return through to an
# IPC-opened panel, because the panel's focus-prime dance is built for a real
# bar-cell click and that rig had no pointer to supply one. Hyprland hands the
# surface keyboard focus on its own, so the route works here.
leg_panel_keys_flag="--panel-keys"
leg_panel_keys_order=250
leg_panel_keys_needs="wtype wpctl"
leg_panel_keys_claims="panel=shared"

panel_keys_baseline_path="$shot_dir/panel-keys-baseline.png"
panel_keys_path="$shot_dir/panel-keys.png"
panel_keys_mute_before_path="$shot_dir/panel-keys-mute-before.txt"
panel_keys_mute_after_path="$shot_dir/panel-keys-mute-after.txt"

leg_panel_keys_timing() {
  leg_timing 45
}

leg_panel_keys_drive() {
  local script="$shot_dir/panel-keys-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# pipewire outlives the session, so an earlier run of this leg leaves the
# sink muted and the keystrokes would flip it the other way. Pinned unmuted
# first, which makes the assertion below one direction every run.
"$wpctl_bin" set-mute @DEFAULT_AUDIO_SINK@ 0 > /dev/null 2>&1
$ipc call panel open audio > /dev/null 2>&1
until_settled
"$wpctl_bin" get-volume @DEFAULT_AUDIO_SINK@ > "$panel_keys_mute_before_path" 2>&1
"$grim_bin" "$panel_keys_baseline_path" > /dev/null 2>&1
"$wtype_bin" -k Down -k Down -k Return
sleep 1
"$grim_bin" "$panel_keys_path" > /dev/null 2>&1
"$wpctl_bin" get-volume @DEFAULT_AUDIO_SINK@ > "$panel_keys_mute_after_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_panel_keys_assert() {
  [ -f "$panel_keys_baseline_path" ] || fail "no panel-keys-baseline screenshot produced"
  echo "SMOKE_PANEL_KEYS_BASELINE $panel_keys_baseline_path"
  [ -f "$panel_keys_path" ] || fail "no panel-keys screenshot produced"
  echo "SMOKE_PANEL_KEYS $panel_keys_path"
  for f in "$panel_keys_mute_before_path" "$panel_keys_mute_after_path"; do
    [ -s "$f" ] || fail "no wpctl readback produced at $f"
  done
  cat "$panel_keys_mute_before_path"
  cat "$panel_keys_mute_after_path"
  # wpctl prints "Volume: 0.30 [MUTED]" only while the sink is muted, so the
  # marker's presence is the whole state.
  if grep -qF '[MUTED]' "$panel_keys_mute_before_path"; then
    fail "the sink is muted despite this leg's own set-mute 0, so nothing here can tell whether the keystrokes landed"
  fi
  if ! grep -qF '[MUTED]' "$panel_keys_mute_after_path"; then
    fail "Down/Down/Return did not reach the audio panel: the sink is still unmuted ($(cat "$panel_keys_mute_after_path"))"
  fi
}
