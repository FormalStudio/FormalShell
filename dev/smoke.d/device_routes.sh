# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --device-routes drives the launcher's four device routes (Wi-Fi, Bluetooth,
# Audio, Radio Stations) over real keys and IPC, asserting off `menu status`
# (its `checked` list is where the tick is readable) and each service's own
# status. Wi-Fi runs against the two hostapd radios --wifi uses: a wrong
# password typed into the launcher's masked step, the real one after it,
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
leg_device_routes_needs="jq wtype pactl ffmpeg mpv"

device_routes_track_path="$shot_dir/device-routes-station.mp3"
device_routes_module_path="$shot_dir/device-routes-sink-module.txt"
device_routes_default_before_path="$shot_dir/device-routes-default-before.txt"
device_routes_default_after_path="$shot_dir/device-routes-default-after.txt"
device_routes_selection_copy="$shot_dir/device-routes-selection.txt"
device_routes_sink="formalshell-routes"
device_routes_station_id="routes-radio-1"
device_routes_station_name="FormalShell Routes Radio"
device_routes_port=18100

leg_device_routes_validate() {
  local other
  for other in wifi radio; do
    if leg_on "$other"; then
      echo "usage: --device-routes drives the same radios, radio-atlas.json and default sink as --${other} and cannot combine with it" >&2
      exit 1
    fi
  done
}

leg_device_routes_fixture() {
  local state_dir="$iso_home/.local/state/formalshell"
  mkdir -p "$state_dir"
  cat > "$state_dir/radio-atlas.json" <<EOF
{"favorites":[{"uuid":"$device_routes_station_id","name":"$device_routes_station_name","url":"http://127.0.0.1:$device_routes_port/station.mp3","homepage":"","favicon":"","country":"","countryCode":"","state":"","language":"","tags":"","codec":"MP3","bitrate":128,"votes":0,"clicks":0,"latitude":null,"longitude":null}],"recent":[],"volume":40,"output":""}
EOF
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=44100" -t 60 \
    -c:a libmp3lame -b:a 128k -y "$device_routes_track_path"
}

leg_device_routes_timing() {
  leg_timing 210 245
}

leg_device_routes_drive() {
  local script="$shot_dir/device-routes-drive.sh" server="$shot_dir/device-routes-server.sh"
  # ffmpeg's listener serves one client and exits; the loop is what lets the
  # station be tuned again.
  write_script "$server" <<EOF
#!/usr/bin/env bash
while true; do
  "$ffmpeg_bin" -nostdin -loglevel error -re -i "$device_routes_track_path" -c copy -f mp3 \
    -listen 1 "http://127.0.0.1:$device_routes_port/station.mp3"
  sleep 0.5
done
EOF
  write_script "$script" <<EOF
#!/usr/bin/env bash
ipc() { $ipc call "\$@"; }
# menu status into \$1, then the index of row id \$2 in its ids, retried
# until the row is there.
row_index() {
  local i=0 idx=""
  while [ "\$i" -lt 20 ]; do
    ipc menu status > "\$1" 2>&1
    idx=\$("$jq_bin" -r --arg id "\$2" '.ids | index(\$id) // empty' "\$1" 2>/dev/null)
    [ -n "\$idx" ] && break
    sleep 0.5
    i=\$((i + 1))
  done
  echo "\$idx"
}
net() { ipc network status > "\$1" 2>&1; }
route() {
  ipc menu summon "\$1" > /dev/null 2>&1
  sleep 1.5
  [ -n "\$2" ] && ipc menu filter "\$2" > /dev/null 2>&1
  sleep 1
}

bash "$server" &
sleep 3

"$pactl_bin" get-default-sink > "$device_routes_default_before_path" 2>&1
"$pactl_bin" load-module module-null-sink sink_name=$device_routes_sink sink_properties=device.description=RoutesSink > "$device_routes_module_path"

# The self-heal in wifi.sh has the why: a wedged supplicant queue and a
# fixture profile left parked on wlan0 by an interrupted run.
sudo systemctl restart wpa_supplicant-wlan0.service
sleep 2
ipc panel open network > /dev/null 2>&1
for ssid in FORMALTEST FORMALTEST-EAP; do
  net "$shot_dir/device-routes-reset.json"
  if grep -qF "\"name\":\"\$ssid\",\"known\":true" "$shot_dir/device-routes-reset.json"; then
    ipc network forget "\$ssid" > /dev/null 2>&1
    SECONDS=0
    while [ "\$SECONDS" -lt 15 ]; do
      net "$shot_dir/device-routes-reset.json"
      grep -qE "\"name\":\"\$ssid\",\"known\":false,\"connected\":false,\"stateChanging\":false" "$shot_dir/device-routes-reset.json" && break
      sleep 1
    done
  fi
done
ipc panel close > /dev/null 2>&1
# The launcher's own scanner hold is what wakes the radios from here on.
ipc menu summon wifi > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 25 ]; do
  net "$shot_dir/device-routes-scan.json"
  grep -qF '"name":"FORMALTEST"' "$shot_dir/device-routes-scan.json" \
    && grep -qF '"name":"FORMALTEST-EAP"' "$shot_dir/device-routes-scan.json" && break
  sleep 1
