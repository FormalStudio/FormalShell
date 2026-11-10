# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --power: the launcher's Shutdown and Reboot rows when logind wants a
# password (on this rig's ssh session even a plain power-off does; a second
# user's session makes it the multiple-sessions action). The rows call
# logind themselves, so the answer is the shell's to show: Shutdown typed
# and chosen on real keys, the first Return only arming it and the second
# raising the shell's own polkit prompt, a wrong password reaching PAM
# (polkitd's journal names the action and the shell as the caller), and
# Escape leaving a critical "Shutdown failed" toast carrying logind's error.
# Reboot, chosen through `menu activate`, is cancelled the same way.
#
# The real password is never typed: once polkit says yes there is no point
# left to stub. A runtime-masked reboot.target and a root block inhibitor
# were both tried, and logind on systemd 261 rebooted the VM through each
# (2026-10-09).
#
# The second session is a throwaway user's PAM login on tty5 through
# systemd-run, a class user session of another uid.
leg_power_flag="--power"
leg_power_order=281
leg_power_claims="launcher=shared toasts=shared"
leg_power_vm_only="it adds a user and a login session with sudo and asks the real logind to power off"
leg_power_needs="jq wtype"

power_user="fspower"
power_unit="fs-power-other-session"
power_p="$shot_dir/power"

leg_power_timing() {
  leg_timing 90
}

leg_power_drive() {
  local script="$shot_dir/power-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# Armed, then confirmed: the row's two Enters.
confirm_row() {
  call menu summon system > /dev/null 2>&1
  sleep 1.5
  local idx
  idx=\$(row_index "$power_p-menu.json" "\$1")
  call menu activate "\$idx" > "$power_p-arm-\$1.txt" 2>&1
  sleep 1.5
  "$hyprctl_bin" layers -j > "$power_p-layers-armed-\$1.json" 2>&1
  "$grim_bin" "$power_p-armed-\$1.png" > /dev/null 2>&1
  call menu activate "\$idx" > /dev/null 2>&1
  sleep 4
  "$hyprctl_bin" layers -j > "$power_p-layers-prompt-\$1.json" 2>&1
}
date +%s > "$power_p-t0.txt"
id "$power_user" > /dev/null 2>&1 || sudo -n useradd -m "$power_user"
sudo -n systemd-run --unit="$power_unit" -p PAMName=login -p User="$power_user" \\
  -p TTYPath=/dev/tty5 -p StandardInput=tty-force -p StandardOutput=journal sleep 300 > /dev/null 2>&1
# logind answers challenge only once the second session is up on seat0;
# the assert names whichever of the two never came.
for _ in \$(seq 40); do
  loginctl list-sessions --no-pager > "$power_p-sessions.txt" 2>&1
  grep -Eq " $power_user +seat0 .* user " "$power_p-sessions.txt" && break
  sleep 0.5
done
for _ in \$(seq 20); do
  busctl call org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager CanPowerOff > "$power_p-can.txt" 2>&1
  grep -q '"challenge"' "$power_p-can.txt" && break
  sleep 0.5
done
# Everything past here asks logind to power off for real, so it runs only
# where logind wants a password: with "yes" it would turn the VM off.
grep -q '"challenge"' "$power_p-can.txt" || exit 0

call menu summon "" > /dev/null 2>&1
sleep 1.5
"$wtype_bin" "shutdown"
sleep 1.5
call menu status > "$power_p-typed.json" 2>&1
"$wtype_bin" -k Return
sleep 1.5
call menu status > "$power_p-armed.json" 2>&1
"$hyprctl_bin" layers -j > "$power_p-layers-armed-system.shutdown.json" 2>&1
"$grim_bin" "$power_p-armed-system.shutdown.png" > /dev/null 2>&1
"$wtype_bin" -k Return
sleep 4
"$hyprctl_bin" layers -j > "$power_p-layers-prompt-system.shutdown.json" 2>&1
"$grim_bin" "$power_p-prompt.png" > /dev/null 2>&1
"$wtype_bin" "wrong-password"
"$wtype_bin" -k Return
sleep 10
"$grim_bin" "$power_p-wrong.png" > /dev/null 2>&1
"$wtype_bin" -k Escape
sleep 4
call debug dump > "$power_p-dump-shutdown.json" 2>&1
"$grim_bin" "$power_p-cancelled.png" > /dev/null 2>&1
call notifications dismissAll > /dev/null 2>&1

confirm_row system.reboot
"$wtype_bin" -k Escape
sleep 4
call debug dump > "$power_p-dump-reboot.json" 2>&1
sudo -n journalctl -u polkit --since "@\$(cat "$power_p-t0.txt")" --no-pager -o cat > "$power_p-polkit.txt" 2>&1
EOF
  add_cleanup "sudo -n systemctl stop $power_unit > /dev/null 2>&1 || true"
  add_cleanup "sudo -n systemctl stop user@\$(id -u $power_user 2>/dev/null).service > /dev/null 2>&1 || true"
  add_cleanup "sudo -n userdel -r $power_user > /dev/null 2>&1 || true"
  hypr_exec_once "bash $script"
}

