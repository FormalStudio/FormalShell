# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --wifi drives the network panel against the two real hostapd radios
# the VM rig stands up on mac80211_hwsim: wait for a genuine scan to
# surface both SSIDs, a wrong-password round trip that must settle to a
# stable disconnected state, the real password reaching connected:true (so
# hostapd's own dnsmasq handed the station an address), forget, then the
# enterprise round trip through hostapd's integrated PEAP/MSCHAPv2 server,
# and a closing forget.
#
# Before the first connect, a saved profile for FORMALGHOST, an SSID neither
# radio broadcasts: it has to stay out of the panel's Networks list and its
# count, and show up under the closed "Known networks" disclosure once real
# keys walk the cursor onto it and press Enter. Read off `network status`
# (`inRange`, `knownOpen`) and off tesseract over the two frames.
#
# Everything is strictly ordered, so it is one script. Each poll writes over
# its own path, so what the loop last saw is exactly what the assertions read
# back, never a stale earlier snapshot passing by accident.
leg_wifi_flag="--wifi"
leg_wifi_order=190
leg_wifi_needs="wtype jq"
leg_wifi_vm_only="it drives NetworkManager against the hostapd radios and restarts wpa_supplicant with sudo"

wifi_reset_status_path="$shot_dir/wifi-reset-status.json"
wifi_scan_status_path="$shot_dir/wifi-scan-status.json"
wifi_ghost_add_path="$shot_dir/wifi-ghost-add.txt"
wifi_ghost_closed_path="$shot_dir/wifi-ghost-closed.png"
wifi_ghost_closed_status_path="$shot_dir/wifi-ghost-closed-status.json"
wifi_ghost_open_path="$shot_dir/wifi-ghost-open.png"
wifi_ghost_open_status_path="$shot_dir/wifi-ghost-open-status.json"
wifi_ghost_gone_status_path="$shot_dir/wifi-ghost-gone-status.json"
wifi_wrong_path="$shot_dir/wifi-wrong.png"
wifi_wrong_status_path="$shot_dir/wifi-wrong-status.json"
wifi_connected_path="$shot_dir/wifi-connected.png"
wifi_connected_status_path="$shot_dir/wifi-connected-status.json"
wifi_forget_status_path="$shot_dir/wifi-forget-status.json"
wifi_eap_connected_path="$shot_dir/wifi-eap-connected.png"
wifi_eap_status_path="$shot_dir/wifi-eap-status.json"
wifi_eap_forget_status_path="$shot_dir/wifi-eap-forget-status.json"

leg_wifi_timing() {
  # Every ceiling below is a poll that breaks the moment the state settles;
  # the sum is the worst case NetworkManager and hostapd can genuinely take.
  leg_timing 205 230
}