done

# Wi-Fi: wrong password through the launcher's masked step.
route wifi FORMALTEST
idx=\$(row_index "$shot_dir/device-routes-wifi-list.json" wifi.net.FORMALTEST)
"$grim_bin" "$shot_dir/device-routes-wifi-list.png" > /dev/null 2>&1
ipc menu activate "\$idx" > /dev/null 2>&1
sleep 1.5
ipc menu status > "$shot_dir/device-routes-wifi-input.json" 2>&1
"$wtype_bin" wrong-formaltest-psk
sleep 1
"$grim_bin" "$shot_dir/device-routes-wifi-masked.png" > /dev/null 2>&1
"$wtype_bin" -k Return
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  net "$shot_dir/device-routes-wifi-wrong.json"
  grep -qE '"name":"FORMALTEST","known":(true|false),"connected":(true|false),"stateChanging":true' "$shot_dir/device-routes-wifi-wrong.json" && break
  sleep 1
done
SECONDS=0
while [ "\$SECONDS" -lt 45 ]; do
  net "$shot_dir/device-routes-wifi-wrong.json"
  grep -qE '"name":"FORMALTEST","known":(true|false),"connected":false,"stateChanging":false' "$shot_dir/device-routes-wifi-wrong.json" && break
  sleep 1
done
sleep 1.5
ipc menu status > "$shot_dir/device-routes-wifi-after-wrong.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-wifi-wrong.png" > /dev/null 2>&1

# The failure was a secret one, so Enter asks again instead of retrying.
route wifi FORMALTEST
idx=\$(row_index "$shot_dir/device-routes-wifi-list2.json" wifi.net.FORMALTEST)
ipc menu activate "\$idx" > /dev/null 2>&1
sleep 1.5
ipc menu status > "$shot_dir/device-routes-wifi-input2.json" 2>&1
"$wtype_bin" formaltest-psk
sleep 0.5
"$wtype_bin" -k Return
SECONDS=0
while [ "\$SECONDS" -lt 30 ]; do
  net "$shot_dir/device-routes-wifi-connected.json"
  grep -qF '"name":"FORMALTEST","known":true,"connected":true' "$shot_dir/device-routes-wifi-connected.json" && break
  sleep 1
done
sleep 2
ipc menu status > "$shot_dir/device-routes-wifi-ticked.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-wifi-connected.png" > /dev/null 2>&1
cp "$iso_home/.local/state/formalshell/menu-selection.txt" "$device_routes_selection_copy" 2>/dev/null

