# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --bluez-player: a Bluetooth device's AVRCP player as a media source. The
# shell's system bus is a private dbus-daemon of the leg's own, where
# dev/bluez-player-stub.py plays g815's BlueZ with the owner's iPad
# streaming to it: a connected "FormalPad" whose object path carries a
# random address and whose Address is the identity one, its
# MediaPlayer1 playing a track with a position and a duration, and
# thirty-two more remotes with a paused player each. pw-cat stands in for the
# A2DP stream BlueZ's PipeWire plugin makes for it, named
# `bluez_input.<identity address>.2`, into a null sink of the leg's own.
#
#  1. Auto lands on the playing remote: `media status` kind `bluetooth`,
#     the stub's title and artist, playing, a position on the stub's
#     clock and its duration, and the volume and output of the pw-cat
#     stream; `media players` lists the remote under the device's name and
#     no `stream:` row for its audio.
#  2. `media playPause`, `next` and `playPause` reach the stub as exactly
#     Pause, Next, Play on that player, and each lands back in `media
#     status` off BlueZ's own PropertiesChanged.
#  3. The source menu, opened and walked on real keys under the stale
#     audit: thirty-four rows float over the card on a list of their own
#     height, the card's rest rect the same with it open, the list inside
#     the output and scrolling itself so the cursor's row stays inside it
#     down to the last one (`debug dump`'s panel `floats` and `cursor`),
#     then a wheel over the list takes it back to the top.
leg_bluez_player_flag="--bluez-player"
leg_bluez_player_order=179
leg_bluez_player_compositors="hyprland shoji"
leg_bluez_player_needs="python3 jq grim wtype wlrctl pactl pw_cat"
leg_bluez_player_claims="bus.system"

bluez_player_dir="$shot_dir/bluez-player"
bluez_player_bus="$bluez_player_dir/bus"
bluez_player_log="$bluez_player_dir/calls.jsonl"
bluez_player_identity="C8:89:F3:45:A7:AA"
bluez_player_random="4A:DC:C3:A2:75:EC"
bluez_player_path="/org/bluez/hci0/dev_4A_DC_C3_A2_75_EC/avrcp/player0"
bluez_player_sink="fs_smoke_bluez_$$"
bluez_player_title="OLED iPad Mini Smoke Track"
bluez_player_next="Second Smoke Track"
bluez_player_extra=32

bluez_player_status() { printf '%s/status-%s.json' "$bluez_player_dir" "$1"; }
bluez_player_dump() { printf '%s/dump-%s.json' "$bluez_player_dir" "$1"; }
bluez_player_png() { printf '%s/bluez-player-%s.png' "$shot_dir" "$1"; }

leg_bluez_player_timing() {
  leg_timing 140
}

leg_bluez_player_fixture() {
  mkdir -p "$bluez_player_dir"
  : > "$bluez_player_log"
  cat > "$bluez_player_dir/tracks.json" <<JSON
[
 {"title": "$bluez_player_title", "artist": "Marques Brownlee", "duration_ms": 779141},
 {"title": "$bluez_player_next", "artist": "Smoke Artist", "album": "Smoke Album", "duration_ms": 200000}
]
JSON
  local daemon pid module
  daemon=$(command -v dbus-daemon) || fail "--bluez-player: no dbus-daemon"
  pid=$("$daemon" --session --address="unix:path=$bluez_player_bus" --fork --print-pid --nopidfile)
  add_cleanup "kill $pid 2>/dev/null"
  "$python3_bin" dev/bluez-player-stub.py --address "unix:path=$bluez_player_bus" \
    --identity "$bluez_player_identity" --random "$bluez_player_random" --alias FormalPad \
    --tracks "$bluez_player_dir/tracks.json" --position-ms 359686 --extra "$bluez_player_extra" \
    --log "$bluez_player_log" --ready "$bluez_player_dir/ready" > "$bluez_player_dir/stub.log" 2>&1 &
  add_cleanup "kill $! 2>/dev/null"
  for _ in $(seq 50); do
    [ -s "$bluez_player_dir/ready" ] && break
    sleep 0.1
  done
  [ -s "$bluez_player_dir/ready" ] || fail "--bluez-player: the stub never owned org.bluez ($(cat "$bluez_player_dir/stub.log"))"
  pw_load_module module-null-sink "sink_name=$bluez_player_sink" sink_properties=device.description=SmokeBluez > /dev/null || fail "--bluez-player: the null sink did not load"
  # Silence, raw, under the node name BlueZ's PipeWire plugin gives an
  # A2DP source's stream.
  "$pw_cat_bin" --playback --raw --target "$bluez_player_sink" --rate 48000 --channels 2 --format s16 \
    -P "{ node.name = \"bluez_input.${bluez_player_identity//:/_}.2\" node.description = \"FormalPad\" }" \
    - < /dev/zero > "$bluez_player_dir/pw-cat.log" 2>&1 &
  add_cleanup "kill $! 2>/dev/null"
  shell_env="${shell_env:+$shell_env }DBUS_SYSTEM_BUS_ADDRESS=unix:path=$bluez_player_bus"
}