power_polkit_layers() {
  "$jq_bin" '[.. | objects | select(.namespace? == "formalshell:polkit")] | length' "$1" 2>/dev/null
}

# A critical toast titled $2 with a non-empty body, in dump $1.
power_toast() {
  "$jq_bin" -e --arg s "$2" '.toasts | any(.summary == $s and .urgency == 2 and (.body | length > 0))' "$1" > /dev/null 2>&1
}

leg_power_assert() {
  local f row
  for f in sessions can; do
    [ -s "$power_p-$f.txt" ] || fail "power: $power_p-$f.txt missing or empty"
  done
  cat "$power_p-sessions.txt"
  grep -Eq " $power_user +seat0 .* user " "$power_p-sessions.txt" \
    || fail "power: no user session of $power_user on seat0 within 20s"
  grep -q '"challenge"' "$power_p-can.txt" || fail "power: CanPowerOff did not answer challenge within 10s: $(cat "$power_p-can.txt")"
  "$jq_bin" -e '.cursorId == "system.shutdown"' "$power_p-typed.json" > /dev/null \
    || fail "power: typing shutdown did not put the cursor on system.shutdown: $(cat "$power_p-typed.json")"
  "$jq_bin" -e '.isOpen == true and .cursorId == "system.shutdown"' "$power_p-armed.json" > /dev/null 2>&1 \
    || fail "power: the first Return closed the launcher instead of arming the row"
  grep -q '^ok$' "$power_p-arm-system.reboot.txt" || fail "power: arming Reboot answered $(cat "$power_p-arm-system.reboot.txt")"
  for row in system.shutdown system.reboot; do
    [ "$(power_polkit_layers "$power_p-layers-armed-$row.json")" = 0 ] \
      || fail "power: a polkit prompt was up after the first Enter on $row, before it was confirmed"
    [ "$(power_polkit_layers "$power_p-layers-prompt-$row.json")" -ge 1 ] \
      || fail "power: confirming $row raised no polkit prompt"
  done
  "$jq_bin" -r '.toasts[] | "\(.summary)|\(.body)|\(.urgency)"' "$power_p-dump-shutdown.json" "$power_p-dump-reboot.json"
  power_toast "$power_p-dump-shutdown.json" "Shutdown failed" \
    || fail "power: no critical Shutdown failed toast after the prompt was cancelled"
  power_toast "$power_p-dump-reboot.json" "Reboot failed" \
    || fail "power: no critical Reboot failed toast after the prompt was cancelled"
  cat "$power_p-polkit.txt"
  grep -q 'FAILED to authenticate to gain authorization for action org.freedesktop.login1.power-off-multiple-sessions for .*formalshell-rs' \
    "$power_p-polkit.txt" || fail "power: polkitd logged no failed power-off-multiple-sessions attempt from the shell"
  grep -q 'power: PowerOff: ' "$shell_log_path" || fail "power: the shell logged no PowerOff refusal"
  grep -q 'power: Reboot: ' "$shell_log_path" || fail "power: the shell logged no Reboot refusal"
  for f in armed-system.shutdown prompt wrong cancelled; do
    [ -f "$power_p-$f.png" ] || fail "power: no $f screenshot"
  done
  echo "SMOKE_POWER_ARMED $power_p-armed-system.shutdown.png"
  echo "SMOKE_POWER_PROMPT $power_p-prompt.png"
  echo "SMOKE_POWER_WRONG $power_p-wrong.png"
  echo "SMOKE_POWER_CANCELLED $power_p-cancelled.png"
}
