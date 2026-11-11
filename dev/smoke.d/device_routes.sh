# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --device-routes drives the launcher's four device routes (Wi-Fi, Bluetooth,
# Audio, Radio Stations) over real keys and IPC, asserting off `menu status`
# (its `checked` list is where the tick is readable) and each service's own
# status. Wi-Fi runs against the two hostapd radios --wifi uses: a wrong
# password typed into the launcher's masked step, the real one after it,
# then Show password drawing the saved one in its secret step (read off
# the frame by tesseract, masked in `menu status`) and Escape dropping it,
# then a Shift+Enter forget, with the password absent from
# menu-selection.txt and the shell log. While connected, → on the network
# lists its details: Security reading WPA2 Personal, Signal a percentage,
# and one IP address row per IPv4 address `ip -4 addr` shows on wlan0. Audio adds its own null sink and
# switches the default to it, read back with pactl (the output half only:
# the sink's monitor is not an input the route lists). Bluetooth has no
# adapter here, so it is the honest NO ADAPTER row while `panel open
# bluetooth` still opens the panel. Radio tunes a favourite served by its
# own looping ffmpeg listener, removes it with Shift+Enter, and waits for
# `:r` to leave its searching row (a failed search passes: the VM's route to
# Radio Browser is not guaranteed). A root query reaches the saved network,
# the sink and the favourite, and never a nearby network or a search result.
#
# Everything is strictly ordered, so it is one script. Each poll writes over
# its own path.
leg_device_routes_flag="--device-routes"
leg_device_routes_order=217
leg_device_routes_vm_only="it connects and forgets networks through NetworkManager against the hostapd radios"
leg_device_routes_needs="jq wtype pactl ffmpeg mpv convert tesseract"
leg_device_routes_claims="wifi.radios radio.favourites"

device_routes_track_path="$shot_dir/device-routes-station.mp3"
device_routes_default_before_path="$shot_dir/device-routes-default-before.txt"
device_routes_default_after_path="$shot_dir/device-routes-default-after.txt"
device_routes_selection_copy="$shot_dir/device-routes-selection.txt"
device_routes_sink="formalshell-routes"
device_routes_station_id="routes-radio-1"
device_routes_station_name="FormalShell Routes Radio"
device_routes_port=18100

leg_device_routes_fixture() {
  radio_favourite "$device_routes_station_id" "$device_routes_station_name" "$device_routes_port"
  radio_station "$device_routes_port" "$device_routes_track_path"
}

leg_device_routes_timing() {
  leg_timing 245
}