# → on the connected network: its details.
route wifi FORMALTEST
idx=\$(row_index "$shot_dir/device-routes-wifi-details-list.json" wifi.net.FORMALTEST)
ipc menu into "\$idx" > "$shot_dir/device-routes-wifi-into.txt" 2>&1
sleep 1.5
ipc menu status > "$shot_dir/device-routes-wifi-details.json" 2>&1
ip -4 -o addr show dev wlan0 | awk '{print \$4}' > "$shot_dir/device-routes-wifi-addresses.txt" 2>&1
"$grim_bin" "$shot_dir/device-routes-wifi-details.png" > /dev/null 2>&1
ipc menu close > /dev/null 2>&1
sleep 1

# A root query reaches what is saved, sunk or a favourite, never what is
# merely nearby.
ipc menu summon "" > /dev/null 2>&1
sleep 2
ipc debug query FORMALTEST > "$shot_dir/device-routes-root-wifi.json" 2>&1
ipc debug query FORMALTEST-EAP > "$shot_dir/device-routes-root-nearby.json" 2>&1
ipc debug query RoutesSink > "$shot_dir/device-routes-root-audio.json" 2>&1
ipc debug query 'Routes Radio' > "$shot_dir/device-routes-root-radio.json" 2>&1
ipc menu close > /dev/null 2>&1
sleep 1

# Shift+Enter forgets the saved network.
route wifi FORMALTEST
idx=\$(row_index "$shot_dir/device-routes-wifi-forget-list.json" wifi.net.FORMALTEST)
"$grim_bin" "$shot_dir/device-routes-wifi-hint.png" > /dev/null 2>&1
ipc menu activateAlternate "\$idx" > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 15 ]; do
  net "$shot_dir/device-routes-wifi-forgotten.json"
  grep -qE '"name":"FORMALTEST","known":false,"connected":false,"stateChanging":false' "$shot_dir/device-routes-wifi-forgotten.json" && break
  sleep 1
done
ipc menu close > /dev/null 2>&1

# Audio: the launcher moves the default sink to the null sink.
route audio RoutesSink
idx=\$(row_index "$shot_dir/device-routes-audio-list.json" audio.output.$device_routes_sink)
ipc menu activate "\$idx" > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  "$pactl_bin" get-default-sink > "$device_routes_default_after_path" 2>&1
  grep -qx $device_routes_sink "$device_routes_default_after_path" && break
  sleep 1
done
sleep 1.5
ipc menu filter "" > /dev/null 2>&1
sleep 1
ipc menu status > "$shot_dir/device-routes-audio-ticked.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-audio.png" > /dev/null 2>&1
ipc menu close > /dev/null 2>&1

# Bluetooth: no adapter in the VM, and the panel is still the panel.
route bluetooth ""
ipc menu status > "$shot_dir/device-routes-bluetooth.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-bluetooth.png" > /dev/null 2>&1
ipc menu close > /dev/null 2>&1
sleep 1
ipc panel open bluetooth > "$shot_dir/device-routes-bluetooth-panel-open.txt" 2>&1
sleep 1
ipc panel state > "$shot_dir/device-routes-bluetooth-panel.json" 2>&1
ipc panel close > /dev/null 2>&1

# Radio: tune the favourite, then Stop and Shift+Enter remove it.
route radio "Routes Radio"
idx=\$(row_index "$shot_dir/device-routes-radio-list.json" radio.fav.$device_routes_station_id)
ipc menu activate "\$idx" > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 15 ]; do
  ipc radio status > "$shot_dir/device-routes-radio-status.json" 2>&1
  grep -qF '"station":"$device_routes_station_name"' "$shot_dir/device-routes-radio-status.json" \
    && grep -qF '"running":true' "$shot_dir/device-routes-radio-status.json" && break
  sleep 1
done
sleep 2
ipc menu filter "" > /dev/null 2>&1
sleep 1
ipc menu status > "$shot_dir/device-routes-radio-ticked.json" 2>&1
"$grim_bin" "$shot_dir/device-routes-radio.png" > /dev/null 2>&1
ipc menu filter "Routes Radio" > /dev/null 2>&1
idx=\$(row_index "$shot_dir/device-routes-radio-list2.json" radio.fav.$device_routes_station_id)
ipc menu activateAlternate "\$idx" > /dev/null 2>&1
sleep 1.5
ipc radio status > "$shot_dir/device-routes-radio-removed.json" 2>&1
ipc menu close > /dev/null 2>&1
ipc radio stop > /dev/null 2>&1

