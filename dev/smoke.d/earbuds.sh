# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --earbuds proves the earbuds panel (spec 2026-09-30-m77-earbuds.md
# "Verification") against PATH-shimmed `nothingctl` and `openscq30`. Every
# line either shim prints is a fixture under dev/smoke.d/fixtures
# (nothing-b175-custom.json, nothingctl's own serialisation; the soundcore-*
# files, captured from a real openscq30 2.12.0; airpods-pro3.json, the
# librepods daemon's own status.json), so the shims never invent a shape. The nothingctl shim logs
# every `watch` it is asked for and every stdin line it gets, and answers a
# line the way nothingctl does (`{"cmd":...,"type":"ack"}`); the openscq30
# shim logs its argv.
#
# BlueZ: the rig has no Bluetooth controller. The Nothing backend does not
# need one (discovery is `nothingctl list --json`, re-run on acquire), but
# the Soundcore backend crosses openscq30's paired list with BlueZ's
# connected devices. The shell's system bus is a private dbus-daemon of the
# leg's own, where dev/bluez-stub.py plays BlueZ with one adapter and the
# Soundcore pair paired and connected.
#
# Which devices exist in each phase is the shims' own list files, rewritten
# by the drive script between phases; closing and reopening the panel
# releases and reacquires the service, which runs discovery again. Phases:
#  1. the B175 alone, on its custom preset: wrapped listening mode and EQ
#     rows, three bands in signed dB. `earbuds set anc transparency` and
#     `earbuds set eq-bass 5` over IPC reach the shim's stdin as exactly
#     `anc transparency` and `eq-custom 5 0 -2`.
#  2. the Soundcore pair alone; `earbuds set mode NoiseCanceling` reaches
#     openscq30 as one `setting --set ambientSoundMode=NoiseCanceling`.
#  3. both at once, the device choice heading the panel.
#  4. the AirPods through dev/librepods-stub.py, the omarchy-pods daemon's
#     socket and status file, its `heart_rate` reporting the service
#     available: opening the panel reaches the socket as `heartrate:on`, the
#     stub's samples settle and `earbuds status` reads the bpm and a history
#     back. The heart rate block is photographed reading Measuring, then
#     the settled bpm and its trace, then eight frames through one beat at
#     a quarter speed across a reopen (earbuds-heart-beat-<n>), which sends
#     `heartrate:off` and `on` again; the panel commits at least 20 frames
#     in 2s while the heart beats and none in 3s once closed. Then
#     `earbuds set noise <mode>` for each of the four modes reaches
#     the socket as exactly `noise:<mode>`, the panel holds the mode the
#     stub reports back, and closing it sends `heartrate:off`. With the
#     spot checks shortened to every 10s with a 4s window
#     (FORMALSHELL_SMOKE_HEART_CHECK; a panel close restarts the interval),
#     the panel stays shut until one runs: `heartrate:on` then `off`, and
#     `earbuds status` carrying one point, its median. The panel opened then
#     is photographed reading that point (earbuds-heart-point) before its
#     own stream settles. The same status with both buds out, then with the
#     service unavailable, each sits through a check with no verb and the
#     shell logging the skip. The same
#     status with the service unavailable gets no heart rate verb at all
#     (sent then, the real daemon drops the link) and no `heartRate`; with
#     conversation awareness on, pw-record reading a pw-loopback source
#     sends `ca:off` and stopping it `ca:on`, the panel's `ca` toggle off
#     then on again. Then
#     the stub listening without ever accepting, as e1504g's daemon did with
#     its event loop stuck: the verb is never taken and the device says
#     "librepods not responding".
#     Head motion rides the same stub: its status carries `motion`
#     available, and the panel opening reaches the socket as `motion:watch`
#     (logged apart from the verbs, in librepods-motion.txt). The stub
#     starts 25 degrees round and turns 40 more over a 6 s hold; with the
#     panel still open from the noise loop, `earbuds status` reads the motion
#     live, yaw 35 to 45 (so measured from the watch's first sample, not
#     65), pitch near 0 and a gravity triple, `earbuds recenter` brings yaw
#     within 3 degrees of 0 mid-hold, and closing the panel ends the watch
#     (`motion:end`; so does each other close). A stub answering the watch
#     with `error: ...` leaves the device with no `motion`, and a status
#     with no `motion` key (the noheart one) is never watched.
#     The head block, on a status with motion and no heart rate service, its
#     watches logged apart (librepods-motion-head.txt): a watch held open
#     with nothing sent photographed reading "Waiting for motion"
#     (earbuds-head-waiting), both buds out reading "Put an AirPod in"
#     (earbuds-head-nobud), then the script played four times slower from
#     its still second before the nod: the head at rest (earbuds-head-rest,
#     read by the panel language check off a dump taken beside it),
#     mid-nod (earbuds-head-nod) and mid-turn (earbuds-head-turn), the panel
#     committing at least 30 frames in 3s through the nod and none in 3s
#     once closed, and `earbuds status` reading the turn live and left.
#  5. a device nothingctl refuses: its `watch` prints the v0.1.1
#     `unsupported-model` line and exits 3. Twelve seconds later (past the
#     5s first backoff every other exit gets) it was started exactly once
#     and no device is listed.
leg_earbuds_flag="--earbuds"
leg_earbuds_order=176
leg_earbuds_needs="jq python3 pw-loopback pw-record"
need_pw_record() { need_tool pw_record_bin pw-record pipewire-tools; }

