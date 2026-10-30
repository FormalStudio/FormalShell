# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --lock-restore: a shell that dies holding the lock, then a new one taking
# it back, the way an update's service restart or a crash leaves a locked
# session. The daemon is SIGKILLed with the lock secure (no exit path of its
# own runs), a second one started through the wrapped launcher, and the lock
# read back off the new one: locked and secure over IPC, its log naming the
# restore, the avatar fixture on screen where Hyprland's dead-locker page
# would be, and the real password typed into it unlocking. Rides the rig's
# `misc.allow_session_lock_restore`, which the shipped config sets too.
leg_lock_restore_flag="--lock-restore"
leg_lock_restore_order=112
leg_lock_restore_needs="wtype convert"

lock_restore_dead_path="$shot_dir/lock-restore-dead.png"
lock_restore_back_path="$shot_dir/lock-restore-back.png"
lock_restore_status_path="$shot_dir/lock-restore-status.json"
lock_restore_after_path="$shot_dir/lock-restore-after.json"
lock_restore_log_path="$shot_dir/lock-restore-second.log"
lock_restore_pids_path="$shot_dir/lock-restore-pids.txt"

leg_lock_restore_validate() {
  # The shell it kills is every other leg's shell too.
  local v
  for v in $(compgen -v | grep -E '^leg_[a-z0-9_]+_on$'); do
    if [ "$v" != leg_lock_restore_on ] && [ "${!v}" = true ]; then
      echo "usage: --lock-restore kills the session's shell and cannot combine with --$(echo "${v#leg_}" | sed 's/_on$//; s/_/-/g')" >&2
      exit 1
    fi
  done
}

leg_lock_restore_fixture() {
  $convert_bin -size 256x256 xc:'#1FB86A' "png:$iso_home/.face"
}

leg_lock_restore_timing() {
  leg_timing 40 70
}

leg_lock_restore_drive() {
  local script="$shot_dir/lock-restore-drive.sh" find_daemons launch
  find_daemons='for pid in $(pgrep -f -- formalshell-rs); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in */formalshell-rs) echo "$pid" ;; esac; done'
  launch="$PWD/result/bin/formalshell"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 5
$ipc call lock lock > /dev/null 2>&1
for _ in \$(seq 1 20); do
  $ipc call lock status 2>/dev/null | grep -q '"secure":true' && break
  sleep 0.25
done
old=\$($find_daemons | head -n1)
kill -9 "\$old"
sleep 2
"$grim_bin" "$lock_restore_dead_path" > /dev/null 2>&1
export LIBGL_ALWAYS_SOFTWARE=1
"$launch" > "$lock_restore_log_path" 2>&1 &
new=\$!
echo "old=\$old new=\$new" > "$lock_restore_pids_path"
for _ in \$(seq 1 60); do
  $ipc call lock status 2>/dev/null | grep -q '"secure":true' && break
  sleep 0.25
done
$ipc call lock status > "$lock_restore_status_path" 2>&1
sleep 3
"$grim_bin" "$lock_restore_back_path" > /dev/null 2>&1
"$wtype_bin" -s 300 "formalshell-test" -k Return
sleep 4
$ipc call lock status > "$lock_restore_after_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_lock_restore_assert() {
  local f avatar_px
  for f in "$lock_restore_dead_path" "$lock_restore_back_path"; do
    [ -f "$f" ] || fail "no screenshot at $f"
  done
  echo "SMOKE_LOCK_RESTORE_DEAD $lock_restore_dead_path"
  echo "SMOKE_LOCK_RESTORE_BACK $lock_restore_back_path"
  cat "$lock_restore_pids_path" 2>/dev/null
  echo "-- second shell's lock lines --"
  grep '^lock:' "$lock_restore_log_path" 2>/dev/null || echo "(none)"
  if ! grep -q 'lock: the session was left locked, taking the lock back' "$lock_restore_log_path" 2>/dev/null; then
    fail "the second shell never found the held lock"
  fi
  cat "$lock_restore_status_path" 2>/dev/null; echo
  if ! grep -q '"locked":true' "$lock_restore_status_path" || ! grep -q '"secure":true' "$lock_restore_status_path"; then
    fail "the second shell's lock is not locked and secure. Got: $(cat "$lock_restore_status_path" 2>/dev/null)"
  fi
  avatar_px=$($convert_bin "$lock_restore_back_path" -fuzz 4% -fill black +opaque '#1FB86A' -fill white -opaque '#1FB86A' -format '%[fx:int(mean*w*h)]' info: 2>/dev/null)
  echo "restored lock avatar pixels: ${avatar_px:-none}"
  if [ "${avatar_px:-0}" -lt 1250 ]; then
    fail "the restored lock drew no avatar (${avatar_px:-0} pixels of its colour): not our lock surface on screen"
  fi
  cat "$lock_restore_after_path" 2>/dev/null; echo
  if ! grep -q '"locked":false' "$lock_restore_after_path"; then
    fail "the real password did not unlock the restored lock. Got: $(cat "$lock_restore_after_path" 2>/dev/null)"
  fi
}
