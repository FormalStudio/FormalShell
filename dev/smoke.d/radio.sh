# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --radio plays Radio Atlas through the shell's own mpv against a station
# served on loopback (ffmpeg's one-client HTTP listener, a sine as mp3), saved
# as a favourite in the state fixture and tuned with `radio play <id>`, so no
# run depends on Radio Browser being reachable. The radio is then read back
# as a media source: `media status` naming it (kind, title off the station,
# playing), `media players` listing it with the panel open and NOT listing
# the radio's own Pipewire stream a second time as an app with no MPRIS, the
# panel photographed, and its source menu opened with real keys (two Tabs to
# the menu section, Enter on the trigger) and photographed open. A player
# left on the radio's socket before the shell starts (what a restart under
# KillMode=process leaves) has to be gone once it is up. A second
# null sink gives the output menu something to offer: `media output` moves
# the radio there and mpv's own `audio-device` follows, read back off both
# `radio status` and `media status`. A stop last, with the media source gone
# after it.
leg_radio_flag="--radio"
leg_radio_order=175
leg_radio_needs="ffmpeg wtype pactl mpv"

need_pactl() { need_tool pactl_bin pactl pulseaudio-utils; }

radio_track_path="$shot_dir/radio-station.mp3"
radio_play_path="$shot_dir/radio-play.txt"
radio_status_path="$shot_dir/radio-status.json"
radio_media_status_path="$shot_dir/radio-media-status.json"
radio_players_path="$shot_dir/radio-players.json"
radio_panel_png_path="$shot_dir/radio-panel.png"
radio_menu_png_path="$shot_dir/radio-menu.png"
radio_stopped_path="$shot_dir/radio-stopped.json"
radio_outputs_path="$shot_dir/radio-outputs.json"
radio_output_path="$shot_dir/radio-output.txt"
radio_routed_path="$shot_dir/radio-routed.json"
radio_routed_media_path="$shot_dir/radio-routed-media.json"
radio_module_path="$shot_dir/radio-sink-module.txt"
radio_sink="formalshell-smoke-alt"
radio_leftover_pid_path="$shot_dir/radio-leftover.pid"
radio_leftover_after_path="$shot_dir/radio-leftover-after.txt"
radio_station_id="smoke-radio-1"
radio_station_name="FormalShell Smoke Radio"
radio_port=18099

leg_radio_timing() {
  leg_timing 24 45
}

leg_radio_fixture() {
  local state_dir="$iso_home/.local/state/formalshell"
  mkdir -p "$state_dir"
  cat > "$state_dir/radio-atlas.json" <<EOF
{"favorites":[{"uuid":"$radio_station_id","name":"$radio_station_name","url":"http://127.0.0.1:$radio_port/station.mp3","homepage":"","favicon":"","country":"","countryCode":"","state":"","language":"","tags":"","codec":"MP3","bitrate":128,"votes":0,"clicks":0,"latitude":null,"longitude":null}],"recent":[],"volume":40,"output":""}
EOF
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=44100" -t 60 \
    -c:a libmp3lame -b:a 128k -y "$radio_track_path"
  local socket_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/formalshell-radio-atlas"
  mkdir -p -m 700 "$socket_dir"
  setsid "$mpv_bin" --no-config --idle=yes --no-terminal \
    --input-ipc-server="$socket_dir/mpv.sock" > /dev/null 2>&1 < /dev/null &
  echo $! > "$radio_leftover_pid_path"
}

