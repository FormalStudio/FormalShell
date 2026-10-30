# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --hotcorner asks the compositor itself what layer surfaces exist, which is
# the only honest headless proof available: the rig has no synthetic pointer,
# so entering a corner cannot be simulated, but whether the corner surfaces
# mapped at all, on the right level, is entirely observable. That is also the
# part most likely to be silently wrong, since PanelWindow.anchors is driven
# per-corner from a resolved config here rather than from the literal edges
# every other surface in this shell uses. The claim is per-level, so the
# layer tree is parsed rather than grepped.
leg_hotcorner_flag="--hotcorner"
leg_hotcorner_order=160
leg_hotcorner_needs="jq"
# Nothing is summoned over the desktop, so this leg keeps the base run's
# focused fixture window in its frame.
leg_hotcorner_fixture_window=keep

hotcorner_layers_path="$shot_dir/hotcorner-layers.json"

leg_hotcorner_drive() {
  local script="$shot_dir/hotcorner-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
"$hyprctl_bin" -j layers > "$hotcorner_layers_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_hotcorner_assert() {
  local hotcorner_count hotcorner_top
  if [ ! -s "$hotcorner_layers_path" ]; then
    fail "no hyprland layer dump produced for the hot corners"
  fi
  cat "$hotcorner_layers_path"; echo
  hotcorner_count=$("$jq_bin" '[.[].levels[]?[]? | select(.namespace == "formalshell:hotcorner")] | length' "$hotcorner_layers_path")
  # Level 2 is Hyprland's `top`, the layer the corners ask for so the
  # screensaver and the lock surface can still cover a corner once fired.
  hotcorner_top=$("$jq_bin" '[.[].levels."2"[]? | select(.namespace == "formalshell:hotcorner")] | length' "$hotcorner_layers_path")
  # Two, not four: the top-right corner is "none" by default (the bar's
  # indicators own it), and the top-left one is the overview's, which has
  # no surface on a Hyprland with no HyprExpo loaded, as this one is. This
  # run writes no `hotCorners` config at all, so the count doubles as proof
  # that neither costs a surface rather than a mapped but inert one.
  if [ "$hotcorner_count" != "2" ]; then
    fail "expected 2 formalshell:hotcorner layer surfaces, found $hotcorner_count"
  fi
  if [ "$("$jq_bin" '[.[].levels[]?[]? | select(.namespace == "formalshell:hotcorner" and .x == 0 and .y == 0)] | length' "$hotcorner_layers_path")" != "0" ]; then
    fail "the overview's hot corner is mapped with no HyprExpo to open"
  fi
  grep -q 'HotCorners: overview: hyprexpo is not loaded, the corner is off' "$shell_log_path" \
    || fail "the shell did not log the overview corner staying off"
  if [ "$hotcorner_top" != "2" ]; then
    fail "expected both hot corner surfaces on the top layer, found $hotcorner_top there"
  fi
  echo "SMOKE_HOTCORNER_LAYERS $hotcorner_layers_path"
}