leg_wifi_drive() {
  local script="$shot_dir/wifi-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3

# Self-heal, part one: wpa_supplicant's internal scan-request queue can wedge
# across runs independently of anything NetworkManager persists ("Reject scan
# trigger since one is already pending" forever, with no SME authenticate
# ever reaching the radio). Restarting the per-interface unit is the fix that
# unwedged it by hand; NM reconnects to the fresh supplicant over D-Bus on
# its own. Cheap and idempotent, so it runs unconditionally.
sudo systemctl restart wpa_supplicant-wlan0.service
sleep 2

$ipc call panel open network > /dev/null 2>&1

# Self-heal, part two: NetworkManager's system-connections dir is not part of
# the isolated per-run HOME, so an interrupted earlier run can leave a
# fixture profile parked on wlan0 with autoconnect=yes, and the station then
# never rescans up the other SSID at all. An SSID this VM has not associated
# with returns "unknown ssid" from the IPC, which arms nothing.
for ssid in FORMALTEST FORMALTEST-EAP; do
  $ipc call network status > "$wifi_reset_status_path" 2>&1
  if grep -qF "\"name\":\"\$ssid\",\"known\":true" "$wifi_reset_status_path"; then
    $ipc call network forget "\$ssid" > /dev/null 2>&1
    SECONDS=0
    while [ "\$SECONDS" -lt 15 ]; do
      $ipc call network status > "$wifi_reset_status_path" 2>&1
      if grep -qE "\"name\":\"\$ssid\",\"known\":false,\"connected\":false,\"stateChanging\":false" "$wifi_reset_status_path"; then
        break
      fi
      sleep 1
    done
  fi
done

sudo nmcli connection delete FORMALGHOST > /dev/null 2>&1 || true

SECONDS=0
while [ "\$SECONDS" -lt 25 ]; do
  $ipc call network status > "$wifi_scan_status_path" 2>&1
  if grep -qF '"name":"FORMALTEST"' "$wifi_scan_status_path" && grep -qF '"name":"FORMALTEST-EAP"' "$wifi_scan_status_path"; then
    break
  fi
  sleep 1
done

sudo nmcli connection add type wifi ifname wlan0 con-name FORMALGHOST ssid FORMALGHOST \\
  connection.autoconnect no wifi-sec.key-mgmt wpa-psk wifi-sec.psk formalghost-psk > "$wifi_ghost_add_path" 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  $ipc call network status > "$wifi_ghost_closed_status_path" 2>&1
  grep -qF '"name":"FORMALGHOST","known":true' "$wifi_ghost_closed_status_path" && break
  sleep 1
done
# Reopened so the disclosure starts closed and the card settles on the new list.
$ipc call panel close > /dev/null 2>&1
sleep 1
$ipc call panel open network > /dev/null 2>&1
sleep 2
$ipc call network status > "$wifi_ghost_closed_status_path" 2>&1
"$grim_bin" "$wifi_ghost_closed_path" > /dev/null 2>&1
# Nothing is connected here, so the stops end Known networks, Share network,
# Speed test: past the end, then two back.
"$wtype_bin" \$(printf -- '-k Down %.0s' \$(seq 1 40)) -k Up -k Up -k Return
sleep 2
$ipc call network status > "$wifi_ghost_open_status_path" 2>&1
"$grim_bin" "$wifi_ghost_open_path" > /dev/null 2>&1
sudo nmcli connection delete FORMALGHOST > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  $ipc call network status > "$wifi_ghost_gone_status_path" 2>&1
  grep -qF '"name":"FORMALGHOST"' "$wifi_ghost_gone_status_path" || break
  sleep 1
done

$ipc call network connect FORMALTEST wrong-formaltest-psk > /dev/null 2>&1
# connect() replies as soon as the IPC call returns, well before NM's own
# ActiveConnection exists, so polling for the settled state right away would
# see the identical pre-attempt idle snapshot and declare victory before
# anything happened. Waiting for stateChanging:true first proves NM started.
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  $ipc call network status > "$wifi_wrong_status_path" 2>&1
  if grep -qE '"name":"FORMALTEST","known":(true|false),"connected":(true|false),"stateChanging":true' "$wifi_wrong_status_path"; then
    break
  fi
  sleep 1
done
SECONDS=0
while [ "\$SECONDS" -lt 45 ]; do
  $ipc call network status > "$wifi_wrong_status_path" 2>&1
  if grep -qE '"name":"FORMALTEST","known":(true|false),"connected":false,"stateChanging":false' "$wifi_wrong_status_path"; then
    break
  fi
  sleep 1
done
"$grim_bin" "$wifi_wrong_path" > /dev/null 2>&1

$ipc call network connect FORMALTEST formaltest-psk > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 25 ]; do
  $ipc call network status > "$wifi_connected_status_path" 2>&1
  if grep -qF '"name":"FORMALTEST","known":true,"connected":true' "$wifi_connected_status_path"; then
    break
  fi
  sleep 1
done
"$grim_bin" "$wifi_connected_path" > /dev/null 2>&1