ipc menu summon "" > /dev/null 2>&1
sleep 1.5
ipc debug query ':r jazz' > "$shot_dir/device-routes-radio-search.json" 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 20 ]; do
  ipc debug query ':r jazz' > "$shot_dir/device-routes-radio-search.json" 2>&1
  grep -qF 'radio.searching' "$shot_dir/device-routes-radio-search.json" || break
  sleep 1
done
ipc debug query jazz > "$shot_dir/device-routes-root-jazz.json" 2>&1
ipc menu close > /dev/null 2>&1
EOF
  add_cleanup "'$pactl_bin' set-default-sink \"\$(cat '$device_routes_default_before_path' 2>/dev/null)\" 2>/dev/null || true"
  add_cleanup "[ -s '$device_routes_module_path' ] && '$pactl_bin' unload-module \"\$(cat '$device_routes_module_path')\" 2>/dev/null || true"
  add_cleanup "pkill -f 'device-routes-server.sh' 2>/dev/null || true"
  add_cleanup "pkill -f 'listen 1 http://127.0.0.1:$device_routes_port' 2>/dev/null || true"
  hypr_exec_once "bash $script"
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
  grep -qF '"name":"FORMALTEST-EAP"' "$p-scan.json" || fail "FORMALTEST-EAP never scanned"
  "$jq_bin" -e '.mode == "input"' "$p-wifi-input.json" > /dev/null \
    || fail "activating an unknown secured network did not open the password step: $(cat "$p-wifi-input.json")"
  grep -qE '"name":"FORMALTEST","known":(true|false),"connected":false,"stateChanging":false' "$p-wifi-wrong.json" \
    || fail "the wrong password never settled to disconnected: $(cat "$p-wifi-wrong.json")"
  "$jq_bin" -e '.mode == "menu" and .level == "wifi"' "$p-wifi-after-wrong.json" > /dev/null \
    || fail "the launcher did not come back to the wifi list after the wrong password: $(cat "$p-wifi-after-wrong.json")"
  "$jq_bin" -e '.mode == "input"' "$p-wifi-input2.json" > /dev/null \
    || fail "Enter after a wrong password did not ask again: $(cat "$p-wifi-input2.json")"
  grep -qF '"name":"FORMALTEST","known":true,"connected":true' "$p-wifi-connected.json" \
    || fail "the real password never reached connected:true: $(cat "$p-wifi-connected.json")"
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
  "$jq_bin" -e 'any(.[]; .id == "wifi.net.FORMALTEST" and .checked == true)' "$p-root-wifi.json" > /dev/null \
    || fail "a root query did not reach the ticked saved network: $(cat "$p-root-wifi.json")"
  "$jq_bin" -e 'any(.[]; .id == "wifi.net.FORMALTEST-EAP") | not' "$p-root-nearby.json" > /dev/null \
    || fail "a nearby network reached the root: $(cat "$p-root-nearby.json")"
  grep -qF '"name":"FORMALTEST","known":false,"connected":false,"stateChanging":false' "$p-wifi-forgotten.json" \
    || fail "Shift+Enter did not forget the network: $(cat "$p-wifi-forgotten.json")"

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
  grep -qF "\"station\":\"$device_routes_station_name\"" "$p-radio-status.json" && grep -qF '"running":true' "$p-radio-status.json" \
    || fail "the favourite never played: $(cat "$p-radio-status.json")"
  "$jq_bin" -e 'any(.[]; .id == "radio.fav.'"$device_routes_station_id"'")' "$p-root-radio.json" > /dev/null \
    || fail "a root query did not reach the favourite: $(cat "$p-root-radio.json")"
  grep -qF '"favorites":0' "$p-radio-removed.json" \
    || fail "Shift+Enter did not remove the favourite: $(cat "$p-radio-removed.json")"
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
