# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --quick-settings-power: the quick settings card's Power button menu
# against the guest's real logind. `quicksettings menu power` opens it, and
# its rows are the ones logind's CanSuspend, CanReboot and CanPowerOff
# allow (busctl's own answers, "yes" or "challenge"), in GNOME's order,
# with Log out last. Real Downs walk to Power off, one Return arms it
# ("Confirm power off?") with the card still up, and two Escapes close the
# menu disarmed and then the card. The second Return is never pressed, and
# Return is never pressed on Suspend, which runs at once.
leg_quick_settings_power_flag="--quick-settings-power"
leg_quick_settings_power_order=282
leg_quick_settings_power_claims="panel=shared"
leg_quick_settings_power_vm_only="it reads the real logind's power answers, which the nested rig's private system bus does not carry"
leg_quick_settings_power_needs="jq wtype grim"

quick_settings_power_p="$shot_dir/quick-settings-power"

leg_quick_settings_power_timing() {
  leg_timing 70
}

leg_quick_settings_power_drive() {
  local script="$shot_dir/quick-settings-power-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
p="$quick_settings_power_p"
for m in CanSuspend CanReboot CanPowerOff; do
  busctl call org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager "\$m" > "\$p-\$m.txt" 2>&1
done
$ipc call panel open quicksettings > /dev/null 2>&1
sleep 1.5
$ipc call quicksettings menu power > "\$p-menu.txt" 2>&1
for i in \$(seq 1 20); do
  $ipc call quicksettings status 2>/dev/null | "$jq_bin" -e '.menu.content == 1' > /dev/null 2>&1 && break
  sleep 0.25
done
status quicksettings "\$p-open.json"
"$grim_bin" "\$p-open.png" > /dev/null 2>&1
n=0
while [ \$n -lt 6 ]; do
  [ "\$($ipc call quicksettings status | "$jq_bin" -r '.cursor // ""')" = "m:power:poweroff" ] && break
  "$wtype_bin" -k Down
  sleep 0.4
  n=\$((n + 1))
done
status quicksettings "\$p-at-poweroff.json"
if [ "\$("$jq_bin" -r '.cursor // ""' "\$p-at-poweroff.json")" = "m:power:poweroff" ]; then
  "$wtype_bin" -k Return
  sleep 1
fi
status quicksettings "\$p-armed.json"
"$grim_bin" "\$p-armed.png" > /dev/null 2>&1
$ipc call panel state > "\$p-panel-armed.txt" 2>&1
"$wtype_bin" -k Escape
sleep 1.2
status quicksettings "\$p-closed.json"
"$wtype_bin" -k Escape
sleep 1
$ipc call panel state > "\$p-panel-closed.txt" 2>&1
EOF
  hypr_exec_once "bash $script"
}

quick_settings_power_jq() {
  local what="$1" file="$quick_settings_power_p-$2" expr="$3"
  [ -s "$file" ] || fail "quick-settings-power: no $2 for $what"
  "$jq_bin" -e "$expr" "$file" > /dev/null 2>&1 \
    || fail "quick-settings-power: $what: $("$jq_bin" -c '{open, cursor, dim, menu}' "$file" 2>/dev/null || head -c 1200 "$file")"
  echo "SMOKE_QUICK_SETTINGS_POWER $what"
}

leg_quick_settings_power_assert() {
  local p="$quick_settings_power_p" want=() m id
  for m in CanSuspend:suspend CanReboot:reboot CanPowerOff:poweroff; do
    id="${m#*:}"
    grep -Eq '"(yes|challenge)"' "$p-${m%%:*}.txt" && want+=("\"m:power:$id\"")
    echo "quick-settings-power: logind ${m%%:*}: $(cat "$p-${m%%:*}.txt")"
  done
  want+=('"m:power:logout"')
  printf -v m '%s,' "${want[@]}"
  [ "$(cat "$p-menu.txt")" = "ok" ] || fail "quick-settings-power: quicksettings menu power answered '$(cat "$p-menu.txt")'"
  quick_settings_power_jq "the rows are what logind allows, Log out last" open.json "
    .menu.id == \"power\" and .dim == true and [.menu.rows[].id] == [${m%,}]
    and ([.menu.rows[].armed] | all(. == false))"
  grep -q 'm:power:poweroff' <<< "$m" || fail "quick-settings-power: the guest's logind allows no power off ($(cat "$p-CanPowerOff.txt"))"
  quick_settings_power_jq "Down reached Power off" at-poweroff.json '.cursor == "m:power:poweroff"'
  quick_settings_power_jq "one Return armed it" armed.json '
    [.menu.rows[] | select(.id == "m:power:poweroff")][0] | .armed == true and .title == "Confirm power off?"'
  [ "$(cat "$p-panel-armed.txt")" = "quicksettings" ] || fail "quick-settings-power: the card went down armed: $(cat "$p-panel-armed.txt")"
  quick_settings_power_jq "Escape closed the menu disarmed" closed.json '.open == true and .menu == null and .dim == false and .cursor == "power"'
  [ -z "$(cat "$p-panel-closed.txt")" ] || fail "quick-settings-power: a second Escape left $(cat "$p-panel-closed.txt") open"
  echo "SMOKE_QUICK_SETTINGS_POWER_OPEN $p-open.png"
  echo "SMOKE_QUICK_SETTINGS_POWER_ARMED $p-armed.png"
}