$ipc call network forget FORMALTEST > /dev/null 2>&1
# known:false alone is not enough to move on: the panel's own action
# bookkeeping only clears once BOTH !known and !stateChanging, and
# connectEap below refuses to run while an action is still in flight.
SECONDS=0
while [ "\$SECONDS" -lt 15 ]; do
  $ipc call network status > "$wifi_forget_status_path" 2>&1
  if grep -qE '"name":"FORMALTEST","known":false,"connected":false,"stateChanging":false' "$wifi_forget_status_path"; then
    break
  fi
  sleep 1
done

$ipc call network connectEap FORMALTEST-EAP formaltest formaltest-eap-pw > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 35 ]; do
  $ipc call network status > "$wifi_eap_status_path" 2>&1
  if grep -qF '"name":"FORMALTEST-EAP","known":true,"connected":true' "$wifi_eap_status_path"; then
    break
  fi
  sleep 1
done
"$grim_bin" "$wifi_eap_connected_path" > /dev/null 2>&1

# Symmetric with the FORMALTEST forget above: leaving this out is exactly how
# an earlier run corrupted the VM's persistent NM state and broke every scan
# after it.
$ipc call network forget FORMALTEST-EAP > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 15 ]; do
  $ipc call network status > "$wifi_eap_forget_status_path" 2>&1
  if grep -qE '"name":"FORMALTEST-EAP","known":false,"connected":false,"stateChanging":false' "$wifi_eap_forget_status_path"; then
    break
  fi
  sleep 1
done
EOF
  hypr_exec_once "bash $script"
}

leg_wifi_assert() {
  if [ ! -s "$wifi_scan_status_path" ] \
    || ! grep -qF '"name":"FORMALTEST"' "$wifi_scan_status_path" \
    || ! grep -qF '"name":"FORMALTEST-EAP"' "$wifi_scan_status_path"; then
    [ -f "$wifi_scan_status_path" ] && cat "$wifi_scan_status_path" >&2
    fail "FORMALTEST/FORMALTEST-EAP never surfaced in a wifi scan"
  fi
  cat "$wifi_scan_status_path"; echo
  leg_wifi_assert_ghost
  # NetworkManager must have genuinely given up before the frame below can be
  # trusted as the failure state rather than a lucky mid-flight capture.
  if [ ! -s "$wifi_wrong_status_path" ] \
    || ! grep -qE '"name":"FORMALTEST","known":(true|false),"connected":false,"stateChanging":false' "$wifi_wrong_status_path"; then
    [ -f "$wifi_wrong_status_path" ] && cat "$wifi_wrong_status_path" >&2
    fail "wrong-password connect never settled to a stable disconnected state within the poll budget"
  fi
  cat "$wifi_wrong_status_path"; echo
  [ -f "$wifi_wrong_path" ] || fail "no wifi-wrong screenshot produced"
  echo "SMOKE_WIFI_WRONG $wifi_wrong_path"
  if [ ! -s "$wifi_connected_status_path" ] \
    || ! grep -qF '"name":"FORMALTEST","known":true,"connected":true' "$wifi_connected_status_path"; then
    [ -f "$wifi_connected_status_path" ] && cat "$wifi_connected_status_path" >&2
    fail "real-password connect never reached connected:true within the poll budget"
  fi
  cat "$wifi_connected_status_path"; echo
  [ -f "$wifi_connected_path" ] || fail "no wifi-connected screenshot produced"
  echo "SMOKE_WIFI_CONNECTED $wifi_connected_path"
  if [ ! -s "$wifi_forget_status_path" ] \
    || ! grep -qE '"name":"FORMALTEST","known":false,"connected":false,"stateChanging":false' "$wifi_forget_status_path"; then
    [ -f "$wifi_forget_status_path" ] && cat "$wifi_forget_status_path" >&2
    fail "forget did not settle FORMALTEST to known:false/stateChanging:false within the poll budget"
  fi
  cat "$wifi_forget_status_path"; echo
  if [ ! -s "$wifi_eap_status_path" ] \
    || ! grep -qF '"name":"FORMALTEST-EAP","known":true,"connected":true' "$wifi_eap_status_path"; then
    [ -f "$wifi_eap_status_path" ] && cat "$wifi_eap_status_path" >&2
    fail "connectEap never reached connected:true for FORMALTEST-EAP within the poll budget"
  fi
  cat "$wifi_eap_status_path"; echo
  [ -f "$wifi_eap_connected_path" ] || fail "no wifi-eap-connected screenshot produced"
  echo "SMOKE_WIFI_EAP_CONNECTED $wifi_eap_connected_path"
  # A leftover profile here is what corrupts the VM's persistent NM state for
  # every run after this one.
  if [ ! -s "$wifi_eap_forget_status_path" ] \
    || ! grep -qE '"name":"FORMALTEST-EAP","known":false,"connected":false,"stateChanging":false' "$wifi_eap_forget_status_path"; then
    [ -f "$wifi_eap_forget_status_path" ] && cat "$wifi_eap_forget_status_path" >&2
    fail "closing forget did not settle FORMALTEST-EAP to known:false/stateChanging:false within the poll budget"
  fi
  cat "$wifi_eap_forget_status_path"; echo
}