leg_device_routes_drive() {
  local script="$shot_dir/device-routes-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
net() { call network status > "\$1" 2>&1; }
route() {
  call menu summon "\$1" > /dev/null 2>&1
  until_settled
  [ -n "\$2" ] && call menu filter "\$2" > /dev/null 2>&1
  sleep 1
}

"${device_routes_track_path%.mp3}-server.sh" &
sleep 3

"$pactl_bin" get-default-sink > "$device_routes_default_before_path" 2>&1
pw_load_module module-null-sink sink_name=$device_routes_sink sink_properties=device.description=RoutesSink > /dev/null

# The self-heal in wifi.sh has the why: a wedged supplicant queue and a
# fixture profile left parked on wlan0 by an interrupted run.
sudo systemctl restart wpa_supplicant-wlan0.service
sleep 2
call panel open network > /dev/null 2>&1
for ssid in FORMALTEST FORMALTEST-EAP; do
  net "$shot_dir/device-routes-reset.json"
  if "$jq_bin" -e --arg s "\$ssid" 'any(.networks[]; .name == \$s and .known == true)' "$shot_dir/device-routes-reset.json" > /dev/null 2>&1; then
    call network forget "\$ssid" > /dev/null 2>&1
    until_jq 15 "$shot_dir/device-routes-reset.json" "any(.networks[]; .name == \"\$ssid\" and .known == false and .connected == false and .stateChanging == false)" call network status
  fi
done
call panel close > /dev/null 2>&1
# The launcher's own scanner hold is what wakes the radios from here on.
call menu summon wifi > /dev/null 2>&1
until_jq 25 "$shot_dir/device-routes-scan.json" 'any(.networks[]; .name == "FORMALTEST") and any(.networks[]; .name == "FORMALTEST-EAP")' call network status

# Wi-Fi: wrong password through the launcher's masked step.
route wifi FORMALTEST
idx=\$(row_index "$shot_dir/device-routes-wifi-list.json" wifi.net.FORMALTEST)
"$grim_bin" "$shot_dir/device-routes-wifi-list.png" > /dev/null 2>&1
call menu activate "\$idx" > /dev/null 2>&1
sleep 1.5
call menu status > "$shot_dir/device-routes-wifi-input.json" 2>&1
"$wtype_bin" wrong-formaltest-psk
sleep 1
"$grim_bin" "$shot_dir/device-routes-wifi-masked.png" > /dev/null 2>&1
"$wtype_bin" -k Return
until_jq 10 "$shot_dir/device-routes-wifi-wrong.json" 'any(.networks[]; .name == "FORMALTEST" and .stateChanging == true)' call network status
until_jq 45 "$shot_dir/device-routes-wifi-wrong.json" 'any(.networks[]; .name == "FORMALTEST" and .connected == false and .stateChanging == false)' call network status
sleep 1.5
call menu status > "$shot_dir/device-routes-wifi-after-wrong.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-wifi-wrong.png" > /dev/null 2>&1

# The failure was a secret one, so Enter asks again instead of retrying.
route wifi FORMALTEST
idx=\$(row_index "$shot_dir/device-routes-wifi-list2.json" wifi.net.FORMALTEST)
call menu activate "\$idx" > /dev/null 2>&1
sleep 1.5
call menu status > "$shot_dir/device-routes-wifi-input2.json" 2>&1
"$wtype_bin" formaltest-psk
sleep 0.5
"$wtype_bin" -k Return
until_jq 30 "$shot_dir/device-routes-wifi-connected.json" 'any(.networks[]; .name == "FORMALTEST" and .known == true and .connected == true)' call network status
sleep 2
call menu status > "$shot_dir/device-routes-wifi-ticked.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-wifi-connected.png" > /dev/null 2>&1
cp "$iso_home/.local/state/formalshell/menu-selection.txt" "$device_routes_selection_copy" 2>/dev/null

# → on the connected network: its details.
route wifi FORMALTEST
idx=\$(row_index "$shot_dir/device-routes-wifi-details-list.json" wifi.net.FORMALTEST)
call menu into "\$idx" > "$shot_dir/device-routes-wifi-into.txt" 2>&1
sleep 1.5
call menu status > "$shot_dir/device-routes-wifi-details.json" 2>&1
ip -4 -o addr show dev wlan0 | awk '{print \$4}' > "$shot_dir/device-routes-wifi-addresses.txt" 2>&1
"$grim_bin" "$shot_dir/device-routes-wifi-details.png" > /dev/null 2>&1
call menu close > /dev/null 2>&1
sleep 1

# Show password on the saved network: NetworkManager's GetSecrets, through
# the shell's polkit agent if it asks, into the secret step drawn in the
# clear, and Escape dropping it.
route wifi FORMALTEST
idx=\$(row_index "$shot_dir/device-routes-wifi-show-list.json" wifi.net.FORMALTEST)
call menu runAction "\$idx" show-password > "$shot_dir/device-routes-wifi-show-run.txt" 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 20 ]; do
  call debug dump > "$shot_dir/device-routes-wifi-show-dump.json" 2>&1
  if "$jq_bin" -e 'any(.modals[]; .namespace == "formalshell:polkit" and .open)' "$shot_dir/device-routes-wifi-show-dump.json" > /dev/null 2>&1 \
    && [ ! -e "$shot_dir/device-routes-wifi-show-polkit" ]; then
    touch "$shot_dir/device-routes-wifi-show-polkit"
    sleep 1
    "$wtype_bin" "$rig_password"
    "$wtype_bin" -k Return
  fi
  call menu status > "$shot_dir/device-routes-wifi-show.json" 2>&1
  "$jq_bin" -e '.secretShown == true' "$shot_dir/device-routes-wifi-show.json" > /dev/null 2>&1 && break
  sleep 0.5
done
sleep 1
call debug dump > "$shot_dir/device-routes-wifi-show-dump.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-wifi-show.png" > /dev/null 2>&1
"$wtype_bin" -k Escape
sleep 1
call menu status > "$shot_dir/device-routes-wifi-show-after.json" 2>&1
call menu close > /dev/null 2>&1
sleep 1

