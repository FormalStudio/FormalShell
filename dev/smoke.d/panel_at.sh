# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --panel-at <n> drives `panel toggleAt <n>`, the positional keybind (M42
# D4, SUPER+CTRL+1..9 in the shipped Hyprland example). What needs proving
# is that the index walked the resolved right region and stopped on a
# panel-bearing cell, so `panel state` is read back beside the route's own
# answer: a toggleAt that returned ok while opening the wrong panel would
# look identical here without it. Its own grim frame, since the run's
# smoke.png is timestamped by the time dev/remote.sh has pulled it.
leg_panel_at_flag="--panel-at <n>"
leg_panel_at_order=70

panel_at_toggle_path="$shot_dir/panel-at-toggle.txt"
panel_at_state_path="$shot_dir/panel-at-state.txt"
panel_at_path="$shot_dir/panel-at.json"
panel_at_shot_path="$shot_dir/panel-at.png"

# The default right region (Bar/layout.js's DEFAULT_LAYOUT) in order, with
# the cells that open no panel dropped (tray, bell, indicators). This run
# writes no bar.layout, so `panel toggleAt <n>` has to land on the nth of
# these, which is the assertion below.
panel_at_expected=""
panel_at_defaults=(power audio network bluetooth weather)

leg_panel_at_validate() {
  local n
  n=$(leg_arg panel_at)
  case "$n" in
    [1-9]) ;;
    *) echo "usage: $0 --panel-at <n>, 1..${#panel_at_defaults[@]}" >&2; exit 1 ;;
  esac
  if [ "$n" -gt "${#panel_at_defaults[@]}" ]; then
    echo "usage: $0 --panel-at <n>, the default right region has only ${#panel_at_defaults[@]} panel cells" >&2
    exit 1
  fi
  panel_at_expected="${panel_at_defaults[$((n - 1))]}"
}

leg_panel_at_drive() {
  local n script="$shot_dir/panel-at-drive.sh"
  n=$(leg_arg panel_at)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3
$ipc call panel toggleAt $n > "$panel_at_toggle_path" 2>&1
sleep 2
$ipc call panel state > "$panel_at_state_path" 2>&1
"$grim_bin" "$panel_at_shot_path" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_panel_at_assert() {
  local n
  n=$(leg_arg panel_at)
  if [ ! -s "$panel_at_toggle_path" ] || [ ! -s "$panel_at_state_path" ]; then
    fail "panel toggleAt $n produced no result"
  fi
  printf '{"n":%s,"toggleAt":"%s","state":"%s","expected":"%s"}\n' \
    "$n" "$(cat "$panel_at_toggle_path")" "$(cat "$panel_at_state_path")" "$panel_at_expected" \
    > "$panel_at_path"
  cat "$panel_at_path"
  if ! grep -q "^ok$" "$panel_at_toggle_path"; then
    fail "panel toggleAt $n did not answer ok, got: $(cat "$panel_at_toggle_path")"
  fi
  if ! grep -q "^$panel_at_expected$" "$panel_at_state_path"; then
    fail "panel toggleAt $n opened '$(cat "$panel_at_state_path")', expected '$panel_at_expected'"
  fi
  if [ ! -f "$panel_at_shot_path" ]; then
    fail "no panel-at screenshot produced"
  fi
  echo "SMOKE_PANEL_AT $panel_at_shot_path"
}
