# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --caffeinate starts the shell with caffeinate.onStartup and a 3s
# screensaver timeout, then reads the session idle state off the real
# ext-idle-notify monitor rather than the toggle: with the inhibitor held the
# session sits well past the timeout without going idle, and after
# `caffeinate disable` the same timeout fires the screensaver on its own. The
# inhibitor's surface is read off `hyprctl -j layers` both ways.
leg_caffeinate_flag="--caffeinate"
leg_caffeinate_order=232
leg_caffeinate_needs="jq"
leg_caffeinate_claims="caffeinate screensaver.timeoutSeconds=3"

caffeinate_on_path="$shot_dir/caffeinate-status-on.json"
caffeinate_on_layers="$shot_dir/caffeinate-layers-on.json"
caffeinate_held_path="$shot_dir/caffeinate-screensaver-held.json"
caffeinate_off_path="$shot_dir/caffeinate-status-off.json"
caffeinate_off_layers="$shot_dir/caffeinate-layers-off.json"
caffeinate_idle_path="$shot_dir/caffeinate-screensaver-idle.json"

leg_caffeinate_fixture() {
  settings_pin '.caffeinate.onStartup = true | .screensaver.timeoutSeconds = 3'
}

leg_caffeinate_timing() {
  leg_timing 50
}

leg_caffeinate_drive() {
  local script="$shot_dir/caffeinate-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# The inhibitor is held once its surface maps, a beat after the toggle.
until_jq 10 "$caffeinate_on_path" '.active and .inhibiting' call caffeinate status
"$hyprctl_bin" -j layers > "$caffeinate_on_layers" 2>&1
# Three timeouts' worth of no input at all.
sleep 9
$ipc call screensaver status > "$caffeinate_held_path" 2>&1
$ipc call caffeinate disable > /dev/null 2>&1
sleep 1
$ipc call caffeinate status > "$caffeinate_off_path" 2>&1
"$hyprctl_bin" -j layers > "$caffeinate_off_layers" 2>&1
sleep 6
$ipc call screensaver status > "$caffeinate_idle_path" 2>&1
$ipc call screensaver stop > /dev/null 2>&1
$ipc call caffeinate enable > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

caffeinate_layer_count() {
  jq '[.. | objects | select(.namespace? == "formalshell:caffeinate")] | length' "$1"
}

leg_caffeinate_assert() {
  local f
  for f in "$caffeinate_on_path" "$caffeinate_held_path" "$caffeinate_off_path" "$caffeinate_idle_path"; do
    [ -s "$f" ] || fail "caffeinate: $f missing or empty"
    cat "$f"; echo
  done
  [ -s "$caffeinate_on_layers" ] || fail "caffeinate: no layer dump while caffeinated"
  [ -s "$caffeinate_off_layers" ] || fail "caffeinate: no layer dump after disable"
  jq -e '.active == true and .inhibiting == true' "$caffeinate_on_path" > /dev/null \
    || fail "caffeinate.onStartup did not start the session caffeinated with the inhibitor held"
  [ "$(caffeinate_layer_count "$caffeinate_on_layers")" = "1" ] \
    || fail "no formalshell:caffeinate layer surface while caffeinated"
  local held_filter='.isIdle == false and .active == false and .caffeinated == true'
  local idle_filter='.isIdle == true and .active == true'
  jq -e "$held_filter" "$caffeinate_held_path" > /dev/null \
    || fail "the session went idle past the screensaver timeout while caffeinated"
  jq -e '.active == false and .inhibiting == false' "$caffeinate_off_path" > /dev/null \
    || fail "caffeinate disable left the inhibitor reported held"
  [ "$(caffeinate_layer_count "$caffeinate_off_layers")" = "0" ] \
    || fail "the formalshell:caffeinate layer surface outlived caffeinate disable"
  jq -e "$idle_filter" "$caffeinate_idle_path" > /dev/null \
    || fail "the screensaver timeout did not fire once caffeinate was off"
}