# A root query reaches what is saved, sunk or a favourite, never what is
# merely nearby.
call menu summon "" > /dev/null 2>&1
until_settled
call debug query FORMALTEST > "$shot_dir/device-routes-root-wifi.json" 2>&1
call debug query FORMALTEST-EAP > "$shot_dir/device-routes-root-nearby.json" 2>&1
call debug query RoutesSink > "$shot_dir/device-routes-root-audio.json" 2>&1
call debug query 'Routes Radio' > "$shot_dir/device-routes-root-radio.json" 2>&1
call menu close > /dev/null 2>&1
sleep 1

# Shift+Enter forgets the saved network.
route wifi FORMALTEST
idx=\$(row_index "$shot_dir/device-routes-wifi-forget-list.json" wifi.net.FORMALTEST)
"$grim_bin" "$shot_dir/device-routes-wifi-hint.png" > /dev/null 2>&1
call menu activateAlternate "\$idx" > /dev/null 2>&1
until_jq 15 "$shot_dir/device-routes-wifi-forgotten.json" 'any(.networks[]; .name == "FORMALTEST" and .known == false and .connected == false and .stateChanging == false)' call network status
call menu close > /dev/null 2>&1

# Audio: the launcher moves the default sink to the null sink.
route audio RoutesSink
idx=\$(row_index "$shot_dir/device-routes-audio-list.json" audio.output.$device_routes_sink)
call menu activate "\$idx" > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  "$pactl_bin" get-default-sink > "$device_routes_default_after_path" 2>&1
  grep -qx $device_routes_sink "$device_routes_default_after_path" && break
  sleep 1
done
sleep 1.5
call menu filter "" > /dev/null 2>&1
sleep 1
call menu status > "$shot_dir/device-routes-audio-ticked.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-audio.png" > /dev/null 2>&1
call menu close > /dev/null 2>&1

# Bluetooth: no adapter in the VM, and the panel is still the panel.
route bluetooth ""
call menu status > "$shot_dir/device-routes-bluetooth.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-bluetooth.png" > /dev/null 2>&1
call menu close > /dev/null 2>&1
sleep 1
call panel open bluetooth > "$shot_dir/device-routes-bluetooth-panel-open.txt" 2>&1
until_settled
call panel state > "$shot_dir/device-routes-bluetooth-panel.json" 2>&1
call panel close > /dev/null 2>&1

# Radio: tune the favourite, then Stop and Shift+Enter remove it.
route radio "Routes Radio"
idx=\$(row_index "$shot_dir/device-routes-radio-list.json" radio.fav.$device_routes_station_id)
call menu activate "\$idx" > /dev/null 2>&1
until_jq 15 "$shot_dir/device-routes-radio-status.json" '.station == "$device_routes_station_name" and .running == true' call radio status
sleep 2
call menu filter "" > /dev/null 2>&1
sleep 1
call menu status > "$shot_dir/device-routes-radio-ticked.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-radio.png" > /dev/null 2>&1
call menu filter "Routes Radio" > /dev/null 2>&1
idx=\$(row_index "$shot_dir/device-routes-radio-list2.json" radio.fav.$device_routes_station_id)
call menu activateAlternate "\$idx" > /dev/null 2>&1
sleep 1.5
call radio status > "$shot_dir/device-routes-radio-removed.json" 2>&1
call menu close > /dev/null 2>&1
call radio stop > /dev/null 2>&1

call menu summon "" > /dev/null 2>&1
until_settled
call debug query ':r jazz' > "$shot_dir/device-routes-radio-search.json" 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 20 ]; do
  call debug query ':r jazz' > "$shot_dir/device-routes-radio-search.json" 2>&1
  grep -qF 'radio.searching' "$shot_dir/device-routes-radio-search.json" || break
  sleep 1
done
call debug query jazz > "$shot_dir/device-routes-root-jazz.json" 2>&1
call menu close > /dev/null 2>&1
EOF
  add_cleanup "'$pactl_bin' set-default-sink \"\$(cat '$device_routes_default_before_path' 2>/dev/null)\" 2>/dev/null || true"
  wm_exec_once "bash $script"
}

