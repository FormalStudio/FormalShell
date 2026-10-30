# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --logind-lock: logind's Lock signal raising the shell's lock, on the VM
# rig's real logind. The shell is started with XDG_SESSION_ID emptied, the
# way uwsm starts it in the user manager outside every session, so it finds
# its session through logind's `auto` as it does on e1504g.
#
# `loginctl lock-session <id>` locks (`lock status`, LockedHint yes);
# `loginctl unlock-session <id>` leaves it locked, since logind lets any
# process of the user send that unauthenticated; the real password typed
# on real keys unlocks, LockedHint no. Then the launcher's Lock row, found
# by typing and chosen through `menu activate`, comes back through the same signal: a second
# "logind asked for the lock" line in the shell log, locked, LockedHint yes,
# and the password again.
leg_logind_lock_flag="--logind-lock"
leg_logind_lock_order=113
leg_logind_lock_vm_only="it needs the real logind session the shell runs in"
leg_logind_lock_needs="jq wtype"

logind_lock_p="$shot_dir/logind-lock"

leg_logind_lock_fixture() {
  shell_env="${shell_env:+$shell_env }XDG_SESSION_ID="
}

leg_logind_lock_timing() {
  leg_timing 50 80
}

leg_logind_lock_drive() {
  local script="$shot_dir/logind-lock-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
p="$logind_lock_p"
hint() { loginctl show-session "\$1" -p LockedHint --value 2>&1; }
sleep 5
$ipc call lock status > "\$p-status-0.json" 2>&1
sid=\$("$jq_bin" -r '.logindSession // empty' "\$p-status-0.json")
echo "\$sid" > "\$p-sid.txt"
hint "\$sid" > "\$p-hint-0.txt"
loginctl lock-session "\$sid" > "\$p-lock-rc.txt" 2>&1; echo "rc=\$?" >> "\$p-lock-rc.txt"
sleep 3
$ipc call lock status > "\$p-status-1.json" 2>&1
hint "\$sid" > "\$p-hint-1.txt"
"$grim_bin" "\$p-locked.png" > /dev/null 2>&1
loginctl unlock-session "\$sid" > /dev/null 2>&1
sleep 2
$ipc call lock isLocked > "\$p-after-unlock-session.txt" 2>&1
"$wtype_bin" -s 300 "formalshell-test" -k Return
sleep 4
$ipc call lock status > "\$p-status-2.json" 2>&1
hint "\$sid" > "\$p-hint-2.txt"
"$grim_bin" "\$p-unlocked.png" > /dev/null 2>&1
$ipc call menu summon "" > /dev/null 2>&1
sleep 1
"$wtype_bin" -s 300 "lock"
sleep 1
$ipc call menu status > "\$p-menu.json" 2>&1
idx=\$("$jq_bin" -r '.ids | index("system.lock") // empty' "\$p-menu.json")
[ -n "\$idx" ] && $ipc call menu activate "\$idx" > "\$p-activate.txt" 2>&1
sleep 3
$ipc call lock status > "\$p-status-3.json" 2>&1
hint "\$sid" > "\$p-hint-3.txt"
"$grim_bin" "\$p-row-locked.png" > /dev/null 2>&1
"$wtype_bin" -s 300 "formalshell-test" -k Return
sleep 4
$ipc call lock status > "\$p-status-4.json" 2>&1
hint "\$sid" > "\$p-hint-4.txt"
EOF
  hypr_exec_once "bash $script"
}

leg_logind_lock_assert() {
  local p=$logind_lock_p sid n
  sid=$(cat "$p-sid.txt" 2>/dev/null)
  [ -n "$sid" ] || fail "lock status named no logind session: $(cat "$p-status-0.json" 2>/dev/null)"
  echo "logind-lock: session $sid ($(loginctl show-session "$sid" -p Type -p Class --value 2>/dev/null | tr '\n' ' '))"
  grep -qx no "$p-hint-0.txt" || fail "LockedHint before the lock was not no: $(cat "$p-hint-0.txt")"
  grep -q '^rc=0$' "$p-lock-rc.txt" || fail "loginctl lock-session $sid failed: $(cat "$p-lock-rc.txt")"
  grep -q '"locked":true' "$p-status-1.json" || fail "loginctl lock-session did not lock: $(cat "$p-status-1.json")"
  grep -q '"secure":true' "$p-status-1.json" || fail "the logind lock never went secure: $(cat "$p-status-1.json")"
  grep -qx yes "$p-hint-1.txt" || fail "LockedHint not yes while locked: $(cat "$p-hint-1.txt")"
  grep -qx true "$p-after-unlock-session.txt" || fail "loginctl unlock-session lifted the lock: $(cat "$p-after-unlock-session.txt")"
  grep -q '"locked":false' "$p-status-2.json" || fail "the real password did not unlock: $(cat "$p-status-2.json")"
  grep -qx no "$p-hint-2.txt" || fail "LockedHint not back to no after the unlock: $(cat "$p-hint-2.txt")"
  "$jq_bin" -e '.ids | index("system.lock")' "$p-menu.json" > /dev/null 2>&1 \
    || fail "typing lock in the launcher offered no system.lock row: $(cat "$p-menu.json")"
  grep -q '"locked":true' "$p-status-3.json" \
    || fail "the launcher's Lock row did not lock (activate: $(cat "$p-activate.txt" 2>/dev/null)): $(cat "$p-status-3.json"); shell log: $(grep -E 'lock|session|menu' "$shell_log_path" | tail -15)"
  grep -qx yes "$p-hint-3.txt" || fail "LockedHint not yes after the Lock row: $(cat "$p-hint-3.txt")"
  n=$(grep -c "lock: logind asked for the lock" "$shell_log_path" 2>/dev/null || true)
  [ "${n:-0}" -eq 2 ] || fail "expected two logind Lock signals in the shell log (lock-session, the Lock row), got ${n:-0}"
  grep -q '"locked":false' "$p-status-4.json" || fail "the password did not unlock the Lock row's lock: $(cat "$p-status-4.json")"
  grep -qx no "$p-hint-4.txt" || fail "LockedHint not no after the second unlock: $(cat "$p-hint-4.txt")"
  echo "SMOKE_LOGIND_LOCK_LOCKED $p-locked.png"
  echo "SMOKE_LOGIND_LOCK_UNLOCKED $p-unlocked.png"
  echo "SMOKE_LOGIND_LOCK_ROW $p-row-locked.png"
}