earbuds_dir="$shot_dir/earbuds"
earbuds_shim_dir="$earbuds_dir/shim"
earbuds_nothing_list="$earbuds_dir/nothing-list.json"
earbuds_nothing_runs="$earbuds_dir/nothing-runs.txt"
earbuds_nothing_stdin="$earbuds_dir/nothing-stdin.txt"
earbuds_scq_paired="$earbuds_dir/soundcore-paired.json"
earbuds_scq_argv="$earbuds_dir/soundcore-argv.txt"
earbuds_bluez_list="$earbuds_dir/bluetooth.json"
earbuds_bus="$earbuds_dir/bluez-bus"

earbuds_b175_address="AA:BB:CC:DD:EE:FF"
earbuds_refused_address="AA:BB:CC:00:00:01"
earbuds_scq_address="AC:12:2F:11:22:33"

earbuds_b175_png="$shot_dir/earbuds-b175.png"
earbuds_soundcore_png="$shot_dir/earbuds-soundcore.png"
earbuds_both_png="$shot_dir/earbuds-both.png"
earbuds_airpods_png="$shot_dir/earbuds-airpods.png"
earbuds_airpods_heart_png="$shot_dir/earbuds-airpods-heart.png"
earbuds_heart_measuring_png="$shot_dir/earbuds-heart-measuring.png"
earbuds_heart_beat_dir="$shot_dir/earbuds-heart-beat"
earbuds_heart_commits="$shot_dir/earbuds-heart-commits.txt"
earbuds_heart_point_png="$shot_dir/earbuds-heart-point.png"
earbuds_head_waiting_png="$shot_dir/earbuds-head-waiting.png"
earbuds_head_nobud_png="$shot_dir/earbuds-head-nobud.png"
earbuds_head_rest_png="$shot_dir/earbuds-head-rest.png"
earbuds_head_nod_png="$shot_dir/earbuds-head-nod.png"
earbuds_head_turn_png="$shot_dir/earbuds-head-turn.png"
earbuds_head_commits="$shot_dir/earbuds-head-commits.txt"
earbuds_head_dump="$earbuds_dir/head-dump.json"
earbuds_airpods_hung_png="$shot_dir/earbuds-airpods-hung.png"
earbuds_refused_png="$shot_dir/earbuds-refused.png"

earbuds_devices_b175="$shot_dir/earbuds-devices-b175.json"
earbuds_devices_soundcore="$shot_dir/earbuds-devices-soundcore.json"
earbuds_devices_both="$shot_dir/earbuds-devices-both.json"
earbuds_devices_airpods="$shot_dir/earbuds-devices-airpods.json"
earbuds_devices_refused="$shot_dir/earbuds-devices-refused.json"
earbuds_status_b175="$shot_dir/earbuds-status-b175.json"
earbuds_replies="$shot_dir/earbuds-set-replies.txt"
earbuds_airpods_replies="$shot_dir/earbuds-airpods-replies.txt"
earbuds_librepods_socket="$iso_home/librepods.sock"
earbuds_librepods_log="$earbuds_dir/librepods-verbs.txt"
earbuds_librepods_motion="$earbuds_dir/librepods-motion.txt"
earbuds_motion_replies="$earbuds_dir/motion-replies.txt"
earbuds_head_motion="$earbuds_dir/librepods-motion-head.txt"
earbuds_head_verbs="$earbuds_dir/librepods-verbs-head.txt"
earbuds_librepods_err="$earbuds_dir/librepods-stub.err"

