# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --profiles-pill: the power profile pill (wayland/profiles.rs, Pond's
# ProfileSelector) against the VM rig guest's own power-profiles-daemon,
# under `layout: sidebar`. `panel open settings` brings the pill up on the
# output's right edge, centred, its chips read off `debug dump`'s
# `profilesPill`; a real click (dev/vpointer.py) on Power Saver's chip
# makes the daemon's active profile power-saver, read back off the daemon
# itself and off the pill, and one on Balanced puts it back. Closing the
# tiles takes the pill down. Photographed with each profile active.
leg_profiles_pill_flag="--profiles-pill"
leg_profiles_pill_order=282
leg_profiles_pill_vm_only="it switches the guest's real power-profiles-daemon"
leg_profiles_pill_needs="jq python3"
leg_profiles_pill_claims="layout=sidebar"

profiles_pill_dir="$shot_dir/profiles-pill"

leg_profiles_pill_fixture() {
  settings_pin '.layout = "sidebar"'
}

leg_profiles_pill_timing() {
  leg_timing 70
}

leg_profiles_pill_drive() {
  local script="$shot_dir/profiles-pill-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
d="$profiles_pill_dir"
mkdir -p "\$d"
active() {
  busctl --system get-property org.freedesktop.UPower.PowerProfiles /org/freedesktop/UPower/PowerProfiles \\
    org.freedesktop.UPower.PowerProfiles ActiveProfile 2>&1 | sed 's/^s "\\(.*\\)"\$/\\1/'
}
pill() { $ipc call debug dump 2>/dev/null | "$jq_bin" -c '.profilesPill' > "\$d/\$1.json"; }
# A real press and release at the centre of chip \$1, from the top left.
chip() {
  local x y
  read -r x y <<< "\$("$jq_bin" -r --arg on "\$1" '.chips[] | select(.on == \$on) | .rect | "\\((.x + .w / 2) | floor) \\((.y + .h / 2) | floor)"' "\$d/\$2.json")"
  "$python3_bin" "$PWD/dev/vpointer.py" move -6000 -6000 move "\$x" "\$y" wait 300 down wait 60 up > /dev/null 2>&1
  "$hyprctl_bin" cursorpos > "\$d/cursor-\$1.txt" 2>&1
}
for _ in \$(seq 1 40); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.5
done
active > "\$d/before.txt"
$ipc call panel open settings > /dev/null 2>&1
until_settled
pill open
"$grim_bin" "\$d/profiles-pill-open.png" > /dev/null 2>&1
chip profile:0 open
sleep 1.5
active > "\$d/saver.txt"
pill saver
"$grim_bin" "\$d/profiles-pill-saver.png" > /dev/null 2>&1
chip profile:1 saver
sleep 1.5
active > "\$d/balanced.txt"
pill balanced
$ipc call panel close > /dev/null 2>&1
sleep 1.5
pill closed
EOS
  hypr_exec_once "bash $script"
}

leg_profiles_pill_assert() {
  local d="$profiles_pill_dir"
  [ -s "$d/open.json" ] || fail "profiles-pill: no dump with the tiles open"
  echo "SMOKE_PROFILES_PILL open $(cat "$d/open.json")"
  echo "profiles-pill log: $(grep -E 'profiles pill|power profile|menu (un)?mapped' "$shell_log_path" 2>/dev/null | tr '\n' ';')"
  "$jq_bin" -e '.rect.w > 0 and ([.chips[] | .on] | index("profile:0") != null and index("profile:1") != null)' "$d/open.json" > /dev/null 2>&1 \
    || fail "profiles-pill: the pill did not come up with Power Saver and Balanced to click: $(cat "$d/open.json")"
  [ "$(cat "$d/saver.txt" 2>/dev/null)" = power-saver ] \
    || fail "profiles-pill: Power Saver's click at $(cat "$d/cursor-profile:0.txt" 2>/dev/null) left the daemon on '$(cat "$d/saver.txt" 2>/dev/null)' (before: $(cat "$d/before.txt" 2>/dev/null))"
  "$jq_bin" -e '.active == "power-saver"' "$d/saver.json" > /dev/null 2>&1 || fail "profiles-pill: the pill does not read power-saver: $(cat "$d/saver.json")"
  [ "$(cat "$d/balanced.txt" 2>/dev/null)" = balanced ] \
    || fail "profiles-pill: Balanced's click left the daemon on '$(cat "$d/balanced.txt" 2>/dev/null)'"
  [ "$(cat "$d/closed.json" 2>/dev/null)" = null ] || fail "profiles-pill: the pill outlived the tiles: $(cat "$d/closed.json")"
  local png
  for png in open saver; do
    [ -s "$d/profiles-pill-$png.png" ] || fail "profiles-pill: no profiles-pill-$png.png"
    echo "SMOKE_PROFILES_PILL_SHOT $d/profiles-pill-$png.png"
  done
}
