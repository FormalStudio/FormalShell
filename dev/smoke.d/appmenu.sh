# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --appmenu: the app menu populated. `--panel appmenu` runs with no focused
# window, so it only ever shows the empty "No window" state; this leg keeps
# the base run's fixture window (app id formalshell-smoke-iconic, whose
# desktop entry names it "Iconic Test App" with a themed icon) focused and
# opens the panel over the real `panel` route: the hero carries the entry's
# name and picture, and the window list holds the fixture window.
leg_appmenu_flag="--appmenu"
leg_appmenu_order=62
leg_appmenu_claims="panel=shared"
leg_appmenu_fixture_window=keep

appmenu_open_path="$shot_dir/appmenu-open.txt"
appmenu_state_path="$shot_dir/appmenu-state.txt"
appmenu_png="$shot_dir/appmenu-populated.png"

leg_appmenu_timing() {
  leg_timing 40
}

leg_appmenu_drive() {
  local script="$shot_dir/appmenu-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# The fixture window is spawned two seconds in and needs a moment to map and
# take focus before the held-focus window the menu reads is it.
$ipc call panel open appmenu > "$appmenu_open_path" 2>&1
sleep 2
$ipc call panel state > "$appmenu_state_path" 2>&1
"$grim_bin" "$appmenu_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_appmenu_assert() {
  if ! grep -q '^ok$' "$appmenu_open_path" 2>/dev/null; then
    fail "panel open appmenu did not answer ok, got: $(cat "$appmenu_open_path" 2>/dev/null)"
  fi
  if [ "$(head -n1 "$appmenu_state_path" 2>/dev/null | tr -d '\r')" != "appmenu" ]; then
    fail "panel state is not appmenu, got: $(cat "$appmenu_state_path" 2>/dev/null)"
  fi
  [ -s "$appmenu_png" ] || fail "no screenshot of the open app menu at $appmenu_png"
  echo "SMOKE_APPMENU $appmenu_png"
}
