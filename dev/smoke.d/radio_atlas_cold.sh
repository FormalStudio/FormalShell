# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --radio-atlas-cold opens Radio Atlas with no world cache at all, the
# first open on a fresh install: the atlas has to ask Radio Browser for the
# world list over the real network and show it. A frame is taken a second
# after the open, while the fetch can still be running, and another once
# `radio status` reports stations or an error (20 s at most). The first
# has to carry the card's content, the second either a world list (with
# world.json then written to the cache) or the honest unavailable line in
# the atlas's own error, on a rig with no network. Both frames are read for
# a card that is not one flat fill: the header and the globe draw in every
# state, so a blank card is the content not reaching the screen.
leg_radio_atlas_cold_flag="--radio-atlas-cold"
leg_radio_atlas_cold_order=176
leg_radio_atlas_cold_needs="convert jq"
leg_radio_atlas_cold_claims="radio.cache=none"

radio_atlas_cold_early_png="$shot_dir/radio-atlas-cold-early.png"
radio_atlas_cold_png="$shot_dir/radio-atlas-cold.png"
radio_atlas_cold_status_path="$shot_dir/radio-atlas-cold.json"

leg_radio_atlas_cold_timing() {
  leg_timing 60
}

leg_radio_atlas_cold_drive() {
  local script="$shot_dir/radio-atlas-cold-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call panel open radio > /dev/null 2>&1
# The early frame is one second in, while the stations may still be on the way.
sleep 1
"$grim_bin" "$radio_atlas_cold_early_png" > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 20 ]; do
  $ipc call radio status > "$radio_atlas_cold_status_path" 2>&1
  "$jq_bin" -e '.atlas.stations > 0 or .atlas.error != ""' "$radio_atlas_cold_status_path" > /dev/null 2>&1 && break
  sleep 1
done
sleep 1
"$grim_bin" "$radio_atlas_cold_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# The card's spread of grey levels, which a flat fill keeps near zero.
radio_atlas_cold_spread() {
  local w h
  read -r w h < <($convert_bin "$1" -format '%w %h' info:)
  $convert_bin "$1" -gravity center -crop "$((w * 50 / 100))x$((h * 50 / 100))+0+0" +repage -colorspace gray \
    -format '%[fx:standard_deviation*100]' info: 2>/dev/null
}

leg_radio_atlas_cold_assert() {
  cat "$radio_atlas_cold_status_path"; echo
  local stations error spread png
  stations=$("$jq_bin" -r '.atlas.stations' "$radio_atlas_cold_status_path" 2>/dev/null)
  error=$("$jq_bin" -r '.atlas.error' "$radio_atlas_cold_status_path" 2>/dev/null)
  "$jq_bin" -e '.atlas.open == true and .atlas.mode == "world"' "$radio_atlas_cold_status_path" > /dev/null 2>&1 \
    || fail "the atlas is not open on the world list"
  if [ "${stations:-0}" -gt 0 ] 2>/dev/null; then
    echo "cold start fetched $stations stations"
    [ -s "$iso_home/.cache/formalshell/radio-atlas/world.json" ] || fail "the fetched world list was not written to the cache"
  elif [ -n "$error" ] && [ "$error" != "null" ]; then
    echo "cold start with no Radio Browser: \"$error\""
  else
    fail "20 s after a cold open the atlas has neither stations nor an error"
  fi
  for png in "$radio_atlas_cold_early_png" "$radio_atlas_cold_png"; do
    [ -s "$png" ] || fail "no frame at $png"
    spread=$(radio_atlas_cold_spread "$png")
    echo "$(basename "$png"): grey spread ${spread}%"
    awk -v d="${spread:-0}" 'BEGIN { exit !(d > 3.0) }' || fail "the atlas card in $png is one flat fill (spread ${spread}%)"
  done
  echo "SMOKE_RADIO_ATLAS_COLD_EARLY $radio_atlas_cold_early_png"
  echo "SMOKE_RADIO_ATLAS_COLD $radio_atlas_cold_png"
}
