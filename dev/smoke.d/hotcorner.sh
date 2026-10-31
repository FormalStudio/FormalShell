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
#
# Then a headless second output right of the first at scale 1.25 and level
# with its top, g815's eDP-1 beside its HDMI-A-1 (owner report, 2026-10-31,
# the overview corner never firing): a corner on an edge the two share can
# never be hit, so the second output's top-left and bottom-left get no
# surface, its bottom-right does, and the first keeps all three of its own
# (its bottom-right sits below the shorter second's bottom edge).
leg_hotcorner_flag="--hotcorner"
leg_hotcorner_order=160
leg_hotcorner_needs="jq"
# Nothing is summoned over the desktop, so this leg keeps the base run's
# focused fixture window in its frame.
leg_hotcorner_fixture_window=keep

hotcorner_layers_path="$shot_dir/hotcorner-layers.json"
hotcorner_pair_path="$shot_dir/hotcorner-pair-layers.json"
hotcorner_second="FS-HOTCORNER-2"

# The scaffold turns the top-left corner off for every leg; this one reads
# the shipped defaults.
leg_hotcorner_fixture() {
  settings_pin 'del(.hotCorners)'
}

leg_hotcorner_drive() {
  local script="$shot_dir/hotcorner-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
"$hyprctl_bin" -j layers > "$hotcorner_layers_path" 2>&1
width=\$("$hyprctl_bin" -j monitors | "$jq_bin" '.[0].width')
"$hyprctl_bin" output create headless "$hotcorner_second" > /dev/null 2>&1
"$hyprctl_bin" eval "hl.monitor({ output = \"$hotcorner_second\", mode = \"1600x1000@60\", position = \"\${width}x0\", scale = 1.25 })" > /dev/null 2>&1
sleep 2
"$hyprctl_bin" -j layers > "$hotcorner_pair_path" 2>&1
"$hyprctl_bin" output remove "$hotcorner_second" > /dev/null 2>&1
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
  # Three, not four: the top-right corner is "none" by default (the bar's
  # indicators own it). This run writes no `hotCorners` config at all, so
  # the count doubles as proof that a "none" corner costs no surface rather
  # than a mapped but inert one, and the top-left overview corner sits at
  # the output's origin.
  if [ "$hotcorner_count" != "3" ]; then
    fail "expected 3 formalshell:hotcorner layer surfaces, found $hotcorner_count"
  fi
  if [ "$("$jq_bin" '[.[].levels[]?[]? | select(.namespace == "formalshell:hotcorner" and .x == 0 and .y == 0)] | length' "$hotcorner_layers_path")" != "1" ]; then
    fail "the overview's top-left hot corner is not mapped"
  fi
  if [ "$hotcorner_top" != "3" ]; then
    fail "expected every hot corner surface on the top layer, found $hotcorner_top there"
  fi
  [ -s "$hotcorner_pair_path" ] || fail "no layer dump with the second output up"
  local corners
  corners=$("$jq_bin" -c --arg second "$hotcorner_second" '[to_entries[] | .key as $m | .value.levels[]?[]? | select(.namespace == "formalshell:hotcorner") | {m: (if $m == $second then "second" else "first" end), x, y}]' "$hotcorner_pair_path")
  echo "hot corners with the second output up: $corners"
  [ "$("$jq_bin" '[.[] | select(.m == "first")] | length' <<< "$corners")" = 3 ] || fail "the first output lost a corner of its own beside the second: $corners"
  [ "$("$jq_bin" '[.[] | select(.m == "second")] | length' <<< "$corners")" = 1 ] || fail "the second output carries a corner on the edge it shares with the first: $corners"
  [ "$("$jq_bin" '[.[] | select(.m == "second" and .y == 0)] | length' <<< "$corners")" = 0 ] || fail "the second output's top-left corner sits on the shared edge, where the pointer never stops: $corners"
  echo "SMOKE_HOTCORNER_LAYERS $hotcorner_layers_path"
  echo "SMOKE_HOTCORNER_SHARED_EDGE no corner on the edge the outputs share"
}
