# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --quick-settings-devices: the quick settings card's split toggles against
# the guest's own daemons (docs/superpowers/specs/2026-11-04-quick-settings.md
# §3). FORMALTEST, one of the two hostapd radios' SSIDs, is saved and left
# disconnected through the shell's own `network` verbs first. Wi-Fi is a
# split toggle, on, and its menu (`quicksettings menu wifi`, which has
# NetworkManager scan while it is open) lists both SSIDs with a lock on each
# and "All networks" last. Real Downs walk to FORMALTEST and a real Return
# connects it, the tile then carrying the SSID as its subtitle and the row
# its check; a Return on FORMALTEST-EAP, secured and unknown, hands the card
# over to the network panel with that network's passphrase step open
# (`network status`'s `prompt`). `quicksettings activate wifi` turns the
# radio off and on, read off `nmcli radio wifi`. The guest's wired NIC is
# the Wired tile carrying its active connection's name, as `nmcli` gives
# it. Bluetooth, BlueZ with no controller, is disabled reading "No adapter"
# with no menu. Power mode's menu lists the daemon's profiles with the
# active one checked; real Downs and Return pick Power saver, read off
# busctl's ActiveProfile, and the tile's own activate flips it back to
# balanced. The Wi-Fi menu and the hand-over photographed, dark and with
# `--light`.
leg_quick_settings_devices_flag="--quick-settings-devices"
leg_quick_settings_devices_order=283
leg_quick_settings_devices_vm_only="it drives the guest's NetworkManager, hostapd radios, BlueZ and power-profiles-daemon"
leg_quick_settings_devices_needs="jq wtype grim"
leg_quick_settings_devices_claims="wifi.radios"

quick_settings_devices_dir="$shot_dir/quick-settings-devices"

leg_quick_settings_devices_timing() {
  # Every wait below is a poll that breaks once its state settles; the sum
  # is what NetworkManager and hostapd can take at worst.
  leg_timing 190
}

