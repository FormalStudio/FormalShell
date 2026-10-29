# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --iphone: IphoneService and the notification filter (M75 Task 2/4) against
# PATH-shimmed `omarchy-iphone-bridge` and `omarchy-iphone-ams`. Both shims
# are `tail -F` over a JSONL file this leg's own drive script appends to in
# real time -- the wire is exactly what IphoneService reads (Iphone/model.js
# parseEvent/parseAmsLine), so the shim never invents a shape, only paces
# it. `invoke`/`command` are one-shot: they record their argv to a calls
# file and exit, which is the one claim no frame can make -- that a card's
# action reached the bridge with the right id and kind.
#
# Six things proven, over IPC and in frames both:
#  - connected state and device name (`iphone status`).
#  - an ordinary arrival becomes a toast carrying the iPhone source mark
#    (`sourceMark`), and its
#    positive action reaches the bridge shim's own record.
#  - `iphone.notifications.focus: respect` routes a silent arrival to the
#    centre's pending tier with no toast (Iphone/model.js focusVerdict's
#    "quiet"); `hide` (retargeted mid-run, see below) drops it from the
#    centre entirely while IphoneService.recent still keeps it, since the
#    phone's own history and the desktop notification centre are separate
#    claims (IphoneService._receive upserts `recent` before the verdict is
#    even checked).
#  - the M75 dedupe rule (default `com.apple.MobileSMS` vs `Messages`):
#    a phone message and the same message from a real `notify-send -a
#    Messages` collapse to one card in both arrival orders, and the local
#    one is what survives (NotificationService.notifyPhone's own dedupe()
#    call drops the phone side when local is already up; the ordinary
#    onNotification handler's superseded() call drops the phone side when
#    local arrives after).
#  - the panel (`panel open iphone`), recent list and Now playing populated
#    from the ams shim's own now-playing line.
#  - the ams shim's first run failing its subscribe the way a phone that is
#    not GATT-ready does (error line, then a non-zero exit): the error shows
#    in `iphone status`, the service starts ams again on its backoff, and
#    the second run's now playing lands with the error gone. The error is
#    BlueZ's "Not connected" as a person reads it, not the GDBus string.
#  - a bond BlueZ holds for a phone that is not connected (what a phone that
#    forgot this laptop looks like, packaging/iphone-bridge-bond.patch's status):
#    `iphone status` not connected and bonded, the ams child and its error
#    gone with the link, the panel offering "Pair again", and `iphone pair`
#    reaching the bridge shim with `--forget <address>`, the shim's own
#    `forgot` and `advertising` lines clearing the bond and a pairing code
#    from the listen stream landing in the panel.
#
# The centre-side checks read `notifications status`'s pending+popups SUM,
# never either alone: a popup ages into pending on its own 6s clock
# (model.js DEFAULT_TIMEOUT_MS), and that move is not the thing under test.
# A collapsed dedupe reads as the sum holding steady across a phase; an
# uncollapsed one reads as it climbing by two instead of one.
#
# The focus retarget rewrites settings.json's `iphone.notifications.focus`
# key IN PLACE (jq to a temp file, then `cat` back over the same inode)
# rather than through --config-reload's symlink-retarget dance: that leg
# exists to prove a retargeted *symlink* is picked up, which nothing here
# needs -- the config watcher covers a same-inode rewrite, the ordinary
# case it handles.
leg_iphone_flag="--iphone"
leg_iphone_order=174
leg_iphone_needs="notify-send jq"

iphone_shim_dir="$shot_dir/iphone-shim"
iphone_bridge_events_path="$shot_dir/iphone-bridge-events.jsonl"
iphone_bridge_calls_path="$shot_dir/iphone-bridge-calls.txt"
iphone_ams_events_path="$shot_dir/iphone-ams-events.jsonl"
iphone_ams_calls_path="$shot_dir/iphone-ams-calls.txt"
iphone_ams_runs_path="$shot_dir/iphone-ams-runs.txt"
iphone_status_ams_error_path="$shot_dir/iphone-status-ams-error.json"
iphone_status_ams_recovered_path="$shot_dir/iphone-status-ams-recovered.json"
iphone_status_stale_path="$shot_dir/iphone-status-stale.json"
iphone_status_pairing_path="$shot_dir/iphone-status-pairing.json"
iphone_pair_reply_path="$shot_dir/iphone-pair-reply.txt"
iphone_ams_runs_stale_path="$shot_dir/iphone-ams-runs-stale.txt"
iphone_device_address="AA:BB:CC:DD:EE:01"