# The host's tesseract, the one `capture text` runs.
wifi_ocr() {
  command -v tesseract >/dev/null 2>&1 && tesseract --list-langs 2>/dev/null | grep -qx eng \
    || fail "no tesseract with eng data on PATH; install the openSUSE packages tesseract-ocr and tesseract-ocr-traineddata-english"
  tesseract "$1" - 2>/dev/null
}

leg_wifi_assert_ghost() {
  local f near closed_text open_text
  for f in "$wifi_ghost_closed_status_path" "$wifi_ghost_open_status_path" "$wifi_ghost_gone_status_path"; do
    [ -s "$f" ] || fail "no network status produced at $f"
  done
  cat "$wifi_ghost_add_path"
  cat "$wifi_ghost_closed_status_path"; echo
  "$jq_bin" -e '.knownOpen == false and any(.networks[]; .name == "FORMALGHOST" and .known and (.inRange | not))' \
    "$wifi_ghost_closed_status_path" > /dev/null \
    || fail "FORMALGHOST is not a saved network out of range under a closed disclosure"
  cat "$wifi_ghost_open_status_path"; echo
  "$jq_bin" -e '.knownOpen == true' "$wifi_ghost_open_status_path" > /dev/null \
    || fail "Down, Up, Up, Return did not open the Known networks disclosure"
  if grep -qF '"name":"FORMALGHOST"' "$wifi_ghost_gone_status_path"; then
    fail "FORMALGHOST's profile outlived the leg: $(cat "$wifi_ghost_gone_status_path")"
  fi
  [ -f "$wifi_ghost_closed_path" ] || fail "no wifi-ghost-closed screenshot produced"
  [ -f "$wifi_ghost_open_path" ] || fail "no wifi-ghost-open screenshot produced"
  echo "SMOKE_WIFI_GHOST_CLOSED $wifi_ghost_closed_path"
  echo "SMOKE_WIFI_GHOST_OPEN $wifi_ghost_open_path"
  near=$("$jq_bin" '[.networks[] | select(.inRange)] | length' "$wifi_ghost_closed_status_path")
  closed_text=$(wifi_ocr "$wifi_ghost_closed_path")
  open_text=$(wifi_ocr "$wifi_ghost_open_path")
  printf 'closed frame text:\n%s\nopen frame text:\n%s\n' "$closed_text" "$open_text"
  # Tesseract reads the count's closing paren as `]` and the header's K as
  # lowercase on some frames.
  grep -qE "^Networks \($near[])]" <<< "$closed_text" || fail "the closed frame does not read \"Networks ($near)\", the in-range count"
  grep -qiF "Known networks" <<< "$closed_text" || fail "the closed frame carries no Known networks header"
  grep -qiF "Known networks" <<< "$open_text" || fail "the open frame lost the Known networks header"
  if grep -qF FORMALGHOST <<< "$closed_text"; then
    fail "FORMALGHOST shows in the panel with the disclosure closed"
  fi
  grep -qF FORMALGHOST <<< "$open_text" || fail "FORMALGHOST is not under the opened Known networks disclosure"
}