leg_bluez_player_drive() {
  local script="$shot_dir/bluez-player-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
dir="$bluez_player_dir"
# cursor <name> <key> <tries> <key to press...>: presses until debug
# dump's panel cursor sits on <key>, the dump kept as dump-<name>.json.
cursor() {
  local name="\$1" want="\$2" tries="\$3" out="\$dir/dump-\$1.json"
  shift 3
  for _ in \$(seq "\$tries"); do
    $ipc call debug dump > "\$out" 2>&1
    [ "\$("$jq_bin" -r '.panel.cursor.key // ""' "\$out")" = "\$want" ] && return 0
    "$wtype_bin" "\$@"
  done
  $ipc call debug dump > "\$out" 2>&1
}
shot() {
  sleep 0.6
  shoot "$shot_dir/bluez-player-\$1.png"
}

until_jq 15 "\$dir/status-playing.json" '.kind == "bluetooth" and .isPlaying' call media status
$ipc call panel open media > /dev/null 2>&1
# The panel open is what lists the audio graph's streams.
until_jq 10 "\$dir/status-routed.json" '.output == "$bluez_player_sink"' call media status
$ipc call media players > "\$dir/players.json" 2>&1
shot playing

$ipc call media playPause > "\$dir/reply-pause.txt" 2>&1
until_jq 5 "\$dir/status-paused.json" '.isPlaying == false' call media status
$ipc call media next > "\$dir/reply-next.txt" 2>&1
until_jq 5 "\$dir/status-next.json" '.title == "$bluez_player_next"' call media status
$ipc call media playPause > "\$dir/reply-play.txt" 2>&1
until_jq 5 "\$dir/status-resumed.json" '.isPlaying and .title == "$bluez_player_next"' call media status

last="pick:\$("$jq_bin" -r '.[-1].id' "\$dir/players.json")"
$(stale_audit_start)
cursor source source 4 -M shift -k Tab -m shift
"$wtype_bin" -k Return
sleep 1
$ipc call debug dump > "\$dir/dump-open.json" 2>&1
shot menu-open
cursor menu-top "pick:" 3 -k Down
cursor menu-end "\$last" $((bluez_player_extra + 8)) -k Down
sleep 1
$ipc call debug dump > "\$dir/dump-menu-end.json" 2>&1
shot menu-end
$(stale_audit_stop bluez-player)
read -r px py <<< "\$("$jq_bin" -r '.panel.floats[0].rect | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"' "\$dir/dump-menu-end.json")"
"$wlrctl_bin" pointer move -4000 -4000 > "\$dir/pointer.txt" 2>&1
sleep 0.5
"$wlrctl_bin" pointer move "\$px" "\$py" >> "\$dir/pointer.txt" 2>&1
sleep 0.5
# One notch a call, a control height each: enough to reach the top.
for _ in \$(seq 40); do
  "$wlrctl_bin" pointer scroll -15 0 >> "\$dir/pointer.txt" 2>&1
  sleep 0.1
done
sleep 1.5
$ipc call debug dump > "\$dir/dump-wheel.json" 2>&1
shot wheel
"$wmctl_bin" monitors -j > "\$dir/monitors.json" 2>&1
$ipc call panel close > /dev/null 2>&1
echo done > "\$dir/drive-done"
EOF
  wm_exec_once "bash $script"
}

bluez_player_calls() {
  "$jq_bin" -r "select(.path == \"$bluez_player_path\") | .call" "$bluez_player_log" 2>/dev/null | tr '\n' ' '
}