iphone_device_name="Fixture iPhone 15"
iphone_device_handle="/org/bluez/hci0/dev_AA_BB_CC_DD_EE_01"

iphone_status_connected_path="$shot_dir/iphone-status-connected.json"
iphone_invoke_reply_path="$shot_dir/iphone-invoke-reply.txt"
iphone_panel_open_path="$shot_dir/iphone-panel-open.txt"

iphone_notify_status_0_path="$shot_dir/iphone-notify-status-0.json"
iphone_notify_status_normal_path="$shot_dir/iphone-notify-status-normal.json"
iphone_notify_status_respect_path="$shot_dir/iphone-notify-status-respect.json"
iphone_notify_status_hide_path="$shot_dir/iphone-notify-status-hide.json"
iphone_notify_status_dedupe_p1_path="$shot_dir/iphone-notify-status-dedupe-p1.json"
iphone_notify_status_dedupe_p2_path="$shot_dir/iphone-notify-status-dedupe-p2.json"
iphone_notify_status_dedupe_l1_path="$shot_dir/iphone-notify-status-dedupe-l1.json"
iphone_notify_status_dedupe_l2_path="$shot_dir/iphone-notify-status-dedupe-l2.json"

iphone_dump_1_path="$shot_dir/iphone-dump-1.json"
iphone_dump_2_path="$shot_dir/iphone-dump-2.json"
iphone_dump_3_path="$shot_dir/iphone-dump-3.json"
iphone_dump_4_path="$shot_dir/iphone-dump-4.json"
iphone_dump_5_path="$shot_dir/iphone-dump-5.json"

iphone_bar_png="$shot_dir/iphone-bar.png"
iphone_normal_toast_png="$shot_dir/iphone-normal-toast.png"
iphone_dedupe_phone_first_png="$shot_dir/iphone-dedupe-phone-first.png"
iphone_dedupe_local_first_png="$shot_dir/iphone-dedupe-local-first.png"
iphone_panel_png="$shot_dir/iphone-panel.png"
iphone_media_png="$shot_dir/iphone-media.png"
iphone_stale_png="$shot_dir/iphone-stale-bond.png"
iphone_pairing_png="$shot_dir/iphone-pairing.png"
iphone_visualizer_a_path="$shot_dir/iphone-visualizer-a.json"
iphone_visualizer_b_path="$shot_dir/iphone-visualizer-b.json"

leg_iphone_fixture() {
  settings_fragment ', "iphone": {"notifications": {"focus": "respect"}}'

  mkdir -p "$iphone_shim_dir"

  # Primed with the bridge's own first line, exactly what a real
  # `bridge listen` prints once ancs4linux reports a bonded, connected
  # phone -- `installed`/`observer`/`connected` all flip on this one line
  # arriving.
  printf '%s\n' \
    '{"type":"status","observer":true,"connected":true,"deviceName":"'"$iphone_device_name"'","battery":81}' \
    > "$iphone_bridge_events_path"
  : > "$iphone_bridge_calls_path"

  printf '%s\n' \
    '{"type":"status","available":true}' \
    '{"type":"nowplaying","title":"Waves","artist":"Fixture Band","album":"Fixture Album","duration":210,"elapsed":12,"playback":"playing","volume":0.6}' \
    > "$iphone_ams_events_path"
  : > "$iphone_ams_calls_path"
  : > "$iphone_ams_runs_path"

  cat > "$iphone_shim_dir/omarchy-iphone-bridge" <<EOF
#!/usr/bin/env bash
case "\$1" in
  listen)
    exec stdbuf -oL tail -n +1 -F "$iphone_bridge_events_path"
    ;;
  invoke|dismiss|clear)
    verb="\$1"; shift
    printf '%s\n' "\$verb \$*" >> "$iphone_bridge_calls_path"
    exit 0
    ;;
  pair)
    printf '%s\n' "\$*" >> "$iphone_bridge_calls_path"
    shift
    while [ \$# -gt 0 ]; do
      if [ "\$1" = "--forget" ]; then
        printf '{"type":"forgot","address":"%s"}\n' "\$2"
      fi
      shift
    done
    printf '%s\n' '{"type":"advertising","hci":"00:00:00:00:00:00","name":"FormalShell"}'
    exit 0
    ;;
  *)
    exit 1
    ;;
