# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --weather-locate: the weather service placing a machine whose network
# geolocates somewhere its clock says it is not. The owner's Lanzarote line
# leaves through Madrid, so beaconDB (and every IP database) answers Madrid
# with its `ipf` fallback while the zone is Atlantic/Canary, an hour behind.
# A PATH-shimmed `curl` gives that beaconDB answer, names a Madrid lookup
# Madrid and a country lookup Spain, logs every call and hands anything else
# (open-meteo) to the real curl; the shell runs under TZ=Atlantic/Canary.
# The nested run's system bus is empty, so GeoClue is absent and the
# shell's own beaconDB lookup is the fix (the base fixture's pinned
# location is dropped). Read off the call log: the
# forecast asked for zone.tab's Canary Islands point (28.1, -15.4), never
# Madrid's; and off the weather panel's title by tesseract: "Canary
# Islands", not Madrid.
leg_weather_locate_flag="--weather-locate"
leg_weather_locate_needs="jq grim convert tesseract"
leg_weather_locate_order=217

weather_locate_shim_dir="$shot_dir/weather-locate-shim"
weather_locate_calls="$shot_dir/weather-locate-curl.log"
weather_locate_png="$shot_dir/weather-locate.png"
weather_locate_dump="$shot_dir/weather-locate-dump.json"

leg_weather_locate_fixture() {
  local real_curl
  real_curl=$(command -v curl) || fail "no curl to forward to"
  mkdir -p "$weather_locate_shim_dir"
  : > "$weather_locate_calls"
  cat > "$weather_locate_shim_dir/curl" <<EOF
#!/usr/bin/env bash
echo "\$*" >> "$weather_locate_calls"
case " \$* " in
  *api.beacondb.net*) echo '{"accuracy":25000,"fallback":"ipf","location":{"lat":40.4168,"lng":-3.70379}}' ;;
  *nominatim.openstreetmap.org*zoom=10*) echo '{"name":"Madrid","address":{"city":"Madrid","state":"Community of Madrid"}}' ;;
  *nominatim.openstreetmap.org*) echo '{"name":"Spain","address":{"country":"Spain"}}' ;;
  *) exec "$real_curl" "\$@" ;;
esac
EOF
  chmod +x "$weather_locate_shim_dir/curl"
  # Hyprland and everything it spawns inherit this PATH (airplay.sh has why).
  export PATH="$weather_locate_shim_dir:$PATH"
  shell_env="${shell_env:+$shell_env }TZ=Atlantic/Canary"
  # The base fixture pins Berlin; this leg is about a machine left to place itself.
  settings_pin 'del(.location)'
}

leg_weather_locate_timing() {
  leg_timing 60 90
}

leg_weather_locate_drive() {
  local script="$shot_dir/weather-locate-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# The access point scan is capped at 15s and open-meteo is real, so wait on
# the forecast's own call rather than a fixed sleep.
for _ in \$(seq 1 100); do
  grep -q 'api.open-meteo.com' "$weather_locate_calls" && break
  sleep 0.5
done
sleep 2
$ipc call panel open weather > /dev/null 2>&1
sleep 2
$ipc call debug dump > "$weather_locate_dump" 2>&1
"$grim_bin" "$weather_locate_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_weather_locate_assert() {
  local box ocr
  cat "$weather_locate_calls"
  grep -q 'api.beacondb.net' "$weather_locate_calls" || fail "the shell never asked beaconDB"
  grep -qE 'api\.open-meteo\.com/v1/forecast\?latitude=28\.1[0-9]*&longitude=-15\.4' "$weather_locate_calls" \
    || fail "the forecast was not asked for the Canary Islands' zone point"
  if grep -qE 'latitude=40\.4|lat=40\.4' "$weather_locate_calls"; then
    fail "the shell looked up Madrid, the IP fix the zone contradicts"
  fi
  [ -f "$weather_locate_png" ] || fail "no weather panel frame"
  box=$(scaled_jq -r "$geometry_jq"'.panel.rest | geometry' "$weather_locate_dump" 2>/dev/null)
  [ -n "$box" ] || fail "no weather panel rect in the dump: $(head -c 300 "$weather_locate_dump")"
  "$convert_bin" "$weather_locate_png" -crop "$box" -resize 200% "$shot_dir/weather-locate-card.png"
  ocr=$("$tesseract_bin" "$shot_dir/weather-locate-card.png" - 2>/dev/null)
  grep -qF 'Canary Islands' <<< "$ocr" || fail "the weather panel is not titled Canary Islands: $(tr '\n' ' ' <<< "$ocr")"
  if grep -qF 'Madrid' <<< "$ocr"; then
    fail "the weather panel names Madrid: $(tr '\n' ' ' <<< "$ocr")"
  fi
  echo "SMOKE_WEATHER_LOCATE $weather_locate_png"
}