leg_bluez_player_assert() {
  local name s players out_h
  for name in playing menu-open menu-end wheel; do
    [ -s "$(bluez_player_png "$name")" ] && echo "SMOKE_BLUEZ_PLAYER_$(tr 'a-z-' 'A-Z_' <<< "$name") $(bluez_player_png "$name")"
  done
  [ -f "$bluez_player_dir/drive-done" ] || fail "--bluez-player: the drive never finished (stub log: $(tail -3 "$bluez_player_dir/stub.log" 2>/dev/null))"
  echo "SMOKE_BLUEZ_PLAYER_CALLS $bluez_player_log"

  s=$(bluez_player_status playing)
  cat "$s"; echo
  "$jq_bin" -e --arg t "$bluez_player_title" --arg id "bluez:$bluez_player_identity" \
    '.id == $id and .kind == "bluetooth" and .identity == "FormalPad" and .title == $t and .artist == "Marques Brownlee" and .isPlaying and .position >= 359 and .length == 779.141 and .canSeek == false' "$s" > /dev/null \
    || fail "--bluez-player: auto did not land on the playing remote: $("$jq_bin" -c '{id, kind, identity, title, artist, isPlaying, position, length}' "$s")"
  "$jq_bin" -e '.volumeSupported and .volume == 1' "$(bluez_player_status routed)" > /dev/null \
    || fail "--bluez-player: the remote did not take its stream's volume and output: $("$jq_bin" -c '{output, canRoute, volume, volumeSupported}' "$(bluez_player_status routed)")"
  players="$bluez_player_dir/players.json"
  echo "SMOKE_BLUEZ_PLAYER_PLAYERS $(cat "$players")"
  "$jq_bin" -e --arg id "bluez:$bluez_player_identity" 'map(select(.id == $id))[0] | .kind == "bluetooth" and .label == "FormalPad" and .isPlaying and .auto' "$players" > /dev/null \
    || fail "--bluez-player: the remote is not a source row of its own"
  "$jq_bin" -e 'map(select(.id | startswith("stream:"))) | length == 0' "$players" > /dev/null \
    || fail "--bluez-player: the remote's stream is still a row of its own: $("$jq_bin" -c 'map(select(.id | startswith("stream:")))' "$players")"
  [ "$("$jq_bin" 'map(select(.kind == "bluetooth")) | length' "$players")" = $((bluez_player_extra + 1)) ] \
    || fail "--bluez-player: not every remote is listed"

  for name in pause next play; do
    grep -qx ok "$bluez_player_dir/reply-$name.txt" || fail "--bluez-player: media $name answered: $(cat "$bluez_player_dir/reply-$name.txt")"
  done
  [ "$(bluez_player_calls)" = "Pause Next Play " ] || fail "--bluez-player: the stub took the transport as: $(bluez_player_calls)"
  "$jq_bin" -e '.isPlaying == false' "$(bluez_player_status paused)" > /dev/null || fail "--bluez-player: the pause never landed"
  "$jq_bin" -e --arg t "$bluez_player_next" '.title == $t and .album == "Smoke Album" and .length == 200' "$(bluez_player_status next)" > /dev/null \
    || fail "--bluez-player: the next track never landed: $("$jq_bin" -c '{title, album, length}' "$(bluez_player_status next)")"
  "$jq_bin" -e '.isPlaying' "$(bluez_player_status resumed)" > /dev/null || fail "--bluez-player: the play never landed"

  stale_audit_assert bluez-player
  local shut open top end wheel
  shut=$(bluez_player_dump source)
  open=$(bluez_player_dump open)
  top=$(bluez_player_dump menu-top)
  end=$(bluez_player_dump menu-end)
  wheel=$(bluez_player_dump wheel)
  echo "SMOKE_BLUEZ_PLAYER_MENU shut=$("$jq_bin" -c '.panel | {rest, floats}' "$shut")"
  echo "SMOKE_BLUEZ_PLAYER_MENU open=$("$jq_bin" -c '.panel | {rest, floats}' "$open")"
  echo "SMOKE_BLUEZ_PLAYER_MENU end=$("$jq_bin" -c '.panel | {rest, cursor, floats, scroll}' "$end")"
  echo "SMOKE_BLUEZ_PLAYER_MENU wheel=$("$jq_bin" -c '.panel | {rest, floats, scroll}' "$wheel")"
  for name in open menu-end wheel; do
    [ "$("$jq_bin" -c .panel.rest "$(bluez_player_dump "$name")")" = "$("$jq_bin" -c .panel.rest "$shut")" ] \
      || fail "--bluez-player: the card moved with the menu ($name): $("$jq_bin" -c .panel.rest "$(bluez_player_dump "$name")") against $("$jq_bin" -c .panel.rest "$shut")"
  done
  "$jq_bin" -e '.panel.floats == []' "$shut" > /dev/null || fail "--bluez-player: a list floated before the menu opened"
  "$jq_bin" -e '.panel.floats | length == 1' "$open" > /dev/null || fail "--bluez-player: the open menu drew no floating list"
  "$jq_bin" -e '.panel.cursor.key == "pick:"' "$top" > /dev/null || fail "--bluez-player: Down from the trigger did not reach Auto"
  "$jq_bin" -e --arg k "pick:$("$jq_bin" -r '.[-1].id' "$players")" '.panel.cursor.key == $k' "$end" > /dev/null \
    || fail "--bluez-player: the keys never reached the last source: $("$jq_bin" -c .panel.cursor "$end")"
  "$jq_bin" -e '.panel | .floats[0].scroll > 0 and .scroll == 0' "$end" > /dev/null \
    || fail "--bluez-player: the list did not scroll itself to the last row: $("$jq_bin" -c '.panel | {floats, scroll}' "$end")"
  "$jq_bin" -e '.panel as $p | $p.cursor.rect as $c | $p.floats[0].rect as $f | $c.y >= $f.y and $c.y + $c.height <= $f.y + $f.height' "$end" > /dev/null \
    || fail "--bluez-player: the last source's row is outside its list: $("$jq_bin" -c '.panel | {cursor, floats}' "$end")"
  out_h=$("$jq_bin" '.[0] | .height / .scale | floor' "$bluez_player_dir/monitors.json")
  "$jq_bin" -e --argjson h "$out_h" '.panel.floats[0].rect | .y >= 0 and .y + .height <= $h' "$end" > /dev/null \
    || fail "--bluez-player: the list ran past the output's $out_h: $("$jq_bin" -c .panel.floats "$end")"
  "$jq_bin" -e '.panel.floats[0].scroll == 0' "$wheel" > /dev/null \
    || fail "--bluez-player: the wheel over the list did not scroll it back: $("$jq_bin" -c .panel.floats "$wheel")"
}