esac
EOF
  chmod +x "$iphone_shim_dir/omarchy-iphone-bridge"

  cat > "$iphone_shim_dir/omarchy-iphone-ams" <<EOF
#!/usr/bin/env bash
case "\$1" in
  listen)
    printf 'run\\n' >> "$iphone_ams_runs_path"
    if [ "\$(wc -l < "$iphone_ams_runs_path")" -eq 1 ]; then
      printf '%s\\n' '{"type":"error","message":"subscribe: g-io-error-quark: GDBus.Error:org.bluez.Error.Failed: Not connected (36)"}'
      sleep 15
      exit 1
    fi
    exec stdbuf -oL tail -n +1 -F "$iphone_ams_events_path"
    ;;
  command)
    shift
    printf '%s\n' "command \$*" >> "$iphone_ams_calls_path"
    exit 0
    ;;
  *)
    exit 1
    ;;
esac
EOF
  chmod +x "$iphone_shim_dir/omarchy-iphone-ams"

  # Not in session_env (dev/smoke.sh's header on clipssh's own PATH trick):
  # Hyprland and everything it spawns inherit whatever PATH this script
  # already has by the time it launches the session.
  export PATH="$iphone_shim_dir:$PATH"
}

leg_iphone_timing() {
  # This leg's own drive script runs to just past 30s of sleeps plus
  # roughly a dozen `ipc call` round trips at ~1s each on llvmpipe (notify.sh's
  # own header note on that cost): screenshot_delay has to clear all of it,
  # since the base run's own teardown (shot.sh) fires at that mark and tears
  # the session down under whatever this leg is still doing.
  leg_timing 75 115
}