leg_radio_drive() {
  local script="$shot_dir/radio-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$ffmpeg_bin" -nostdin -loglevel error -re -i "$radio_track_path" -c copy -f mp3 \
  -listen 1 "http://127.0.0.1:$radio_port/station.mp3" &
"$pactl_bin" load-module module-null-sink sink_name=$radio_sink sink_properties=device.description=SmokeAlt > "$radio_module_path"
sleep 4
if kill -0 "\$(cat "$radio_leftover_pid_path")" 2>/dev/null; then echo alive; else echo gone; fi > "$radio_leftover_after_path"
$ipc call radio play "$radio_station_id" > "$radio_play_path" 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  $ipc call radio status > "$radio_status_path" 2>&1
  grep -q '"loaded":true' "$radio_status_path" && break
  sleep 1
done
$ipc call media status > "$radio_media_status_path" 2>&1
$ipc call panel open media > /dev/null 2>&1
sleep 2
$ipc call media players > "$radio_players_path" 2>&1
"$grim_bin" "$radio_panel_png_path" > /dev/null 2>&1
"$wtype_bin" -k Tab -k Tab -k Return
sleep 1.5
"$grim_bin" "$radio_menu_png_path" > /dev/null 2>&1
"$wtype_bin" -k Escape
$ipc call media outputs > "$radio_outputs_path" 2>&1
$ipc call media output $radio_sink > "$radio_output_path" 2>&1
sleep 1
$ipc call radio status > "$radio_routed_path" 2>&1
$ipc call media status > "$radio_routed_media_path" 2>&1
$ipc call radio stop > /dev/null 2>&1
sleep 3
$ipc call media status > "$radio_stopped_path" 2>&1
EOF
  add_cleanup "[ -s '$radio_module_path' ] && '$pactl_bin' unload-module \"\$(cat '$radio_module_path')\" 2>/dev/null || true"
  add_cleanup "kill \"\$(cat '$radio_leftover_pid_path')\" 2>/dev/null || true"
  add_cleanup "pkill -f 'listen 1 http://127.0.0.1:$radio_port' 2>/dev/null || true"
  hypr_exec_once "bash $script"
}

leg_radio_assert() {
  if ! grep -q "^gone$" "$radio_leftover_after_path" 2>/dev/null; then
    fail "the mpv left on the radio's socket survived the shell starting"
  fi
  if ! grep -q "^ok$" "$radio_play_path" 2>/dev/null; then
    fail "radio play did not answer ok, got: $(cat "$radio_play_path" 2>/dev/null)"
  fi
  cat "$radio_status_path"; echo
  if ! grep -q '"loaded":true' "$radio_status_path"; then
    fail "the radio never loaded the loopback station"
  fi
  cat "$radio_media_status_path"; echo
  for want in '"kind":"radio"' "\"title\":\"$radio_station_name\"" '"isPlaying":true' '"identity":"Radio"'; do
    if ! grep -qF "$want" "$radio_media_status_path"; then
      fail "media status is missing $want"
    fi
  done
  cat "$radio_players_path"; echo
  if ! grep -q '"id":"radio"' "$radio_players_path"; then
    fail "media players does not list the radio"
  fi
  if grep -q '"kind":"stream"' "$radio_players_path"; then
    fail "the radio's own stream is listed again as an app with no MPRIS"
  fi
  cat "$radio_outputs_path"; echo
  if ! grep -q "\"id\":\"$radio_sink\"" "$radio_outputs_path"; then
    fail "media outputs does not list the second sink"
  fi
  if ! grep -q "^ok$" "$radio_output_path"; then
    fail "media output did not answer ok, got: $(cat "$radio_output_path")"
  fi
  if ! grep -q "\"output\":\"$radio_sink\"" "$radio_routed_path" \
    || ! grep -q "\"output\":\"$radio_sink\"" "$radio_routed_media_path"; then
    fail "the radio did not move to $radio_sink: $(cat "$radio_routed_path") $(cat "$radio_routed_media_path")"
  fi
  cat "$radio_stopped_path"; echo
  if ! grep -q '"available":false' "$radio_stopped_path"; then
    fail "the media source outlived radio stop"
  fi
  for png in "$radio_panel_png_path" "$radio_menu_png_path"; do
    [ -s "$png" ] || fail "no frame at $png"
  done
  echo "SMOKE_RADIO_PANEL $radio_panel_png_path"
  echo "SMOKE_RADIO_MENU $radio_menu_png_path"
}
