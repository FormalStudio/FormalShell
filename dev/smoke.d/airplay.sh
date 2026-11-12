# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --airplay: AirplayService against a PATH-shimmed `uxplay`. The real
# receiver decodes AirPlay's video into a GL texture, and this rig's card is
# the kernel's software `vkms` KMS device with no GL path at all, so the
# shim stands in for it the way --clipssh's does for a real ssh host: it
# speaks uxplay's own output contract (the exact line and file shapes are
# checked against FDH2/UxPlay's source) rather
# than one invented for the test.
#
# uxplay only prints its connect line and writes `-md`/`-ca`/`-dacp` once a
# real client shows up over the network, which this rig has no way to drive,
# so the shim does that at its own startup -- standing in for "a client is
# already connected" from the first frame -- and only the DISCONNECT half
# runs on the drive script's own clock: a trigger file it touches, which the
# shim polls for before it clears the three files and prints the drop line.
# That is what proves `active` (AirplayService's presence read, the `-dacp`
# file existing), the metadata and cover art, and MediaService's `airplay`
# media source (`_airplayRows`, gated on `active` and a non-empty title) all
# go back to idle rather than latching on.
leg_airplay_flag="--airplay"
leg_airplay_order=173
leg_airplay_compositors="hyprland shoji"
leg_airplay_claims="airplay media.player=shared panel=shared"
leg_airplay_needs="convert"

airplay_shim_dir="$shot_dir/airplay-shim"
airplay_cover_fixture_path="$shot_dir/airplay-cover-fixture.jpg"
airplay_disconnect_trigger_path="$shot_dir/airplay-disconnect-trigger"

airplay_client_name="Fixture iPhone"
airplay_title="Waves"
airplay_artist="Fixture Band"
airplay_album="Fixture Album"

airplay_status_connected_path="$shot_dir/airplay-status-connected.json"
airplay_media_connected_path="$shot_dir/airplay-media-connected.json"
airplay_status_disconnected_path="$shot_dir/airplay-status-disconnected.json"
airplay_media_disconnected_path="$shot_dir/airplay-media-disconnected.json"
airplay_active_png="$shot_dir/airplay-active.png"
airplay_panel_png="$shot_dir/airplay-panel.png"
airplay_gone_png="$shot_dir/airplay-gone.png"

leg_airplay_fixture() {
  settings_pin '.airplay.enable = true | .airplay.name = "FormalShell Fixture"'

  mkdir -p "$airplay_shim_dir"
  # Well past UxPlay's 95-byte reset placeholder (isPlaceholderCover), so
  # AirplayService.hasCover reads true rather than "still the placeholder".
  $convert_bin -size 64x64 xc:'#F59E0B' "$airplay_cover_fixture_path"

  cat > "$airplay_shim_dir/uxplay" <<EOF
#!/usr/bin/env bash
md="" ca="" dacp=""
while [ \$# -gt 0 ]; do
  case "\$1" in
    -md) md="\$2"; shift 2 ;;
    -ca) ca="\$2"; shift 2 ;;
    -dacp) dacp="\$2"; shift 2 ;;
    *) shift ;;
  esac
done

# report_client_request()'s own format string, uxplay.cpp.
echo "connection request from $airplay_client_name (iPhone) with deviceID = AA:BB:CC:DD:EE:01"
printf 'Title: %s\nArtist: %s\nAlbum: %s\nGenre: %s\n' "$airplay_title" "$airplay_artist" "$airplay_album" "Electronic" > "\$md"
cp "$airplay_cover_fixture_path" "\$ca"
: > "\$dacp"

while [ ! -f "$airplay_disconnect_trigger_path" ]; do sleep 0.3; done
# conn_destroy()'s LOGI text, matched by AirplayModel.parseLine on substring.
echo "lost connection with client"
rm -f "\$md" "\$ca" "\$dacp" "$airplay_disconnect_trigger_path"

sleep infinity
EOF
  chmod +x "$airplay_shim_dir/uxplay"
  # Not in session_env (dev/smoke.sh's header on clipssh's own PATH trick):
  # Hyprland and everything it spawns inherit whatever PATH this script
  # already has by the time it launches the session.
  export PATH="$airplay_shim_dir:$PATH"
}

leg_airplay_timing() {
  leg_timing 50
}

leg_airplay_drive() {
  local script="$shot_dir/airplay-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call airplay status > "$airplay_status_connected_path" 2>&1
$ipc call media status > "$airplay_media_connected_path" 2>&1
$ipc call panel open media > /dev/null 2>&1
until_settled
"$grim_bin" "$airplay_active_png" > /dev/null 2>&1
"$grim_bin" "$airplay_panel_png" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1

sleep 2
: > "$airplay_disconnect_trigger_path"
sleep 3
$ipc call airplay status > "$airplay_status_disconnected_path" 2>&1
$ipc call media status > "$airplay_media_disconnected_path" 2>&1
"$grim_bin" "$airplay_gone_png" > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_airplay_assert() {
  if [ ! -s "$airplay_status_connected_path" ]; then fail "no airplay status reply while connected"; fi
  cat "$airplay_status_connected_path"; echo
  expect_jq "$airplay_status_connected_path" '.active == true and .client == "'"$airplay_client_name"'" and .title == "'"$airplay_title"'" and .hasCover == true' \
    "airplay status did not report a connected client with metadata and cover art"

  if [ ! -s "$airplay_media_connected_path" ]; then fail "no media status reply while airplay was active"; fi
  cat "$airplay_media_connected_path"; echo
  expect_jq "$airplay_media_connected_path" '.kind == "airplay"' "MediaService did not pick up the airplay source"

  if [ ! -s "$airplay_status_disconnected_path" ]; then fail "no airplay status reply after disconnect"; fi
  cat "$airplay_status_disconnected_path"; echo
  expect_jq "$airplay_status_disconnected_path" '.active == false and .client == "" and .hasCover == false' \
    "airplay status still reports the client after the shim's disconnect"

  if [ ! -s "$airplay_media_disconnected_path" ]; then fail "no media status reply after disconnect"; fi
  cat "$airplay_media_disconnected_path"; echo
  expect_jq "$airplay_media_disconnected_path" '.kind != "airplay"' "MediaService still carries the airplay source after disconnect"

  for f in "$airplay_active_png" "$airplay_panel_png" "$airplay_gone_png"; do
    if [ ! -f "$f" ]; then fail "no airplay screenshot produced at $f"; fi
  done
  echo "SMOKE_AIRPLAY_ACTIVE $airplay_active_png"
  echo "SMOKE_AIRPLAY_PANEL $airplay_panel_png"
  echo "SMOKE_AIRPLAY_GONE $airplay_gone_png"
}