leg_device_routes_assert() {
  local f n p
  p="$shot_dir/device-routes"
  for f in wifi-list wifi-input wifi-after-wrong wifi-input2 wifi-ticked wifi-forget-list audio-list audio-ticked bluetooth radio-list radio-ticked radio-list2; do
    [ -s "$p-$f.json" ] || fail "no $f menu status"
    n=${f^^}
    echo "SMOKE_DEVICE_ROUTES_${n//-/_}_STATUS $p-$f.json"
  done

  "$jq_bin" -e '.level == "wifi"' "$p-wifi-list.json" > /dev/null \
    || fail "summon wifi did not land on the wifi level: $(cat "$p-wifi-list.json")"
  expect_jq "$p-scan.json" 'any(.networks[]; .name == "FORMALTEST-EAP")' "FORMALTEST-EAP never scanned" '[.networks[].name]'
  "$jq_bin" -e '.mode == "input"' "$p-wifi-input.json" > /dev/null \
    || fail "activating an unknown secured network did not open the password step: $(cat "$p-wifi-input.json")"
  expect_jq "$p-wifi-wrong.json" 'any(.networks[]; .name == "FORMALTEST" and .connected == false and .stateChanging == false)' \
    "the wrong password never settled to disconnected"
  "$jq_bin" -e '.mode == "menu" and .level == "wifi"' "$p-wifi-after-wrong.json" > /dev/null \
    || fail "the launcher did not come back to the wifi list after the wrong password: $(cat "$p-wifi-after-wrong.json")"
  "$jq_bin" -e '.mode == "input"' "$p-wifi-input2.json" > /dev/null \
    || fail "Enter after a wrong password did not ask again: $(cat "$p-wifi-input2.json")"
  expect_jq "$p-wifi-connected.json" 'any(.networks[]; .name == "FORMALTEST" and .known == true and .connected == true)' \
    "the real password never reached connected:true"
  "$jq_bin" -e '.checked | index("wifi.net.FORMALTEST") != null' "$p-wifi-ticked.json" > /dev/null \
    || fail "the connected network carries no tick: $(cat "$p-wifi-ticked.json")"
  grep -q '^ok$' "$p-wifi-into.txt" || fail "menu into on the network answered: $(cat "$p-wifi-into.txt")"
  "$jq_bin" -e '.token[-1] == "FORMALTEST" and .ids[0:2] == ["wifi.details.security", "wifi.details.signal"]
      and .drawn[0].value == "WPA2 Personal" and (.drawn[1].value | test("^[0-9]+%$"))' "$p-wifi-details.json" > /dev/null \
    || fail "→ on the network did not list its security and signal: $("$jq_bin" -c '{token, drawn}' "$p-wifi-details.json")"
  local drawn_ips real_ips
  drawn_ips=$("$jq_bin" -r '.drawn[] | select(.id | startswith("wifi.details.address.")) | .value' "$p-wifi-details.json" | sort)
  real_ips=$(sort "$p-wifi-addresses.txt")
  [ "$drawn_ips" = "$real_ips" ] || fail "the details' IP addresses ($drawn_ips) are not wlan0's ($real_ips)"
  echo "SMOKE_DEVICE_ROUTES_WIFI_DETAILS $("$jq_bin" -c '[.drawn[] | [.label, .value]]' "$p-wifi-details.json")"
  [ -e "$p-wifi-show-polkit" ] && echo "SMOKE_DEVICE_ROUTES_WIFI_SHOW_POLKIT asked"
  "$jq_bin" -e '.mode == "input" and .prompt == "Password for FORMALTEST" and .secretShown == true
      and .query == ("\u2022" * ("formaltest-psk" | length))' "$p-wifi-show.json" > /dev/null \
    || fail "Show password did not open the step with the saved password: $(cat "$p-wifi-show-run.txt") $("$jq_bin" -c '{mode, prompt, secretShown, query}' "$p-wifi-show.json")"
  echo "SMOKE_DEVICE_ROUTES_WIFI_SHOW $p-wifi-show.png"
  local box ocr
  box=$(scaled_jq -r "$geometry_jq"'.modals[] | select(.namespace == "formalshell:menu") | .card | geometry' "$p-wifi-show-dump.json")
  [ -n "$box" ] || fail "no launcher card in the dump under Show password"
  "$convert_bin" "$p-wifi-show.png" -crop "$box" -resize 200% "$p-wifi-show-card.png"
  ocr=$("$tesseract_bin" "$p-wifi-show-card.png" - 2>/dev/null)
  grep -qiE 'formaltest.?psk' <<< "$ocr" \
    || fail "the saved password is not drawn in the step: $(tr '\n' ' ' <<< "$ocr")"
  "$jq_bin" -e '.secretShown == false and .mode != "input"' "$p-wifi-show-after.json" > /dev/null \
    || fail "Escape did not drop the shown password: $("$jq_bin" -c '{mode, secretShown, query}' "$p-wifi-show-after.json")"
  "$jq_bin" -e 'any(.[]; .id == "wifi.net.FORMALTEST" and .checked == true)' "$p-root-wifi.json" > /dev/null \
    || fail "a root query did not reach the ticked saved network: $(cat "$p-root-wifi.json")"
  "$jq_bin" -e 'any(.[]; .id == "wifi.net.FORMALTEST-EAP") | not' "$p-root-nearby.json" > /dev/null \
    || fail "a nearby network reached the root: $(cat "$p-root-nearby.json")"
  expect_jq "$p-wifi-forgotten.json" 'any(.networks[]; .name == "FORMALTEST" and .known == false and .connected == false and .stateChanging == false)' "Shift+Enter did not forget the network"

  if grep -qi 'formaltest-psk' "$iso_home/.local/state/formalshell/menu-selection.txt" "$device_routes_selection_copy" "$shell_log_path" 2>/dev/null; then
    fail "a typed password reached menu-selection.txt or the shell log"
  fi

  [ "$(cat "$device_routes_default_after_path" 2>/dev/null)" = "$device_routes_sink" ] \
    || fail "the default sink is $(cat "$device_routes_default_after_path" 2>/dev/null), not $device_routes_sink (was $(cat "$device_routes_default_before_path"))"
  "$jq_bin" -e --arg id "audio.output.$device_routes_sink" '.level == "audio" and (.checked | index($id) != null)' "$p-audio-ticked.json" > /dev/null \
    || fail "the new default sink carries no tick: $(cat "$p-audio-ticked.json")"
  "$jq_bin" -e 'any(.[]; .id == "audio.output.'"$device_routes_sink"'")' "$p-root-audio.json" > /dev/null \
    || fail "a root query did not reach the audio device: $(cat "$p-root-audio.json")"

  "$jq_bin" -e '.level == "bluetooth" and .empty == "bluetooth.unavailable" and .rows == 0' "$p-bluetooth.json" > /dev/null \
    || fail "bluetooth is not the honest no-adapter row: $(cat "$p-bluetooth.json")"
  grep -q '^ok$' "$p-bluetooth-panel-open.txt" || fail "panel open bluetooth answered: $(cat "$p-bluetooth-panel-open.txt")"
  grep -q 'bluetooth' "$p-bluetooth-panel.json" || fail "the bluetooth panel did not open: $(cat "$p-bluetooth-panel.json")"

  "$jq_bin" -e --arg id "radio.fav.$device_routes_station_id" '.level == "radio" and (.checked | index($id) != null)' "$p-radio-ticked.json" > /dev/null \
    || fail "the tuned favourite carries no tick: $(cat "$p-radio-ticked.json")"
  expect_jq "$p-radio-status.json" '.station == "'"$device_routes_station_name"'" and .running == true' "the favourite never played"
  "$jq_bin" -e 'any(.[]; .id == "radio.fav.'"$device_routes_station_id"'")' "$p-root-radio.json" > /dev/null \
    || fail "a root query did not reach the favourite: $(cat "$p-root-radio.json")"
  expect_jq "$p-radio-removed.json" '.favorites == 0' "Shift+Enter did not remove the favourite"
  grep -qF 'radio.searching' "$p-radio-search.json" \
    && fail "the :r search never left its searching row: $(cat "$p-radio-search.json")"
  grep -qF 'radio.result.' "$p-root-jazz.json" \
    && fail "a station search result reached the root: $(cat "$p-root-jazz.json")"

  for f in wifi-list wifi-masked wifi-wrong wifi-connected wifi-details wifi-hint audio bluetooth radio; do
    [ -s "$p-$f.png" ] || fail "no $f frame"
    n=${f^^}
    echo "SMOKE_DEVICE_ROUTES_${n//-/_} $p-$f.png"
  done
}
