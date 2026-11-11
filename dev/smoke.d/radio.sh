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
leg_radio_claims="radio.favourites"

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
radio_sink="formalshell-smoke-alt"
radio_leftover_pid_path="$shot_dir/radio-leftover.pid"
radio_leftover_after_path="$shot_dir/radio-leftover-after.txt"
radio_station_id="smoke-radio-1"
radio_station_name="FormalShell Smoke Radio"
# Two runs side by side never play each other's listener.
radio_port=$(slot_port 20000)

leg_radio_timing() {
  leg_timing 45
}

leg_radio_fixture() {
  radio_favourite "$radio_station_id" "$radio_station_name" "$radio_port"
  radio_station "$radio_port" "$radio_track_path"
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
"${radio_track_path%.mp3}-server.sh" &
pw_load_module module-null-sink sink_name=$radio_sink sink_properties=device.description=SmokeAlt > /dev/null
sleep 4
if kill -0 "\$(cat "$radio_leftover_pid_path")" 2>/dev/null; then echo alive; else echo gone; fi > "$radio_leftover_after_path"
$ipc call radio play "$radio_station_id" > "$radio_play_path" 2>&1
until_jq 10 "$radio_status_path" '.loaded == true' call radio status
$ipc call media status > "$radio_media_status_path" 2>&1
$ipc call panel open media > /dev/null 2>&1
# A playing station keeps the panel moving, so its rest is never reported.
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
  add_cleanup "kill \"\$(cat '$radio_leftover_pid_path')\" 2>/dev/null || true"
  wm_exec_once "bash $script"
}

leg_radio_assert() {
  if ! grep -q "^gone$" "$radio_leftover_after_path" 2>/dev/null; then
    fail "the mpv left on the radio's socket survived the shell starting"
  fi
  if ! grep -q "^ok$" "$radio_play_path" 2>/dev/null; then
    fail "radio play did not answer ok, got: $(cat "$radio_play_path" 2>/dev/null)"
  fi
  cat "$radio_status_path"; echo
  expect_jq "$radio_status_path" '.loaded == true' "the radio never loaded the loopback station" '{loaded, running, station, error, playerError}'
  cat "$radio_media_status_path"; echo
  expect_jq "$radio_media_status_path" '.kind == "radio" and .title == "'"$radio_station_name"'" and .isPlaying == true and .identity == "Radio"' \
    "media status does not carry the playing radio" '{kind, title, isPlaying, identity}'
  cat "$radio_players_path"; echo
  expect_jq "$radio_players_path" 'any(.[]; .id == "radio")' "media players does not list the radio"
  expect_jq "$radio_players_path" 'all(.[]; .kind != "stream")' "the radio's own stream is listed again as an app with no MPRIS"
  cat "$radio_outputs_path"; echo
  expect_jq "$radio_outputs_path" 'any(.[]; .id == "'"$radio_sink"'")' "media outputs does not list the second sink"
  if ! grep -q "^ok$" "$radio_output_path"; then
    fail "media output did not answer ok, got: $(cat "$radio_output_path")"
  fi
  expect_jq "$radio_routed_path" '.output == "'"$radio_sink"'"' "the radio did not move to $radio_sink" '.output'
  expect_jq "$radio_routed_media_path" '.output == "'"$radio_sink"'"' "the media source did not move to $radio_sink" '.output'
  cat "$radio_stopped_path"; echo
  expect_jq "$radio_stopped_path" '.available == false' "the media source outlived radio stop" '{available, id, kind}'
  for png in "$radio_panel_png_path" "$radio_menu_png_path"; do
    [ -s "$png" ] || fail "no frame at $png"
  done
  echo "SMOKE_RADIO_PANEL $radio_panel_png_path"
  echo "SMOKE_RADIO_MENU $radio_menu_png_path"
}