leg_quick_settings_devices_drive() {
  local script="$shot_dir/quick-settings-devices-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
d="$quick_settings_devices_dir"
mkdir -p "\$d"
shot() {
  "$grim_bin" "\$d/quick-settings-devices-\$1.png" > /dev/null 2>&1
  status quicksettings "\$d/status-shot-\$1.json"
}
# Real Downs inside the open menu until the cursor is on \$1.
down_to() {
  local n=0
  while [ \$n -lt 12 ]; do
    [ "\$(call quicksettings status 2>/dev/null | "$jq_bin" -r '.cursor // ""')" = "\$1" ] && return 0
    "$wtype_bin" -k Down
    sleep 0.4
    n=\$((n + 1))
  done
}
active_profile() {
  busctl --system get-property org.freedesktop.UPower.PowerProfiles /org/freedesktop/UPower/PowerProfiles \\
    org.freedesktop.UPower.PowerProfiles ActiveProfile 2>&1 | sed 's/^s "\\(.*\\)"\$/\\1/'
}
nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device > "\$d/nmcli-devices.txt" 2>&1

# Both SSIDs in a scan, then FORMALTEST saved and down.
until_jq 45 "\$d/scan.json" '[.networks[] | select(.inRange) | .name] | index("FORMALTEST") != null and index("FORMALTEST-EAP") != null' call network status
if ! "$jq_bin" -e '.networks[] | select(.name == "FORMALTEST") | .known' "\$d/scan.json" > /dev/null 2>&1; then
  call network connect FORMALTEST formaltest-psk > "\$d/save.txt" 2>&1
  until_jq 30 "\$d/connected-once.json" '.networks[] | select(.name == "FORMALTEST") | .connected and (.stateChanging | not)' call network status
fi
call network status > "\$d/saved.json" 2>&1
if "$jq_bin" -e '.networks[] | select(.name == "FORMALTEST") | .connected' "\$d/saved.json" > /dev/null 2>&1; then
  call network connect FORMALTEST "" > "\$d/down.txt" 2>&1
fi
until_jq 30 "\$d/ready.json" '.networks[] | select(.name == "FORMALTEST") | .known and (.connected | not) and (.stateChanging | not)' call network status

call panel open quicksettings > /dev/null 2>&1
until_settled
status quicksettings "\$d/status-open.json"
call quicksettings menu bluetooth > "\$d/menu-bluetooth.txt" 2>&1
call quicksettings menu wifi > "\$d/menu-wifi.txt" 2>&1
until_jq 20 "\$d/status-wifi-menu.json" '.menu.content == 1 and ([.menu.rows[].id] | index("m:wifi:FORMALTEST") != null and index("m:wifi:FORMALTEST-EAP") != null)' call quicksettings status
down_to m:wifi:FORMALTEST
status quicksettings "\$d/status-at-formaltest.json"
"$wtype_bin" -k Return
until_jq 30 "\$d/status-wifi-connected.json" '.rows[] | select(.id == "wifi") | .subtitle == "FORMALTEST"' call quicksettings status
call network status > "\$d/network-connected.json" 2>&1
sleep 1
shot wifi-menu
down_to m:wifi:FORMALTEST-EAP
status quicksettings "\$d/status-at-eap.json"
"$wtype_bin" -k Return
sleep 1.5
call panel state > "\$d/panel-handed.txt" 2>&1
call network status > "\$d/network-prompt.json" 2>&1
"$grim_bin" "\$d/quick-settings-devices-prompt.png" > /dev/null 2>&1
call panel close > /dev/null 2>&1
sleep 1.2

call panel open quicksettings > /dev/null 2>&1
until_settled
status quicksettings "\$d/status-reopen.json"
nmcli radio wifi > "\$d/radio-before.txt" 2>&1
call quicksettings activate wifi > "\$d/activate-off.txt" 2>&1
for i in \$(seq 1 20); do
  [ "\$(nmcli radio wifi 2>/dev/null)" = disabled ] && break
  sleep 0.5
done
nmcli radio wifi > "\$d/radio-off.txt" 2>&1
until_jq 10 "\$d/status-wifi-off.json" '.rows[] | select(.id == "wifi") | .on == false' call quicksettings status
call quicksettings activate wifi > "\$d/activate-on.txt" 2>&1
for i in \$(seq 1 20); do
  [ "\$(nmcli radio wifi 2>/dev/null)" = enabled ] && break
  sleep 0.5
done
nmcli radio wifi > "\$d/radio-on.txt" 2>&1
until_jq 10 "\$d/status-wifi-on.json" '.rows[] | select(.id == "wifi") | .on == true' call quicksettings status

active_profile > "\$d/profile-before.txt"
call quicksettings menu power-mode > "\$d/menu-power-mode.txt" 2>&1
until_jq 10 "\$d/status-power-menu.json" '.menu.id == "power-mode" and .menu.content == 1' call quicksettings status
down_to m:power-mode:power-saver
status quicksettings "\$d/status-at-saver.json"
"$wtype_bin" -k Return
for i in \$(seq 1 20); do
  [ "\$(active_profile)" = power-saver ] && break
  sleep 0.5
done
active_profile > "\$d/profile-saver.txt"
until_jq 10 "\$d/status-saver.json" '.rows[] | select(.id == "power-mode") | .on == true' call quicksettings status
call quicksettings back > /dev/null 2>&1
sleep 1
call quicksettings activate power-mode > "\$d/activate-power-mode.txt" 2>&1
for i in \$(seq 1 20); do
  [ "\$(active_profile)" = balanced ] && break
  sleep 0.5
done
active_profile > "\$d/profile-flipped.txt"
until_jq 10 "\$d/status-balanced.json" '.rows[] | select(.id == "power-mode") | .on == false' call quicksettings status
call panel close > /dev/null 2>&1
sleep 1
EOF
  wm_exec_once "bash $script"
}

quick_settings_devices_jq() {
  local what="$1" file="$quick_settings_devices_dir/$2" expr="$3"
  [ -s "$file" ] || fail "quick-settings-devices: no $2 for $what"
  "$jq_bin" -e "$expr" "$file" > /dev/null 2>&1 \
    || fail "quick-settings-devices: $what: $("$jq_bin" -c '{cursor, menu, rows: [.rows[]? | {id, kind, on, available, reason, subtitle}]}' "$file" 2>/dev/null || head -c 1500 "$file")"
  echo "SMOKE_QUICK_SETTINGS_DEVICES $what"
}

quick_settings_devices_text() {
  local what="$1" file="$quick_settings_devices_dir/$2" want="$3"
  [ "$(cat "$file" 2>/dev/null)" = "$want" ] || fail "quick-settings-devices: $what: expected '$want', got '$(cat "$file" 2>/dev/null)'"
  echo "SMOKE_QUICK_SETTINGS_DEVICES $what"
}