leg_earbuds_validate() {
  local other
  for other in bluez_rssi bluez_player; do
    if leg_on "$other"; then
      echo "usage: --earbuds serves its own BlueZ and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_earbuds_fixture() {
  local b175 settings values airpods
  mkdir -p "$earbuds_shim_dir"
  b175=$(cat dev/smoke.d/fixtures/nothing-b175-custom.json)
  settings=$(cat dev/smoke.d/fixtures/soundcore-settings-earbuds.json)
  values=$(cat dev/smoke.d/fixtures/soundcore-values-edited.json)
  airpods=$(cat dev/smoke.d/fixtures/airpods-pro2.json)
  cp dev/smoke.d/fixtures/soundcore-paired.json "$earbuds_dir/soundcore-paired-fixture.json"
  printf '%s\n' "$airpods" | "$jq_bin" -c '.heart_rate = {"wanted":false,"available":true,"streaming":false,"bpm":0,"settled":false,"sampled_at_ms":0}' \
    > "$earbuds_dir/airpods-status.json"
  printf '%s\n' "$airpods" | "$jq_bin" -c '.conversational_awareness = true | .heart_rate = {"wanted":false,"available":false,"streaming":false,"bpm":0,"settled":false,"sampled_at_ms":0}' \
    > "$earbuds_dir/airpods-status-noheart.json"
  "$jq_bin" -c '.motion = {"available":true,"streaming":false}' "$earbuds_dir/airpods-status.json" > "$earbuds_dir/airpods-status-motion.json"
  mv "$earbuds_dir/airpods-status-motion.json" "$earbuds_dir/airpods-status.json"
  "$jq_bin" -c '.motion = {"available":true,"streaming":false}' "$earbuds_dir/airpods-status-noheart.json" > "$earbuds_dir/airpods-status-moterr.json"
  "$jq_bin" -c '.left.in_ear = false | .right.in_ear = false' "$earbuds_dir/airpods-status.json" \
    > "$earbuds_dir/airpods-status-nobud.json"
  "$jq_bin" -c '.left.in_ear = false | .right.in_ear = false' "$earbuds_dir/airpods-status-moterr.json" \
    > "$earbuds_dir/airpods-status-head-nobud.json"
  for f in "$b175" "$settings" "$values" "$airpods"; do
    [ -n "$f" ] || { echo "earbuds: a fixture under dev/smoke.d/fixtures is missing" >&2; exit 1; }
  done

  printf '[{"address":"%s","name":"CMF Headphone Pro","connected":true}]\n' "$earbuds_b175_address" > "$earbuds_nothing_list"
  printf '[]\n' > "$earbuds_scq_paired"
  : > "$earbuds_nothing_runs"
  : > "$earbuds_nothing_stdin"
  : > "$earbuds_scq_argv"

  cat > "$earbuds_shim_dir/nothingctl" <<EOF
#!/usr/bin/env bash
case "\$1" in
  list)
    cat "$earbuds_nothing_list"
    ;;
  watch)
    printf 'watch %s %s\n' "\$3" "\$(date +%s)" >> "$earbuds_nothing_runs"
    if [ "\$3" = "$earbuds_refused_address" ]; then
      printf '%s\n' '{"code":"unsupported-model","message":"model id 0A1B2C on $earbuds_refused_address is not supported (supported: B175 CMF Headphone Pro); nothing was sent","modelId":"0A1B2C","type":"error"}'
      echo "error: model id 0A1B2C on $earbuds_refused_address is not supported (supported: B175 CMF Headphone Pro); nothing was sent" >&2
      exit 3
    fi
    printf '%s\n' '$b175'
    while IFS= read -r line; do
      printf '%s\n' "\$line" >> "$earbuds_nothing_stdin"
      printf '{"cmd":"%s","type":"ack"}\n' "\$line"
    done
    ;;
  *)
    exit 2
    ;;
esac
EOF
  chmod +x "$earbuds_shim_dir/nothingctl"

  cat > "$earbuds_shim_dir/openscq30" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$earbuds_scq_argv"
case "\$*" in
  "paired-devices list --json")
    cat "$earbuds_scq_paired"
    ;;
  "device -a $earbuds_scq_address list-settings --no-categories --json")
    printf '%s\n' '$settings'
    ;;
  "device -a $earbuds_scq_address setting --get"*)
    printf '%s\n' '$values'
    ;;
  "device -a $earbuds_scq_address setting --set"*)
    ;;
  *)
    exit 1
    ;;
esac
EOF
  chmod +x "$earbuds_shim_dir/openscq30"

  # Not in session_env (dev/smoke.sh's header on clipssh's own PATH trick):
  # Hyprland and everything it spawns inherit this script's environment.
  export PATH="$earbuds_shim_dir:$PATH"
  export FORMALSHELL_SMOKE_LIBREPODS_SOCKET="$earbuds_librepods_socket"
  export FORMALSHELL_SMOKE_HEART_CHECK="10,4"
  : > "$earbuds_librepods_log"
  : > "$earbuds_librepods_motion"
  : > "$earbuds_motion_replies"
  : > "$earbuds_head_motion"
  : > "$earbuds_head_verbs"
  printf '[{"address":"%s","name":"Liberty 4 NC","connected":true}]\n' "$earbuds_scq_address" > "$earbuds_bluez_list"
  local daemon pid
  daemon=$(command -v dbus-daemon) || fail "--earbuds: no dbus-daemon"
  pid=$("$daemon" --session --address="unix:path=$earbuds_bus" --fork --print-pid --nopidfile)
  add_cleanup "kill $pid 2>/dev/null"
  "$python3_bin" dev/bluez-stub.py --address "unix:path=$earbuds_bus" --devices "$earbuds_bluez_list" \
    --ready "$earbuds_dir/bluez-ready" > "$earbuds_dir/bluez-stub.log" 2>&1 &
  add_cleanup "kill $! 2>/dev/null"
  for _ in $(seq 50); do
    [ -s "$earbuds_dir/bluez-ready" ] && break
    sleep 0.1
  done
  [ -s "$earbuds_dir/bluez-ready" ] || fail "--earbuds: the stub never owned org.bluez ($(cat "$earbuds_dir/bluez-stub.log"))"
  shell_env="${shell_env:+$shell_env }DBUS_SYSTEM_BUS_ADDRESS=unix:path=$earbuds_bus"
}

leg_earbuds_timing() {
  # Five phases of sleeps (~135s, a spot check's wait up to 30s more) plus
  # some thirty `ipc call` round trips at about a second each on llvmpipe.
  leg_timing 230 280
}