leg_iphone_drive() {
  local script="$shot_dir/iphone-drive.sh"
  local settings_path="$iso_home/.config/formalshell/settings.json"
  write_script "$script" <<EOF
#!/usr/bin/env bash
append() { printf '%s\n' "\$1" >> "$iphone_bridge_events_path"; }

# The bridge chain is three nested execs deep (sh -c's own probe, the shim
# script, stdbuf into tail) rather than the single binary most other legs'
# shims are, so it earns a longer startup margin before the first check
# that depends on it.
sleep 8
$ipc call iphone status > "$iphone_status_connected_path" 2>&1
cp "$iphone_status_connected_path" "$iphone_status_ams_error_path"

sleep 2
$ipc call notifications status > "$iphone_notify_status_0_path" 2>&1
"$grim_bin" "$iphone_bar_png" > /dev/null 2>&1

# A normal arrival: a toast with an action, positive action wired to the
# reminders app's own "Complete" label.
append '{"type":"notification","id":1,"appId":"com.apple.reminders","appName":"Reminders","title":"Buy milk","subtitle":"","body":"Before the shops close","deviceName":"$iphone_device_name","deviceHandle":"$iphone_device_handle","positiveAction":"Complete","negativeAction":"","category":0,"categoryCount":0,"silent":false,"important":false,"preexisting":false,"session":1,"ts":0}'
sleep 2
$ipc call notifications status > "$iphone_notify_status_normal_path" 2>&1
$ipc call debug dump > "$iphone_dump_1_path" 2>&1
"$grim_bin" "$iphone_normal_toast_png" > /dev/null 2>&1
$ipc call iphone invoke 1 positive > "$iphone_invoke_reply_path" 2>&1

# Silent under "respect": pending, no toast.
sleep 1
append '{"type":"notification","id":2,"appId":"com.apple.weather","appName":"Weather","title":"Storm Watch","subtitle":"","body":"Heavy rain tonight","deviceName":"$iphone_device_name","deviceHandle":"$iphone_device_handle","positiveAction":"","negativeAction":"","category":0,"categoryCount":0,"silent":true,"important":false,"preexisting":false,"session":1,"ts":0}'
sleep 2
$ipc call notifications status > "$iphone_notify_status_respect_path" 2>&1
$ipc call debug dump > "$iphone_dump_2_path" 2>&1

# Retarget iphone.notifications.focus to "hide" in place: same inode, an
# ordinary inotify MODIFY, nothing exotic about the write itself.
"$jq_bin" '.iphone.notifications.focus = "hide"' "$settings_path" > "$settings_path.tmp"
cat "$settings_path.tmp" > "$settings_path"
rm -f "$settings_path.tmp"
sleep 8

# Silent under "hide": dropped from the centre outright.
append '{"type":"notification","id":3,"appId":"com.apple.news","appName":"News","title":"Breaking","subtitle":"","body":"A thing happened","deviceName":"$iphone_device_name","deviceHandle":"$iphone_device_handle","positiveAction":"","negativeAction":"","category":0,"categoryCount":0,"silent":true,"important":false,"preexisting":false,"session":1,"ts":0}'
sleep 2
$ipc call notifications status > "$iphone_notify_status_hide_path" 2>&1
$ipc call debug dump > "$iphone_dump_3_path" 2>&1

# Dedupe, phone first: the phone card alone, then the same message over a
# real notify-send collapses it to one, the local one surviving.
append '{"type":"notification","id":4,"appId":"com.apple.MobileSMS","appName":"Messages","title":"Sam","subtitle":"","body":"Running 10 late, sorry!","deviceName":"$iphone_device_name","deviceHandle":"$iphone_device_handle","positiveAction":"","negativeAction":"","category":0,"categoryCount":0,"silent":false,"important":false,"preexisting":false,"session":1,"ts":0}'
sleep 2
$ipc call notifications status > "$iphone_notify_status_dedupe_p1_path" 2>&1
"$notify_send_bin" -a Messages 'Sam' 'Running 10 late, sorry!'
sleep 2
$ipc call notifications status > "$iphone_notify_status_dedupe_p2_path" 2>&1
$ipc call debug dump > "$iphone_dump_4_path" 2>&1
"$grim_bin" "$iphone_dedupe_phone_first_png" > /dev/null 2>&1

# Dedupe, local first: notify-send lands, then the phone's own copy of the
# same message never becomes a card at all.
"$notify_send_bin" -a Messages 'Jamie' 'On my way, 5 mins'
sleep 2
$ipc call notifications status > "$iphone_notify_status_dedupe_l1_path" 2>&1
append '{"type":"notification","id":5,"appId":"com.apple.MobileSMS","appName":"Messages","title":"Jamie","subtitle":"","body":"On my way, 5 mins","deviceName":"$iphone_device_name","deviceHandle":"$iphone_device_handle","positiveAction":"","negativeAction":"","category":0,"categoryCount":0,"silent":false,"important":false,"preexisting":false,"session":1,"ts":0}'
sleep 2
$ipc call notifications status > "$iphone_notify_status_dedupe_l2_path" 2>&1
$ipc call debug dump > "$iphone_dump_5_path" 2>&1
"$grim_bin" "$iphone_dedupe_local_first_png" > /dev/null 2>&1

$ipc call iphone status > "$iphone_status_ams_recovered_path" 2>&1

# The panel: Recent carrying all five phone-side entries and Now playing
# off the ams shim's own line.
$ipc call panel open iphone > "$iphone_panel_open_path" 2>&1
sleep 2
"$grim_bin" "$iphone_panel_png" > /dev/null 2>&1

# The media panel on the phone as its source: no audio reaches cava, so the
# spectrum is the tempo frame, which moves between two reads.
$ipc call panel open media > /dev/null 2>&1
sleep 2
$ipc call visualizer status > "$iphone_visualizer_a_path" 2>&1
sleep 0.3
$ipc call visualizer status > "$iphone_visualizer_b_path" 2>&1
"$grim_bin" "$iphone_media_png" > /dev/null 2>&1

# The phone forgot this laptop: BlueZ still holds the bond, LE is down.
append '{"type":"status","observer":true,"connected":false,"paired":true,"deviceName":"$iphone_device_name","address":"$iphone_device_address","battery":-1}'
sleep 2
cp "$iphone_ams_runs_path" "$iphone_ams_runs_stale_path"
$ipc call iphone status > "$iphone_status_stale_path" 2>&1
$ipc call panel open iphone > /dev/null 2>&1
sleep 2
"$grim_bin" "$iphone_stale_png" > /dev/null 2>&1

$ipc call iphone pair > "$iphone_pair_reply_path" 2>&1
sleep 1
append '{"type":"status","observer":true,"connected":false,"paired":false,"deviceName":"","address":"","battery":-1}'
append '{"type":"pairingCode","code":"482913"}'
sleep 2
$ipc call iphone status > "$iphone_status_pairing_path" 2>&1
"$grim_bin" "$iphone_pairing_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_iphone_assert() {
  local total0 total1 total2 total3 total4a total4b total5a total5b recent1 recent2 recent3 recent4 recent5

  if [ ! -s "$iphone_status_connected_path" ]; then fail "no iphone status reply"; fi
  cat "$iphone_status_connected_path"; echo
  if ! grep -qF '"connected":true' "$iphone_status_connected_path" \
      || ! grep -qF "\"deviceName\":\"$iphone_device_name\"" "$iphone_status_connected_path"; then
    fail "iphone status did not report the fixture phone connected: $(cat "$iphone_status_connected_path")"
  fi

  total0=$("$jq_bin" -r '.pending + .popups' "$iphone_notify_status_0_path")
  total1=$("$jq_bin" -r '.pending + .popups' "$iphone_notify_status_normal_path")
  total2=$("$jq_bin" -r '.pending + .popups' "$iphone_notify_status_respect_path")
  total3=$("$jq_bin" -r '.pending + .popups' "$iphone_notify_status_hide_path")
  total4a=$("$jq_bin" -r '.pending + .popups' "$iphone_notify_status_dedupe_p1_path")
  total4b=$("$jq_bin" -r '.pending + .popups' "$iphone_notify_status_dedupe_p2_path")
  total5a=$("$jq_bin" -r '.pending + .popups' "$iphone_notify_status_dedupe_l1_path")
  total5b=$("$jq_bin" -r '.pending + .popups' "$iphone_notify_status_dedupe_l2_path")
  echo "notification centre totals (pending+popups): baseline=$total0 normal=$total1 respect=$total2 hide=$total3 dedupe(phone,+local)=$total4a,$total4b dedupe(local,+phone)=$total5a,$total5b"

  [ "$total1" -eq $((total0 + 1)) ] || fail "a normal phone arrival did not raise one card: baseline=$total0 got=$total1"
  [ "$total2" -eq $((total1 + 1)) ] || fail "a silent arrival under 'respect' did not land one entry in the centre: $total1 -> $total2"
  [ "$total3" -eq "$total2" ] || fail "a silent arrival under 'hide' still reached the centre: $total2 -> $total3"
  [ "$total4a" -eq $((total3 + 1)) ] || fail "the phone-only MobileSMS message did not raise a card: $total3 -> $total4a"
  [ "$total4b" -eq "$total4a" ] || fail "the local notify-send did not collapse into the phone's card, expected the total to hold at $total4a, got $total4b"
  [ "$total5a" -eq $((total4b + 1)) ] || fail "the local-first notify-send did not raise a card: $total4b -> $total5a"
  [ "$total5b" -eq "$total5a" ] || fail "the phone's copy of an already-local message still raised a second card: $total5a -> $total5b"

  recent1=$("$jq_bin" -r '.iphone.recentCount' "$iphone_dump_1_path")
  recent2=$("$jq_bin" -r '.iphone.recentCount' "$iphone_dump_2_path")
  recent3=$("$jq_bin" -r '.iphone.recentCount' "$iphone_dump_3_path")
  recent4=$("$jq_bin" -r '.iphone.recentCount' "$iphone_dump_4_path")
  recent5=$("$jq_bin" -r '.iphone.recentCount' "$iphone_dump_5_path")
  echo "IphoneService.recent count: $recent1 $recent2 $recent3 $recent4 $recent5"
  [ "$recent1" -eq 1 ] && [ "$recent2" -eq 2 ] && [ "$recent3" -eq 3 ] && [ "$recent4" -eq 4 ] && [ "$recent5" -eq 5 ] \
    || fail "IphoneService.recent did not carry the phone's own copy of every arrival, including the hidden one: $recent1 $recent2 $recent3 $recent4 $recent5"

  if ! grep -q '^ok$' "$iphone_invoke_reply_path" 2>/dev/null; then
    fail "iphone invoke 1 positive did not answer ok, got: $(cat "$iphone_invoke_reply_path" 2>/dev/null)"
  fi
  if ! grep -q -- '--id 1 --kind positive' "$iphone_bridge_calls_path" 2>/dev/null; then
    fail "the card's positive action never reached the bridge shim's own record: $(cat "$iphone_bridge_calls_path" 2>/dev/null)"
  fi
  cat "$iphone_bridge_calls_path"

  if [ "$("$jq_bin" -r '.lastError == "The iPhone is not connected over Bluetooth LE"' "$iphone_status_ams_error_path")" != "true" ] \
      || [ "$("$jq_bin" -r '.mediaAvailable' "$iphone_status_ams_error_path")" != "false" ]; then
    fail "the failed ams subscribe did not surface as an error with no media: $(cat "$iphone_status_ams_error_path")"
  fi
  if [ "$(wc -l < "$iphone_ams_runs_path")" -lt 2 ]; then
    fail "ams was not started again after its first run exited non-zero: $(cat "$iphone_ams_runs_path")"
  fi
  if [ "$("$jq_bin" -r '.mediaAvailable and .mediaTitle == "Waves" and .lastError == ""' "$iphone_status_ams_recovered_path")" != "true" ]; then
    fail "the second ams run did not bring now playing up with the error cleared: $(cat "$iphone_status_ams_recovered_path")"
  fi

  if ! grep -q '^ok$' "$iphone_panel_open_path" 2>/dev/null; then
    fail "panel open iphone did not answer ok, got: $(cat "$iphone_panel_open_path" 2>/dev/null)"
  fi

  if [ "$("$jq_bin" -r '.running' "$iphone_visualizer_a_path" 2>/dev/null)" != "true" ]; then
    fail "visualizer not running on the phone's track: $(cat "$iphone_visualizer_a_path")"
  fi
  if [ "$("$jq_bin" -c '.levels' "$iphone_visualizer_a_path")" = "$("$jq_bin" -c '.levels' "$iphone_visualizer_b_path")" ]; then
    fail "the tempo frame did not move between two reads"
  fi
  if pgrep -x cava > /dev/null; then fail "cava running with the phone as the source"; fi

  cat "$iphone_status_stale_path"; echo
  if [ "$("$jq_bin" -r '(.connected | not) and .bonded and .bondAddress == "'"$iphone_device_address"'" and .lastError == "" and (.mediaAvailable | not)' "$iphone_status_stale_path")" != "true" ]; then
    fail "a bonded phone with its LE link down did not read as not connected and bonded with no media and no error: $(cat "$iphone_status_stale_path")"
  fi
  if [ "$(wc -l < "$iphone_ams_runs_path")" -ne "$(wc -l < "$iphone_ams_runs_stale_path")" ]; then
    fail "ams was started again with the phone's LE link down: $(wc -l < "$iphone_ams_runs_stale_path") runs, then $(wc -l < "$iphone_ams_runs_path")"
  fi
  if ! grep -q '^ok$' "$iphone_pair_reply_path" 2>/dev/null; then
    fail "iphone pair did not answer ok, got: $(cat "$iphone_pair_reply_path" 2>/dev/null)"
  fi
  if ! grep -qF -- "--forget $iphone_device_address" "$iphone_bridge_calls_path"; then
    fail "pair on a stale bond did not ask the bridge to forget it first: $(cat "$iphone_bridge_calls_path")"
  fi
  cat "$iphone_status_pairing_path"; echo
  if [ "$("$jq_bin" -r '.advertising and .pairingCode == "482913" and (.bonded | not) and .bondAddress == ""' "$iphone_status_pairing_path")" != "true" ]; then
    fail "the forgotten bond and the new pairing code did not land: $(cat "$iphone_status_pairing_path")"
  fi

  for f in "$iphone_bar_png" "$iphone_normal_toast_png" "$iphone_dedupe_phone_first_png" "$iphone_dedupe_local_first_png" "$iphone_panel_png" "$iphone_media_png" "$iphone_stale_png" "$iphone_pairing_png"; do
    if [ ! -f "$f" ]; then fail "no iphone screenshot produced at $f"; fi
  done
  echo "SMOKE_IPHONE_BAR $iphone_bar_png"
  echo "SMOKE_IPHONE_NORMAL_TOAST $iphone_normal_toast_png"
  echo "SMOKE_IPHONE_DEDUPE_PHONE_FIRST $iphone_dedupe_phone_first_png"
  echo "SMOKE_IPHONE_DEDUPE_LOCAL_FIRST $iphone_dedupe_local_first_png"
  echo "SMOKE_IPHONE_PANEL $iphone_panel_png"
  echo "SMOKE_IPHONE_MEDIA $iphone_media_png"
  echo "SMOKE_IPHONE_STALE_BOND $iphone_stale_png"
  echo "SMOKE_IPHONE_PAIRING $iphone_pairing_png"
}
