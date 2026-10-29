# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --systemupdate: the flake-inputs-behind panel against a fixture flake
# (fixtures/flake, the lock this repo carried while it was one). Naming the
# bar cell in bar.layout IS the opt-in to background polling (the widget
# flips the panel's pollEnabled), so the cell and the panel read the same
# real poll rather than two independent ones. That is
# what this leg has over `--panel systemupdate`, which only opens the panel.
#
# The probes are real `git ls-remote`-class calls against real forges, so the
# settle wait is generous and every end state is a real answer: N BEHIND, ALL
# CURRENT, NO NETWORK or CHECKING. Nothing here asserts a count: that would
# be asserting the state of github, not the state of this panel.
leg_systemupdate_flag="--systemupdate"
leg_systemupdate_order=270
leg_systemupdate_needs="jq"

systemupdate_open_reply_path="$shot_dir/systemupdate-open-reply.txt"
systemupdate_state_path="$shot_dir/systemupdate-panel-state.txt"
systemupdate_panel_png="$shot_dir/systemupdate-panel.png"
# The same file --dump's own leg writes.
systemupdate_dump_path="$shot_dir/dump.json"
systemupdate_room_path="$shot_dir/systemupdate-room.json"
systemupdate_flake_dir="$PWD/dev/smoke.d/fixtures/flake"

leg_systemupdate_fixture() {
  # --panel systemupdate writes the identical key, so only one of the two
  # ever emits it: a settings.json carrying it twice is last-wins JSON that
  # would silently disagree with whichever half is being read.
  if ! leg_on panel || [ "$(leg_arg panel)" != "systemupdate" ]; then
    settings_fragment ', "systemUpdate": {"flakeDir": "'"$systemupdate_flake_dir"'"}'
  fi
  settings_fragment ', "bar": {"layout": {"right": ["systemUpdate", "battery", "audio", "network", "bluetooth", "weather", "tray", "bell", "indicators"]}}'
}

leg_systemupdate_timing() {
  leg_timing 30 60
}

leg_systemupdate_drive() {
  local script="$shot_dir/systemupdate-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
$ipc call panel open systemupdate > "$systemupdate_open_reply_path" 2>&1
sleep 1
$ipc call panel state > "$systemupdate_state_path" 2>&1
sleep 20
"$grim_bin" "$systemupdate_panel_png" > /dev/null 2>&1
$ipc call debug dump > "$systemupdate_dump_path" 2>&1
$ipc call bar room > "$systemupdate_room_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_systemupdate_assert() {
  if ! grep -q '^ok$' "$systemupdate_open_reply_path" 2>/dev/null; then
    fail "panel open systemupdate did not return ok, got: $(cat "$systemupdate_open_reply_path" 2>/dev/null)"
  fi
  if [ "$(head -n1 "$systemupdate_state_path" 2>/dev/null | tr -d '\r')" != "systemupdate" ]; then
    fail "panel state did not report systemupdate open, got: $(cat "$systemupdate_state_path" 2>/dev/null)"
  fi
  if [ ! -s "$systemupdate_dump_path" ]; then
    fail "no debug dump produced"
  fi
  if ! grep -qF "\"flakeDir\":\"$systemupdate_flake_dir\"" "$systemupdate_dump_path"; then
    fail "the resolved settings do not point systemUpdate.flakeDir at the fixture flake ($systemupdate_flake_dir)"
  fi
  if ! grep -qF '"right":["systemUpdate"' "$systemupdate_dump_path"; then
    fail "the resolved settings do not lead bar.layout's right region with systemUpdate, the opt-in cell was never placed, so nothing flipped the panel's pollEnabled"
  fi
  # One shared poll behind the cell: the cell has to be on the strip, whole.
  jq -e '.[].cells[] | select(.name == "systemUpdate" and .whole == true and .width > 0)' "$systemupdate_room_path" > /dev/null \
    || fail "the systemUpdate cell is not whole on the strip: $(cat "$systemupdate_room_path" 2>/dev/null)"
  [ -f "$systemupdate_panel_png" ] || fail "no systemupdate screenshot produced at $systemupdate_panel_png"
  echo "SMOKE_SYSTEMUPDATE $systemupdate_panel_png (against $systemupdate_flake_dir/flake.lock; whatever the real probes answered is what rendered)"
}