leg_earbuds_drive() {
  local script="$shot_dir/earbuds-drive.sh"
  local state_dir="$iso_home/.local/state/librepods"
  write_script "$script" <<EOF
#!/usr/bin/env bash
ipc() { $ipc call "\$@"; }
nothing_list() { printf '%s\n' "\$1" > "$earbuds_nothing_list"; }
scq_paired() { cp "\$1" "$earbuds_scq_paired"; }
none='[]'

sleep 3
ipc panel open earbuds > /dev/null 2>&1
sleep 3
ipc earbuds devices > "$earbuds_devices_b175" 2>&1
ipc earbuds status > "$earbuds_status_b175" 2>&1
"$grim_bin" "$earbuds_b175_png" > /dev/null 2>&1
{
  echo "anc: \$(ipc earbuds set anc transparency 2>&1)"
  echo "eq-bass: \$(ipc earbuds set eq-bass 5 2>&1)"
} >> "$earbuds_replies"
sleep 1
ipc panel close > /dev/null 2>&1

nothing_list "\$none"
scq_paired "$earbuds_dir/soundcore-paired-fixture.json"
sleep 1
ipc panel open earbuds > /dev/null 2>&1
sleep 4
ipc earbuds devices > "$earbuds_devices_soundcore" 2>&1
"$grim_bin" "$earbuds_soundcore_png" > /dev/null 2>&1
echo "mode: \$(ipc earbuds set mode NoiseCanceling 2>&1)" >> "$earbuds_replies"
sleep 2
ipc panel close > /dev/null 2>&1

nothing_list '[{"address":"$earbuds_b175_address","name":"CMF Headphone Pro","connected":true}]'
sleep 1
ipc panel open earbuds > /dev/null 2>&1
sleep 5
ipc earbuds devices > "$earbuds_devices_both" 2>&1
"$grim_bin" "$earbuds_both_png" > /dev/null 2>&1
ipc panel close > /dev/null 2>&1

nothing_list "\$none"
printf '[]\n' > "$earbuds_scq_paired"
mkdir -p "$state_dir"
"$python3_bin" "$PWD/dev/librepods-stub.py" --socket "$earbuds_librepods_socket" --status "$state_dir/status.json" \
  --fixture "$earbuds_dir/airpods-status.json" --log "$earbuds_librepods_log" --motion-log "$earbuds_librepods_motion" \
  2> "$earbuds_librepods_err" &
stub=\$!
sleep 1
commits() { grep -c '^commit surface=panel ' "$shell_log_path"; }
ipc panel open earbuds > /dev/null 2>&1
sleep 1.2
"$grim_bin" "$earbuds_heart_measuring_png" > /dev/null 2>&1
sleep 1.8
ipc earbuds devices > "$earbuds_devices_airpods" 2>&1
"$grim_bin" "$earbuds_airpods_png" > /dev/null 2>&1
sleep 5
ipc earbuds status > "$earbuds_dir/airpods-status-heart.json" 2>&1
"$grim_bin" "$earbuds_airpods_heart_png" > /dev/null 2>&1
c0=\$(commits)
sleep 2
c1=\$(commits)
# Reopened at a quarter speed, a panel taking the scale it opens at: one
# beat stretched to ~3s, so the burst runs through the swell and the ring.
ipc panel close > /dev/null 2>&1
sleep 1
ipc debug motionScale 400 > /dev/null 2>&1
ipc panel open earbuds > /dev/null 2>&1
sleep 7
for i in 0 1 2 3 4 5 6 7; do
  "$grim_bin" "$earbuds_heart_beat_dir-\$i.png" > /dev/null 2>&1
  sleep 0.4
done
ipc debug motionScale 100 > /dev/null 2>&1
for mode in anc transparency adaptive off; do
  echo "\$mode: \$(ipc earbuds set noise \$mode 2>&1)" >> "$earbuds_airpods_replies"
  sleep 2
  ipc earbuds status > "$earbuds_dir/airpods-status-\$mode.json" 2>&1
done
# The stub's head turns 40 degrees and holds it for 6s; the watch has run
# since the reopen, so catch the hold wherever the cycle is.
for _ in \$(seq 40); do
  ipc earbuds status > "$earbuds_dir/airpods-motion-turn.json" 2>&1
  [ "\$("$jq_bin" -r '(.device.motion.yaw // 0) > 35' "$earbuds_dir/airpods-motion-turn.json" 2>/dev/null)" = true ] && break
  sleep 0.2
done
echo "recenter: \$(ipc earbuds recenter 2>&1)" >> "$earbuds_motion_replies"
sleep 0.5
ipc earbuds status > "$earbuds_dir/airpods-motion-recentered.json" 2>&1
ipc panel close > /dev/null 2>&1
sleep 4
c2=\$(commits)
sleep 3
c3=\$(commits)
echo "\$c0 \$c1 \$c2 \$c3" > "$earbuds_heart_commits"
# The spot check, due 10s after that close: on, settled samples for 4s, off.
for _ in \$(seq 60); do
  [ "\$(wc -l < "$earbuds_librepods_log")" -ge 10 ] && break
  sleep 0.5
done
ipc earbuds status > "$earbuds_dir/airpods-status-point.json" 2>&1
ipc panel open earbuds > /dev/null 2>&1
sleep 0.8
"$grim_bin" "$earbuds_heart_point_png" > /dev/null 2>&1
sleep 1
ipc panel close > /dev/null 2>&1
sleep 1
kill \$stub
"$python3_bin" "$PWD/dev/librepods-stub.py" --socket "$earbuds_librepods_socket" --status "$state_dir/status.json" \
  --fixture "$earbuds_dir/airpods-status-nobud.json" --log "$earbuds_librepods_log" 2>> "$earbuds_librepods_err" &
stub=\$!
sleep 13
kill \$stub
rm -f "$state_dir/status.json"
"$python3_bin" "$PWD/dev/librepods-stub.py" --motion-error --socket "$earbuds_librepods_socket" --status "$state_dir/status.json" \
  --fixture "$earbuds_dir/airpods-status-moterr.json" --log "$earbuds_librepods_log" --motion-log "$earbuds_librepods_motion" \
  2>> "$earbuds_librepods_err" &
stub=\$!
sleep 1
ipc panel open earbuds > /dev/null 2>&1
sleep 3
ipc earbuds status > "$earbuds_dir/airpods-motion-error.json" 2>&1
ipc panel close > /dev/null 2>&1
sleep 1
kill \$stub
rm -f "$state_dir/status.json"
head_stub() {
  "$python3_bin" "$PWD/dev/librepods-stub.py" --socket "$earbuds_librepods_socket" --status "$state_dir/status.json" \
    --log "$earbuds_head_verbs" --motion-log "$earbuds_head_motion" "\$@" 2>> "$earbuds_librepods_err" &
  stub=\$!
  sleep 1
}
head_stop() {
  kill \$stub
  rm -f "$state_dir/status.json"
}
head_stub --motion-silent --fixture "$earbuds_dir/airpods-status-moterr.json"
ipc panel open earbuds > /dev/null 2>&1
sleep 2.5
"$grim_bin" "$earbuds_head_waiting_png" > /dev/null 2>&1
ipc panel close > /dev/null 2>&1
sleep 1
head_stop
head_stub --fixture "$earbuds_dir/airpods-status-head-nobud.json"
ipc panel open earbuds > /dev/null 2>&1
sleep 2.5
"$grim_bin" "$earbuds_head_nobud_png" > /dev/null 2>&1
ipc panel close > /dev/null 2>&1
sleep 1
head_stop
# From 9s in at a quarter speed: still for 4s, the nod over 4s (its deepest
# at 6s), still for 8s, then the turn over 4s (half way at 18s). Timed off
# the watch's own log line rather than the open.
head_stub --motion-from 9 --motion-pace 4 --fixture "$earbuds_dir/airpods-status-moterr.json"
ipc panel open earbuds > /dev/null 2>&1
for _ in \$(seq 100); do
  [ "\$(grep -c '^motion:watch' "$earbuds_head_motion")" -ge 3 ] && break
  sleep 0.05
done
start=\$(date +%s%N)
# Sleeps until \$1 tenths of a second past the watch's start.
at() {
  local left=\$(( start + \$1 * 100000000 - \$(date +%s%N) ))
  [ \$left -gt 0 ] && sleep "\$(printf '%d.%09d' \$((left / 1000000000)) \$((left % 1000000000)))"
}
ipc debug dump > "$earbuds_head_dump" 2>&1
at 20
"$grim_bin" "$earbuds_head_rest_png" > /dev/null 2>&1
at 45
c4=\$(commits)
at 60
"$grim_bin" "$earbuds_head_nod_png" > /dev/null 2>&1
at 75
c5=\$(commits)
at 180
"$grim_bin" "$earbuds_head_turn_png" > /dev/null 2>&1
ipc earbuds status > "$earbuds_dir/airpods-head-turn.json" 2>&1
ipc panel close > /dev/null 2>&1
sleep 1
c6=\$(commits)
sleep 3
c7=\$(commits)
echo "\$c4 \$c5 \$c6 \$c7" > "$earbuds_head_commits"
head_stop
"$python3_bin" "$PWD/dev/librepods-stub.py" --socket "$earbuds_librepods_socket" --status "$state_dir/status.json" \
  --fixture "$earbuds_dir/airpods-status-noheart.json" --log "$earbuds_librepods_log" 2>> "$earbuds_librepods_err" &
stub=\$!
sleep 9
ipc panel open earbuds > /dev/null 2>&1
sleep 3
ipc earbuds status > "$earbuds_dir/airpods-noheart-status.json" 2>&1
# Conversation awareness off while something records, back on after.
setsid "$pw_loopback_bin" -n ea-mic -i "{ media.class=Audio/Sink node.name=ea_mic_in }" \
  -o "{ media.class=Audio/Source node.name=ea_mic node.description=EarbudsMic }" > /dev/null 2>&1 < /dev/null &
loop=\$!
sleep 1.5
"$pw_record_bin" --target ea_mic "$earbuds_dir/mic.wav" > /dev/null 2>&1 &
rec=\$!
sleep 2.5
ipc earbuds status > "$earbuds_dir/airpods-ca-recording.json" 2>&1
kill \$rec 2>/dev/null
wait \$rec 2>/dev/null
sleep 2.5
ipc earbuds status > "$earbuds_dir/airpods-ca-after.json" 2>&1
kill \$loop 2>/dev/null
rm -f "$earbuds_dir/mic.wav"
kill \$stub
"$python3_bin" "$PWD/dev/librepods-stub.py" --hang --socket "$earbuds_librepods_socket" --status "$state_dir/status.json" \
  --fixture "$earbuds_dir/airpods-status-noheart.json" --log "$earbuds_librepods_log" 2>> "$earbuds_librepods_err" &
stub=\$!
sleep 1
echo "hung: \$(ipc earbuds set noise anc 2>&1)" >> "$earbuds_airpods_replies"
sleep 4
ipc earbuds status > "$earbuds_dir/airpods-status-hung.json" 2>&1
"$grim_bin" "$earbuds_airpods_hung_png" > /dev/null 2>&1
ipc panel close > /dev/null 2>&1
kill \$stub
rm -f "$state_dir/status.json"

nothing_list '[{"address":"$earbuds_refused_address","name":"Ear (3)","connected":true}]'
sleep 1
ipc panel open earbuds > /dev/null 2>&1
sleep 12
ipc earbuds devices > "$earbuds_devices_refused" 2>&1
"$grim_bin" "$earbuds_refused_png" > /dev/null 2>&1
ipc panel close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_earbuds_assert() {
  local png count expected
  for png in "$earbuds_b175_png" "$earbuds_soundcore_png" "$earbuds_both_png" "$earbuds_airpods_png" "$earbuds_airpods_heart_png" "$earbuds_heart_measuring_png" "$earbuds_heart_point_png" "$earbuds_heart_beat_dir-0.png" "$earbuds_head_waiting_png" "$earbuds_head_nobud_png" "$earbuds_head_rest_png" "$earbuds_head_nod_png" "$earbuds_head_turn_png" "$earbuds_airpods_hung_png" "$earbuds_refused_png"; do
    [ -s "$png" ] || fail "earbuds: no screenshot at $png"
  done
  echo "SMOKE_EARBUDS_B175 $earbuds_b175_png"
  echo "SMOKE_EARBUDS_SOUNDCORE $earbuds_soundcore_png"
  echo "SMOKE_EARBUDS_BOTH $earbuds_both_png"
  echo "SMOKE_EARBUDS_AIRPODS $earbuds_airpods_png"
  echo "SMOKE_EARBUDS_AIRPODS_HEART $earbuds_airpods_heart_png"
  echo "SMOKE_EARBUDS_HEART_MEASURING $earbuds_heart_measuring_png"
  echo "SMOKE_EARBUDS_HEART_POINT $earbuds_heart_point_png"
  local beat n
  for beat in "$earbuds_heart_beat_dir"-*.png; do
    n=${beat##*-}
    echo "SMOKE_EARBUDS_HEART_BEAT_${n%.png} $beat"
  done
  echo "SMOKE_EARBUDS_HEAD_WAITING $earbuds_head_waiting_png"
  echo "SMOKE_EARBUDS_HEAD_NOBUD $earbuds_head_nobud_png"
  echo "SMOKE_EARBUDS_HEAD_REST $earbuds_head_rest_png"
  echo "SMOKE_EARBUDS_HEAD_NOD $earbuds_head_nod_png"
  echo "SMOKE_EARBUDS_HEAD_TURN $earbuds_head_turn_png"
  echo "SMOKE_EARBUDS_AIRPODS_HUNG $earbuds_airpods_hung_png"
  echo "SMOKE_EARBUDS_REFUSED $earbuds_refused_png"
  echo "SMOKE_EARBUDS_STATUS $earbuds_status_b175"

  cat "$earbuds_devices_b175"; echo
  [ "$("$jq_bin" -r 'map(.key) | join(",")' "$earbuds_devices_b175" 2>/dev/null)" = "nothing:$earbuds_b175_address" ] \
    || fail "earbuds: phase 1 should list the B175 alone, got $(cat "$earbuds_devices_b175")"
  [ "$("$jq_bin" -r '.device.controls | map(select(.key == "eq-bass"))[0].unit' "$earbuds_status_b175" 2>/dev/null)" = "db" ] \
    || fail "earbuds: the B175's bass band is not a dB range: $(cat "$earbuds_status_b175")"

  cat "$earbuds_replies"
  expected="anc: ok
eq-bass: ok
mode: ok"
  [ "$(cat "$earbuds_replies")" = "$expected" ] || fail "earbuds: an IPC set was refused: $(cat "$earbuds_replies")"

  echo "nothingctl stdin:"; cat "$earbuds_nothing_stdin"
  expected="anc transparency
eq-custom 5 0 -2"
  [ "$(cat "$earbuds_nothing_stdin")" = "$expected" ] \
    || fail "earbuds: nothingctl's stdin should be exactly '$expected', got '$(cat "$earbuds_nothing_stdin")'"

  count=$(grep -c -- "--set" "$earbuds_scq_argv" || true)
  grep -- "--set" "$earbuds_scq_argv" || true
  [ "$count" = "1" ] && grep -qxF "device -a $earbuds_scq_address setting --set ambientSoundMode=NoiseCanceling --json" "$earbuds_scq_argv" \
    || fail "earbuds: openscq30 should get one ambientSoundMode=NoiseCanceling set, got: $(grep -- "--set" "$earbuds_scq_argv")"

  cat "$earbuds_devices_soundcore"; echo
  [ "$("$jq_bin" -r 'map(.key) | join(",")' "$earbuds_devices_soundcore" 2>/dev/null)" = "soundcore:$earbuds_scq_address" ] \
    || fail "earbuds: phase 2 should list the Soundcore pair alone, got $(cat "$earbuds_devices_soundcore")"

  cat "$earbuds_devices_both"; echo
  [ "$("$jq_bin" -r 'map(.key) | sort | join(",")' "$earbuds_devices_both" 2>/dev/null)" = "nothing:$earbuds_b175_address,soundcore:$earbuds_scq_address" ] \
    || fail "earbuds: phase 3 should list both devices, got $(cat "$earbuds_devices_both")"

  cat "$earbuds_devices_airpods"; echo
  [ "$("$jq_bin" -r 'map(.backend) | join(",")' "$earbuds_devices_airpods" 2>/dev/null)" = "airpods" ] \
    || fail "earbuds: phase 4 should list the AirPods alone, got $(cat "$earbuds_devices_airpods")"
  cat "$earbuds_airpods_replies"
  [ "$(cat "$earbuds_airpods_replies")" = "anc: ok
transparency: ok
adaptive: ok
off: ok
hung: ok" ] || fail "earbuds: an AirPods listening mode set was refused: $(cat "$earbuds_airpods_replies")"
  echo "librepods verbs:"; cat "$earbuds_librepods_log"
  cat "$earbuds_librepods_err"
  [ "$(cat "$earbuds_librepods_log")" = "heartrate:on
heartrate:off
heartrate:on
noise:anc
noise:transparency
noise:adaptive
noise:off
heartrate:off
heartrate:on
heartrate:off
heartrate:on
heartrate:off
ca:off
ca:on" ] || fail "earbuds: librepods should get exactly heartrate:on, off and on again across the reopen, the four noise verbs, heartrate:off, the spot check's on and off, on and off around the point's frame (none with no bud in or the service unavailable), and ca:off then ca:on around a recording, got '$(cat "$earbuds_librepods_log")'"
  echo "head motion:"; cat "$earbuds_librepods_motion"
  [ "$(cat "$earbuds_librepods_motion")" = "motion:watch
motion:end
motion:watch
motion:end
motion:watch
motion:end
motion:watch" ] || fail "earbuds: librepods should get a motion:watch per panel open on the motion status (each closed one ended), the erroring stub's last, and none with no motion key or no panel, got '$(cat "$earbuds_librepods_motion")'"
  got=$("$jq_bin" -c '.device.motion' "$earbuds_dir/airpods-motion-turn.json" 2>/dev/null)
  echo "turned: $got"
  got=$("$jq_bin" -r '.device.motion | [.live, (.yaw > 35 and .yaw < 45), (.pitch > -3 and .pitch < 3), (.gravity | length == 3 and any(. != 0)), (.sampledAtMs > 0)] | map(tostring) | join(",")' "$earbuds_dir/airpods-motion-turn.json" 2>/dev/null)
  [ "$got" = "true,true,true,true,true" ] \
    || fail "earbuds: the open panel should read live head motion 35 to 45 degrees round from its first sample, level, with gravity, got '$got' from $(cat "$earbuds_dir/airpods-motion-turn.json")"
  [ "$(cat "$earbuds_motion_replies")" = "recenter: ok" ] || fail "earbuds: recenter was refused: $(cat "$earbuds_motion_replies")"
  got=$("$jq_bin" -r '.device.motion | [.live, (.yaw > -3 and .yaw < 3)] | map(tostring) | join(",")' "$earbuds_dir/airpods-motion-recentered.json" 2>/dev/null)
  [ "$got" = "true,true" ] \
    || fail "earbuds: recenter should bring yaw within 3 degrees of 0, got '$got' from $(cat "$earbuds_dir/airpods-motion-recentered.json")"
  got=$("$jq_bin" -r '[(.device != null), (.device.motion == null)] | map(tostring) | join(",")' "$earbuds_dir/airpods-motion-error.json" 2>/dev/null)
  [ "$got" = "true,true" ] \
    || fail "earbuds: a watch answered with an error should leave the device with no motion, got '$got' from $(cat "$earbuds_dir/airpods-motion-error.json")"
  got=$("$jq_bin" -r '.device.motion' "$earbuds_dir/airpods-noheart-status.json" 2>/dev/null)
  [ "$got" = "null" ] || fail "earbuds: AirPods with no motion key should carry no motion, got '$got'"
  echo "head block watches:"; cat "$earbuds_head_motion"
  [ "$(cat "$earbuds_head_motion")" = "motion:watch
motion:end
motion:watch
motion:end
motion:watch
motion:end" ] || fail "earbuds: the head block's three opens should each watch and end, got '$(cat "$earbuds_head_motion")'"
  [ ! -s "$earbuds_head_verbs" ] || fail "earbuds: the head block's stubs (no heart rate service) should get no verbs, got '$(cat "$earbuds_head_verbs")'"
  got=$("$jq_bin" -r '.device.motion | [.live, (.yaw > 5 and .yaw < 45)] | map(tostring) | join(",")' "$earbuds_dir/airpods-head-turn.json" 2>/dev/null)
  [ "$got" = "true,true" ] \
    || fail "earbuds: mid-turn the head should read live and turned left, got '$got' from $(cat "$earbuds_dir/airpods-head-turn.json")"
  local rest header=no space=rowInset
  rest=$("$jq_bin" -c '.panel.rest' "$earbuds_head_dump" 2>/dev/null)
  if [ "$("$jq_bin" -r '.panel.header' "$earbuds_head_dump" 2>/dev/null)" = true ]; then
    header=yes space=inset
  fi
  panel_language_assert "earbuds head" "$earbuds_head_rest_png" "$rest" "$header" "$earbuds_head_dump" "$space"
  panel_language_verdict
  local c4 c5 c6 c7
  read -r c4 c5 c6 c7 < "$earbuds_head_commits"
  echo "panel commits: through the nod $((c5 - c4)) in 3s, closed $((c7 - c6)) in 3s"
  [ $((c5 - c4)) -ge 30 ] || fail "earbuds: the turning head should commit the panel while samples arrive, got $((c5 - c4)) commits in 3s"
  [ $((c7 - c6)) -eq 0 ] || fail "earbuds: the closed panel committed $((c7 - c6)) frames in 3s after the head block"
  echo "spot check:"; "$jq_bin" -c '.device.heartRate | {streaming, points}' "$earbuds_dir/airpods-status-point.json"
  got=$("$jq_bin" -r '.device.heartRate | [.streaming, (.points | length), (.points[0].bpm >= 70 and .points[0].bpm <= 80), (.points[0].atMs > 0)] | map(tostring) | join(",")' "$earbuds_dir/airpods-status-point.json" 2>/dev/null)
  [ "$got" = "false,1,true,true" ] \
    || fail "earbuds: a finished spot check should leave the stream stopped and one point of 70 to 80 bpm, got '$got' from $(cat "$earbuds_dir/airpods-status-point.json")"
  for line in "heart check skipped, no bud in" "heart check skipped, heart rate unavailable"; do
    grep -qF "earbuds: $line" "$shell_log_path" || fail "earbuds: no spot check was skipped with '$line' in the shell log"
  done
  echo "heart rate:"; "$jq_bin" -c '.device.heartRate' "$earbuds_dir/airpods-status-heart.json"
  got=$("$jq_bin" -r '.device.heartRate | [.streaming, .settled, .inEar, (.bpm >= 70 and .bpm <= 80), (.history | length >= 2), (.history | all(. >= 70 and . <= 80))] | map(tostring) | join(",")' "$earbuds_dir/airpods-status-heart.json" 2>/dev/null)
  [ "$got" = "true,true,true,true,true,true" ] \
    || fail "earbuds: a settled heart rate stream should read streaming, settled, in ear, a bpm of 70 to 80 and two or more settled samples, got '$got' from $(cat "$earbuds_dir/airpods-status-heart.json")"
  local c0 c1 c2 c3
  read -r c0 c1 c2 c3 < "$earbuds_heart_commits"
  echo "panel commits: open with a settled bpm $((c1 - c0)) in 2s, closed $((c3 - c2)) in 3s"
  [ $((c1 - c0)) -ge 20 ] || fail "earbuds: the beating heart should commit the panel every frame while open, got $((c1 - c0)) commits in 2s"
  [ $((c3 - c2)) -eq 0 ] || fail "earbuds: the closed panel committed $((c3 - c2)) frames in 3s"
  got=$("$jq_bin" -r '.device.heartRate' "$earbuds_dir/airpods-noheart-status.json" 2>/dev/null)
  [ "$got" = "null" ] || fail "earbuds: AirPods without the heart rate service should carry no heartRate, got '$got'"
  got=$(for f in noheart-status ca-recording ca-after; do "$jq_bin" -r '.device.controls[] | select(.key == "ca") | .value' "$earbuds_dir/airpods-$f.json" 2>/dev/null; done | paste -sd, -)
  echo "conversation awareness before, while recording, after: $got"
  [ "$got" = "true,false,true" ] \
    || fail "earbuds: conversation awareness should read on, off while pw-record reads a source, and on again after, got '$got'"
  local pair mode line got
  for pair in "anc:Noise cancellation" "transparency:Transparency" "adaptive:Adaptive" "off:Off"; do
    mode=${pair%%:*}; line=${pair#*:}
    got=$("$jq_bin" -r '[(.device.controls[] | select(.key == "noise") | .value), .device.stateLine] | join("|")' "$earbuds_dir/airpods-status-$mode.json" 2>/dev/null)
    [ "$got" = "$mode|$line" ] \
      || fail "earbuds: after noise:$mode the panel should hold $mode|$line as librepods reports it, got '$got'"
  done
  got=$("$jq_bin" -r '.device.stateLine' "$earbuds_dir/airpods-status-hung.json" 2>/dev/null)
  [ "$got" = "librepods not responding" ] \
    || fail "earbuds: a verb the hung daemon never took should say so on the device, got '$got'"

  echo "nothingctl watch runs:"; cat "$earbuds_nothing_runs"
  cat "$earbuds_devices_refused"; echo
  count=$(grep -c "^watch $earbuds_refused_address " "$earbuds_nothing_runs" || true)
  [ "$count" = "1" ] || fail "earbuds: the refused device was started $count times in 12s, expected once"
  [ "$("$jq_bin" -r 'length' "$earbuds_devices_refused" 2>/dev/null)" = "0" ] \
    || fail "earbuds: the refused device is listed: $(cat "$earbuds_devices_refused")"
}
