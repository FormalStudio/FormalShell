# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --light pins `theme.mode` light in the settings fixture (a `settings_pin`)
# and rides any other leg, so every surface that leg photographs is read in
# light mode, the way `--no-morph` reads it as a popover. Its own claim is
# the one the frames cannot make: `theme status` reports mode light once
# the shell has the key.
leg_light_flag="--light"
leg_light_order=8
leg_light_fixture_window=keep

light_status_path="$shot_dir/light-status.json"

leg_light_fixture() {
  settings_pin '.theme.mode = "light"'
}

leg_light_drive() {
  local script="$shot_dir/light-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
for _ in \$(seq 1 40); do
  $ipc call theme status > "$light_status_path" 2>&1
  grep -qF '"mode":"light"' "$light_status_path" && break
  sleep 0.5
done
EOS
  hypr_exec_once "bash $script"
}

leg_light_assert() {
  [ -s "$light_status_path" ] || fail "no theme status produced for the light check"
  grep -qF '"mode":"light"' "$light_status_path" \
    || fail "theme status does not report mode light: $(head -c 300 "$light_status_path")"
  echo "SMOKE_LIGHT mode=light"
}