leg_quick_settings_devices_assert() {
  local d="$quick_settings_devices_dir" wired
  quick_settings_devices_jq "both SSIDs came up in a scan" scan.json '[.networks[] | select(.inRange) | .name] | index("FORMALTEST") != null and index("FORMALTEST-EAP") != null'
  quick_settings_devices_jq "FORMALTEST saved and down before the card opened" ready.json '.networks[] | select(.name == "FORMALTEST") | .known and (.connected | not)'
  quick_settings_devices_jq "Wi-Fi a split toggle, on and usable" status-open.json '.rows[] | select(.id == "wifi") | .kind == "split" and .on and .available and .arrow != null'
  quick_settings_devices_jq "Bluetooth disabled, No adapter" status-open.json '.rows[] | select(.id == "bluetooth") | .kind == "split" and .available == false and .reason == "No adapter"'
  quick_settings_devices_text "Bluetooth has no menu to open" menu-bluetooth.txt "error: bluetooth has no menu"
  quick_settings_devices_jq "Power mode a split toggle off on balanced" status-open.json '.rows[] | select(.id == "power-mode") | .kind == "split" and .available and .on == false and .subtitle == "Balanced"'
  # The wired NIC NetworkManager has up, by its connection's name.
  wired=$(awk -F: '$2 == "ethernet" && $3 == "connected" { print $4; exit }' "$d/nmcli-devices.txt")
  if [ -n "$wired" ]; then
    quick_settings_devices_jq "Wired carries the connection '$wired'" status-open.json \
      ".rows[] | select(.id == \"wired\") | .kind == \"split\" and .on and .subtitle == \"$wired\""
  else
    fail "quick-settings-devices: nmcli shows no connected ethernet device to read the Wired tile against: $(tr '\n' ' ' < "$d/nmcli-devices.txt")"
  fi
  quick_settings_devices_text "quicksettings menu wifi answered ok" menu-wifi.txt "ok"
  quick_settings_devices_jq "the Wi-Fi menu lists both SSIDs, All networks last" status-wifi-menu.json '
    .menu.id == "wifi" and .dim == true
    and ([.menu.rows[].id] | index("m:wifi:FORMALTEST") != null and index("m:wifi:FORMALTEST-EAP") != null)
    and (.menu.rows[-1] | .id == "m:settings" and .title == "All networks" and .panel == "network")
    and .menu.header.title == "Wi-Fi" and .menu.header.on == true'
  quick_settings_devices_jq "real Downs reached FORMALTEST" status-at-formaltest.json '.cursor == "m:wifi:FORMALTEST"'
  quick_settings_devices_jq "Return connected it, the SSID the tile's subtitle" status-wifi-connected.json '.rows[] | select(.id == "wifi") | .on and .subtitle == "FORMALTEST"'
  quick_settings_devices_jq "NetworkManager has it connected" network-connected.json '.networks[] | select(.name == "FORMALTEST") | .connected'
  quick_settings_devices_jq "the menu checks it" status-shot-wifi-menu.json '[.menu.rows[] | select(.id == "m:wifi:FORMALTEST")][0].checked == true'
  quick_settings_devices_jq "real Downs reached FORMALTEST-EAP" status-at-eap.json '.cursor == "m:wifi:FORMALTEST-EAP"'
  quick_settings_devices_text "Return on the secured unknown network handed over to the network panel" panel-handed.txt "network"
  quick_settings_devices_jq "with FORMALTEST-EAP's passphrase step open" network-prompt.json '.prompt == "FORMALTEST-EAP"'
  quick_settings_devices_text "the radio was on" radio-before.txt "enabled"
  quick_settings_devices_text "activate wifi turned NetworkManager's radio off" radio-off.txt "disabled"
  quick_settings_devices_jq "the tile reads off" status-wifi-off.json '.rows[] | select(.id == "wifi") | .on == false and .subtitle == null'
  quick_settings_devices_text "activate wifi again turned it on" radio-on.txt "enabled"
  quick_settings_devices_jq "the tile reads on" status-wifi-on.json '.rows[] | select(.id == "wifi") | .on == true'
  quick_settings_devices_text "the daemon started on balanced" profile-before.txt "balanced"
  quick_settings_devices_jq "Power mode's menu lists the profiles, Balanced checked, Power settings last" status-power-menu.json '
    ([.menu.rows[].id] | index("m:power-mode:balanced") != null and index("m:power-mode:power-saver") != null)
    and ([.menu.rows[] | select(.checked)] | map(.id)) == ["m:power-mode:balanced"]
    and (.menu.rows[-1] | .id == "m:settings" and .panel == "power")'
  quick_settings_devices_jq "real Downs reached Power saver" status-at-saver.json '.cursor == "m:power-mode:power-saver"'
  quick_settings_devices_text "Return made the daemon's profile power-saver" profile-saver.txt "power-saver"
  quick_settings_devices_jq "the tile reads on, Power saver" status-saver.json '.rows[] | select(.id == "power-mode") | .on and .subtitle == "Power saver"'
  quick_settings_devices_text "the tile's activate flipped it back to balanced" profile-flipped.txt "balanced"
  local png
  for png in wifi-menu prompt; do
    [ -s "$d/quick-settings-devices-$png.png" ] || fail "quick-settings-devices: no quick-settings-devices-$png.png"
    echo "SMOKE_QUICK_SETTINGS_DEVICES_SHOT $d/quick-settings-devices-$png.png"
  done
}
